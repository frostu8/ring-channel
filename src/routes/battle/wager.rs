//! Wager routes.

use duel_channel_model::battle::BattleWager;

use crate::{
    app::{AppError, AppJson},
    auth::AuthenticatedUser,
};

/// Creates a personal wager.
pub async fn create(user: AuthenticatedUser) -> Result<AppJson<BattleWager>, AppError> {
    todo!()
}
