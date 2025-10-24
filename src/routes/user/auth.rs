//! User authentication routes.

use axum::{
    extract::{Query, State},
    response::Redirect,
};

use chrono::Utc;
use derive_more::{Display, Error};
use oauth2::{
    AuthorizationCode, CsrfToken, HttpClientError, RefreshToken, RequestTokenError, Scope,
    StandardRevocableToken, TokenResponse as _,
};

use ring_channel_model::user::to_username_lossy;

use twilight_model::user::CurrentUser as DiscordUser;

use serde::Deserialize;

use sqlx::{FromRow, SqliteConnection};

use tracing::instrument;

use crate::{
    app::{AppError, error::AppErrorKind},
    auth::oauth2::{OauthState, Session},
};

#[derive(FromRow)]
struct ExistingUserQuery {
    pub id: i32,
    pub refresh_token: String,
}

/// A response from the Oauth resource holder.
#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub code: String,
    pub state: String,
}

/// Redirects a user to the application authorization.
#[instrument(skip(oauth_state))]
pub async fn redirect(
    mut session: Session,
    State(oauth_state): State<OauthState>,
) -> Result<Redirect, AppError> {
    session.shuffle_csrf().await?;

    // we now have a session, build the url
    let (auth_url, _csrf_token) = oauth_state
        .client
        .authorize_url(|| CsrfToken::new(session.state.clone()))
        .add_scope(Scope::new("identify".into()))
        .url();

    Ok(Redirect::to(auth_url.as_str()))
}

/// Processes a complete grant request.
#[instrument(skip(oauth_state))]
pub async fn login(
    Query(query): Query<LoginResponse>,
    mut session: Session,
    State(oauth_state): State<OauthState>,
) -> Result<Redirect, AppError> {
    // Check for CSRF
    if session.state != query.state {
        tracing::warn!("suspicious request w/ invalid state: {}", query.state);
        // FIXME: It doesn't seem right to send API errors on an endpoint
        // browsers are accessing?
        return Err(AppErrorKind::InvalidState { state: query.state }.into());
    }

    let now = Utc::now();

    let token_result = oauth_state
        .client
        .exchange_code(AuthorizationCode::new(query.code))
        .request_async(&oauth_state.http_client)
        .await;

    // Get token and update session
    let token_result = match token_result {
        Ok(token_result) => token_result,
        Err(RequestTokenError::Request(HttpClientError::Reqwest(err))) => {
            Err(AppErrorKind::HttpClient(*err))?
        }
        Err(err) => Err(AppError::new(err))?,
    };

    // Fetch user from Discord api
    tracing::debug!("requesting user info from Discord API");

    let access_token = token_result.access_token().clone().into_secret();
    let refresh_token = token_result
        .refresh_token()
        .cloned()
        .ok_or(UpdateTokenError::MissingRefreshToken)
        .map_err(AppError::new)?
        .into_secret();
    let _expires_in = token_result
        .expires_in()
        .ok_or(UpdateTokenError::MissingExpiresIn)
        .map(|duration| now + duration)
        .map_err(AppError::new)?;

    let token = format!("Bearer {access_token}");
    let http_client = twilight_http::Client::builder().token(token).build();

    let remote_user = http_client
        .current_user()
        .await?
        .model()
        .await
        .map_err(AppError::new)?;

    tracing::debug!("committing authenticated Discord user");

    let mut tx = oauth_state.db.begin().await?;

    let existing_user = sqlx::query_as::<_, ExistingUserQuery>(
        r#"
        SELECT
            u.id, da.refresh_token
        FROM
            user u, discord_auth da
        WHERE
            u.id = da.user_id
            AND da.discord_id = $1
        "#,
    )
    .bind(remote_user.id.get() as i64)
    .fetch_optional(&mut *tx)
    .await?;

    let user_id = if let Some(existing_user) = existing_user {
        // revoke refresh token
        let revoke_result = oauth_state
            .client
            .revoke_token(StandardRevocableToken::RefreshToken(RefreshToken::new(
                existing_user.refresh_token,
            )))
            .expect("properly configured client")
            .request_async(&oauth_state.http_client)
            .await;

        if let Err(err) = revoke_result {
            tracing::warn!("failed to revoke token: {}", err);
        }

        existing_user.id
    } else {
        try_create_user(&remote_user, &mut *tx).await?
    };

    // replace discord refresh token
    sqlx::query(
        r#"
        INSERT INTO discord_auth
            (user_id, discord_id, refresh_token, last_fetched_at, inserted_at, updated_at)
        VALUES
            ($1, $2, $3, $4, $4, $4)
        ON CONFLICT (user_id) DO UPDATE
        SET
            refresh_token = $3,
            last_fetched_at = $4,
            updated_at = $4
        "#,
    )
    .bind(user_id)
    .bind(remote_user.id.get() as i64)
    .bind(&refresh_token)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    session.shuffle_csrf().await?;
    session.set_user(user_id).await?; // attach user to session

    if let Some(redirect_url) = oauth_state.redirect_to.as_ref() {
        Ok(Redirect::to(&redirect_url))
    } else {
        // TODO: default behavior?
        todo!()
    }
}

/// An error for updating tokens in-database.
#[derive(Clone, Copy, Debug, Display, Error)]
pub enum UpdateTokenError {
    #[display("missing refresh token")]
    MissingRefreshToken,
    #[display("missing expiry time")]
    MissingExpiresIn,
}

async fn try_create_user(
    remote_user: &DiscordUser,
    tx: &mut SqliteConnection,
) -> Result<i32, AppError> {
    let now = Utc::now();

    // user needs to be created
    let username = if remote_user.discriminator > 0 {
        // Old, tag-style username
        format!("{}_{}", remote_user.name, remote_user.discriminator())
    } else {
        // New username
        remote_user.name.clone()
    };
    let username = to_username_lossy(username);

    let display_name = remote_user
        .global_name
        .as_ref()
        .unwrap_or(&remote_user.name);

    let avatar_url = remote_user.avatar.map(|avatar_hash| {
        format!(
            "https://cdn.discordapp.com/avatars/{}/{}.png",
            remote_user.id, avatar_hash
        )
    });

    let res = sqlx::query_as::<_, (i32,)>(
        r#"
        INSERT INTO user (username, display_name, avatar, inserted_at, updated_at)
        VALUES ($1, $2, $3, $4, $4)
        RETURNING id
        "#,
    )
    .bind(&username)
    .bind(display_name)
    .bind(avatar_url)
    .bind(now)
    .fetch_one(&mut *tx)
    .await;

    // check for unique violation
    match res {
        Ok((new_user_id,)) => {
            tracing::info!(id={new_user_id}, %username, "creating new user");
            Ok(new_user_id)
        }
        Err(sqlx::Error::Database(err)) if err.is_unique_violation() => {
            // create plain user
            let (new_user_id,) = sqlx::query_as::<_, (i32,)>(
                r#"
                INSERT INTO user (username, display_name, inserted_at, updated_at)
                VALUES (NULL, $1, $2, $2)
                RETURNING id
                "#,
            )
            .bind(display_name)
            .bind(now)
            .fetch_one(&mut *tx)
            .await?;

            tracing::info!(id = { new_user_id }, "creating new user w/ null username");
            Ok(new_user_id)
        }
        Err(err) => Err(err.into()),
    }
}
