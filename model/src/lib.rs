//! API model representations.

pub mod battle;
pub mod error;
pub mod message;
pub mod player;
pub mod request;
pub mod response;
pub mod user;

pub use battle::{Battle, BattleWager};
pub use error::ApiError;
pub use player::{Player, Rrid};
pub use user::User;
