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
    pub id: i32,
    #[sqlx(rename = "inserted_at")]
    pub started_at: DateTime<Utc>,
    #[sqlx(skip)]
    pub period_elapsed: f32,
}

/// A single player rating.
#[derive(Clone, Debug, FromRow)]
pub struct CurrentPlayerRating {
    /// The id of the player this is for.
    pub player_id: i32,
    /// The player's actual rating.
    pub rating: f32,
    /// The rating deviation of the player.
    pub deviation: f32,
    /// The player's "skill volatility." Only applies to Glicko-2.
    pub volatility: f32,
}

impl CurrentPlayerRating {
    /// The player's ordinal.
    ///
    /// This is a number where the player's true skill rating is above with a
    /// 95% chance.
    pub fn ordinal(&self) -> f32 {
        self.rating - self.deviation * 2.0
    }
}

/// A historic player rating.
#[derive(Clone, Debug, FromRow)]
pub struct PlayerRating {
    /// The id of the player this is for.
    pub player_id: i32,
    /// The period this rating belongs to.
    pub period_id: i32,
    /// The player's actual rating.
    pub rating: f32,
    /// The rating deviation of the player.
    pub deviation: f32,
    /// The player's "skill volatility." Only applies to Glicko-2.
    pub volatility: f32,
    /// When the rating period started for this player.
    pub inserted_at: DateTime<Utc>,
}

impl From<PlayerRating> for CurrentPlayerRating {
    fn from(value: PlayerRating) -> Self {
        CurrentPlayerRating {
            player_id: value.player_id,
            rating: value.rating,
            deviation: value.deviation,
            volatility: value.volatility,
        }
    }
}

/// Catalogs a player rating.
async fn catalog_rating(
    period: &RatingPeriod,
    rating: &CurrentPlayerRating,
    conn: &mut SqliteConnection,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        INSERT INTO rating
            (player_id, period_id, rating, deviation, volatility, inserted_at)
        VALUES
            ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(rating.player_id)
    .bind(period.id)
    .bind(rating.rating)
    .bind(rating.deviation)
    .bind(rating.volatility)
    .bind(period.started_at)
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
) -> Result<(), AppError> {
    let now = Utc::now();

    let result = sqlx::query(
        r#"
        INSERT INTO rating
            (period_id, player_id, rating, deviation, volatility, inserted_at)
        SELECT
            p.id, $1, $2, $3, $4, $5
        FROM
            rating_period p
        ORDER BY inserted_at DESC
        LIMIT 1
        "#,
    )
    .bind(player_id)
    .bind(config.defaults.rating)
    .bind(config.defaults.deviation)
    .bind(config.defaults.volatility)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    if result.rows_affected() > 0 {
        Ok(())
    } else {
        // make a new rating period and use that id instead
        let period = sqlx::query_as::<_, RatingPeriod>(
            r#"
            INSERT INTO rating_period (inserted_at)
            VALUES ($1)
            RETURNING id, inserted_at
            "#,
        )
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;

        tracing::info!(?period, "no mmr logged! creating a new period now...!");

        sqlx::query(
            r#"
            INSERT INTO rating
                (period_id, player_id, rating, deviation, volatility, inserted_at)
            VALUES
                ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(period.id)
        .bind(player_id)
        .bind(config.defaults.rating)
        .bind(config.defaults.deviation)
        .bind(config.defaults.volatility)
        .bind(now)
        .execute(&mut *conn)
        .await?;

        Ok(())
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
) -> Result<CurrentPlayerRating, AppError> {
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
    let mut new_rating = glicko2::rate(config, rating, &matchups, period.period_elapsed);

    // Cap deviation at certain value
    new_rating.deviation = new_rating.deviation.min(config.defaults.deviation);

    tracing::debug!(?new_rating, "updating rating for");

    // Update the rating in-database
    sqlx::query(
        r#"
        UPDATE player
        SET rating = $2, deviation = $3, volatility = $4, updated_at = $5
        WHERE id = $1
        "#,
    )
    .bind(new_rating.player_id)
    .bind(new_rating.rating)
    .bind(new_rating.deviation)
    .bind(new_rating.volatility)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    Ok(new_rating)
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
        let period = sqlx::query_as::<_, RatingPeriod>(
            r#"
            INSERT INTO rating_period (inserted_at)
            VALUES ($1)
            RETURNING id, inserted_at
            "#,
        )
        .bind(now)
        .fetch_one(&mut *conn)
        .await?;

        tracing::info!(?period, "no mmr logged! creating a new period now...!");

        return Ok(period);
    };

    // Close any pending periods
    let delta = now - period.started_at;
    let mut elapsed_periods = delta.as_seconds_f32() / config.period.as_seconds_f32();

    period.period_elapsed = f32::min(elapsed_periods, 1.0);

    while elapsed_periods >= 1.0 {
        let ended_at = period.started_at + config.period;

        tracing::debug!(
            ?period,
            "closing rating period {} - {}",
            period.started_at,
            ended_at
        );

        // Insert a new period into the database
        let mut new_period = sqlx::query_as::<_, RatingPeriod>(
            r#"
            INSERT INTO rating_period (inserted_at)
            VALUES ($1)
            RETURNING id, inserted_at
            "#,
        )
        .bind(ended_at)
        .fetch_one(&mut *conn)
        .await?;
        new_period.period_elapsed = f32::min(elapsed_periods, 1.0);

        let players = sqlx::query_as::<_, PlayerRating>(
            r#"
            SELECT r.*
            FROM player p, rating r
            WHERE r.id IN (
                SELECT id
                FROM rating r
                WHERE r.player_id = p.id
                ORDER BY inserted_at DESC
                LIMIT 1
            )
            "#,
        )
        .fetch_all(&mut *conn)
        .await?;

        // Update all player's ratings
        for player in players {
            // All players get their rating rolled over if they had one.
            // Fetch the player's matchups
            let matchups =
                fetch_matchups(player.player_id, period.started_at, ended_at, &mut *conn)
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
            let new_rating = glicko2::rate(config, &player, &matchups, 1.0);

            let now = Utc::now();

            // Update the player's existing rating
            sqlx::query(
                r#"
                UPDATE player
                SET rating = $2, deviation = $3, volatility = $4, updated_at = $5
                WHERE id = $1
                "#,
            )
            .bind(player.player_id)
            .bind(new_rating.rating)
            .bind(new_rating.deviation)
            .bind(new_rating.volatility)
            .bind(now)
            .execute(&mut *conn)
            .await?;

            // Insert it into the rating period
            catalog_rating(&new_period, &new_rating, &mut *conn).await?;
        }

        // Add started at to continue onto next period
        period = new_period;
        elapsed_periods -= 1.0;
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
    writer.write(b"ID,Player Name,Total Matches,Win/Loss Rate,MMR,Deviation,X Factor\n")?;

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
            SELECT r.*
            FROM player p, rating r
            WHERE
                p.id = $1
                AND r.id IN (
                    SELECT id
                    FROM rating r
                    WHERE r.player_id = p.id
                    ORDER BY inserted_at DESC
                    LIMIT 1
                )
            "#,
        )
        .bind(player_id)
        .fetch_one(&mut *conn)
        .await?;

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
                "{},\"{}\",{},{:.2}%,{},{},{}\n",
                short_id,
                csv_name,
                matchups.len(),
                wl_rate * 100.0,
                new_rating.rating,
                new_rating.deviation,
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
