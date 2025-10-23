//! Users endpoints.

use axum::extract::State;
use ring_channel_model::user::CurrentUser;
use sqlx::FromRow;

use crate::{
    app::{AppError, AppJson, AppState, error::AppErrorKind},
    session::Session,
};

pub mod auth;

/// Returns the currently authenticated user's details.
pub async fn show_me(
    session: Session,
    State(state): State<AppState>,
) -> Result<AppJson<CurrentUser>, AppError> {
    #[derive(FromRow)]
    struct MaybeUserQuery {
        username: Option<String>,
        display_name: String,
        mobiums: i64,
    }

    if let Some(identity) = session.identity {
        // fetch identity
        let user = sqlx::query_as::<_, MaybeUserQuery>(
            r#"
            SELECT username, display_name, mobiums
            FROM user
            WHERE id = $1
            "#,
        )
        .bind(identity)
        .fetch_optional(&state.db)
        .await?;

        if let Some(user) = user {
            Ok(AppJson(CurrentUser {
                username: user.username,
                display_name: user.display_name,
                mobiums: user.mobiums,
            }))
        } else {
            Err(AppErrorKind::InvalidSession.into())
        }
    } else {
        Err(AppErrorKind::UserUnauthenticated.into())
    }
}
