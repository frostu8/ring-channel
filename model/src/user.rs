//! User representations.

use serde::{Deserialize, Serialize};

/// A single user.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct User {
    /// The unique username of the user.
    pub username: String,
    /// How many mobiums they have.
    pub mobiums: i64,
}
