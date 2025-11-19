//! Active player routes.

use axum::extract::{Path, State};

use chrono::Utc;

use http::StatusCode;

use rand::{Rng, distr::Alphanumeric};

use ring_channel_model::{Player, request::player::RegisterPlayerRequest};

use sqlx::FromRow;

use tracing::instrument;

use crate::{
    app::{AppError, AppJson, AppState, Payload, error::AppErrorKind},
    auth::api_key::ServerAuthentication,
    player::{
        get_player,
        mmr::{CurrentPlayerRating, init_rating},
    },
};

pub const MAX_INSERT_ATTEMPTS: usize = 25;

/// Shows a player.
#[instrument(skip(state))]
pub async fn show(
    Path((short_id,)): Path<(String,)>,
    State(state): State<AppState>,
) -> Result<AppJson<Player>, AppError> {
    let mut conn = state.db.acquire().await?;

    get_player(&short_id, &mut conn)
        .await
        .and_then(|f| {
            f.ok_or_else(|| AppError::not_found(format!("Player {} not found", short_id)))
        })
        .map(|player| AppJson(player.into()))
}

/// Registers a joined player.
///
/// All players must be registered to create matches for them!
#[instrument(skip(state))]
pub async fn register(
    _auth_guard: ServerAuthentication,
    State(state): State<AppState>,
    Payload(request): Payload<RegisterPlayerRequest>,
) -> Result<(StatusCode, AppJson<Player>), AppError> {
    #[derive(FromRow)]
    struct UpsertQuery {
        #[sqlx(rename = "player_id")]
        id: i32,
        short_id: String,
        display_name: String,
        #[sqlx(flatten)]
        rating: CurrentPlayerRating,
    }

    let mut tx = state.db.begin().await?;

    let now = Utc::now();

    // find existing player
    let player_query = sqlx::query_as::<_, UpsertQuery>(
        r#"
        SELECT id AS player_id, short_id, display_name, rating, deviation, volatility
        FROM player
        WHERE public_key = $1
        "#,
    )
    .bind(request.public_key.as_str())
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(mut player) = player_query {
        // a player exists already, we just need to update them
        if player.display_name != request.display_name {
            sqlx::query(
                r#"
                UPDATE player
                SET display_name = $1, updated_at = $3
                WHERE short_id = $2
                "#,
            )
            .bind(&request.display_name)
            .bind(&player.short_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;

            player.display_name = request.display_name.clone();
        }

        tx.commit().await?;

        // return result
        Ok((
            StatusCode::CREATED,
            AppJson(Player {
                id: player.short_id,
                mmr: player.rating.ordinal() as i32,
                display_name: player.display_name,
                public_key: Some(request.public_key),
            }),
        ))
    } else {
        // this is a new player
        let mut inserted_player = None::<UpsertQuery>;

        for _ in 0..MAX_INSERT_ATTEMPTS {
            // generate a short id
            let short_id = rand::rng()
                .sample_iter(Alphanumeric)
                .take(6)
                .map(char::from)
                .map(|c| char::to_ascii_uppercase(&c))
                .collect::<String>();

            // try to insert with short_id
            let result = sqlx::query_as::<_, UpsertQuery>(
                r#"
                INSERT INTO player
                    (
                        short_id,
                        public_key,
                        display_name,
                        rating,
                        deviation,
                        volatility,
                        inserted_at,
                        updated_at
                    )
                VALUES ($1, $2, $3, $5, $6, $7, $4, $4)
                RETURNING id AS player_id, short_id, display_name, rating, deviation, volatility
                "#,
            )
            .bind(&short_id)
            .bind(request.public_key.as_str())
            .bind(&request.display_name)
            .bind(now)
            .bind(state.config.mmr.defaults.rating)
            .bind(state.config.mmr.defaults.deviation)
            .bind(state.config.mmr.defaults.volatility)
            .fetch_one(&mut *tx)
            .await;

            match result {
                Ok(player) => {
                    inserted_player = Some(player);
                    break;
                }
                Err(err) => {
                    if let Some(db_err) = err.as_database_error() {
                        // if this is a unique violation, simply try again
                        if db_err.is_unique_violation() {
                            tracing::debug!("unique key {} failed, regenerating", short_id);
                        } else {
                            return Err(err.into());
                        }
                    } else {
                        return Err(err.into());
                    }
                }
            }
        }

        if let Some(player) = inserted_player {
            // Add a historic rating for glicko2 to work
            init_rating(player.id, &state.config.mmr, &mut *tx).await?;

            tx.commit().await?;

            Ok((
                StatusCode::CREATED,
                AppJson(Player {
                    id: player.short_id,
                    mmr: player.rating.ordinal() as i32,
                    display_name: player.display_name,
                    public_key: Some(request.public_key),
                }),
            ))
        } else {
            Err(AppErrorKind::OutOfIds.into())
        }
    }
}
