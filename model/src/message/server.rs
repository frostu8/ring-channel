//! Messages sent by servers.

use serde::{Deserialize, Serialize};

use crate::battle::Battle;

/// A notification for a new match.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NewBattle(pub Battle);

/// A notification that a match has closed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BattleConcluded(pub Battle);

/// A notification of a mobiums change.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MobiumsChange {
    /// How many mobiums you have now.
    pub mobiums: i64,
    /// Whether or not the final result of this change was affected by a
    /// bailout.
    pub bailout: bool,
}
