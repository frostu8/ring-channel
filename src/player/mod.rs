pub mod mmr;

use ring_channel_model::Player;
use sqlx::{FromRow, SqliteConnection};

use crate::app::AppError;

/// A row in the database representing a player.
#[derive(FromRow)]
pub struct PlayerRow {
    pub id: i32,
    pub short_id: String,
    pub display_name: String,
    pub rating: f32,
}

impl From<PlayerRow> for Player {
    fn from(player: PlayerRow) -> Self {
        Player {
            id: player.short_id.to_string(),
            mmr: player.rating as i32,
            display_name: player.display_name,
            public_key: None,
        }
    }
}

/// Gets a player by their short id.
pub async fn get_player(
    short_id: &str,
    conn: &mut SqliteConnection,
) -> Result<Option<PlayerRow>, AppError> {
    sqlx::query_as::<_, PlayerRow>(
        r#"
        SELECT id, short_id, display_name, rating
        FROM player
        WHERE short_id = $1
        "#,
    )
    .bind(short_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(AppError::from)
}
