use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;

use crate::config::Config;

pub mod memory;
mod server;
mod tools;

use memory::Brain;

const HTTP_SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

pub async fn run(config: Config) -> Result<()> {
    tracing::info!(home = %config.home.display(), project = config.default_project.as_str(), "starting pentacore");
    let brain = Arc::new(Brain::open(
        &config.home,
        config.default_project,
        config.all_tools,
    )?);

    let (stop_http, http_stopped) = oneshot::channel::<()>();
    let http = config.http.map(|http_config| {
        let brain = Arc::clone(&brain);
        tokio::spawn(server::http::serve(brain, http_config, async {
            let _ = http_stopped.await;
        }))
    });

    tokio::select! {
        result = server::mcp::serve_stdio(Arc::clone(&brain)) => match result {
            Ok(()) => tracing::info!("MCP client disconnected"),
            Err(error) => tracing::error!("MCP stdio failed: {error:#}"),
        },
        _ = tokio::signal::ctrl_c() => tracing::info!("received Ctrl+C"),
    }

    drop(stop_http);
    if let Some(mut http) = http {
        // Open connections must not keep the process alive after the agent is gone.
        match tokio::time::timeout(HTTP_SHUTDOWN_GRACE, &mut http).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => tracing::warn!("HTTP API failed: {error:#}"),
            Ok(Err(error)) => tracing::warn!("HTTP API task panicked: {error}"),
            Err(_) => http.abort(),
        }
    }
    Ok(())
}
