//! Wager routes.

use ring_channel_model::battle::BattleWager;

use crate::app::{AppError, AppJson};

/// Creates a personal wager.
pub async fn create() -> Result<AppJson<BattleWager>, AppError> {
    todo!()
}
