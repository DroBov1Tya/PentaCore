use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteConnection, SqlitePoolOptions},
};
use std::path::Path;
use std::str::FromStr;

pub async fn init_pool(database_url: &str) -> Result<SqlitePool, sqlx::Error> {
    let db_path = database_url
        .strip_prefix("sqlite://")
        .or_else(|| database_url.strip_prefix("sqlite:"))
        .unwrap_or(database_url);

    if let Some(parent) = Path::new(db_path).parent() {
        if !parent.as_os_str().is_empty() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| sqlx::Error::Configuration(e.to_string().into()))?;
        }
    }

    let options = SqliteConnectOptions::from_str(database_url)
        .map_err(|e| sqlx::Error::Configuration(e.to_string().into()))?
        .create_if_missing(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .after_connect(|conn: &mut SqliteConnection, _| {
            Box::pin(async move {
                sqlx::query("PRAGMA journal_mode=WAL;")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("PRAGMA foreign_keys=ON;")
                    .execute(&mut *conn)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await?;

    tracing::info!("🔄 Running migrations...");

    let migrator = sqlx::migrate!("src/app/database/migrations");
    tracing::info!("📦 Migrations in binary: {}", migrator.migrations.len());

    migrator.run(&pool).await?;

    tracing::info!("✅ Migrations done");

    Ok(pool)
}
