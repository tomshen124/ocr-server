use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::traits::Database;
use super::sqlite::SqliteDatabase;

/// 数据库类型
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseType {
    Sqlite,
    #[serde(rename = "dm")]
    Dm,
}

/// 数据库配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DatabaseConfig {
    #[serde(rename = "type")]
    pub db_type: DatabaseType,
    
    /// SQLite配置
    pub sqlite: Option<SqliteConfig>,
    
    /// DM数据库配置
    pub dm: Option<DmConfig>,
}

/// SQLite配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SqliteConfig {
    pub path: String,
}

/// DM数据库配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DmConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub database: String,
}

/// 创建数据库实例
pub async fn create_database(config: &DatabaseConfig) -> Result<Box<dyn Database>> {
    match config.db_type {
        DatabaseType::Sqlite => {
            let sqlite_config = config.sqlite.as_ref()
                .ok_or_else(|| anyhow::anyhow!("SQLite configuration missing"))?;
            
            let db = SqliteDatabase::new(&sqlite_config.path).await?;
            db.initialize().await?;
            
            tracing::info!("SQLite database initialized at: {}", sqlite_config.path);
            Ok(Box::new(db))
        }
        
        DatabaseType::Dm => {
            // TODO: 实现DM数据库连接
            tracing::error!("DM database not yet implemented");
            Err(anyhow::anyhow!("DM database not yet implemented"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    
    #[tokio::test]
    async fn test_create_sqlite_database() {
        let config = DatabaseConfig {
            db_type: DatabaseType::Sqlite,
            sqlite: Some(SqliteConfig {
                path: ":memory:".to_string(),
            }),
            dm: None,
        };
        
        let db = create_database(&config).await.unwrap();
        assert!(db.health_check().await.unwrap());
    }
}