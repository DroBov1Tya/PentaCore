use crate::config::{self, Config};
use anyhow::Result;
use sqlx::{Pool, Sqlite};
use std::sync::Arc;
use tokio::signal;
use tokio::sync::{Mutex, broadcast};

mod client;
mod database;
pub mod rag;
mod server;

use rag::knowledge_seed;
use rag::store::MemoryStore;
use server::mcp_server;
use server::mcp_server::tool_registry::ToolRegistry;

#[derive(Clone)]
pub struct AppState {
    pub cfg: &'static Config,
    pub db: Pool<Sqlite>,
    pub memory_store: Arc<Mutex<MemoryStore>>,
    pub registry: Arc<ToolRegistry>,
    pub shutdown: broadcast::Sender<()>,
}

pub struct Application {
    state: Arc<AppState>,
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
        let mut store = MemoryStore::new(lancedb_path.replace("sqlite:", "").as_str()).await?;

        let binary_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        if let Err(e) = knowledge_seed::seed(&mut store, &db, &binary_dir).await {
            tracing::warn!("⚠️  Base knowledge seed failed (non-fatal): {}", e);
        }

        let memory_store = Arc::new(Mutex::new(store));

        let state = Arc::new(AppState {
            cfg,
            db,
            memory_store,
            registry: Arc::new(ToolRegistry::build()),
            shutdown,
        });

        Ok(Self { state })
    }

    pub async fn run(self) -> Result<()> {
        tracing::info!("🚀 Starting application...");

        let state = Arc::clone(&self.state);
        let server_state = Arc::clone(&self.state);
        let mcp_state = Arc::clone(&self.state);

        tokio::spawn(async move {
            match server::server::start(server_state).await {
                Ok(()) => tracing::info!("✅ REST API server stopped"),
                Err(e) => tracing::warn!("⚠️ REST API server error (MCP stdio unaffected): {}", e),
            }
        });

        tracing::info!("✅ Application ready");

        tokio::select! {
            result = mcp_server::start(mcp_state) => {
                match result {
                    Ok(()) => tracing::info!("✅ MCP stdio stopped gracefully"),
                    Err(e) => tracing::error!("❌ MCP stdio error: {}", e),
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
