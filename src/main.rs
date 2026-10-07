use anyhow::Result;

mod app;
mod config;

#[tokio::main]
async fn main() -> Result<()> {
    config::load_env_file();
    config::init_logger();

    let config = config::Config::from_env()?;
    app::run(config).await
}
