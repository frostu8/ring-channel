//! WebSocket events.

use serde::{Deserialize, Serialize};

use crate::message::server::NewBattle;

pub mod server;

/// A WebSocket message.
///
/// This has both client and server messages.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", content = "d", rename_all = "kebab-case")]
pub enum Message {
    /// A server notification for a new battle.
    NewBattle(NewBattle),
}
