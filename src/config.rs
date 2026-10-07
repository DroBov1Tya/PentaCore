use anyhow::{Context, Result, bail};
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;
use subtle::ConstantTimeEq;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::app::memory::model::ProjectName;

const MIN_TOKEN_BYTES: usize = 32;

// Dependencies are noisy at info, so only this crate gets it.
const DEFAULT_LOG_FILTER: &str = "warn,pentacore=info";

pub fn init_logger() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false)
                .with_ansi(false),
        )
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| DEFAULT_LOG_FILTER.into()),
        )
        .init();
}

// Only the .env beside the executable is read.
// The working directory may be an untrusted checkout.
pub fn load_env_file() {
    let Some(path) = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(".env")))
    else {
        return;
    };
    match dotenvy::from_path(&path) {
        Ok(()) => {}
        Err(dotenvy::Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!("pentacore: ignoring {}: {error}", path.display()),
    }
}

#[derive(Debug)]
pub struct Config {
    pub home: PathBuf,
    pub default_project: ProjectName,
    pub http: Option<HttpConfig>,
}

#[derive(Debug, Clone)]
pub struct HttpConfig {
    pub addr: SocketAddr,
    pub token: Token,
}

#[derive(Clone)]
pub struct Token(String);

impl Token {
    pub fn matches(&self, candidate: &[u8]) -> bool {
        self.0.as_bytes().ct_eq(candidate).into()
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(<redacted>)")
    }
}

impl TryFrom<String> for Token {
    type Error = anyhow::Error;

    fn try_from(value: String) -> Result<Self> {
        if value.len() < MIN_TOKEN_BYTES {
            bail!("PENTACORE_HTTP_TOKEN must be at least {MIN_TOKEN_BYTES} characters");
        }
        Ok(Self(value))
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            home: home_dir()?,
            default_project: default_project()?,
            http: http_config(env("PENTACORE_HTTP_ADDR"), env("PENTACORE_HTTP_TOKEN"))?,
        })
    }
}

fn home_dir() -> Result<PathBuf> {
    if let Some(home) = env("PENTACORE_HOME") {
        return Ok(PathBuf::from(home));
    }
    let user_home =
        std::env::home_dir().context("cannot determine the home directory; set PENTACORE_HOME")?;
    Ok(user_home.join(".pentacore"))
}

fn default_project() -> Result<ProjectName> {
    if let Some(name) = env("PENTACORE_PROJECT") {
        return ProjectName::try_from(name).map_err(|e| anyhow::anyhow!("PENTACORE_PROJECT: {e}"));
    }
    let from_working_dir = std::env::current_dir()
        .ok()
        .and_then(|dir| {
            dir.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .and_then(|name| ProjectName::sanitized(&name));
    match from_working_dir {
        Some(project) => Ok(project),
        None => ProjectName::try_from("default".to_string()).map_err(anyhow::Error::msg),
    }
}

// HTTP needs an address and a token, and the address must be loopback.
fn http_config(addr: Option<String>, token: Option<String>) -> Result<Option<HttpConfig>> {
    let Some(addr) = addr else {
        return Ok(None);
    };
    let addr: SocketAddr = addr
        .parse()
        .context("PENTACORE_HTTP_ADDR must be an IP address and port, e.g. 127.0.0.1:8082")?;
    if !addr.ip().is_loopback() {
        bail!("PENTACORE_HTTP_ADDR must be a loopback address, got {addr}");
    }
    let token = token.context("PENTACORE_HTTP_ADDR is set, so PENTACORE_HTTP_TOKEN is required")?;
    Ok(Some(HttpConfig {
        addr,
        token: Token::try_from(token)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token() -> Option<String> {
        Some("t".repeat(MIN_TOKEN_BYTES))
    }

    #[test]
    fn http_is_off_without_an_address() {
        assert!(http_config(None, None).unwrap().is_none());
        assert!(http_config(None, token()).unwrap().is_none());
    }

    #[test]
    fn http_requires_loopback_and_a_strong_token() {
        assert!(
            http_config(Some("127.0.0.1:8082".into()), token())
                .unwrap()
                .is_some()
        );
        assert!(
            http_config(Some("[::1]:8082".into()), token())
                .unwrap()
                .is_some()
        );
        assert!(http_config(Some("0.0.0.0:8082".into()), token()).is_err());
        assert!(http_config(Some("192.168.1.5:8082".into()), token()).is_err());
        assert!(http_config(Some("localhost:8082".into()), token()).is_err());
        assert!(http_config(Some("127.0.0.1:8082".into()), None).is_err());
        assert!(http_config(Some("127.0.0.1:8082".into()), Some("short".into())).is_err());
    }

    #[test]
    fn token_compares_exactly_and_hides_itself() {
        let token = Token::try_from("s".repeat(MIN_TOKEN_BYTES)).unwrap();
        assert!(token.matches("s".repeat(MIN_TOKEN_BYTES).as_bytes()));
        assert!(!token.matches("s".repeat(MIN_TOKEN_BYTES - 1).as_bytes()));
        assert!(!token.matches(b""));
        assert!(!format!("{token:?}").contains('s'));
    }
}
