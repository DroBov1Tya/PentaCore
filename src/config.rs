use std::sync::OnceLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub fn init_logger() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false),
        )
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
        let mut db_loc =
            std::env::var("DB_LOCATION").unwrap_or_else(|_| "sqlite:./db/mcp.db".to_string());
        if db_loc.starts_with("sqlite:./") {
            if let Ok(exe_path) = std::env::current_exe() {
                if let Some(parent) = exe_path.parent() {
                    let new_prefix = format!("sqlite:{}/", parent.to_string_lossy());
                    db_loc = db_loc.replace("sqlite:./", &new_prefix);
                }
            }
        }

        Self {
            db_location: db_loc,
            server_path: std::env::var("SERVER_PATH")
                .unwrap_or_else(|_| "localhost:8082".to_string()),
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
