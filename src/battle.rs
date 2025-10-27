//! Battle functions and utilities.

use chrono::{DateTime, Utc};
use ring_channel_model::{
    Battle,
    battle::{BattleStatus, PlayerTeam},
    message::server::MobiumsChange,
};

use sqlx::{FromRow, SqliteConnection};

use crate::{app::AppError, room::Room};

/// A schema for battles stored in database.
///
/// Used primarily to construct [`Battle`]s.
#[derive(Clone, Debug, FromRow)]
pub struct BattleSchema {
    pub uuid: String,
    pub level_name: String,
    #[sqlx(try_from = "u8")]
    pub status: BattleStatus,
    pub inserted_at: DateTime<Utc>,
    pub closed_at: DateTime<Utc>,
}

impl From<BattleSchema> for Battle {
    fn from(value: BattleSchema) -> Self {
        (&value).into()
    }
}

impl From<&BattleSchema> for Battle {
    fn from(value: &BattleSchema) -> Self {
        let now = Utc::now();
        let accepting_bets = now < value.closed_at;

        Battle {
            id: value.uuid.clone(),
            level_name: value.level_name.clone(),
            participants: vec![],
            status: value.status,
            started_at: value.inserted_at,
            accepting_bets,
            closes_in: if accepting_bets {
                Some((value.closed_at - now).abs().num_milliseconds())
            } else {
                None
            },
        }
    }
}

/// Closes a match, divying up the pots in each.
pub async fn calculate_winnings(
    battle_id: i32,
    room: &Room,
    tx: &mut SqliteConnection,
) -> Result<(), AppError> {
    #[derive(FromRow)]
    struct ParticipantQuery {
        #[sqlx(try_from = "u8")]
        team: PlayerTeam,
    }

    #[derive(FromRow)]
    struct WagerQuery {
        user_id: i32,
        #[sqlx(try_from = "u8")]
        victor: PlayerTeam,
        mobiums: i64,
        user_mobiums: i64,
    }

    // To figure out how much money we owe to each player, we first need to
    // figure out the total sum of each pot alone

    let red_pot = get_total_pot(battle_id, PlayerTeam::Red, &mut *tx).await?;
    let blue_pot = get_total_pot(battle_id, PlayerTeam::Blue, &mut *tx).await?;

    // If a pot has 0 mobiums to its name, nullify the wagers
    if red_pot <= 0 || blue_pot <= 0 {
        return Ok(());
    }

    let total_winnings = red_pot + blue_pot;

    // We need to figure out who won first
    let winner = sqlx::query_as::<_, ParticipantQuery>(
        r#"
        SELECT team
        FROM participant
        WHERE
            match_id = $1
            AND NOT no_contest
        ORDER BY finish_time ASC
        LIMIT 1
        "#,
    )
    .bind(battle_id)
    .fetch_optional(&mut *tx)
    .await?;

    // Do not divy pot up if there are no winners
    let Some(winner) = winner else {
        return Ok(());
    };

    // Go over all wagers to see what players are entitled to what
    let wagers = sqlx::query_as::<_, WagerQuery>(
        r#"
        SELECT
            w.user_id, w.victor, w.mobiums,
            u.mobiums AS user_mobiums
        FROM
            wager w, user u
        WHERE
            w.user_id = u.id
            AND match_id = $1
        "#,
    )
    .bind(battle_id)
    .fetch_all(&mut *tx)
    .await?;

    for wager in wagers {
        // Did this user win or lose money?
        let mobiums_change = if wager.victor == winner.team {
            // They won! Give them some of the winnings
            let pot = if wager.victor == PlayerTeam::Red {
                red_pot
            } else {
                blue_pot
            };
            let pie_slice = total_winnings * wager.mobiums / pot;
            // Do not re-award them the money they put on the bet
            pie_slice - wager.mobiums
        } else {
            // They lost... STEAL their money.
            -wager.mobiums
        };

        let mut new_mobiums = wager.user_mobiums + mobiums_change;

        // GG bro...
        let mut bailout = false;
        if new_mobiums <= 0 {
            bailout = true;
            new_mobiums = 100; // TODO: magic number?
        }

        // Update database record
        sqlx::query(
            r#"
            UPDATE user
            SET
                mobiums = $1,
                bailout_count = bailout_count + $2
            WHERE
                id = $3
            "#,
        )
        .bind(new_mobiums)
        .bind(if bailout { 1 } else { 0 })
        .bind(wager.user_id)
        .execute(&mut *tx)
        .await?;

        // Send mobiums change to player
        room.send_mobiums_change(
            wager.user_id,
            MobiumsChange {
                mobiums: new_mobiums,
                bailout,
            },
        );
    }

    // All the dirty work has been done
    Ok(())
}

async fn get_total_pot(
    battle_id: i32,
    team: PlayerTeam,
    conn: &mut SqliteConnection,
) -> Result<i64, AppError> {
    sqlx::query_as::<_, (i64,)>(
        r#"
        SELECT SUM(w.mobiums)
        FROM wager w
        WHERE
            match_id = $1
            AND w.victor = $2
        "#,
    )
    .bind(battle_id)
    .bind(u8::from(team))
    .fetch_one(&mut *conn)
    .await
    .map(|(mobiums,)| mobiums)
    .map_err(AppError::from)
}
