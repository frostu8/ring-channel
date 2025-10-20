//! WebSocket gateway.

use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};

use crate::{app::AppState, auth::AuthenticatedUser};

/// Establishes a connection to the websocket gateway.
pub async fn handler(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_failed_upgrade(|error| {
        tracing::error!("failed to upgrade websocket: {}", error);
    })
    .on_upgrade(move |websocket| state.room.serve(websocket, None))
}
