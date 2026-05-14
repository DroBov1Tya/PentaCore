use anyhow::{Ok, Result};

mod api;
mod app;
mod config;
mod constants;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    config::init_logger();

    let app = app::Application::build().await?;

    app.run().await?;

    Ok(())
}
