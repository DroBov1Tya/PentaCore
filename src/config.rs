use std::sync::OnceLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logger() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
}

#[derive(Debug)]
pub struct Config {
    // pub bot_token: String,
    pub db_location: String,
    pub server_path: String,
}

static CONFIG: OnceLock<Config> = OnceLock::new();

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("Missing env var: {}", key))
}

pub fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    fn load() -> Self {
        Self {
            // bot_token: env("BOT_TOKEN"),
            db_location: env("DB_LOCATION"),
            server_path: env("SERVER_PATH"),
        }
    }
}

pub fn init() {
    CONFIG
        .set(Config::load())
        .expect("Config already initialized");
}

pub fn cfg() -> &'static Config {
    CONFIG
        .get()
        .expect("Config not initialized — call config::init() first")
}
