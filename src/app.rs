use crate::config::{self, Config};
use anyhow::Result;
use colored::*;
use sqlx::{Pool, Sqlite};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{Mutex, broadcast};

mod client;
mod database;
pub mod rag;
mod server;

use rag::store::MemoryStore;
use server::mcp_server;

#[derive(Clone)]
pub struct AppState {
    pub cfg: &'static Config,
    pub db: Pool<Sqlite>,
    pub memory_store: Arc<Mutex<MemoryStore>>,
    pub shutdown: broadcast::Sender<()>,
}

pub struct Application {
    state: Arc<AppState>,
}

pub enum ShutdownMessage {
    Countdown { seconds_left: u32 },
    Complete,
}

impl Application {
    pub async fn build() -> Result<Self> {
        tracing::info!("🔧 Building application...");

        let _ = config::init();
        let cfg = config::cfg();
        let db = database::pool::init_pool(cfg.db_location.as_str()).await?;
        let (shutdown, _) = broadcast::channel(1);

        tracing::info!("🧠 Initializing embedded RAG memory (fastembed + lancedb)...");
        let lancedb_path = cfg.db_location.replace("mcp.db", ".lancedb");
        let memory_store = Arc::new(Mutex::new(
            MemoryStore::new(lancedb_path.replace("sqlite:", "").as_str()).await?,
        ));

        let state = Arc::new(AppState {
            cfg,
            db,
            memory_store,
            shutdown,
        });

        Ok(Self { state })
    }

    pub async fn run(self) -> Result<()> {
        tracing::info!("🚀 Starting application...");

        let state = Arc::clone(&self.state);
        let server_state = Arc::clone(&self.state);
        let mcp_state = Arc::clone(&self.state);

        let server_handle = tokio::spawn(async move { server::server::start(server_state).await });
        let mcp_handle = tokio::spawn(async move { mcp_server::start(mcp_state).await });

        tracing::info!("✅ Application ready");

        tokio::select! {
            result = server_handle => {
                match result {
                    Ok(Ok(())) => tracing::info!("✅ Server stopped gracefully"),
                    Ok(Err(e)) => return Err(e),
                    Err(e) => return Err(e.into()),
                }
            }
            result = mcp_handle => {
                match result {
                    Ok(Ok(())) => tracing::info!("✅ MCP stdio stopped gracefully"),
                    Ok(Err(e)) => return Err(e),
                    Err(e) => return Err(e.into()),
                }
            }
            _ = signal::ctrl_c() => {
                tracing::info!("⚠️  Received Ctrl+C, shutting down...");
            }
        }

        let _ = state.shutdown.send(());

        Ok(())
    }
}
