//! 데이터베이스 연결 관리 모듈.

use std::time::Duration;

use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use tokio::sync::OnceCell;
use tracing::info;

use super::APP_CONFIG;

/// Global database pool instance.
static DB_POOL: OnceCell<SqlitePool> = OnceCell::const_new();

/// Initializes the database connection pool.
///
/// This function is idempotent - calling it multiple times will return
/// the same pool instance.
///
/// # Errors
///
/// Returns an error if the database connection fails.
pub async fn init_db() -> Result<SqlitePool, sqlx::Error> {
    let pool = DB_POOL
        .get_or_try_init(|| async {
            let pool = SqlitePoolOptions::new()
                .max_connections(APP_CONFIG.db_max_connections)
                .min_connections(APP_CONFIG.db_min_connections)
                .acquire_timeout(Duration::from_secs(APP_CONFIG.db_acquire_timeout_secs))
                .idle_timeout(Duration::from_secs(APP_CONFIG.db_idle_timeout_secs))
                .connect("sqlite://sqlite3.db?mode=rwc")
                .await?;

            run_migrations(&pool).await?;
            apply_sqlite_optimizations(&pool).await?;

            info!(
                max_connections = APP_CONFIG.db_max_connections,
                min_connections = APP_CONFIG.db_min_connections,
                "Database pool initialized"
            );

            Ok::<_, sqlx::Error>(pool)
        })
        .await?;

    Ok(pool.clone())
}

/// Runs database migrations.
async fn run_migrations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| sqlx::Error::Configuration(e.into()))?;
    info!("Database migrations applied");
    Ok(())
}

/// Applies SQLite-specific optimizations.
async fn apply_sqlite_optimizations(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    // Journal and sync settings
    sqlx::query("PRAGMA journal_mode=WAL").execute(pool).await?;
    sqlx::query("PRAGMA synchronous=NORMAL")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA busy_timeout=5000")
        .execute(pool)
        .await?;

    // Memory and cache settings
    sqlx::query("PRAGMA mmap_size=268435456")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA cache_size=-64000")
        .execute(pool)
        .await?;
    sqlx::query("PRAGMA temp_store=MEMORY")
        .execute(pool)
        .await?;

    // Storage optimization
    sqlx::query("PRAGMA page_size=4096").execute(pool).await?;
    sqlx::query("PRAGMA auto_vacuum=INCREMENTAL")
        .execute(pool)
        .await?;

    // Integrity
    sqlx::query("PRAGMA foreign_keys=ON").execute(pool).await?;

    info!("SQLite optimized: WAL, mmap=256MB, cache=64MB, temp=MEMORY");
    Ok(())
}

/// Closes the database connection pool gracefully.
pub async fn close_db() {
    if let Some(pool) = DB_POOL.get() {
        pool.close().await;
        info!("Database pool closed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_pool_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SqlitePool>();
    }
}
