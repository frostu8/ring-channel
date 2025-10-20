//! Battle endpoint request bodies.

use serde::{Deserialize, Serialize};

use crate::battle::Participant;

/// A body to create a battle.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateBattleRequest {
    /// The level the battle is taking place on.
    pub level_name: String,
    /// The players to register for this battle.
    pub participants: Vec<Participant>,
    /// How long bets should last for, in seconds.
    ///
    /// Uses `20` seconds as the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bet_time: Option<i64>,
}
