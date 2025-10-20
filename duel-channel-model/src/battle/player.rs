//! Player model.

use derive_more::{Deref, Display};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{Error as _, Unexpected},
};

/// A player on the Ring Racers server.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Player {
    /// The id of the player.
    ///
    /// The base16 encoded public key of the player, which is a 64-character
    /// string.
    pub id: Rrid,
    /// The last display name used by the player.
    pub display_name: String,
}

/// Ring Racers ID.
#[derive(Clone, Debug, Deref, Display)]
pub struct Rrid(String);

impl Rrid {
    /// Represents the Rrid as a string.
    pub fn as_str(&self) -> &str {
        <Self as AsRef<str>>::as_ref(self)
    }
}

impl AsRef<str> for Rrid {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Rrid {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let id = String::deserialize(deserializer)?;

        if id.len() == 64 {
            Ok(Rrid(id))
        } else {
            Err(D::Error::invalid_value(
                Unexpected::Str(&id),
                &"an rrid of length 64",
            ))
        }
    }
}

impl Serialize for Rrid {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}
