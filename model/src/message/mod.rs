//! WebSocket events.

use derive_more::From;

use serde::{Deserialize, Serialize};

use crate::message::server::{BattleUpdate, MobiumsChange, NewBattle, WagerUpdate};

pub mod server;

/// A WebSocket message.
///
/// This has both client and server messages.
#[derive(Clone, Debug, Deserialize, Serialize, From)]
#[serde(tag = "op", content = "d", rename_all = "kebab-case")]
pub enum Message {
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
