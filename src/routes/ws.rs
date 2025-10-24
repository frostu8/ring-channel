//! WebSocket gateway.

use axum::{
    extract::{State, WebSocketUpgrade},
    response::Response,
};

use crate::{
    app::{AppError, AppState},
    session::SessionUser,
};

/// Establishes a connection to the websocket gateway.
#[axum::debug_handler]
pub async fn handler(
    user: Result<SessionUser, AppError>,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_failed_upgrade(|error| {
        tracing::error!("failed to upgrade websocket: {}", error);
    })
    .on_upgrade(move |websocket| state.room.serve(websocket, user.ok()))
}
