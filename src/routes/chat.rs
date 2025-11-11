//! Chat crossposting routes.
//!
//! Since chat is already tracked by clients in logs, it's only fair they can
//! be stored persistently as long as they can't be accessed anonymously.

use axum::extract::State;

use chrono::Utc;
use ring_channel_model::{chat::Message, request::chat::CreateChatMessage};

use crate::{
    app::{AppError, AppJson, AppState, Payload},
    auth::api_key::ServerAuthentication,
    player::get_player,
};

/// Processes a chat message from the server.
pub async fn create(
    State(state): State<AppState>,
    _auth_guard: ServerAuthentication,
    Payload(request): Payload<CreateChatMessage>,
) -> Result<AppJson<Message>, AppError> {
    let now = Utc::now();

    let mut conn = state.db.acquire().await?;

    // fetch player
    let player = get_player(&request.player_id, &mut conn)
        .await
        .and_then(|f| {
            f.ok_or_else(|| AppError::not_found(format!("Player {} not found", request.player_id)))
        })?;

    sqlx::query(
        r#"
        INSERT INTO message (player_id, content, inserted_at)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind(player.id)
    .bind(&request.content)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    let message = Message {
        player: player.into(),
        content: request.content,
        created_at: now.format("%+").to_string(),
    };

    // log chat message
    state.room.send_message(message.clone()).await;

    Ok(AppJson(message))
}
