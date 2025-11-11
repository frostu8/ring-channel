//! WebSocket events.

pub mod client;
pub mod server;

use derive_more::From;

use serde::{Deserialize, Serialize};

use crate::message::{
    client::Heartbeat,
    server::{BattleUpdate, HeartbeatAck, MobiumsChange, NewBattle, NewMessage, WagerUpdate},
};

/// A WebSocket message.
///
/// This has both client and server messages.
#[derive(Clone, Debug, Deserialize, Serialize, From)]
#[serde(tag = "op", content = "d", rename_all = "kebab-case")]
pub enum Message {
    /// Periodic keepalive meessage from client.
    Heartbeat(Heartbeat),
    /// Response for a [`Message::Heartbeat`].
    HeartbeatAck(HeartbeatAck),
    /// A new message was sent in the server.
    NewMessage(NewMessage),
    /// A server notification for a new match.
    NewBattle(NewBattle),
    /// A server notification for a concluded match.
    BattleUpdate(BattleUpdate),
    /// A server notification that a user has made a wager on the match.
    WagerUpdate(WagerUpdate),
    /// A server notification for mobiums change on your acc.
    ///
    /// This is most of the time because a wager resolved
    MobiumsChange(MobiumsChange),
}
