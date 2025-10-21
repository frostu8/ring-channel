//! Player request bodies.

use serde::{Deserialize, Serialize};

use crate::Rrid;

/// Request body for registering a player.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RegisterPlayerRequest {
    /// The public key of the player.
    pub public_key: Rrid,
    /// The display name of the player.
    pub display_name: String,
}
