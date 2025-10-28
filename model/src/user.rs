//! User representations.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use bytemuck::cast;

/// The current user returned by `/users/~me`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct CurrentUser {
    /// The unique username of the user.
    ///
    /// If this is `None`, the user may have to set their username before they
    /// can play or access endpoints.
    pub username: Option<String>,
    /// The URL of the user's avatar.
    pub avatar: Option<String>,
    /// The display name of the user.
    pub display_name: String,
    /// How many mobiums they have.
    pub mobiums: i64,
    /// How many mobiums they have gained in their lifetime.
    pub mobiums_gained: i64,
    /// How many mobiums they have lost in their lifetime.
    pub mobiums_lost: i64,
    /// The user flags.
    pub flags: UserFlags,
}

/// A single user.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct User {
    /// The unique username of the user.
    pub username: String,
    /// The URL of the user's avatar.
    pub avatar: Option<String>,
    /// The display name of the user.
    pub display_name: String,
    /// How many mobiums they have.
    pub mobiums: i64,
    /// How many mobiums they have gained in their lifetime.
    pub mobiums_gained: i64,
    /// How many mobiums they have lost in their lifetime.
    pub mobiums_lost: i64,
    /// The user flags.
    pub flags: UserFlags,
}

bitflags::bitflags! {
    /// User flags.
    #[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
    pub struct UserFlags: u32 {
        /// The player may bet with money that they don't have.
        ///
        /// This prevents the player from getting bailouts, but in return they
        /// can bet money they don't have, allowing them to dip into negative
        /// mobiums.
        const UNLIMITED_WAGERS = 0b00000001;
        /// The user is a bot managed by the server.
        const AUTOMATED_USER = 0b00000010;
        /// This user helped beta test. Thanks!
        const BETA_TESTER = 0b00000100;
    }
}

impl From<i32> for UserFlags {
    fn from(value: i32) -> Self {
        let value: u32 = cast(value);
        UserFlags::from_bits_truncate(value)
    }
}

impl From<UserFlags> for i32 {
    fn from(value: UserFlags) -> Self {
        cast(value.bits())
    }
}

/// Converts plaintext to a "usable username."
pub fn to_username_lossy<'a>(username: impl Into<Cow<'a, str>>) -> Cow<'a, str> {
    let username = username.into();

    let mut buf = String::new();

    let mut mark = 0;
    let mut ix = 0;
    let mut is_valid = true;

    for ch in username.chars() {
        if is_username_char(&ch) {
            // valid username char
            if !is_valid {
                // unset flatten
                is_valid = true;
                mark = ix;
            }
        } else {
            if is_valid {
                buf.push_str(&username[mark..ix]);
                is_valid = false;
            }

            if ch.is_ascii_uppercase() {
                // make lowercase
                buf.extend(ch.to_lowercase());
            }
        }

        ix += ch.len_utf8();
    }

    if !is_valid {
        mark = username.len();
    }

    if mark > 0 {
        buf.push_str(&username[mark..]);
        Cow::Owned(buf)
    } else {
        username
    }
}

/// Checks if a char is valid in a username.
///
/// Some endpoints start with a special char (like "~") so they don't end up
/// masking a valid username.
///
/// A username like `login` might get masked by the `/users/login` endpoint,
/// for example.
pub fn is_username_char(ch: &char) -> bool {
    ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    pub fn test_to_username_lossy() {
        // Valid usernames
        assert_eq!(to_username_lossy("frostu8"), "frostu8");
        assert_eq!(to_username_lossy("the_slime"), "the_slime");
        assert_eq!(to_username_lossy("kebab-hero"), "kebab-hero");
        assert_eq!(
            to_username_lossy("__xx__destroyer__xx__"),
            "__xx__destroyer__xx__"
        );

        // Valid but incorrectly case
        assert_eq!(to_username_lossy("SCREAMER"), "screamer");
        assert_eq!(to_username_lossy("-The_Giggler"), "-the_giggler");

        // Invalid
        assert_eq!(to_username_lossy("~login"), "login");
        assert_eq!(to_username_lossy("@everyone"), "everyone");
        assert_eq!(to_username_lossy("__+Cursed+String***"), "__cursedstring");

        // Sanity check
        assert_eq!(to_username_lossy(""), "");
    }
}
