//! Messages sent by servers.

use serde::{Deserialize, Serialize};

use crate::battle::Battle;

/// A notification for a new battle.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct NewBattle(pub Battle);
