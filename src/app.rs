use crate::config::{self, Config};
use anyhow::Result;
use colored::*;
use sqlx::{Pool, Sqlite};
use std::io::{self, Write};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::broadcast;

mod client;
pub mod database;
mod server;

use server::mcp_server;

#[derive(Clone)]
pub struct AppState {
    pub cfg: &'static Config,
    pub db: Pool<Sqlite>,
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

        let state = Arc::new(AppState { cfg, db, shutdown });

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

        for remaining in (1..=5).rev() {
            // print_shutdown_status(ShutdownMessage::Countdown {
            //     seconds_left: remaining,
            // });
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        // print_shutdown_status(ShutdownMessage::Complete);

        let _ = state.shutdown.send(());

        Ok(())
    }
}

pub fn print_shutdown_status(message: ShutdownMessage) {
    match message {
        ShutdownMessage::Countdown { seconds_left } => {
            let status = format!(
                "\r{} {}",
                "⏳ [Shutting down]".bold().cyan(),
                format!("{}s", seconds_left).bold().yellow()
            );
            eprint!("{:<80}", status);
            io::stderr().flush().unwrap();
        }
        ShutdownMessage::Complete => {
            eprintln!("\n{}", "☑️ Shutdown complete".bold().green());
        }
    }
}
