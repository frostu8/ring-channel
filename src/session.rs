//! OAuth sessions.

use axum::{RequestPartsExt as _, extract::FromRequestParts};

use chrono::{DateTime, Utc};

use derive_more::{Deref, Display, Error};

use std::fmt::{self, Debug, Formatter};

use http::request::Parts;

use oauth2::{
    EmptyExtraTokenFields, StandardTokenResponse, TokenResponse as _, basic::BasicTokenType,
};

use rand::{Rng, distr::Distribution};

use serde::{Deserialize, Serialize};

use tower_sessions::Session as TowerSession;

use crate::app::{AppError, error::AppErrorKind};

/// A session, used to keep state.
///
/// **Warning!** These sessions are short-lived and are simply to carry some
/// basic information about the client.
///
/// These are not for credentials!
#[derive(Clone, Deref)]
pub struct Session {
    session: TowerSession,
    #[deref]
    data: SessionData,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SessionData {
    /// A randomly generated state string.
    ///
    /// See [discord's docs] on how they suggest to use state.
    ///
    /// [discord's docs]: https://discord.com/developers/docs/topics/oauth2#state-and-security
    pub state: String,
    #[serde(flatten)]
    pub token: Option<AccessToken>,
}

impl Session {
    /// The name of the key this struct is stored in on the session.
    pub const SESSION_KEY: &'static str = "oauth_session";
}

impl Debug for Session {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.data.state)
            .field("token", &self.data.token)
            .finish()
    }
}

/// The token granted by an Oauth flow.
#[derive(Clone, Deserialize, Serialize)]
pub struct AccessToken {
    /// The access token of the user.
    pub access_token: String,
    /// The refresh token of the user.
    pub refresh_token: String,
    /// When the token expires.
    pub expires_at: DateTime<Utc>,
}

impl Debug for AccessToken {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("AccessToken")
            .field("expires_at", &self.expires_at)
            .finish_non_exhaustive()
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
                token: None,
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
