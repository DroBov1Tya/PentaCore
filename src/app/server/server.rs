use anyhow::Result;
use axum_listener::DualListener;
use std::path::Path;
use std::sync::Arc;

use super::routes;
use crate::app::AppState;

pub async fn start(state: Arc<AppState>) -> Result<()> {
    let server_path = state.cfg.server_path.clone();
    let mut shutdown_rx = state.shutdown.subscribe();

    if Path::new(&server_path).exists() {
        tokio::fs::remove_file(&server_path).await?;
        tracing::info!("🧹 Removed old socket");
    }

    let listener = match DualListener::bind(format!("{}", &server_path)).await {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(
                "⚠️ Could not bind API listener on {}: {}. REST API will not be available, but MCP stdio will continue.",
                &server_path,
                e
            );
            return Ok(());
        }
    };
    tracing::info!("🚀 API listening on {}", &server_path);

    let app = routes::build_router(state);

    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            let _ = shutdown_rx.recv().await;
        })
        .await?;

    Ok(())
}
