//! Match management routes.

pub mod placement;
pub mod wager;

use axum::extract::State;

use chrono::{TimeDelta, Utc};

use ring_channel_model::{
    Player,
    battle::{Battle, Participant},
    message::server::NewBattle,
    request::battle::CreateBattleRequest,
};

use http::StatusCode;

use sqlx::FromRow;

use tracing::instrument;

use uuid::Uuid;

use crate::app::{AppError, AppJson, AppState, Payload};

/// Creates a match.
#[instrument(skip(state))]
pub async fn create(
    State(state): State<AppState>,
    Payload(request): Payload<CreateBattleRequest>,
) -> Result<(StatusCode, AppJson<Battle>), AppError> {
    #[derive(FromRow)]
    struct PlayerQuery {
        id: i32,
        short_id: String,
        display_name: String,
    }

    let mut tx = state.db.begin().await?;

    // Create the battle
    let uuid = Uuid::new_v4().hyphenated().to_string();
    let now = Utc::now();

    let closed_at = now + TimeDelta::seconds(request.bet_time.unwrap_or(20));

    let (match_id,) = sqlx::query_as::<_, (i32,)>(
        r#"
        INSERT INTO battle (uuid, level_name, closed_at, inserted_at)
        VALUES ($1, $2, $4, $3)
        RETURNING id
        "#,
    )
    .bind(&uuid)
    .bind(&request.level_name)
    .bind(now)
    .bind(closed_at)
    .fetch_one(&mut *tx)
    .await?;

    // register players
    let mut participants = Vec::with_capacity(request.participants.len());
    for input_player in request.participants.iter() {
        // find player
        let player = sqlx::query_as::<_, PlayerQuery>(
            r#"
            SELECT id, short_id, display_name
            FROM player
            WHERE short_id = $1
            "#,
        )
        .bind(&input_player.id)
        .fetch_optional(&mut *tx)
        .await?;

        if let Some(player) = player {
            // add player to match
            sqlx::query(
                r#"
                INSERT INTO participant (match_id, player_id, team, no_contest)
                VALUES ($1, $2, $3, FALSE)
                "#,
            )
            .bind(match_id)
            .bind(player.id)
            .bind(u8::from(input_player.team))
            .execute(&mut *tx)
            .await?;

            // insert players to vec
            participants.push(Participant {
                player: Player {
                    id: player.short_id,
                    public_key: None,
                    display_name: player.display_name,
                },
                team: input_player.team,
                finish_time: None,
                no_contest: false,
            })
        } else {
            tx.rollback().await?;
            return Err(AppError::not_found(format!(
                "Participant w/ ID {} not found",
                input_player.id
            )));
        }
    }

    tx.commit().await?;

    // Create battle model
    let battle = Battle {
        id: uuid,
        level_name: request.level_name,
        participants,
        accepting_bets: true,
        victor: None,
        closed_at: Some(closed_at),
    };

    // Send the notice of the new battle to all connected clients
    state.room.broadcast(NewBattle(battle.clone()).into());

    Ok((StatusCode::CREATED, AppJson(battle)))
}
