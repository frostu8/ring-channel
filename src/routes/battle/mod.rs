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

use crate::{
    app::{AppError, AppJson, AppState, Payload, error::AppErrorKind},
    player::UpsertPlayer,
};

/// Creates a match.
#[instrument(skip(state))]
pub async fn create(
    State(state): State<AppState>,
    Payload(request): Payload<CreateBattleRequest>,
) -> Result<(StatusCode, AppJson<Battle>), AppError> {
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

    // re-register players
    for player in request.participants.iter() {
        let UpsertPlayer { id, .. } = crate::player::upsert_player(player, &mut *tx).await?;

        // add player to match
        sqlx::query(
            r#"
            INSERT INTO participant (match_id, player_id, team)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(match_id)
        .bind(id)
        .bind(u8::from(player.team))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    // Create battle model
    let battle = Battle {
        id: uuid,
        level_name: request.level_name,
        participants: request.participants,
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
    Path((uuid, rrid)): Path<(Uuid, Rrid)>,
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
        team: Option<u8>,
        no_contest: Option<bool>,
        display_name: String,
    }

    // find the battle participant
    let participant = sqlx::query_as::<_, ParticipantQuery>(
        r#"
        SELECT
            pt.no_contest,
            pt.team,
            p.display_name
        FROM
            player p
        LEFT OUTER JOIN
            participant pt
            ON pt.player_id = p.id
        WHERE
            p.public_key = $1
            AND pt.match_id = $2
        "#,
    )
    .bind(rrid.as_str())
    .bind(battle.id)
    .fetch_optional(&state.db)
    .await?;

    let Some(participant) = participant else {
        // The player with that RRID does not exist.
        return Err(AppError::not_found(format!(
            "Player w/ RRID {} does not exist.",
            rrid
        )));
    };

    let Some((team, no_contest)) = participant.team.zip(participant.no_contest) else {
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
        SET finish_time = $1
        WHERE player_id = $2 AND match_id = $3
        "#,
    )
    .bind(request.finish_time)
    .bind(rrid.as_str())
    .bind(uuid.hyphenated().to_string())
    .execute(&state.db)
    .await?;

    Ok(AppJson(Participant {
        player: Player {
            id: rrid,
            display_name: participant.display_name,
        },
        team: PlayerTeam::try_from(team).map_err(AppError::new)?,
        finish_time: Some(request.finish_time),
        no_contest,
    }))
}
