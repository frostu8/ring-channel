//! Placement API.

use axum::{
    Extension,
    extract::{Path, State},
};

use ring_channel_model::{
    Player,
    battle::{BattleStatus, Participant, PlayerTeam},
    request::battle::UpdatePlayerPlacementRequest,
};

use sqlx::FromRow;

use tracing::instrument;

use uuid::Uuid;

use crate::{
    app::{AppError, AppJson, AppState, Model, Payload, error::AppErrorKind},
    auth::api_key::ServerAuthentication,
    player::mmr::{self, Rating, RawRating},
};

/// Updates the placement of a player for a given match.
#[instrument(skip(state, model))]
pub async fn update<T>(
    _auth_guard: ServerAuthentication,
    Path((uuid, short_id)): Path<(Uuid, String)>,
    Extension(model): Extension<Model<T>>,
    State(state): State<AppState>,
    Payload(request): Payload<UpdatePlayerPlacementRequest>,
) -> Result<AppJson<Participant>, AppError>
where
    T: mmr::Model + 'static,
{
    #[derive(FromRow)]
    struct BattleQuery {
        id: i32,
        #[sqlx(try_from = "u8")]
        status: BattleStatus,
    }

    #[derive(FromRow)]
    struct ParticipantQuery {
        id: Option<i32>,
        player_id: i32,
        team: Option<u8>,
        no_contest: Option<bool>,
        finish_time: Option<i32>,
        skin: Option<String>,
        kart_speed: Option<i32>,
        kart_weight: Option<i32>,
        display_name: String,
        rating: Option<f32>,
        deviation: Option<f32>,
        #[sqlx(rename = "rating_extra")]
        extra: Option<String>,
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
            pt.*,
            p.id AS player_id,
            p.display_name,
            p.rating,
            p.deviation,
            p.rating_extra
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

    let rating = if !model.ratings_enabled() {
        None
    } else if let Some((rating, deviation)) = participant.rating.zip(participant.deviation) {
        let rating = RawRating {
            player_id: participant.player_id,
            rating,
            deviation,
            extra: participant.extra,
        };

        Some(Rating::<T::Data>::try_from(rating).map_err(AppError::new)?)
    } else {
        None
    };

    Ok(AppJson(Participant {
        player: Player {
            id: short_id,
            mmr: rating.map(|r| r.ordinal() as i32),
            public_key: None,
            display_name: participant.display_name,
        },
        team: PlayerTeam::try_from(team).map_err(AppError::new)?,
        finish_time: finish_time.or(request.finish_time),
        no_contest,
        skin: participant.skin,
        kart_speed: participant.kart_speed,
        kart_weight: participant.kart_weight,
    }))
}
