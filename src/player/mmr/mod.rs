//! Skill-based placements.

pub mod glicko2;

use glicko2::Outcome;

use chrono::{DateTime, Utc};

use ring_channel_model::battle::BattleStatus;
use sqlx::{FromRow, SqliteConnection};

use crate::{app::AppError, config::MmrConfig};

/// The rating period.
#[derive(Clone, Debug, FromRow)]
pub struct RatingPeriod {
    #[sqlx(rename = "inserted_at")]
    pub started_at: DateTime<Utc>,
    #[sqlx(skip)]
    pub period_elapsed: f32,
}

/// A single player rating.
#[derive(Clone, Debug, FromRow)]
pub struct PlayerRating {
    /// The id of the player this is for.
    pub player_id: i32,
    /// The player's actual rating.
    pub rating: f32,
    /// The rating deviation of the player.
    pub deviation: f32,
    /// The player's "skill volatility." Only applies to Glicko-2.
    pub volatility: f32,
    /// When the rating period started for this player.
    pub inserted_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Inserts a player rating.
async fn insert_rating(rating: &PlayerRating, conn: &mut SqliteConnection) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO rating
            (player_id, rating, deviation, volatility, inserted_at, updated_at)
        VALUES
            ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(rating.player_id)
    .bind(rating.rating)
    .bind(rating.deviation)
    .bind(rating.volatility)
    .bind(rating.inserted_at)
    .bind(rating.updated_at)
    .execute(&mut *conn)
    .await
    .map(|_| ())
    .map_err(AppError::from)
}

/// Initializes a player rating.
pub async fn init_rating(
    player_id: i32,
    config: &MmrConfig,
    conn: &mut SqliteConnection,
) -> Result<PlayerRating, AppError> {
    let now = Utc::now();

    // create a new default rating for this player
    let rating = PlayerRating {
        player_id,
        rating: config.defaults.rating,
        deviation: config.defaults.deviation,
        volatility: config.defaults.volatility,
        inserted_at: now,
        updated_at: now,
    };

    insert_rating(&rating, conn).await?;

    Ok(rating)
}

/// Gets a player's rating.
pub async fn get_rating(
    player_id: i32,
    conn: &mut SqliteConnection,
) -> Result<Option<PlayerRating>, AppError> {
    sqlx::query_as::<_, PlayerRating>(
        r#"
        SELECT *
        FROM rating
        WHERE player_id = $1
        ORDER BY inserted_at DESC
        LIMIT 1
        "#,
    )
    .bind(player_id)
    .fetch_optional(&mut *conn)
    .await
    .map_err(AppError::from)
}

/// Gets a player's rating, or initializes it to default if it does not exist.
pub async fn get_rating_or_init(
    player_id: i32,
    config: &MmrConfig,
    conn: &mut SqliteConnection,
) -> Result<PlayerRating, AppError> {
    // Get the player's record, or insert it if it doesn't exist.
    let rating = get_rating(player_id, conn).await?;
    match rating {
        Some(rating) => Ok(rating),
        None => init_rating(player_id, config, &mut *conn).await,
    }
}

/// Updates a player's current rating.
///
/// Should be called when a match is finished.
///
/// Ensure both player's ratings exist (by calling [`get_rating`] for each of
/// them) before calling this!
pub async fn update_rating(
    rating: &PlayerRating,
    config: &MmrConfig,
    conn: &mut SqliteConnection,
) -> Result<PlayerRating, AppError> {
    let now = Utc::now();

    // Get the current period start
    let period = next_rating_period(config, &mut *conn).await?;
    let ends_at = period.started_at + config.period;

    let matchups = fetch_matchups(rating.player_id, period.started_at, ends_at, &mut *conn)
        .await?
        .into_iter()
        .map(|matchup| glicko2::Matchup {
            opponent: matchup.opponent,
            outcome: if matchup.position > 1 {
                Outcome::Lose
            } else {
                Outcome::Win
            },
        })
        .collect::<Vec<_>>();

    // Get the player's new rating
    let new_rating = glicko2::rate(config, rating, &matchups, period.period_elapsed);

    // Update the rating in-database
    sqlx::query(
        r#"
        UPDATE rating
        SET
            rating = $2,
            deviation = $3,
            volatility = $4,
            updated_at = $5
        WHERE
            id IN (
                SELECT id
                FROM rating
                WHERE player_id = $1
                ORDER BY inserted_at DESC
                LIMIT 1
            )
        "#,
    )
    .bind(rating.player_id)
    .bind(rating.rating)
    .bind(rating.deviation)
    .bind(rating.volatility)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    Ok(new_rating)
}

#[derive(FromRow)]
struct PlayerRatingRollover {
    player_id: i32,
    rating: Option<f32>,
    /// The rating deviation of the player.
    deviation: Option<f32>,
    /// The player's "skill volatility." Only applies to Glicko-2.
    volatility: Option<f32>,
    /// When the rating period started for this player.
    inserted_at: Option<DateTime<Utc>>,
    updated_at: Option<DateTime<Utc>>,
}

impl PlayerRatingRollover {
    pub fn rating(&self) -> Option<PlayerRating> {
        match (
            self.rating,
            self.deviation,
            self.volatility,
            self.inserted_at,
            self.updated_at,
        ) {
            (
                Some(rating),
                Some(deviation),
                Some(volatility),
                Some(inserted_at),
                Some(updated_at),
            ) => Some(PlayerRating {
                player_id: self.player_id,
                rating,
                deviation,
                volatility,
                inserted_at,
                updated_at,
            }),
            _ => None,
        }
    }
}

/// Fetches the last start of the rating period.
///
/// If there are no rating periods, this initializes a rating period and
/// returns it. If there is one, but it has expired, this closes rating
/// periods until falling on a single rating period.
pub async fn next_rating_period(
    config: &MmrConfig,
    conn: &mut SqliteConnection,
) -> Result<RatingPeriod, AppError> {
    let now = Utc::now();

    let period = sqlx::query_as::<_, RatingPeriod>(
        r#"
        SELECT *
        FROM rating_period
        ORDER BY inserted_at DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(&mut *conn)
    .await?;

    let Some(mut period) = period else {
        let period = RatingPeriod {
            started_at: Utc::now(),
            period_elapsed: 0.0,
        };

        tracing::info!(?period, "no mmr logged! creating a new period now...!");

        sqlx::query(
            r#"
            INSERT INTO rating_period (inserted_at)
            VALUES ($1)
            "#,
        )
        .bind(period.started_at)
        .execute(&mut *conn)
        .await?;

        return Ok(period);
    };

    let mut started_at = period.started_at;

    // Close any pending periods
    let delta = now - started_at;
    let mut elapsed_periods = delta.as_seconds_f32() / config.period.as_seconds_f32();

    while elapsed_periods >= 1.0 {
        let ended_at = started_at + config.period;

        let players = sqlx::query_as::<_, PlayerRatingRollover>(
            r#"
            SELECT
                p.id AS player_id, r.rating, r.deviation, r.volatility,
                r.inserted_at, r.updated_at
            FROM player p, rating r
            WHERE
                inserted_at >= $1
                AND inserted_at < $2
            "#,
        )
        .bind(started_at)
        .bind(ended_at)
        .fetch_all(&mut *conn)
        .await?;

        for player in players {
            // All players get their rating rolled over if they had one.
            if let Some(rating) = player.rating() {
                // Fetch the player's matchups
                let matchups = fetch_matchups(player.player_id, started_at, ended_at, &mut *conn)
                    .await?
                    .into_iter()
                    .map(|matchup| glicko2::Matchup {
                        opponent: matchup.opponent,
                        outcome: if matchup.position > 1 {
                            Outcome::Lose
                        } else {
                            Outcome::Win
                        },
                    })
                    .collect::<Vec<_>>();

                // Get the player's new rating
                let new_rating = glicko2::rate(config, &rating, &matchups, 1.0);

                // Insert it into the rating period
                insert_rating(
                    &PlayerRating {
                        inserted_at: started_at,
                        updated_at: started_at,
                        ..new_rating
                    },
                    &mut *conn,
                )
                .await?;
            }
        }

        // Add started at to continue onto next period
        started_at = ended_at;
        elapsed_periods -= 1.0;

        // Insert a new period into the database
        period.started_at = started_at;
        period.period_elapsed = f32::min(elapsed_periods, 1.0);

        sqlx::query(
            r#"
            INSERT INTO rating_period (inserted_at)
            VALUES ($1)
            "#,
        )
        .bind(period.started_at)
        .execute(&mut *conn)
        .await?;
    }

    Ok(period)
}

async fn fetch_matchups(
    player_id: i32,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    conn: &mut SqliteConnection,
) -> Result<Vec<Matchup>, sqlx::Error> {
    Ok(
        sqlx::query_as::<_, Matchup>(include_str!("find_matchups.sql"))
            .bind(player_id)
            .bind(from)
            .bind(to)
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            // Filter short matches if they were cancelled
            .filter(|matchup| match matchup.status {
                BattleStatus::Concluded => true,
                BattleStatus::Cancelled => matchup.finish_time > 35 * 30,
                BattleStatus::Ongoing => false,
            })
            .collect::<Vec<_>>(),
    )
}

/// Calculates the MMR for all players in the last rating period.
pub async fn dump_rating<W: std::io::Write>(
    mut writer: W,
    config: &MmrConfig,
    conn: &mut SqliteConnection,
) -> Result<(), anyhow::Error> {
    let now = Utc::now();
    let from = now - config.period;

    // Write header
    writer.write(b"ID,Player Name,Total Matches,Win/Loss Rate,MMR,X Factor\n")?;

    let players = sqlx::query_as::<_, (i32, String, String)>(
        r#"
        SELECT id, short_id, display_name FROM player
        "#,
    )
    .fetch_all(&mut *conn)
    .await?;

    for (player_id, short_id, display_name) in players {
        // Get the player's record, or insert it if it doesn't exist.
        let rating = sqlx::query_as::<_, PlayerRating>(
            r#"
            SELECT * FROM rating WHERE player_id = $1
            "#,
        )
        .bind(player_id)
        .fetch_optional(&mut *conn)
        .await?
        .unwrap_or_else(|| PlayerRating {
            player_id,
            rating: config.defaults.rating,
            deviation: config.defaults.deviation,
            volatility: config.defaults.volatility,
            inserted_at: now,
            updated_at: now,
        });

        let matchups = fetch_matchups(player_id, from, now, &mut *conn).await?;

        let matches = matchups
            .iter()
            .map(|matchup| glicko2::Matchup {
                opponent: matchup.opponent.clone(),
                outcome: if matchup.position > 1 {
                    Outcome::Lose
                } else {
                    Outcome::Win
                },
            })
            .collect::<Vec<_>>();

        if matches.len() > 0 {
            let new_rating = glicko2::rate(config, &rating, &matches, 1.0);

            let csv_name = display_name.replace("\"", "\"\"");

            let total = matchups.len() as f32;
            let wl_rate = matchups
                .iter()
                .filter(|m| !m.no_contest)
                .map(|_| 1.0)
                .sum::<f32>()
                / total;
            let wl_rate = wl_rate.abs(); // fucked up -0 insanity

            write!(
                writer,
                "{},\"{}\",{},{:.2}%,{},{}\n",
                short_id,
                csv_name,
                matchups.len(),
                wl_rate * 100.0,
                new_rating.rating,
                new_rating.volatility
            )?;
        }
    }

    Ok(())
}

#[derive(Debug, FromRow)]
struct Matchup {
    #[sqlx(flatten)]
    pub opponent: PlayerRating,
    #[sqlx(try_from = "u8")]
    pub status: BattleStatus,
    pub position: i32,
    pub no_contest: bool,
    pub finish_time: i32,
}
