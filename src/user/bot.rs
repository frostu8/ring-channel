//! Wager bot.

use super::UserSchema;

use chrono::Utc;

use ring_channel_model::user::UserFlags;

use sqlx::SqliteConnection;

use crate::{app::AppError, config::WagerBotConfig};

/// Gets the user information of the wager bot.
///
/// If it doesn't exist, it will make the wager bot first.
pub async fn get_wager_bot(
    config: &WagerBotConfig,
    conn: &mut SqliteConnection,
) -> Result<UserSchema, AppError> {
    let now = Utc::now();

    let query = sqlx::query_as::<_, UserSchema>(
        r#"
        SELECT
            id, username, avatar, display_name, mobiums, mobiums_gained,
            mobiums_lost, flags
        FROM
            user
        WHERE
            username = $1
            AND flags & $2
        "#,
    )
    .bind(&config.username)
    .bind(i32::from(UserFlags::AUTOMATED_USER))
    .fetch_optional(&mut *conn)
    .await?;

    if let Some(query) = query {
        Ok(query)
    } else {
        // Create a new bot user
        tracing::info!(?config.username, "creating a new automated user...");

        let query = sqlx::query_as::<_, UserSchema>(
            r#"
            INSERT INTO user
                (username, display_name, avatar, flags, inserted_at, updated_at)
            VALUES
                ($1, $2, $3, $4, $5, $5)
            RETURNING
                id, username, avatar, display_name, mobiums, mobiums_gained,
                mobiums_lost, flags
            "#,
        )
        .bind(&config.username)
        .bind(&config.display_name)
        .bind(config.avatar.as_ref())
        .bind(i32::from(
            UserFlags::AUTOMATED_USER | UserFlags::UNLIMITED_WAGERS,
        ))
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;

        Ok(query)
    }
}
