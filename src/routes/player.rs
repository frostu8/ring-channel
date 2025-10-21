//! Active player routes.

use axum::extract::{Path, State};

use ring_channel_model::{Player, Rrid, request::player::RegisterPlayerRequest};

use tracing::instrument;

use crate::app::{AppError, AppJson, AppState, Payload};

/// Registers a joined player, or updates an existing player.
///
/// When a player is registered with this endpoint, they have to be removed
/// with the opposite endpoint.
///
/// All players must be registered to create matches for them! The match
/// creation endpoint makes sure all players are created.
#[instrument(skip(state))]
pub async fn register(
    Path((rrid,)): Path<(Rrid,)>,
    State(state): State<AppState>,
    Payload(request): Payload<RegisterPlayerRequest>,
) -> Result<AppJson<Player>, AppError> {
    let mut tx = state.db.begin().await?;

    let player = Player {
        id: rrid,
        display_name: request.display_name,
    };

    // add player to database
    crate::player::upsert_player(&player, &mut *tx).await?;

    tx.commit().await?;

    // return updated record
    Ok(AppJson(player))
}
