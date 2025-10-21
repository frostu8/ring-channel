//! Player interactions.

use chrono::Utc;

use ring_channel_model::Player;

use sqlx::{Executor, Sqlite, prelude::FromRow};

use crate::app::AppError;

/// The result of a player upsert.
#[derive(FromRow)]
pub struct UpsertPlayer {
    /// The ID of the player that got upserted.
    pub id: i32,
    /// The display_name of the player.
    pub display_name: String,
    /// The public key of the player.
    pub public_key: String,
}

/// Upserts a player into the database.
pub async fn upsert_player<'a, 'c, E>(
    player: &Player,
    tx: &'a mut E,
) -> Result<UpsertPlayer, AppError>
where
    &'a mut E: Executor<'c, Database = Sqlite>,
{
    let now = Utc::now();

    sqlx::query_as::<_, UpsertPlayer>(
        r#"
        INSERT INTO player (public_key, display_name, inserted_at, updated_at)
        VALUES ($1, $2, $3, $3)
        ON CONFLICT (public_key) DO UPDATE
        SET
            display_name = $2,
            updated_at = $3
        RETURNING
            id, display_name, public_key
        "#,
    )
    .bind(player.id.as_str())
    .bind(&player.display_name)
    .bind(now)
    .fetch_one(&mut *tx)
    .await
    .map_err(From::from)
}
