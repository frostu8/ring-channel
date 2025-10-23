//! Users endpoints.

use ring_channel_model::User;

use crate::{
    app::{AppError, AppJson},
    session::SessionUser,
};

pub mod auth;

/// Returns the currently authenticated user's details.
pub async fn show_me(user: SessionUser) -> Result<AppJson<User>, AppError> {
    Ok(AppJson(user.into_inner()))
}
