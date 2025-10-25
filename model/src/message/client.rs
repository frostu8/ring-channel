//! Client messages.

use serde::{Deserialize, Serialize};

/// A heartbeat.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Heartbeat {
    /// The sequence number of the heartbeat.
    pub seq: i32,
}
