//! Match management routes.

pub mod player;
pub mod wager;

use axum::extract::{Path, State};

use chrono::{DateTime, TimeDelta, Utc};

use ring_channel_model::{
    Player,
    battle::{Battle, BattleStatus, Participant, PlayerTeam},
    message::server::{BattleConcluded, NewBattle},
    request::battle::{CreateBattleRequest, UpdateBattleRequest},
};

use http::StatusCode;

use sqlx::{FromRow, SqliteConnection};

use tracing::instrument;

use uuid::Uuid;

use crate::app::{AppError, AppJson, AppState, Payload, error::AppErrorKind};

/// Creates a match.
#[instrument(skip(state))]
pub async fn create(
    State(state): State<AppState>,
    Payload(request): Payload<CreateBattleRequest>,
) -> Result<(StatusCode, AppJson<Battle>), AppError> {
    #[derive(FromRow)]
    struct PlayerQuery {
        id: i32,
        short_id: String,
        display_name: String,
    }

    let uuid = Uuid::new_v4();
    let now = Utc::now();

    let closed_at = now + TimeDelta::seconds(request.bet_time.unwrap_or(20));

    let mut tx = state.db.begin().await?;

    // Create the battle
    let (match_id,) = sqlx::query_as::<_, (i32,)>(
        r#"
        INSERT INTO battle (uuid, level_name, inserted_at, closed_at, status)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id
        "#,
    )
    .bind(uuid.hyphenated().to_string())
    .bind(&request.level_name)
    .bind(now)
    .bind(closed_at)
    .bind(u8::from(BattleStatus::Ongoing))
    .fetch_one(&mut *tx)
    .await?;

    // register players
    let mut participants = Vec::with_capacity(request.participants.len());
    for input_player in request.participants.iter() {
        // find player
        let player = sqlx::query_as::<_, PlayerQuery>(
            r#"
            SELECT id, short_id, display_name
            FROM player
            WHERE short_id = $1
            "#,
        )
        .bind(&input_player.id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(player) = player {
            // add player to match
            sqlx::query(
                r#"
                INSERT INTO participant
                    (match_id, player_id, team, no_contest)
                VALUES ($1, $2, $3, FALSE)
                "#,
            )
            .bind(match_id)
            .bind(player.id)
            .bind(u8::from(input_player.team))
            .execute(&mut *tx)
            .await?;

            // insert players to vec
            participants.push(Participant {
                player: Player {
                    id: player.short_id,
                    public_key: None,
                    display_name: player.display_name,
                },
                team: input_player.team,
                finish_time: None,
                no_contest: false,
            })
        } else {
            tx.rollback().await?;
            return Err(AppErrorKind::MissingParticipant(input_player.id.clone()).into());
        }
    }

    tx.commit().await?;

    // Create battle model
    let battle = Battle {
        id: uuid.hyphenated().to_string(),
        level_name: request.level_name,
        status: BattleStatus::Ongoing,
        participants,
        accepting_bets: true,
        closes_at: Some(closed_at),
    };

    // Send the notice of the new battle to all connected clients
    state.room.broadcast(NewBattle(battle.clone()).into());

    Ok((StatusCode::CREATED, AppJson(battle)))
}

/// Updates a match.
#[instrument(skip(state))]
pub async fn update(
    Path((uuid,)): Path<(Uuid,)>,
    State(state): State<AppState>,
    Payload(request): Payload<UpdateBattleRequest>,
) -> Result<AppJson<Battle>, AppError> {
    #[derive(FromRow)]
    struct BattleQuery {
        id: i32,
        level_name: String,
        #[sqlx(try_from = "u8")]
        status: BattleStatus,
        closed_at: DateTime<Utc>,
    }

    let now = Utc::now();

    let mut tx = state.db.begin().await?;

    let battle = sqlx::query_as::<_, BattleQuery>(
        r#"
        SELECT
            id, level_name, status, closed_at
        FROM
            battle
        WHERE
            uuid = $1
        "#,
    )
    .bind(uuid.hyphenated().to_string())
    .fetch_optional(&mut *tx)
    .await?;

    let Some(mut battle) = battle else {
        return Err(AppError::not_found(format!("Match {} not found", uuid)));
    };

    // Verify changes
    let is_status_changed = request.status.map(|s| s != battle.status).unwrap_or(false);
    if battle.status != BattleStatus::Ongoing {
        return Err(AppErrorKind::AlreadyConcluded(uuid).into());
    }

    let mut set_concluded = None::<DateTime<Utc>>;

    // CHECK! We may need to process the end of a match here.
    if is_status_changed {
        // is_status_changed conditional gaurantees this is `Some`
        let new_status = request.status.unwrap();

        tracing::debug!("setting {} match status to {:?}", uuid, new_status);

        // Set all participants without a clear time to NO CONTEST
        sqlx::query(
            r#"
            UPDATE
                participant
            SET
                no_contest = TRUE
            WHERE
                finish_time IS NULL
                AND match_id = $1
            "#,
        )
        .bind(battle.id)
        .execute(&mut *tx)
        .await?;

        set_concluded = Some(now);

        // if this cancels the betting session, we need to stop accepting bets
        if now < battle.closed_at {
            battle.closed_at = now;
        }
    }

    // Update match details
    sqlx::query(
        r#"
        UPDATE
            battle
        SET
            status = IFNULL($2, status),
            closed_at = $3,
            concluded_at = IFNULL($4, concluded_at)
        WHERE
            id = $1
        "#,
    )
    .bind(battle.id)
    .bind(request.status.map(|s| u8::from(s)))
    .bind(battle.closed_at)
    .bind(set_concluded)
    .execute(&mut *tx)
    .await?;

    // Create battle struct
    let accepting_bets = now < battle.closed_at;
    let mut battle = Battle {
        id: uuid.hyphenated().to_string(),
        level_name: battle.level_name,
        // We will preload this in a sec
        participants: vec![],
        status: request.status.unwrap_or(battle.status),
        accepting_bets,
        closes_at: if accepting_bets {
            Some(battle.closed_at)
        } else {
            None
        },
    };

    preload_participants(&mut battle, &mut *tx).await?;

    tx.commit().await?;

    // Update websocket listeners
    if set_concluded.is_some() {
        state.room.broadcast(BattleConcluded(battle.clone()).into());
    }

    Ok(AppJson(battle))
}

/// Preloads the `participants` field of a [`Battle`].
///
/// If this function fails, `battle` will not be modified.
pub async fn preload_participants(
    battle: &mut Battle,
    conn: &mut SqliteConnection,
) -> Result<(), AppError> {
    #[derive(FromRow)]
    struct ParticipantsQuery {
        short_id: String,
        display_name: String,
        #[sqlx(try_from = "u8")]
        team: PlayerTeam,
        finish_time: Option<i32>,
        no_contest: bool,
    }

    let participants = sqlx::query_as::<_, ParticipantsQuery>(
        r#"
        SELECT
            p.short_id,
            p.display_name,
            pt.team,
            pt.finish_time,
            pt.no_contest
        FROM
            participant pt, battle b, player p
        WHERE
            pt.match_id = b.id
            AND pt.player_id = p.id
            AND b.uuid = $1
        "#,
    )
    .bind(&battle.id)
    .fetch_all(&mut *conn)
    .await?;

    battle.participants = participants
        .into_iter()
        .map(|p| Participant {
            player: Player {
                id: p.short_id,
                display_name: p.display_name,
                public_key: None,
            },
            team: p.team,
            finish_time: p.finish_time,
            no_contest: p.no_contest,
        })
        .collect();

    Ok(())
}
