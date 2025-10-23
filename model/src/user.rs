//! User representations.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// The current user returned by `/users/~me`.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct CurrentUser {
    /// The unique username of the user.
    ///
    /// If this is `None`, the user may have to set their username before they
    /// can play or access endpoints.
    pub username: Option<String>,
    /// The display name of the user.
    pub display_name: String,
    /// How many mobiums they have.
    pub mobiums: i64,
}

/// A single user.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct User {
    /// The unique username of the user.
    pub username: String,
    /// The display name of the user.
    pub display_name: String,
    /// How many mobiums they have.
    pub mobiums: i64,
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
