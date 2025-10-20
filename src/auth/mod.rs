//! User authentication.

use axum::extract::{FromRef, FromRequestParts};

use derive_more::Deref;

use ring_channel_model::user::User;

use http::request::Parts;

use crate::app::{AppError, AppState};

/// An authenticated user.
#[derive(Clone, Debug, Deref)]
pub struct AuthenticatedUser {
    /// The user that was authenticated.
    ///
    /// This stores the basic API model information about the user. No secret
    /// information is kept here!
    #[deref]
    pub user: User,
    /// The database ID of the user.
    pub id: i32,
}

impl<S> FromRequestParts<S> for AuthenticatedUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        todo!()
    }
}
