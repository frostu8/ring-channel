use std::{env, io, net::SocketAddr, path::PathBuf, sync::Arc};

use http::{Method, header};

// :(
use time::Duration;

use clap::Parser;

use axum::{
    Router,
    extract::{MatchedPath, Request},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};

use axum_server::Handle;

use ring_channel::{
    app::{AppError, AppState},
    auth::oauth2::OauthState,
    cli::{Args, Command, register_server},
    config::read_config,
    routes, ws,
};

use anyhow::Error;

use sqlx::{Connection, SqliteConnection, pool::PoolOptions};

use tokio::{main, select, signal};

use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use tower_sessions::{Expiry, SessionManagerLayer, cookie::SameSite};
use tower_sessions_moka_store::MokaStore;

use tracing_subscriber::{
    filter::{EnvFilter, LevelFilter},
    fmt,
};

const OPENAPI_FILE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/openapi/openapi.yaml"));

#[main]
async fn main() -> Result<(), Error> {
    dotenv::dotenv().ok();
    fmt::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env_lossy(),
        )
        .with_writer(io::stderr)
        .init();

    let cli = Args::parse();

    let config_path = match cli.config {
        Some(path) => path,
        None => PathBuf::from("config_toml"),
    };

    // Read config file
    let config = Arc::new(read_config(config_path)?);

    let database_url = config
        .server
        .database_url
        .clone()
        .ok_or_else(|| Error::msg("No `DATABASE_URL` set!"))?;

    // Run any pending commands
    if let Some(command) = cli.command.as_ref() {
        match command {
            Command::RegisterServer(server) => {
                // establish connection
                let mut conn = SqliteConnection::connect(&database_url).await?;
                let mut tx = conn.begin().await?;

                tracing::info!("registering server {}", server.server_name);

                register_server(server, &mut tx).await?;

                tx.commit().await?;
                conn.close().await?;
            }
        }

        return Ok(());
    }

    tracing::info!("establishing connection to database");

    // Connect to sqlite database
    let db = PoolOptions::new().connect(&database_url).await?;

    // Create app state
    let state = AppState {
        db: db.clone(),
        room: Arc::new(ws::Room::new()),
    };

    // Create session management for oauth flows
    let session_store = MokaStore::new(Some(2_000));
    let session_layer = SessionManagerLayer::new(session_store)
        .with_name("sid")
        .with_expiry(Expiry::OnInactivity(Duration::minutes(30)))
        .with_same_site(SameSite::Lax)
        .with_secure(config.server.secure_sessions);

    // Build routes
    let mut router = Router::<AppState>::new()
        //.route("/ws", get(routes::ws::handler))
        .nest(
            "/players",
            Router::<AppState>::new()
                .route("/", post(routes::player::register))
                .route("/{player_id}", get(routes::player::show)),
        )
        .nest(
            "/matches",
            Router::<AppState>::new()
                .route("/", post(routes::battle::create))
                .nest(
                    "/{battle_id}",
                    Router::<AppState>::new()
                        .route("/", patch(routes::battle::update))
                        .route("/players/{short_id}", patch(routes::battle::player::update))
                        .route("/wagers", post(routes::battle::wager::create)),
                ),
        )
        // serve openapi spec
        .route("/openapi.yaml", get(serve_openapi))
        .with_state(state);

    if let Some(discord_config) = config.discord.as_ref() {
        let oauth_state = OauthState::new(&config.server.base_url, db.clone(), &discord_config)?
            .with_redirect_to(config.server.redirect_url.clone());

        router = router.nest(
            "/auth",
            Router::<OauthState>::new()
                .route("/redirect", get(routes::auth::redirect))
                .route("/token", get(routes::auth::token))
                .with_state(oauth_state)
                .layer(session_layer),
        );

        tracing::info!(
            client_id = { discord_config.client_id },
            "discord integration setup"
        );
    }

    // Finalize router
    let router = router
        .layer(
            CorsLayer::new()
                .allow_methods([Method::GET])
                .allow_origin(Any),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|req: &Request| {
                    let method = req.method();
                    let uri = req.uri();

                    // axum automatically adds this extension.
                    let matched_path = req
                        .extensions()
                        .get::<MatchedPath>()
                        .map(|matched_path| matched_path.as_str());

                    tracing::debug_span!("request", %method, %uri, matched_path)
                })
                // By default `TraceLayer` will log 5xx responses but we're doing our specific
                // logging of errors so disable that
                .on_failure(()),
        )
        .layer(from_fn(log_app_errors));

    let handle = Handle::new();

    // run shutdown task to detect shutdowns
    tokio::spawn(shutdown_signal(handle.clone()));

    let addr: SocketAddr = ([0, 0, 0, 0], config.http.port).into();

    tracing::info!("listening on {} (http)", addr);

    axum_server::bind(addr)
        .handle(handle)
        .serve(router.into_make_service())
        .await?;

    tracing::info!("shutting down");

    db.close().await;

    Ok(())
}

async fn serve_openapi() -> impl IntoResponse {
    (
        [(
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"openapi.yaml\"",
        )],
        OPENAPI_FILE,
    )
}

// Stolen from: https://github.com/tokio-rs/axum/blob/main/examples/error-handling/src/main.rs
async fn log_app_errors(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    // If the response contains an AppError Extension, log it.
    if let Some(err) = response.extensions().get::<Arc<AppError>>() {
        tracing::error!(?err, "an unexpected error occurred inside a handler");
    }
    response
}

// Stolen from: https://github.com/maxcountryman/tower-sessions-stores/tree/main/sqlx-store
// Lol
async fn shutdown_signal(handle: Handle) {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    select! {
        _ = ctrl_c => { handle.shutdown() }
        _ = terminate => { handle.shutdown() }
    }
}
