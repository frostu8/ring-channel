//! API error structs.

use derive_more::{Display, Error};

use serde::{Deserialize, Serialize};

/// An API error.
#[derive(Clone, Debug, Display, Deserialize, Error, Serialize)]
#[display("{message}")]
pub struct ApiError {
    pub message: String,
}
