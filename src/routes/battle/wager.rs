//! Wager routes.

use axum::extract::{Path, State};

use chrono::{DateTime, Duration, Utc};

use ring_channel_model::{
    User,
    battle::{BattleStatus, BattleWager, PlayerTeam},
    request::battle::UpdateWager,
    user::UserFlags,
};

use sqlx::{Acquire, FromRow, SqliteConnection};

use uuid::Uuid;

use crate::{
    app::{AppError, AppJson, AppState, Payload, error::AppErrorKind},
    routes::battle::get_battle_id,
    session::{Session, SessionUser},
    user::{UserSchema, bot::get_wager_bot},
};

/// Lists all wagers on a match.
pub async fn list(
    Path((match_id,)): Path<(Uuid,)>,
    State(state): State<AppState>,
) -> Result<AppJson<Vec<BattleWager>>, AppError> {
    let mut conn = state.db.acquire().await?;

    #[derive(FromRow)]
    struct WagerQuery {
        #[sqlx(try_from = "u8")]
        victor: PlayerTeam,
        mobiums: i64,
        updated_at: DateTime<Utc>,
        // user structs
        username: String,
        avatar: Option<String>,
        display_name: String,
        user_mobiums: i64,
        mobiums_gained: i64,
        mobiums_lost: i64,
        #[sqlx(try_from = "i32")]
        flags: UserFlags,
    }

    let battle_id = get_battle_id(match_id, &mut *conn).await?;

    // Fetch all wagers
    let query = sqlx::query_as::<_, WagerQuery>(
        r#"
        SELECT
            w.victor, w.mobiums, w.updated_at,
            u.username, u.display_name, u.avatar, u.mobiums AS user_mobiums,
            u.mobiums_gained, u.mobiums_lost, u.flags
        FROM
            wager w, user u
        WHERE
            w.user_id = u.id
            AND w.mobiums > 0
            AND match_id = $1
        "#,
    )
    .bind(battle_id)
    .fetch_all(&mut *conn)
    .await?;

    Ok(AppJson(
        query
            .into_iter()
            .map(|query| BattleWager {
                user: Some(User {
                    username: query.username,
                    avatar: query.avatar,
                    display_name: query.display_name,
                    mobiums: query.user_mobiums,
                    mobiums_gained: query.mobiums_gained,
                    mobiums_lost: query.mobiums_lost,
                    flags: query.flags,
                }),
                victor: query.victor,
                mobiums: query.mobiums,
                updated_at: query.updated_at,
            })
            .collect(),
    ))
}

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
        avatar: Option<String>,
        display_name: String,
        user_mobiums: i64,
        mobiums_gained: i64,
        mobiums_lost: i64,
        #[sqlx(try_from = "i32")]
        flags: UserFlags,
    }

    let battle_id = get_battle_id(match_id, &mut *conn).await?;

    // Fetch the user's wager
    let query = sqlx::query_as::<_, WagerQuery>(
        r#"
        SELECT
            w.victor, w.mobiums, w.updated_at,
            u.username, u.display_name, u.avatar, u.mobiums AS user_mobiums,
            u.mobiums_gained, u.mobiums_lost, u.flags
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
            avatar: query.avatar,
            display_name: query.display_name,
            mobiums: query.user_mobiums,
            mobiums_gained: query.mobiums_gained,
            mobiums_lost: query.mobiums_lost,
            flags: query.flags,
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
        avatar: Option<String>,
        display_name: String,
        user_mobiums: i64,
        mobiums_gained: i64,
        mobiums_lost: i64,
        #[sqlx(try_from = "i32")]
        flags: UserFlags,
    }

    let battle_id = get_battle_id(match_id, &mut *conn).await?;

    // Fetch the user's wager
    let query = sqlx::query_as::<_, WagerQuery>(
        r#"
        SELECT
            w.victor, w.mobiums, w.updated_at,
            u.username, u.display_name, u.avatar, u.mobiums AS user_mobiums,
            u.mobiums_gained, u.mobiums_lost, u.flags
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
            avatar: query.avatar,
            display_name: query.display_name,
            mobiums: query.user_mobiums,
            mobiums_gained: query.mobiums_gained,
            mobiums_lost: query.mobiums_lost,
            flags: query.flags,
        }),
        victor: query.victor,
        mobiums: query.mobiums,
        updated_at: query.updated_at,
    }))
}

/// Creates a personal wager.
pub async fn create(
    Path((match_id,)): Path<(Uuid,)>,
    user: SessionUser,
    mut session: Session,
    State(state): State<AppState>,
    Payload(update_wager): Payload<UpdateWager>,
) -> Result<AppJson<BattleWager>, AppError> {
    #[derive(FromRow)]
    struct BattleQuery {
        id: i32,
        #[sqlx(try_from = "u8")]
        status: BattleStatus,
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

    let mut conn = state.db.acquire().await?;

    // Fetch the wager bot, if we can.
    let wager_bot = if state.config.server.bot.enabled {
        Some(get_wager_bot(&state.config.server.bot, &mut *conn).await?)
    } else {
        None
    };

    let mut tx = conn.begin().await?;

    let battle = sqlx::query_as::<_, BattleQuery>(
        r#"
        SELECT
            id, status, closed_at
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

    // matches that aren't ongoing are automatically closed
    if battle.status != BattleStatus::Ongoing {
        return Err(AppErrorKind::InvalidData("Bets have closed for this match.".into()).into());
    }

    // give a little bit of wiggle room to prevent jebaits
    if battle.closed_at + Duration::seconds(3) < now {
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
        ON CONFLICT (user_id, match_id) DO UPDATE
        SET
            victor = $3,
            mobiums = $4,
            updated_at = $5
        "#,
    )
    .bind(user.identity())
    .bind(battle.id)
    .bind(u8::from(update_wager.victor))
    .bind(update_wager.mobiums)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    // New! Do bot wager if it needs to be added or removed
    // This has to happen in the same transaction to prevent insanity
    if let Some(wager_bot) = wager_bot {
        rebalance_automated_wagers(&state, &wager_bot, battle.id, &mut *tx).await?;
    }

    tx.commit().await?;

    // shuffle csrf after the action is done
    session.shuffle_csrf().await?;

    let wager = BattleWager {
        user: Some(User {
            username: user.username.clone(),
            avatar: user.avatar.clone(),
            display_name: user.display_name.clone(),
            mobiums: user.mobiums,
            mobiums_gained: user.mobiums_gained,
            mobiums_lost: user.mobiums_lost,
            flags: user.flags,
        }),
        victor: update_wager.victor,
        mobiums: update_wager.mobiums,
        updated_at: now,
    };

    // update clients
    state.room.send_wager_update(wager.clone());

    Ok(AppJson(wager))
}

async fn rebalance_automated_wagers(
    state: &AppState,
    wager_bot: &UserSchema,
    battle_id: i32,
    conn: &mut SqliteConnection,
) -> Result<(), AppError> {
    #[derive(Debug, FromRow)]
    struct WagerCountQuery {
        #[sqlx(try_from = "u8")]
        victor: PlayerTeam,
        wager_count: i32,
        bot_wagers: i32,
    }

    let now = Utc::now();

    let wager_counts = sqlx::query_as::<_, WagerCountQuery>(
        r#"
        WITH subq AS (
            SELECT *, w.user_id = $2 AS is_bot_wager
            FROM wager w
            WHERE w.match_id = $1
        )
        SELECT
            p.team AS victor,
            SUM(w.mobiums > 0) AS wager_count,
            SUM(w.is_bot_wager AND w.mobiums > 0) AS bot_wagers
        FROM
            (
                SELECT DISTINCT p.team
                FROM participant p
                WHERE p.match_id = $1
            ) p
        LEFT OUTER JOIN
            subq w ON p.team = w.victor
        GROUP BY
            w.victor
        "#,
    )
    .bind(battle_id)
    .bind(wager_bot.id)
    .fetch_all(&mut *conn)
    .await?;

    // if there is only one team without love, give them some love!
    let empty_wagers = wager_counts
        .iter()
        .filter(|q| q.wager_count - q.bot_wagers <= 0)
        .collect::<Vec<_>>();
    if empty_wagers.len() == 1 {
        let wager_info = empty_wagers.iter().next().expect("len check");

        if wager_info.bot_wagers <= 0 {
            let mobiums = state.config.server.bot.wager_amount;

            sqlx::query(
                r#"
                INSERT INTO wager
                    (user_id, match_id, victor, mobiums, inserted_at, updated_at)
                VALUES
                    ($1, $2, $3, $4, $5, $5)
                ON CONFLICT DO UPDATE
                SET
                    victor = $3,
                    mobiums = $4,
                    updated_at = $5
                "#,
            )
            .bind(wager_bot.id)
            .bind(battle_id)
            .bind(u8::from(wager_info.victor))
            .bind(mobiums)
            .bind(now)
            .execute(&mut *conn)
            .await?;

            state.room.send_wager_update(BattleWager {
                user: Some(User::from(wager_bot)),
                mobiums,
                victor: wager_info.victor,
                updated_at: now,
            });
        }
    } else {
        // Remove existing bot wagers
        for wager_info in wager_counts {
            if wager_info.bot_wagers <= 0 {
                continue;
            }

            sqlx::query(
                r#"
                UPDATE wager
                SET mobiums = 0, updated_at = $3
                WHERE user_id = $1 AND match_id = $2
                "#,
            )
            .bind(wager_bot.id)
            .bind(battle_id)
            .bind(now)
            .execute(&mut *conn)
            .await?;

            state.room.send_wager_update(BattleWager {
                user: Some(User::from(wager_bot)),
                mobiums: 0,
                victor: wager_info.victor,
                updated_at: now,
            });
        }
    }

    Ok(())
}
