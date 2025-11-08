//! Application configuration.

use std::path::Path;

use chrono::TimeDelta;

use figment::{
    Figment,
    providers::{Env, Format, Serialized, Toml},
    value::Uncased,
};

use humantime::format_duration;
use ring_channel_model::user::to_username_lossy;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use anyhow::Error;

/// Full application configuration.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Config {
    /// General server configuration.
    pub server: ServerConfig,
    /// Mmr config.
    pub mmr: MmrConfig,
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

/// Configuration for MMR.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MmrConfig {
    /// Enables MMR.
    pub enabled: bool,
    /// The rating period.
    ///
    /// This should be set to a reasonable value for a single player to get at
    /// least 10 matches in, but it shouldn't be too high.
    #[serde(
        deserialize_with = "deserialize_duration",
        serialize_with = "serialize_duration"
    )]
    pub period: TimeDelta,
    /// Constrains the change in volatility over time.
    ///
    /// Higher values may make skill volatility change more frequently, and
    /// lower values make it stay around the same.
    ///
    /// See the [Glicko-2] paper for more.
    ///
    /// [Glicko-2]: https://www.glicko.net/glicko/glicko2.pdf
    pub tau: f32,
    /// Default settings for new players.
    pub defaults: PlayerRatingDefaults,
}

impl Default for MmrConfig {
    fn default() -> Self {
        MmrConfig {
            enabled: true,
            period: TimeDelta::seconds(86_400),
            tau: 0.5,
            defaults: PlayerRatingDefaults::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PlayerRatingDefaults {
    /// The rating new players start at.
    pub rating: f32,
    pub deviation: f32,
    pub volatility: f32,
}

impl Default for PlayerRatingDefaults {
    fn default() -> Self {
        PlayerRatingDefaults {
            rating: 1500.0,
            deviation: 350.0,
            volatility: 0.06,
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

fn deserialize_duration<'de, D>(deserializer: D) -> Result<TimeDelta, D::Error>
where
    D: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    let duration = humantime::parse_duration(&text).map_err(D::Error::custom)?;

    TimeDelta::from_std(duration).map_err(D::Error::custom)
}

fn serialize_duration<S>(delta: &TimeDelta, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    format_duration(delta.to_std().expect("positive time delta"))
        .to_string()
        .serialize(serializer)
}
