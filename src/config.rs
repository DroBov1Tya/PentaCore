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
    let user_home =
        || std::env::home_dir().context("cannot determine the home directory; set PENTACORE_HOME");
    match env("PENTACORE_HOME") {
        Some(home) => match under_user_home(&home) {
            Some(rest) => Ok(user_home()?.join(rest)),
            None => resolve_home(&home, exe_dir()),
        },
        None => Ok(user_home()?.join(".pentacore")),
    }
}

// A shell expands `~`; an env file or an MCP client config does not, and the
// data would land in a directory literally named `~`.
fn under_user_home(value: &str) -> Option<&str> {
    if value == "~" {
        return Some("");
    }
    value.strip_prefix("~/")
}

fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .ok()?;
    exe.parent().map(PathBuf::from)
}

// A relative path is taken from the executable's directory, never from the
// working directory, which the caller of the agent controls.
fn resolve_home(value: &str, exe_dir: Option<PathBuf>) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Ok(path);
    }
    let base =
        exe_dir.context("cannot locate the executable to resolve a relative PENTACORE_HOME")?;
    Ok(base.join(path))
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

