pub mod mmr;

use ring_channel_model::Player;
use sqlx::{FromRow, SqliteConnection};

use crate::app::{AppError, Model};

use mmr::{Rating, RawRating};

/// A row in the database representing a player.
#[derive(FromRow)]
pub struct RawPlayer {
    #[sqlx(rename = "player_id")]
    pub id: i32,
    pub short_id: String,
    pub display_name: String,
    pub rating: Option<f32>,
    pub deviation: Option<f32>,
    #[sqlx(rename = "rating_extra")]
    pub extra: Option<String>,
}

impl RawPlayer {
    /// Converts a raw player into an API-ready player.
    pub fn normalize<T>(self, model: &Model<T>) -> Result<Player, AppError>
    where
        T: mmr::Model + 'static,
    {
        let rating = if !model.ratings_enabled() {
            None
        } else if let Some((rating, deviation)) = self.rating.zip(self.deviation) {
            let rating = RawRating {
                player_id: self.id,
                rating,
                deviation,
                extra: self.extra,
            };

            Some(Rating::<T::Data>::try_from(rating).map_err(AppError::new)?)
        } else {
            None
        };

        Ok(Player {
            id: self.short_id,
            display_name: self.display_name,
            mmr: rating.map(|rating| rating.ordinal() as i32),
            public_key: None,
        })
    }
}

/// Gets a player by their short id.
pub async fn get_player(
    short_id: &str,
    conn: &mut SqliteConnection,
) -> Result<Option<RawPlayer>, AppError> {
    sqlx::query_as::<_, RawPlayer>(
        r#"
        SELECT
            id AS player_id,
            short_id,
            display_name,
            rating,
            deviation,
            rating_extra
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
