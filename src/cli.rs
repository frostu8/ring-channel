//! Ring Channel server command-line interface.

use std::path::PathBuf;

use chrono::Utc;

use clap::{Parser, Subcommand};

use anyhow::Error;
use sqlx::SqliteConnection;

use crate::auth::api_key::{generate_api_key, hash_api_key};

/// The command line arguments.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Configuration file path.
    #[arg(short, long)]
    pub config: Option<PathBuf>,
    /// The command to run.
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// Operational commands.
#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(name = "register")]
    RegisterServer(RegisterServer),
}

/// Registers a server with the ring channel API.
#[derive(clap::Args, Debug)]
pub struct RegisterServer {
    /// The name of the server to register.
    pub server_name: String,
}

/// Registers a server.
pub async fn register_server(
    command: &RegisterServer,
    conn: &mut SqliteConnection,
) -> Result<(), Error> {
    // generate api token
    let api_key = generate_api_key();
    let hash = hash_api_key(&api_key);

    let now = Utc::now();

    // insert new server
    sqlx::query(
        r#"
        INSERT INTO server (server_name, key_hash, inserted_at, updated_at)
        VALUES ($1, $2, $3, $3)
        "#,
    )
    .bind(&command.server_name)
    .bind(hash)
    .bind(now)
    .execute(&mut *conn)
    .await?;

    // export key
    println!("{}", api_key);

    Ok(())
}
