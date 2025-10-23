//! OAuth sessions.

use axum::{
    RequestPartsExt as _,
    extract::{FromRef, FromRequestParts},
};

use derive_more::Deref;
use ring_channel_model::User;
use sqlx::FromRow;

use std::fmt::{self, Debug, Formatter};

use http::request::Parts;

use rand::{Rng, distr::Distribution};

use serde::{Deserialize, Serialize};

use tower_sessions::Session as TowerSession;

use crate::app::{AppError, AppState, error::AppErrorKind};

/// A session, used to keep state.
///
/// **Warning!** These sessions are short-lived and are simply to carry some
/// basic information about the client.
///
/// These are not for credentials!
#[derive(Clone, Deref)]
pub struct Session {
    #[allow(dead_code)]
    session: TowerSession,
    #[deref]
    data: SessionData,
}

/// Inner session data.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionData {
    /// A randomly generated state string.
    ///
    /// See [discord's docs] on how they suggest to use state.
    ///
    /// [discord's docs]: https://discord.com/developers/docs/topics/oauth2#state-and-security
    pub state: String,
    /// The identity of the user.
    ///
    /// This is the user's ID in the database. If this is `None`, this is an
    /// anonymous session.
    pub identity: Option<i32>,
}

impl Session {
    /// The name of the key this struct is stored in on the session.
    pub const SESSION_KEY: &'static str = "oauth_session";

    /// Sets the user of the session.
    ///
    /// **Only call this if you are confident the user has followed the proper
    /// authentication flow!**
    pub async fn set_user(&mut self, user_id: i32) -> Result<(), tower_sessions::session::Error> {
        self.data.identity = Some(user_id);
        self.session
            .insert(Session::SESSION_KEY, &self.data)
            .await?;

        Ok(())
    }
}

impl Debug for Session {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.data.state)
            .field("identity", &self.data.identity)
            .finish()
    }
}

impl<S> FromRequestParts<S> for Session
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let session = parts
            .extract::<TowerSession>()
            .await
            .map_err(AppErrorKind::SessionFetch)?;

        let session_data = if let Some(session_data) = session.get(Session::SESSION_KEY).await? {
            session_data
        } else {
            // create new session
            tracing::trace!("creating new session");
            let session_data = SessionData {
                state: random_state(),
                identity: None,
            };
            session.insert(Session::SESSION_KEY, &session_data).await?;
            session_data
        };

        Ok(Session {
            session,
            data: session_data,
        })
    }
}

/// An authenticated user.
///
/// This type dereferences into the stored user [`User`], which stores basic
/// information about the user that is typically suitable for most endpoints.
#[derive(Clone, Debug, Deref)]
pub struct SessionUser {
    #[deref]
    user: User,
    identity: i32,
}

impl SessionUser {
    /// Unwraps the inner user model.
    pub fn into_inner(self) -> User {
        self.user
    }

    /// The database ID of the user.
    ///
    /// This is simply a copy of [`SessionData::identity`], but you don't have
    /// to work with an [`Option`].
    pub fn identity(self) -> i32 {
        self.identity
    }
}

impl<S> FromRequestParts<S> for SessionUser
where
    AppState: FromRef<S>,
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        #[derive(FromRow)]
        struct UserQuery {
            username: String,
            mobiums: i64,
        }

        let session = parts.extract_with_state::<Session, S>(state).await?;

        let state = AppState::from_ref(state);

        if let Some(identity) = session.identity {
            // fetch identity
            let user = sqlx::query_as::<_, UserQuery>(
                r#"
                SELECT username, mobiums
                FROM user
                WHERE id = $1
                "#,
            )
            .bind(identity)
            .fetch_optional(&state.db)
            .await?;

            if let Some(UserQuery { username, mobiums }) = user {
                Ok(SessionUser {
                    user: User { username, mobiums },
                    identity,
                })
            } else {
                Err(AppErrorKind::InvalidSession.into())
            }
        } else {
            Err(AppErrorKind::UserUnauthenticated.into())
        }
    }
}

/// A random distribution for base 64.
#[derive(Clone, Copy, Debug, Default)]
pub struct Base64;

impl Distribution<u8> for Base64 {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> u8 {
        const GEN_ASCII_STR_CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                abcdefghijklmnopqrstuvwxyz\
                0123456789\
                -_";

        let ix = rng.next_u32() >> (32 - 6);
        GEN_ASCII_STR_CHARSET[ix as usize]
    }
}

/// Generates a random state with thread-local entropy.
pub fn random_state() -> String {
    let mut rng = rand::rng();
    random_state_with(&mut rng)
}

/// Generates a random state with a provided random generator.
pub fn random_state_with<R>(rng: &mut R) -> String
where
    R: Rng,
{
    rng.sample_iter(Base64).take(64).map(char::from).collect()
}
