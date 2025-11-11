use std::{env, io, net::SocketAddr, path::PathBuf, sync::Arc};

use http::{HeaderValue, Method, header};

// :(
use time::Duration;

use clap::{CommandFactory as _, Parser};

use axum::{
    Router,
    extract::{MatchedPath, Request},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, patch, post, put},
};

use axum_server::Handle;

use ring_channel::{
    app::{AppError, AppState},
    auth::oauth2::OauthState,
    cli::{self, Args, Command, MmrCommand, MmrDump},
    config::read_config,
    player::mmr::{init_rating, next_rating_period},
    room, routes,
};

use anyhow::Error;

use sqlx::{Connection, SqliteConnection, pool::PoolOptions};

use tokio::{main, select, signal, sync::Semaphore};

use tokio_cron_scheduler::{Job, JobScheduler};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

use tower_sessions::{CachingSessionStore, Expiry, SessionManagerLayer, cookie::SameSite};
use tower_sessions_moka_store::MokaStore;
use tower_sessions_sqlx_store::SqliteStore;

use cookie::Key;

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
        None => PathBuf::from("config.toml"),
    };

    // Read config file
    let mut config = read_config(config_path)?;

    let database_url = config
        .server
        .database_url
        .take()
        .ok_or_else(|| Error::msg("No `DATABASE_URL` set!"))?;

    // Run any pending commands
    if let Some(command) = cli.command.as_ref() {
        match command {
            Command::RegisterServer(server) => {
                // establish connection
                let mut conn = SqliteConnection::connect(&database_url).await?;
                let mut tx = conn.begin().await?;

                tracing::info!("registering server {}", server.server_name);

                cli::register_server(server, &mut tx).await?;

                tx.commit().await?;
                conn.close().await?;
            }
            Command::GenerateKey(_) => {
                tracing::info!("generated! set ENCRYPTION_KEY or server.encryption_key on boot");

                let key = Key::generate();
                let key = base16::encode_lower(key.master());
                println!("{}", key);
            }
            Command::Mmr(cli::Mmr {
                command: Some(MmrCommand::Reset(_)),
            }) => {
                // establish connection
                let mut conn = SqliteConnection::connect(&database_url).await?;
                let mut tx = conn.begin().await?;

                tracing::info!("resetting all mmr...");

                // clear records
                sqlx::query("DELETE FROM rating").execute(&mut *tx).await?;
                sqlx::query("DELETE FROM rating_period")
                    .execute(&mut *tx)
                    .await?;

                // update all players ratings
                let player_ids = sqlx::query_as::<_, (i32,)>("SELECT id FROM player")
                    .fetch_all(&mut *tx)
                    .await?;

                for (id,) in player_ids {
                    // reset player's rating
                    sqlx::query(
                        r#"
                        UPDATE player
                        SET rating = $1, deviation = $2, volatility = $3, updated_at = $5
                        WHERE id = $4
                        "#,
                    )
                    .bind(config.mmr.defaults.rating)
                    .bind(config.mmr.defaults.deviation)
                    .bind(config.mmr.defaults.volatility)
                    .bind(id)
                    .bind(chrono::Utc::now())
                    .execute(&mut *tx)
                    .await?;

                    // init player rating
                    init_rating(id, &config.mmr, &mut *tx).await?;
                }

                tx.commit().await?;
            }
            Command::Mmr(cli::Mmr {
                command: Some(MmrCommand::Dump(MmrDump { exclude })),
            }) => {
                // establish connection
                let mut conn = SqliteConnection::connect(&database_url).await?;
                let mut tx = conn.begin().await?;

                // delete excluded participants
                for id in exclude {
                    sqlx::query(
                        r#"
                        DELETE FROM participant
                        WHERE player_id IN (
                            SELECT id
                            FROM player
                            WHERE short_id = $1
                        )
                        "#,
                    )
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
                }

                ring_channel::player::mmr::dump_rating(std::io::stdout(), &config.mmr, &mut *tx)
                    .await?;

                // rollback transaction
                tx.rollback().await?;
            }
            Command::Mmr(cli::Mmr { command: None }) => {
                Args::command().print_help().unwrap();
            }
        }

        return Ok(());
    }

    let encryption_key = if let Some(key_str) = config.server.encryption_key.take() {
        if key_str.len() > 128 {
            tracing::error!("encryption key too long! generate with `ring-channel generate-key`");
            std::process::exit(1);
        }

        let mut key = [0u8; 64];
        base16::decode_slice(&key_str[..], &mut key)?;

        match Key::try_from(&key[..]) {
            Ok(key) => key,
            Err(err) => {
                tracing::error!("bad encryption key: {}", err);
                std::process::exit(1);
            }
        }
    } else {
        tracing::warn!(
            "generating runtime encryption key! sessions will stop working after restart"
        );
        tracing::warn!("generate a permanent key with `ring-channel generate-key`");
        Key::generate()
    };

    tracing::info!("establishing connection to database");

    // Connect to sqlite database
    let db = PoolOptions::new().connect(&database_url).await?;

    // Create app state
    let state = AppState {
        config: Arc::new(config.clone()),
        db: db.clone(),
        room: room::Room::new(),
    };

    // Build routes
    let mut api_routes = Router::<AppState>::new()
        .route("/socket", get(routes::ws::handler))
        .nest(
            "/players",
            Router::<AppState>::new()
                .route("/", post(routes::player::register))
                .route("/{player_id}", get(routes::player::show)),
        )
        .nest(
            "/matches",
            Router::<AppState>::new()
                .route("/", get(routes::battle::list))
                .route("/", post(routes::battle::create))
                .nest(
                    "/{battle_id}",
                    Router::<AppState>::new()
                        .route("/", get(routes::battle::show))
                        .route("/", patch(routes::battle::update))
                        .route("/players/{short_id}", patch(routes::battle::player::update))
                        .route("/wagers", get(routes::battle::wager::list))
                        .route("/wagers/~me", get(routes::battle::wager::show_self))
                        .route("/wagers/~me", put(routes::battle::wager::create))
                        .route("/wagers/{username}", get(routes::battle::wager::show)),
                ),
        )
        .nest(
            "/chat",
            Router::<AppState>::new().route("/messages", post(routes::chat::create)),
        )
        .nest(
            "/users",
            Router::<AppState>::new().route("/~me", get(routes::user::show_me)),
        )
        .with_state(state.clone());

    if let Some(discord_config) = config.discord.as_ref() {
        let oauth_state = OauthState::new(&config.server.base_url, db.clone(), &discord_config)?
            .with_redirect_to(config.server.redirect_url.clone());

        let oauth_router = Router::<OauthState>::new()
            .route("/users/~redirect", get(routes::user::auth::redirect))
            .route("/users/~login", get(routes::user::auth::login))
            .with_state(oauth_state);

        api_routes = api_routes.merge(oauth_router);

        tracing::info!(
            client_id = { discord_config.client_id },
            "discord integration setup"
        );
    }

    // Create session management
    let db_session_store = SqliteStore::new(db.clone())
        .with_table_name("_session")
        .map_err(Error::msg)?;
    db_session_store.migrate().await?;

    let caching_session_store = MokaStore::new(Some(2_000));

    let session_store = CachingSessionStore::new(caching_session_store, db_session_store);
    let session_layer = SessionManagerLayer::new(session_store)
        .with_name("id")
        .with_expiry(Expiry::OnInactivity(Duration::days(30)))
        .with_http_only(true)
        .with_same_site(SameSite::Lax)
        .with_private(encryption_key)
        .with_secure(config.server.secure_sessions);

    // Finalize router
    let router = Router::new()
        .merge(api_routes.layer(from_fn(security_headers)))
        // serve openapi spec
        .merge(
            Router::new()
                .route("/openapi.yaml", get(serve_openapi))
                .layer(
                    CorsLayer::new()
                        .allow_methods([Method::GET])
                        .allow_origin(Any),
                ),
        )
        .layer(session_layer)
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

    // start cron jobs
    let sched = JobScheduler::new().await?;
    let state_clone = state.clone();

    // Start the rating period updater
    // This has to be locked so multiple threads aren't doing this together
    let semaphore = Arc::new(Semaphore::new(1));
    sched
        .add(Job::new_async("1/60 * * * * *", move |_uuid, _l| {
            let state = state_clone.clone();
            let semaphore = semaphore.clone();

            Box::pin(async move {
                if let Ok(_permit) = semaphore.try_acquire() {
                    let mut conn = state.db.acquire().await.expect("conn acquire");
                    let _period = next_rating_period(&state.config.mmr, &mut conn)
                        .await
                        .expect("update period");
                }
            })
        })?)
        .await?;

    sched.shutdown_on_ctrl_c();
    sched.start().await?;

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

async fn security_headers(request: Request, next: Next) -> Response {
    let mut res = next.run(request).await;

    res.headers_mut().extend([
        (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("frame-ancestors 'none'"),
        ),
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
        (header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
        // (
        //     header::STRICT_TRANSPORT_SECURITY,
        //     HeaderValue::try_from(format!("max-age={}", hsts_time)).expect("valid hsts time"),
        // ),
    ]);

    res
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
