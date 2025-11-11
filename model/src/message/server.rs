//! Messages sent by servers.

use serde::{Deserialize, Serialize};

use crate::{BattleWager, battle::Battle, chat::Message};

/// Heartbeat acknowledgement.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HeartbeatAck {
    /// The sequence number this is acknowledging.
    pub seq: i32,
}

/// A chat message notification.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NewMessage(pub Message);

/// A notification for a new match.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NewBattle(pub Battle);

/// A notification that a match has closed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BattleUpdate(pub Battle);

/// A notification that someone has made a wager on the room's battle.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WagerUpdate(pub BattleWager);

/// A notification of a mobiums change.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MobiumsChange {
    /// How many mobiums you have now.
    pub mobiums: i64,
    /// Whether or not the final result of this change was affected by a
    /// bailout.
    pub bailout: bool,
}
