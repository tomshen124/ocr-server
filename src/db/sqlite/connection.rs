
use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use std::path::Path;
use tracing::info;

pub struct ConnectionManager;

impl ConnectionManager {
    pub async fn create_pool(db_path: &str) -> Result<SqlitePool> {
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        if !Path::new(db_path).exists() {
            std::fs::File::create(db_path)?;
            info!("Created SQLite database file: {}", db_path);
        }

        let connection_string = format!("sqlite:{}", db_path);
        let pool = SqlitePool::connect(&connection_string).await?;

        info!("Successfully connected to SQLite database: {}", db_path);
        Ok(pool)
    }

    pub async fn validate_pool(pool: &SqlitePool) -> Result<()> {
        sqlx::query("SELECT 1").fetch_one(pool).await?;
        Ok(())
    }

    pub fn get_pool_info(pool: &SqlitePool) -> PoolInfo {
        let max_connections = pool.size();
        let idle_connections = pool.num_idle();
        let active_connections = if max_connections as usize >= idle_connections {
            max_connections as usize - idle_connections
        } else {
            0
        };

        PoolInfo {
            max_connections,
            idle_connections,
            active_connections,
        }
    }
}

#[derive(Debug)]
pub struct PoolInfo {
    pub max_connections: u32,
    pub idle_connections: usize,
    pub active_connections: usize,
}
