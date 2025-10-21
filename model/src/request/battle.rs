//! Match endpoint request bodies.

use serde::{Deserialize, Serialize};

use crate::battle::PlayerTeam;

/// Request to create a match.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateBattleRequest {
    /// The level the battle is taking place on.
    pub level_name: String,
    /// The players to register for this battle.
    pub participants: Vec<CreateBattleParticipant>,
    /// How long bets should last for, in seconds.
    ///
    /// Uses `20` seconds as the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bet_time: Option<i64>,
}

/// A participant in a [`CreateBattleRequest`].
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CreateBattleParticipant {
    /// The ID of the participant.
    pub id: String,
    /// What team they are on.
    pub team: PlayerTeam,
}

/// Request to set the placement of a player.
///
/// This may be updated continuously until the match is ended.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct UpdatePlayerPlacementRequest {
    /// The finishing time of the player.
    pub finish_time: i32,
}
