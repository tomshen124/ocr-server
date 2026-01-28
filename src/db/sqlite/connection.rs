//! SQLite连接管理
//! 处理数据库连接池的创建和管理

use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use std::path::Path;
use tracing::info;

/// SQLite连接管理器
pub struct ConnectionManager;

impl ConnectionManager {
    /// 创建新的SQLite连接池
    pub async fn create_pool(db_path: &str) -> Result<SqlitePool> {
        // 确保数据库目录存在
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 如果数据库文件不存在，创建空文件
        // 这确保SQLite连接池能够正常初始化
        if !Path::new(db_path).exists() {
            std::fs::File::create(db_path)?;
            info!("Created SQLite database file: {}", db_path);
        }

        // 创建连接池
        let connection_string = format!("sqlite:{}", db_path);
        let pool = SqlitePool::connect(&connection_string).await?;

        info!("Successfully connected to SQLite database: {}", db_path);
        Ok(pool)
    }

    /// 验证连接池状态
    pub async fn validate_pool(pool: &SqlitePool) -> Result<()> {
        sqlx::query("SELECT 1").fetch_one(pool).await?;
        Ok(())
    }

    /// 获取连接池配置信息
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

/// 连接池信息
#[derive(Debug)]
pub struct PoolInfo {
    pub max_connections: u32,
    pub idle_connections: usize,
    pub active_connections: usize,
}
