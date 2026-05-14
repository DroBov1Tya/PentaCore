use anyhow::Result;
use std::sync::LazyLock;
use tokio::sync::RwLock;

#[derive(Default, Clone)]
pub struct Session {
    pub cookie: Vec<String>,
    pub auth_token: Option<String>,
}

pub static SESSION: LazyLock<RwLock<Session>> = LazyLock::new(|| RwLock::new(Session::default()));

pub async fn save_session(cookie: Vec<String>, auth_token: Option<String>) -> Result<()> {
    let mut session = SESSION.write().await;
    session.cookie = cookie;
    session.auth_token = auth_token;
    Ok(())
}

pub async fn load_session() -> Result<Session> {
    let session = SESSION.read().await;
    Ok(session.clone())
}

pub async fn remove_session() -> Result<()> {
    let mut session = SESSION.write().await;
    session.cookie.clear();
    session.auth_token = None;
    Ok(())
}
