//! Match management routes.

pub mod player;
pub mod wager;

use axum::extract::{Path, State};

use chrono::{DateTime, TimeDelta, Utc};

use derive_more::{Deref, DerefMut};

use garde::Validate;

use ring_channel_model::{
    Player,
    battle::{Battle, BattleStatus, Participant, PlayerTeam},
    request::battle::{CreateBattleRequest, UpdateBattleRequest},
};

use http::StatusCode;

use serde::Deserialize;

use sqlx::{FromRow, SqliteConnection};

use tracing::instrument;

use uuid::Uuid;

use crate::{
    app::{AppError, AppForm, AppGarde, AppJson, AppState, Payload, error::AppErrorKind},
    auth::api_key::ServerAuthentication,
    battle::{BattleSchema, calculate_winnings},
    player::mmr::{PlayerRating, update_rating},
    room::BattleData,
};

/// A query for [`list`].
#[derive(Deserialize, Debug, Validate)]
#[garde(context(AppState as state))]
pub struct ListBattlesQuery {
    #[garde(range(min = 1, max = 50))]
    #[serde(default = "list_battle_count_default")]
    pub count: i32,
    #[garde(skip)]
    pub before: Option<DateTime<Utc>>,
    #[garde(skip)]
    pub after: Option<DateTime<Utc>>,
}

fn list_battle_count_default() -> i32 {
    50
}

/// Lists all matches.
#[instrument(skip(state))]
#[axum::debug_handler]
pub async fn list(
    State(state): State<AppState>,
    AppGarde(AppForm(query)): AppGarde<AppForm<ListBattlesQuery>>,
) -> Result<AppJson<Vec<Battle>>, AppError> {
    let mut conn = state.db.acquire().await?;

    let mut battles = sqlx::query_as::<_, BattleSchema>(
        r#"
        SELECT
            uuid, level_name, status, inserted_at, closed_at
        FROM
            battle
        WHERE
            ($1 IS NULL OR inserted_at < $1)
            AND ($2 IS NULL OR inserted_at > $2)
        ORDER BY
            inserted_at DESC
        LIMIT $3
        "#,
    )
    .bind(query.before)
    .bind(query.after)
    .bind(query.count)
    .fetch_all(&mut *conn)
    .await?
    .into_iter()
    .map(|b| Battle::from(b))
    .collect::<Vec<_>>();

    // Preload all battles
    for battle in battles.iter_mut() {
        preload_participants(battle, &mut *conn).await?;
    }

    Ok(AppJson(battles))
}

/// Shows an existing match.
#[instrument(skip(state))]
pub async fn show(
    Path((uuid,)): Path<(Uuid,)>,
    State(state): State<AppState>,
) -> Result<AppJson<Battle>, AppError> {
    let mut conn = state.db.acquire().await?;

    let battle = sqlx::query_as::<_, BattleSchema>(
        r#"
        SELECT uuid, level_name, status, inserted_at, closed_at
        FROM battle
        WHERE uuid = $1
        "#,
    )
    .bind(uuid.hyphenated().to_string())
    .fetch_optional(&mut *conn)
    .await?;

    let Some(battle) = battle else {
        return Err(AppError::not_found(format!("Match {} not found", uuid)));
    };

    // Create battle struct
    let mut battle = Battle::from(battle);

    preload_participants(&mut battle, &mut *conn).await?;

    Ok(AppJson(battle))
}

/// Creates a match.
#[instrument(skip(state))]
pub async fn create(
    _auth_guard: ServerAuthentication,
    State(state): State<AppState>,
    Payload(request): Payload<CreateBattleRequest>,
) -> Result<(StatusCode, AppJson<Battle>), AppError> {
    #[derive(FromRow)]
    struct PlayerQuery {
        id: i32,
        short_id: String,
        display_name: String,
        rating: f32,
    }

    let uuid = Uuid::new_v4();
    let now = Utc::now();

    let closes_in = TimeDelta::seconds(request.bet_time.unwrap_or(20));
    let closed_at = now + closes_in;

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
            SELECT p.id, p.short_id, p.display_name, p.rating
            FROM player p
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
                    mmr: player.rating as i32,
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
    let schema = BattleSchema {
        uuid: uuid.hyphenated().to_string(),
        level_name: request.level_name,
        status: BattleStatus::Ongoing,
        inserted_at: now,
        closed_at: closed_at,
    };
    let mut battle = Battle::from(&schema);
    battle.participants = participants.clone();
    battle.accepting_bets = true;
    battle.closes_in = Some(closes_in.num_milliseconds());

    // Send the notice of the new battle to all connected clients
    state
        .room
        .update_battle(BattleData {
            schema,
            participants,
        })
        .await;

    Ok((StatusCode::CREATED, AppJson(battle)))
}

/// Updates a match.
#[instrument(skip(state))]
pub async fn update(
    _auth_guard: ServerAuthentication,
    Path((uuid,)): Path<(Uuid,)>,
    State(state): State<AppState>,
    Payload(request): Payload<UpdateBattleRequest>,
) -> Result<AppJson<Battle>, AppError> {
    #[derive(FromRow, Deref, DerefMut)]
    struct BattleQuery {
        id: i32,
        #[sqlx(flatten)]
        #[deref]
        #[deref_mut]
        schema: BattleSchema,
    }

    let now = Utc::now();

    let mut tx = state.db.begin().await?;

    let battle_query = sqlx::query_as::<_, BattleQuery>(
        r#"
        SELECT
            id, uuid, level_name, status, inserted_at, closed_at
        FROM
            battle
        WHERE
            uuid = $1
        "#,
    )
    .bind(uuid.hyphenated().to_string())
    .fetch_optional(&mut *tx)
    .await?;

    let Some(mut battle_query) = battle_query else {
        return Err(AppError::not_found(format!("Match {} not found", uuid)));
    };

    // Verify changes
    let is_status_changed = request
        .status
        .map(|s| s != battle_query.status)
        .unwrap_or(false);
    if battle_query.status != BattleStatus::Ongoing {
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
        .bind(battle_query.id)
        .execute(&mut *tx)
        .await?;

        set_concluded = Some(now);

        // if this cancels the betting session, we need to stop accepting bets
        if now < battle_query.closed_at {
            battle_query.closed_at = now;
        }

        // Update base schema value
        battle_query.schema.status = new_status;
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
    .bind(battle_query.id)
    .bind(request.status.map(|s| u8::from(s)))
    .bind(battle_query.closed_at)
    .bind(set_concluded)
    .execute(&mut *tx)
    .await?;

    if request.status == Some(BattleStatus::Concluded)
        || request.status == Some(BattleStatus::Cancelled)
    {
        // update ratings for all players
        let ratings = sqlx::query_as::<_, PlayerRating>(
            r#"
            SELECT r.*, pl.id
            FROM participant p, player pl, rating r
            WHERE
                p.player_id = pl.id
                AND r.player_id = pl.id
                AND p.match_id = $1
                AND r.id IN (
                    SELECT id
                    FROM rating
                    WHERE player_id = pl.id
                    ORDER BY inserted_at DESC
                    LIMIT 1
                )
            "#,
        )
        .bind(battle_query.id)
        .fetch_all(&mut *tx)
        .await?;

        for rating in ratings {
            update_rating(&rating, &state.config.mmr, &mut *tx).await?;
        }
    }

    // Create battle struct
    let mut battle = Battle::from(&battle_query.schema);

    preload_participants(&mut battle, &mut *tx).await?;

    // Update websocket listeners
    state
        .room
        .update_battle(BattleData {
            schema: battle_query.schema,
            participants: battle.participants.clone(),
        })
        .await;

    if request.status == Some(BattleStatus::Concluded) {
        // distribute pots!
        calculate_winnings(battle_query.id, &state.room, &mut *tx).await?;
    }

    tx.commit().await?;

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
        rating: f32,
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
            pt.no_contest,
            p.rating
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
                mmr: p.rating as i32,
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

async fn get_battle_id(match_id: Uuid, conn: &mut SqliteConnection) -> Result<i32, AppError> {
    #[derive(FromRow)]
    struct BattleQuery {
        id: i32,
    }

    let battle = sqlx::query_as::<_, BattleQuery>(
        r#"
        SELECT id FROM battle WHERE uuid = $1
        "#,
    )
    .bind(match_id.hyphenated().to_string())
    .fetch_optional(&mut *conn)
    .await?;

    let Some(battle) = battle else {
        return Err(AppError::not_found(format!("Match {} not found", match_id)));
    };

    Ok(battle.id)
}
