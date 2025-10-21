//! Match management routes.

pub mod wager;

use axum::extract::State;

use chrono::{TimeDelta, Utc};

use ring_channel_model::{
    battle::Battle, message::server::NewBattle, request::battle::CreateBattleRequest,
};

use http::StatusCode;

use tracing::instrument;
use uuid::Uuid;

use crate::{
    app::{AppError, AppJson, AppState, Payload},
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
        INSERT INTO battle (uuid, level_name, closed_at, inserted_at, updated_at)
        VALUES ($1, $2, $4, $3, $3)
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
