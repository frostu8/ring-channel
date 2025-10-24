//! Wager routes.

use axum::extract::{Path, State};

use chrono::{DateTime, Duration, Utc};

use ring_channel_model::{
    User,
    battle::{BattleWager, PlayerTeam},
    request::battle::UpdateWager,
};

use sqlx::FromRow;
use uuid::Uuid;

use crate::{
    app::{AppError, AppJson, AppState, Payload, error::AppErrorKind},
    routes::battle::get_battle_id,
    session::{Session, SessionUser},
};

/// Shows your wager on a match.
pub async fn show_self(
    Path((match_id,)): Path<(Uuid,)>,
    session: SessionUser,
    State(state): State<AppState>,
) -> Result<AppJson<BattleWager>, AppError> {
    let mut conn = state.db.acquire().await?;

    #[derive(FromRow)]
    struct WagerQuery {
        #[sqlx(try_from = "u8")]
        victor: PlayerTeam,
        mobiums: i64,
        updated_at: DateTime<Utc>,
        // user structs
        username: String,
        display_name: String,
        user_mobiums: i64,
    }

    let battle_id = get_battle_id(match_id, &mut *conn).await?;

    // Fetch the user's wager
    let query = sqlx::query_as::<_, WagerQuery>(
        r#"
        SELECT
            w.victor, w.mobiums, w.updated_at,
            u.username, u.display_name, u.mobiums AS user_mobiums
        FROM
            wager w, user u
        WHERE
            w.user_id = u.id
            AND w.mobiums > 0
            AND w.user_id = $1
            AND match_id = $2
        "#,
    )
    .bind(session.identity())
    .bind(battle_id)
    .fetch_optional(&mut *conn)
    .await?;

    let Some(query) = query else {
        return Err(AppError::not_found("Wager not found"));
    };

    Ok(AppJson(BattleWager {
        user: Some(User {
            username: query.username,
            display_name: query.display_name,
            mobiums: query.user_mobiums,
        }),
        victor: query.victor,
        mobiums: query.mobiums,
        updated_at: query.updated_at,
    }))
}

/// Shows another player's wager on the match.
pub async fn show(
    Path((match_id, username)): Path<(Uuid, String)>,
    State(state): State<AppState>,
) -> Result<AppJson<BattleWager>, AppError> {
    let mut conn = state.db.acquire().await?;

    #[derive(FromRow)]
    struct WagerQuery {
        #[sqlx(try_from = "u8")]
        victor: PlayerTeam,
        mobiums: i64,
        updated_at: DateTime<Utc>,
        // user structs
        username: String,
        display_name: String,
        user_mobiums: i64,
    }

    let battle_id = get_battle_id(match_id, &mut *conn).await?;

    // Fetch the user's wager
    let query = sqlx::query_as::<_, WagerQuery>(
        r#"
        SELECT
            w.victor, w.mobiums, w.updated_at,
            u.username, u.display_name, u.mobiums AS user_mobiums
        FROM
            wager w, user u
        WHERE
            w.user_id = u.id
            AND w.username = $1
            AND match_id = $2
        "#,
    )
    .bind(username)
    .bind(battle_id)
    .fetch_optional(&mut *conn)
    .await?;

    let Some(query) = query else {
        return Err(AppError::not_found("Wager not found"));
    };

    Ok(AppJson(BattleWager {
        user: Some(User {
            username: query.username,
            display_name: query.display_name,
            mobiums: query.user_mobiums,
        }),
        victor: query.victor,
        mobiums: query.mobiums,
        updated_at: query.updated_at,
    }))
}

/// Creates a personal wager.
pub async fn create_self(
    Path((match_id,)): Path<(Uuid,)>,
    user: SessionUser,
    mut session: Session,
    State(state): State<AppState>,
    Payload(update_wager): Payload<UpdateWager>,
) -> Result<AppJson<BattleWager>, AppError> {
    #[derive(FromRow)]
    struct BattleQuery {
        id: i32,
        closed_at: DateTime<Utc>,
    }

    // reject any suspicious requests
    if session.csrf != update_wager.csrf {
        return Err(AppErrorKind::InvalidCsrfToken.into());
    }

    if update_wager.mobiums < 0 {
        return Err(AppErrorKind::InvalidData("Mobiums must be non-negative".into()).into());
    }

    if update_wager.mobiums > user.mobiums {
        return Err(AppErrorKind::NotEnoughMobiums.into());
    }

    let now = Utc::now();

    let mut tx = state.db.begin().await?;

    let battle = sqlx::query_as::<_, BattleQuery>(
        r#"
        SELECT
            id, closed_at
        FROM
            battle
        WHERE
            uuid = $1
        "#,
    )
    .bind(match_id.hyphenated().to_string())
    .fetch_optional(&mut *tx)
    .await?;

    let Some(battle) = battle else {
        return Err(AppError::not_found(format!("Match {} not found", match_id)));
    };

    // give a little bit of wiggle room to prevent jebaits
    if battle.closed_at + Duration::seconds(30) < now {
        return Err(AppErrorKind::InvalidData("Bets have closed for this match.".into()).into());
    }

    // check if the user's team actually exists
    let (team_count,) = sqlx::query_as::<_, (i32,)>(
        r#"
        SELECT COUNT(*)
        FROM participant
        WHERE match_id = $1 AND team = $2
        "#,
    )
    .bind(battle.id)
    .bind(u8::from(update_wager.victor))
    .fetch_one(&mut *tx)
    .await?;

    if team_count <= 0 {
        return Err(AppErrorKind::InvalidData(format!(
            "Team {:?} has no participants",
            update_wager.victor
        ))
        .into());
    }

    // update thing
    sqlx::query(
        r#"
        INSERT INTO wager
            (user_id, match_id, victor, mobiums, inserted_at, updated_at)
        VALUES
            ($1, $2, $3, $4, $5, $5)
        "#,
    )
    .bind(user.identity())
    .bind(battle.id)
    .bind(u8::from(update_wager.victor))
    .bind(update_wager.mobiums)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // shuffle csrf after the action is done
    session.shuffle_csrf().await?;

    let wager = BattleWager {
        user: Some(User {
            username: user.username.clone(),
            display_name: user.display_name.clone(),
            mobiums: user.mobiums,
        }),
        victor: update_wager.victor,
        mobiums: update_wager.mobiums,
        updated_at: now,
    };

    // update clients
    state.room.send_wager_update(wager.clone());

    Ok(AppJson(wager))
}
