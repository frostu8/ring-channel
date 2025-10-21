//! Player request bodies.

use serde::{Deserialize, Serialize};

/// Request body for registering a player.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RegisterPlayer {
    /// The display name of the player.
    pub display_name: String,
}
