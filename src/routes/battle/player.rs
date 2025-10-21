//! Placement API.

use axum::extract::{Path, State};

use ring_channel_model::{
    Player,
    battle::{BattleStatus, Participant, PlayerTeam},
    request::battle::UpdatePlayerPlacementRequest,
};

use sqlx::FromRow;

use tracing::instrument;

use uuid::Uuid;

use crate::app::{AppError, AppJson, AppState, Payload, error::AppErrorKind};

/// Updates the placement of a player for a given match.
#[instrument(skip(state))]
pub async fn update(
    Path((uuid, short_id)): Path<(Uuid, String)>,
    State(state): State<AppState>,
    Payload(request): Payload<UpdatePlayerPlacementRequest>,
) -> Result<AppJson<Participant>, AppError> {
    #[derive(FromRow)]
    struct BattleQuery {
        id: i32,
        #[sqlx(try_from = "u8")]
        status: BattleStatus,
    }

    #[derive(FromRow)]
    struct ParticipantQuery {
        id: Option<i32>,
        team: Option<u8>,
        no_contest: Option<bool>,
        finish_time: Option<i32>,
        display_name: String,
    }

    // find match first
    let battle = sqlx::query_as::<_, BattleQuery>(
        r#"
        SELECT id, status
        FROM battle
        WHERE uuid = $1
        "#,
    )
    .bind(uuid.hyphenated().to_string())
    .fetch_optional(&state.db)
    .await?;

    let Some(battle) = battle else {
        return Err(AppError::not_found(format!("Match {} not found", uuid)));
    };

    // if the battle is closed, it cannot be updated anymore
    if battle.status != BattleStatus::Ongoing {
        return Err(AppErrorKind::AlreadyConcluded(uuid).into());
    }

    // find the battle participant
    let participant = sqlx::query_as::<_, ParticipantQuery>(
        r#"
        SELECT
            pt.id,
            pt.no_contest,
            pt.team,
            pt.finish_time,
            p.display_name
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
            "Player w/ id {} does not exist",
            short_id
        )));
    };

    // Get non-nullables
    let (Some(participant_id), Some(team), Some(no_contest)) =
        (participant.id, participant.team, participant.no_contest)
    else {
        // the player is not participating!
        return Err(AppError::not_found(format!(
            "Player {} not participating in match",
            participant.display_name
        )));
    };

    // Get other fields
    let ParticipantQuery { finish_time, .. } = participant;

    // UPDATE THAT SHIT KAKAROT!
    sqlx::query(
        r#"
        UPDATE
            participant
        SET
            finish_time = IFNULL($2, finish_time)
        WHERE
            id = $1
        "#,
    )
    .bind(participant_id)
    .bind(request.finish_time)
    .execute(&state.db)
    .await?;

    Ok(AppJson(Participant {
        player: Player {
            id: short_id,
            public_key: None,
            display_name: participant.display_name,
        },
        team: PlayerTeam::try_from(team).map_err(AppError::new)?,
        finish_time: finish_time.or(request.finish_time),
        no_contest,
    }))
}
