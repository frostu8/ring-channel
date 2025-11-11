//! Chat crossposting.

use serde::{Deserialize, Serialize};

/// A player sent a chat message.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateChatMessage {
    /// The ID of the player that sent the chat message.
    pub player_id: String,
    /// The content of their message.
    pub content: String,
}
