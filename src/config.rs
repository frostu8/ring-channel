//! Application configuration.

use std::path::Path;

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
    value::Uncased,
};
use ring_channel_model::user::to_username_lossy;
use serde::{Deserialize, Serialize};

use anyhow::Error;

/// Full application configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Config {
    /// General server configuration.
    pub server: ServerConfig,
    /// HTTP server configuration.
    pub http: HttpConfig,
    /// Discord configuration.
    pub discord: Option<DiscordConfig>,
}

/// General server configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ServerConfig {
    /// The base url of the API.
    pub base_url: String,
    /// Where to send the client after they are done authenticating with the
    /// API.
    pub redirect_url: Option<String>,
    /// The database url to connect to.
    pub database_url: Option<String>,
    /// Whether to send session cookies (used for auth) with `Secure`.
    ///
    /// By default, this is `true` to avoid misconfiguration.
    pub secure_sessions: bool,
    /// Key used to encrypt cookies.
    pub encryption_key: Option<String>,
    /// Wager bot config.
    pub bot: WagerBotConfig,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            base_url: "http://localhost:4000".into(),
            redirect_url: None,
            database_url: None,
            secure_sessions: true,
            encryption_key: None,
            bot: WagerBotConfig::default(),
        }
    }
}

/// Wager bot configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct WagerBotConfig {
    /// Enables the wager bot.
    pub enabled: bool,
    /// The username of the wager bot.
    ///
    /// This will identify the bot on the server. If this is changed to
    /// something else, the server will make a new bot account.
    pub username: String,
    /// The display name of the wager bot.
    pub display_name: String,
    /// A URL to the avatar of the wager bot.
    pub avatar: Option<String>,
    /// How much money the bot will wager on an empty side.
    pub wager_amount: i64,
}

impl Default for WagerBotConfig {
    fn default() -> Self {
        WagerBotConfig {
            enabled: false,
            username: to_username_lossy("xxmetalxx").into(),
            display_name: "Metal Sonic".into(),
            avatar: None,
            wager_amount: 400,
        }
    }
}

/// HTTP server configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HttpConfig {
    /// The port to listen on.
    pub port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        HttpConfig { port: 4000 }
    }
}

/// Discord OAuth2 configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DiscordConfig {
    /// The client ID.
    pub client_id: u64,
    /// The client secret.
    pub client_secret: String,
}

/// Reads the configuration.
pub fn read_config(config_file: impl AsRef<Path>) -> Result<Config, Error> {
    Figment::from(Serialized::defaults(Config::default()))
        .merge(Toml::file(config_file))
        .merge(Env::prefixed("DUELCHANNEL_"))
        .merge(Env::raw().filter_map(|k| match k.as_str() {
            "DATABASE_URL" => Some(Uncased::from("server.database_url")),
            "DISCORD_CLIENT_ID" => Some(Uncased::from("discord.client_id")),
            "DISCORD_CLIENT_SECRET" => Some(Uncased::from("discord.client_secret")),
            "ENCRYPTION_KEY" => Some(Uncased::from("server.encryption_key")),
            "PORT" => Some(Uncased::from("http.port")),
            _ => None,
        }))
        .extract()
        .map_err(From::from)
}
