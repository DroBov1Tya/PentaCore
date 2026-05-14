use anyhow::{Ok, Result};

mod api;
mod app;
mod args;
mod config;

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    config::init_logger();

    // let arg = args::parse_args();
    // let app = app::Application::build(arg).await?;
    let app = app::Application::build().await?;

    app.run().await?;

    Ok(())
}
