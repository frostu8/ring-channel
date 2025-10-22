//! Active player routes.

use axum::extract::State;

use chrono::Utc;

use http::StatusCode;

use rand::{Rng, distr::Alphanumeric};

use ring_channel_model::{Player, request::player::RegisterPlayerRequest};

use sqlx::FromRow;

use tracing::instrument;

use crate::{
    app::{AppError, AppJson, AppState, Payload, error::AppErrorKind},
    auth::api_key::ServerAuthentication,
};

pub const MAX_INSERT_ATTEMPTS: usize = 25;

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
        /// The "short ID" of the player.
        ///
        /// This is used on the frontend to uniquely identify a player in lieu of
        /// their public key.
        pub short_id: String,
        /// The display_name of the player.
        pub display_name: String,
    }

    let mut tx = state.db.begin().await?;

    let now = Utc::now();

    // find existing player
    let player_query = sqlx::query_as::<_, UpsertQuery>(
        r#"
        SELECT short_id, display_name
        FROM player
        WHERE public_key = $1
        "#,
    )
    .bind(request.public_key.as_str())
    .fetch_optional(&mut *tx)
    .await?;

    if let Some(player) = player_query {
        // a player exists already, we just need to update them
        if player.display_name != player.display_name {
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
        }

        tx.commit().await?;

        // return result
        Ok((
            StatusCode::CREATED,
            AppJson(Player {
                id: player.short_id,
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
                INSERT INTO player (short_id, public_key, display_name, inserted_at, updated_at)
                VALUES ($1, $2, $3, $4, $4)
                RETURNING short_id, display_name
                "#,
            )
            .bind(&short_id)
            .bind(request.public_key.as_str())
            .bind(&request.display_name)
            .bind(now)
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

        tx.commit().await?;

        if let Some(player) = inserted_player {
            Ok((
                StatusCode::CREATED,
                AppJson(Player {
                    id: player.short_id,
                    display_name: player.display_name,
                    public_key: Some(request.public_key),
                }),
            ))
        } else {
            Err(AppErrorKind::OutOfIds.into())
        }
    }
}
