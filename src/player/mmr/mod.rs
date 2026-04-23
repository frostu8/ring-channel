//! Skill-based placements.

pub mod glicko2;
pub mod openskill;

use std::any::Any;
use std::fmt::Debug;

use derive_more::{Deref, DerefMut};

use chrono::{DateTime, TimeDelta, Utc};

use ring_channel_model::battle::BattleStatus;
use serde::{
    Deserialize, Serialize,
    de::{DeserializeOwned, value::UnitDeserializer},
};
use sqlx::{FromRow, SqliteConnection};
use tracing::instrument;

use crate::app::AppError;

/// A rating model.
pub trait Model: Send + Sync {
    /// The associated data type used to make the model function.
    type Data: ModelData + Serialize + DeserializeOwned + 'static;

    /// Initializes a new rating.
    fn create_rating(
        &self,
        player_id: i32,
    ) -> impl Future<Output = Result<Rating<Self::Data>, AppError>> + Send + Sync;

    /// Rates a player's performance.
    ///
    /// This also passes a `period_elapsed` delta.
    fn rate(
        &self,
        rating: &RatingRecord<Self::Data>,
        matchups: &[Matchup<Self::Data>],
        period_elapsed: f32,
    ) -> impl Future<Output = Result<Rating<Self::Data>, AppError>> + Send + Sync;

    /// The time between rating periods.
    fn period(&self) -> TimeDelta;
}

pub trait ModelData: Send + Sync + Sized + 'static {
    /// The ordinal of the rating.
    fn ordinal(rating: &Rating<Self>) -> f32 {
        rating.rating - rating.deviation * 2.0
    }
}

impl ModelData for () {}

/// The rating period.
#[derive(Clone, Debug, FromRow)]
pub struct RatingPeriod {
    pub id: i32,
    #[sqlx(rename = "inserted_at")]
    pub started_at: DateTime<Utc>,
    #[sqlx(skip)]
    pub period_elapsed: f32,
}

/// A matchup between two players.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Matchup<T = ()> {
    /// The opponent of the player.
    pub opponent: RatingRecord<T>,
    /// The status of the match that the player participated in.
    pub status: BattleStatus,
    /// The player's finish position.
    pub position: i32,
    /// The player's finish time.
    pub finish_time: i32,
    /// Whether the player NO CONTEST'd.
    pub no_contest: bool,
}

#[derive(Debug, FromRow)]
struct MatchupQuery {
    #[sqlx(flatten)]
    pub opponent: RawRatingRecord,
    #[sqlx(try_from = "u8")]
    pub status: BattleStatus,
    pub position: i32,
    pub no_contest: bool,
    pub finish_time: i32,
}

impl<T> TryFrom<MatchupQuery> for Matchup<T>
where
    T: DeserializeOwned + 'static,
{
    type Error = ron::Error;

    fn try_from(value: MatchupQuery) -> Result<Self, Self::Error> {
        value.opponent.try_into().map(|opponent| Matchup {
            opponent,
            status: value.status,
            position: value.position,
            finish_time: value.finish_time,
            no_contest: value.no_contest,
        })
    }
}

/// A single player rating.
///
/// The rating may also contain arbitrary info `T` for the relevant MMR system
/// to query.
#[derive(Clone, Debug, Deref, DerefMut, Deserialize, Serialize)]
pub struct Rating<T = ()> {
    /// The id of the player this is for.
    pub player_id: i32,
    /// The player's actual rating.
    pub rating: f32,
    /// The rating deviation of the player.
    pub deviation: f32,
    /// Extra data for the rating system.
    #[deref]
    #[deref_mut]
    #[serde(flatten)]
    pub extra: T,
}

impl<T> Rating<T>
where
    T: ModelData,
{
    /// The player's ordinal.
    ///
    /// This is a number where the player's true skill rating is above with a
    /// 95% chance.
    pub fn ordinal(&self) -> f32 {
        T::ordinal(self)
    }
}

/// A historic player rating.
///
/// These are fetched from the database and are associated with a rating
/// period.
#[derive(Clone, Debug, Deref, DerefMut, Deserialize, Serialize)]
pub struct RatingRecord<T = ()> {
    /// The id of the player this is for.
    pub player_id: i32,
    /// The period this rating belongs to.
    pub period_id: i32,
    /// The player's actual rating.
    pub rating: f32,
    /// The rating deviation of the player.
    pub deviation: f32,
    /// When the record was inserted.
    pub inserted_at: DateTime<Utc>,
    /// Extra data for the rating system.
    #[deref]
    #[deref_mut]
    #[serde(flatten)]
    pub extra: T,
}

impl<T> From<RatingRecord<T>> for Rating<T> {
    fn from(value: RatingRecord<T>) -> Self {
        Rating {
            player_id: value.player_id,
            rating: value.rating,
            deviation: value.deviation,
            extra: value.extra,
        }
    }
}

/// A raw rating.
#[derive(Clone, Debug, FromRow)]
pub struct RawRating {
    /// The id of the player this is for.
    pub player_id: i32,
    /// The player's actual rating.
    pub rating: f32,
    /// The rating deviation of the player.
    pub deviation: f32,
    /// Extra data for the rating system.
    pub extra: Option<String>,
}

impl<T> TryFrom<RawRating> for Rating<T>
where
    T: DeserializeOwned + 'static,
{
    type Error = ron::Error;

    fn try_from(value: RawRating) -> Result<Self, Self::Error> {
        // Deserialize extra
        let extra = deserialize_extra(value.extra.as_deref())?;

        Ok(Rating {
            player_id: value.player_id,
            rating: value.rating,
            deviation: value.deviation,
            extra,
        })
    }
}

/// Inner struct for querying the database.
#[derive(Clone, Debug, FromRow)]
pub struct RawRatingRecord {
    /// The id of the player this is for.
    pub player_id: i32,
    /// The period this rating belongs to.
    pub period_id: i32,
    /// The player's actual rating.
    pub rating: f32,
    /// The rating deviation of the player.
    pub deviation: f32,
    /// When the record was inserted.
    pub inserted_at: DateTime<Utc>,
    /// Serialized extra data.
    pub extra: Option<String>,
}

impl<T> TryFrom<RawRatingRecord> for RatingRecord<T>
where
    T: DeserializeOwned + 'static,
{
    type Error = ron::Error;

    fn try_from(value: RawRatingRecord) -> Result<Self, Self::Error> {
        // Deserialize extra
        let extra = deserialize_extra(value.extra.as_deref())?;

        Ok(RatingRecord {
            player_id: value.player_id,
            period_id: value.period_id,
            rating: value.rating,
            deviation: value.deviation,
            inserted_at: value.inserted_at,
            extra,
        })
    }
}

/// Catalogs a player rating.
async fn catalog_rating<T>(
    period: &RatingPeriod,
    rating: &Rating<T>,
    conn: &mut SqliteConnection,
) -> Result<(), AppError>
where
    T: Serialize + 'static,
{
    let now = Utc::now();

    // serialize extra data
    let extra = serialize_extra(&rating.extra).map_err(AppError::new)?;

    sqlx::query(
        r#"
        INSERT INTO rating
            (player_id, period_id, rating, deviation, extra, inserted_at)
        VALUES
            ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(rating.player_id)
    .bind(period.id)
    .bind(rating.rating)
    .bind(rating.deviation)
    .bind(extra)
    .bind(now)
    //.bind(period.started_at)
    .execute(&mut *conn)
    .await
    .map(|_| ())
    .map_err(AppError::from)
}

/// Initializes a player rating, and inserts it into the database.
pub async fn init_rating<T>(
    player_id: i32,
    model: &T,
    conn: &mut SqliteConnection,
) -> Result<Rating<T::Data>, AppError>
where
    T: Model,
{
    let now = Utc::now();

    let default_rating = model.create_rating(player_id).await?;

    // serialize extra data
    let extra = serialize_extra(&default_rating.extra).map_err(AppError::new)?;

    let result = sqlx::query(
        r#"
        INSERT INTO rating
            (period_id, player_id, rating, deviation, extra, inserted_at)
        SELECT
            p.id, $1, $2, $3, $4, $5
        FROM
            rating_period p
        ORDER BY inserted_at DESC
        LIMIT 1
        "#,
    )
    .bind(player_id)
    .bind(default_rating.rating)
    .bind(default_rating.deviation)
    .bind(&extra)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    // Update user
    sqlx::query(
        r#"
        UPDATE player
        SET rating = $2, deviation = $3, rating_extra = $4, updated_at = $5
        WHERE id = $1
        "#,
    )
    .bind(player_id)
    .bind(default_rating.rating)
    .bind(default_rating.deviation)
    .bind(&extra)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    if result.rows_affected() > 0 {
        Ok(default_rating)
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
                (period_id, player_id, rating, deviation, extra, inserted_at)
            VALUES
                ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(period.id)
        .bind(player_id)
        .bind(default_rating.rating)
        .bind(default_rating.deviation)
        .bind(&extra)
        .bind(now)
        .execute(&mut *conn)
        .await?;

        Ok(default_rating)
    }
}

/// Updates a player's current rating.
///
/// Should be called when a match is finished.
///
/// Ensure both player's ratings exist (by calling [`get_rating`] for each of
/// them) before calling this!
#[instrument(skip(conn))]
pub async fn update_rating<T>(
    rating: &RatingRecord<T::Data>,
    model: &T,
    conn: &mut SqliteConnection,
) -> Result<Rating<T::Data>, AppError>
where
    T: Model + Debug,
    T::Data: Debug,
{
    let now = Utc::now();

    // Get the current period start
    let period = next_rating_period(model, &mut *conn).await?;
    let ends_at = period.started_at + model.period();

    let matchups = fetch_matchups(rating.player_id, period.started_at, ends_at, &mut *conn).await?;

    // Get the player's new rating
    let new_rating = model.rate(rating, &matchups, period.period_elapsed).await?;

    // Cap deviation at certain value
    // TODO: move this into the glicko2 mod
    //new_rating.deviation = f32::min(new_rating.deviation, config.defaults.deviation);

    tracing::debug!(?new_rating, "updating rating for");

    // serialize extra data
    let extra = serialize_extra(&new_rating.extra).map_err(AppError::new)?;

    // Update the rating in-database
    sqlx::query(
        r#"
        UPDATE player
        SET rating = $2, deviation = $3, rating_extra = $4, updated_at = $5
        WHERE id = $1
        "#,
    )
    .bind(new_rating.player_id)
    .bind(new_rating.rating)
    .bind(new_rating.deviation)
    .bind(extra)
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
pub async fn next_rating_period<T>(
    model: &T,
    conn: &mut SqliteConnection,
) -> Result<RatingPeriod, AppError>
where
    T: Model,
{
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
    let mut elapsed_periods = delta.as_seconds_f32() / model.period().as_seconds_f32();

    period.period_elapsed = f32::min(elapsed_periods, 1.0);

    while elapsed_periods >= 1.0 {
        let ended_at = period.started_at + model.period();

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

        let players = sqlx::query_as::<_, RawRatingRecord>(
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
        .await?
        .into_iter()
        .map(|player| RatingRecord::<T::Data>::try_from(player));

        // Update all player's ratings
        for player in players {
            let player = player.map_err(AppError::new)?;

            // All players get their rating rolled over if they had one.
            // Fetch the player's matchups
            let matchups =
                fetch_matchups(player.player_id, period.started_at, ended_at, &mut *conn).await?;

            // Get the player's new rating
            let new_rating = model
                .rate(&player, &matchups, period.period_elapsed)
                .await?;

            let now = Utc::now();

            // serialize extra data
            let extra = serialize_extra(&new_rating.extra).map_err(AppError::new)?;

            // Update the player's existing rating
            sqlx::query(
                r#"
                UPDATE player
                SET rating = $2, deviation = $3, rating_extra = $4, updated_at = $5
                WHERE id = $1
                "#,
            )
            .bind(player.player_id)
            .bind(new_rating.rating)
            .bind(new_rating.deviation)
            .bind(extra)
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

#[instrument(skip(conn))]
async fn fetch_matchups<T>(
    player_id: i32,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    conn: &mut SqliteConnection,
) -> Result<Vec<Matchup<T>>, AppError>
where
    T: DeserializeOwned + 'static,
{
    sqlx::query_as::<_, MatchupQuery>(include_str!("find_matchups.sql"))
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
        .map(|matchup| Matchup::<T>::try_from(matchup))
        .collect::<Result<Vec<_>, _>>()
        .map_err(AppError::new)
}

/// Calculates the MMR for all players in the last rating period.
pub async fn dump_rating<T, W: std::io::Write>(
    mut writer: W,
    model: &T,
    conn: &mut SqliteConnection,
) -> Result<(), anyhow::Error>
where
    T: Model,
{
    let now = Utc::now();
    let from = now - model.period();

    // Write header
    writer.write(b"ID,Player Name,Total Matches,Win/Loss Rate,MMR,Deviation\n")?;

    let players = sqlx::query_as::<_, (i32, String, String)>(
        r#"
        SELECT id, short_id, display_name FROM player
        "#,
    )
    .fetch_all(&mut *conn)
    .await?;

    for (player_id, short_id, display_name) in players {
        // Get the player's record, or insert it if it doesn't exist.
        let rating = sqlx::query_as::<_, RawRatingRecord>(
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

        let rating = RatingRecord::<T::Data>::try_from(rating)?;

        let matchups = fetch_matchups::<T::Data>(player_id, from, now, &mut *conn).await?;

        if matchups.len() > 0 {
            // Get the player's new rating
            let new_rating = model.rate(&rating, &matchups, 1.0).await?;

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
                new_rating.deviation,
            )?;
        }
    }

    Ok(())
}

pub fn serialize_extra<S>(data: &S) -> Result<Option<String>, ron::Error>
where
    S: Any + Serialize,
{
    if (data as &dyn Any).is::<()>() {
        // No extra data needs to be serialized if type is empty.
        Ok(None)
    } else {
        ron::to_string(data).map(Some)
    }
}

pub fn deserialize_extra<D>(extra: Option<&str>) -> Result<D, ron::Error>
where
    D: Any + DeserializeOwned,
{
    match extra {
        Some(data) => ron::from_str(data).map_err(|error| error.code),
        // No extra data should have been serialized
        None => D::deserialize(UnitDeserializer::new()),
    }
}
