pub mod mmr;

use ring_channel_model::Player;
use sqlx::{FromRow, SqliteConnection};

use crate::app::AppError;

/// A row in the database representing a player.
#[derive(FromRow)]
pub struct PlayerRow {
    #[sqlx(rename = "player_id")]
    pub id: i32,
    pub short_id: String,
    pub display_name: String,
    #[sqlx(flatten)]
    pub rating: mmr::CurrentPlayerRating,
}

impl From<PlayerRow> for Player {
    fn from(player: PlayerRow) -> Self {
        Player {
            id: player.short_id.to_string(),
            mmr: player.rating.ordinal() as i32,
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
        SELECT
            id AS player_id,
            short_id,
            display_name,
            rating,
            deviation,
            volatility
        FROM
            player
        WHERE
            short_id = $1
        "#,
    )
    .bind(short_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(AppError::from)
}
