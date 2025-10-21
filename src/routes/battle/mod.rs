//! Match management routes.

pub mod wager;

use axum::extract::{Path, State};

use chrono::{TimeDelta, Utc};

use ring_channel_model::{
    Player, Rrid,
    battle::{Battle, Participant, PlayerTeam},
    message::server::NewBattle,
    request::battle::{CreateBattleRequest, UpdatePlayerPlacementRequest},
};

use http::StatusCode;

use sqlx::FromRow;

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
        #[sqlx(try_from = "String")]
        public_key: Rrid,
    }

    let mut tx = state.db.begin().await?;

    // Create the battle
    let uuid = Uuid::new_v4().hyphenated().to_string();
    let now = Utc::now();

    let closed_at = now + TimeDelta::seconds(request.bet_time.unwrap_or(20));

    let (match_id,) = sqlx::query_as::<_, (i32,)>(
        r#"
        INSERT INTO battle (uuid, level_name, closed_at, inserted_at)
        VALUES ($1, $2, $4, $3)
        RETURNING id
        "#,
    )
    .bind(&uuid)
    .bind(&request.level_name)
    .bind(now)
    .bind(closed_at)
    .fetch_one(&mut *tx)
    .await?;

    // register players
    let mut participants = Vec::with_capacity(request.participants.len());
    for input_player in request.participants.iter() {
        // find player
        let player = sqlx::query_as::<_, PlayerQuery>(
            r#"
            SELECT id, short_id, display_name, public_key
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
                INSERT INTO participant (match_id, player_id, team, no_contest)
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
                    public_key: player.public_key,
                    display_name: player.display_name,
                },
                team: input_player.team,
                finish_time: None,
                no_contest: false,
            })
        } else {
            tx.rollback().await?;
            return Err(AppError::not_found(format!(
                "Participant w/ ID {} not found",
                input_player.id
            )));
        }
    }

    tx.commit().await?;

    // Create battle model
    let battle = Battle {
        id: uuid,
        level_name: request.level_name,
        participants,
        accepting_bets: true,
        victor: None,
        closed_at: Some(closed_at),
    };

    // Send the notice of the new battle to all connected clients
    state.room.broadcast(NewBattle(battle.clone()).into());

    Ok((StatusCode::CREATED, AppJson(battle)))
}

/// Updates the placement of a player for a given match.
#[instrument(skip(state))]
pub async fn update_placement(
    Path((uuid, short_id)): Path<(Uuid, String)>,
    State(state): State<AppState>,
    Payload(request): Payload<UpdatePlayerPlacementRequest>,
) -> Result<AppJson<Participant>, AppError> {
    #[derive(FromRow)]
    struct BattleQuery {
        id: i32,
        concluded: bool,
    }

    // find match first
    let battle = sqlx::query_as::<_, BattleQuery>(
        r#"
        SELECT id, concluded
        FROM battle
        WHERE uuid = $1
        "#,
    )
    .bind(uuid.hyphenated().to_string())
    .fetch_optional(&state.db)
    .await?;

    let Some(battle) = battle else {
        return Err(AppError::not_found(format!("Match {} not found.", uuid)));
    };

    // if the battle is closed, it cannot be updated anymore
    if battle.concluded {
        return Err(AppErrorKind::AlreadyConcluded(uuid).into());
    }

    #[derive(FromRow)]
    struct ParticipantQuery {
        id: Option<i32>,
        team: Option<u8>,
        no_contest: Option<bool>,
        display_name: String,
        #[sqlx(try_from = "String")]
        public_key: Rrid,
    }

    // find the battle participant
    let participant = sqlx::query_as::<_, ParticipantQuery>(
        r#"
        SELECT
            pt.id,
            pt.no_contest,
            pt.team,
            p.display_name,
            p.public_key
        FROM
            player p
        LEFT OUTER JOIN
            participant pt
            ON pt.player_id = p.id
        WHERE
            p.short_id = $1
            AND pt.match_id = $2
        "#,
    )
    .bind(&short_id)
    .bind(battle.id)
    .fetch_optional(&state.db)
    .await?;

    let Some(participant) = participant else {
        // The player with that RRID does not exist.
        return Err(AppError::not_found(format!(
            "Player w/ id {} does not exist.",
            short_id
        )));
    };

    let (Some(participant_id), Some(team), Some(no_contest)) =
        (participant.id, participant.team, participant.no_contest)
    else {
        // the player is not participating!
        return Err(AppError::not_found(format!(
            "Player {} not participating in match.",
            participant.display_name
        )));
    };

    // UPDATE THAT SHIT KAKAROT!
    sqlx::query(
        r#"
        UPDATE participant
        SET finish_time = $2
        WHERE id = $1
        "#,
    )
    .bind(participant_id)
    .bind(request.finish_time)
    .execute(&state.db)
    .await?;

    Ok(AppJson(Participant {
        player: Player {
            id: short_id,
            public_key: participant.public_key,
            display_name: participant.display_name,
        },
        team: PlayerTeam::try_from(team).map_err(AppError::new)?,
        finish_time: Some(request.finish_time),
        no_contest,
    }))
}
