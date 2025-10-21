//! Messages sent by servers.

use serde::{Deserialize, Serialize};

use crate::battle::Battle;

/// A notification for a new match.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NewBattle(pub Battle);

/// A notification that a match has closed.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BattleConcluded(pub Battle);
