//! User structs and utilities.

pub mod bot;

use ring_channel_model::{User, user::UserFlags};

use sqlx::FromRow;

/// A user schema.
#[derive(FromRow)]
pub struct UserSchema {
    pub id: i32,
    pub username: String,
    pub avatar: Option<String>,
    pub display_name: String,
    pub mobiums: i64,
    pub mobiums_gained: i64,
    pub mobiums_lost: i64,
    #[sqlx(try_from = "i32")]
    pub flags: UserFlags,
}

impl From<UserSchema> for User {
    fn from(value: UserSchema) -> Self {
        User {
            username: value.username,
            avatar: value.avatar,
            display_name: value.display_name,
            mobiums: value.mobiums,
            mobiums_gained: value.mobiums_gained,
            mobiums_lost: value.mobiums_lost,
            flags: value.flags,
        }
    }
}

impl From<&UserSchema> for User {
    fn from(value: &UserSchema) -> Self {
        User {
            username: value.username.clone(),
            avatar: value.avatar.clone(),
            display_name: value.display_name.clone(),
            mobiums: value.mobiums,
            mobiums_gained: value.mobiums_gained,
            mobiums_lost: value.mobiums_lost,
            flags: value.flags,
        }
    }
}
