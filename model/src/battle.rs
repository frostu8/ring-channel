//! Battle data representations.

use derive_more::Deref;
use num_enum::{IntoPrimitive, TryFromPrimitive};

use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};

use serde_repr::{Deserialize_repr, Serialize_repr};

use crate::{player::Player, user::User};

/// A single match.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Battle {
    /// The unique identifier of the battle.
    pub id: String,
    /// The level name the battle played on.
    pub level_name: String,
    /// The participants.
    pub participants: Vec<Participant>,
    /// Whether the battle is accepting bets or not.
    pub accepting_bets: bool,
    /// The victor of the battle.
    ///
    /// May be `None` if the battle hasn't concluded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub victor: Option<PlayerTeam>,
    /// When wagers close at. Get them in!
    pub closes_at: Option<DateTime<Utc>>,
}

/// A participant in a match.
#[derive(Clone, Debug, Deref, Deserialize, Serialize)]
pub struct Participant {
    /// The player participating.
    #[deref]
    #[serde(flatten)]
    pub player: Player,
    /// The team they are on.
    pub team: PlayerTeam,
    /// The player's finish time, if they finished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_time: Option<i32>,
    /// If the player no contest'd.
    #[serde(default)]
    pub no_contest: bool,
}

/// A team side.
#[derive(
    Clone,
    Copy,
    Debug,
    Deserialize_repr,
    Serialize_repr,
    PartialEq,
    Eq,
    Hash,
    TryFromPrimitive,
    IntoPrimitive,
)]
#[repr(u8)]
pub enum PlayerTeam {
    /// The red team.
    ///
    /// Player 1 is on this team.
    Red = 0,
    /// The blue team.
    ///
    /// Player 2 is on this team.
    Blue = 1,
}

/// A battle bet.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BattleWager {
    /// The user that made this wager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<User>,
    /// The match that made this wager.
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "match")]
    pub battle: Option<Battle>,
    /// What team the player is betting to win.
    pub victor: PlayerTeam,
    /// The wager amount.
    pub mobiums: i64,
    /// The creation time of the wager.
    pub created_at: DateTime<Utc>,
    /// The updated time of the wager.
    pub updated_at: DateTime<Utc>,
}
