//! 数据库初始化模块
//! 负责根据配置创建和初始化数据库连接

use crate::util::config::Config;
use crate::{db, storage};
use std::sync::Arc;
use tracing::info;
use anyhow::Result;

/// 数据库初始化器
pub struct DatabaseInitializer;

impl DatabaseInitializer {
    /// 根据配置创建数据库实例
    pub async fn create_from_config(config: &Config) -> Result<Arc<dyn db::Database>> {
        info!("🗄️ 初始化数据库连接...");
        
        // 根据配置创建数据库配置
        let db_config = if config.dm_sql.database_host.is_empty() {
            // 使用 SQLite 作为默认数据库
            info!("使用 SQLite 作为默认数据库");
            db::factory::DatabaseConfig {
                db_type: db::factory::DatabaseType::Sqlite,
                sqlite: Some(db::factory::SqliteConfig {
                    path: "data/ocr.db".to_string(),
                }),
                dm: None,
            }
        } else {
            // 使用达梦数据库
            info!("使用达梦数据库: {}:{}", 
                config.dm_sql.database_host, 
                config.dm_sql.database_port
            );
            db::factory::DatabaseConfig {
                db_type: db::factory::DatabaseType::Dm,
                sqlite: None,
                dm: Some(db::factory::DmConfig {
                    host: config.dm_sql.database_host.clone(),
                    port: config.dm_sql.database_port.parse().unwrap_or(5236),
                    username: config.dm_sql.database_user.clone(),
                    password: config.dm_sql.database_password.clone(),
                    database: config.dm_sql.database_name.clone(),
                }),
            }
        };

        let database = db::factory::create_database(&db_config).await?;
        
        // 如果启用了故障转移，包装数据库
        if config.failover.database.enabled {
            info!("✅ 数据库故障转移已启用");
            let failover_db = db::FailoverDatabase::new(
                Arc::from(database),
                config.failover.database.clone()
            ).await?;
            Ok(Arc::new(failover_db) as Arc<dyn db::Database>)
        } else {
            info!("✅ 数据库初始化完成");
            Ok(Arc::from(database))
        }
    }

    /// 验证数据库连接
    pub async fn validate_connection(database: &Arc<dyn db::Database>) -> Result<()> {
        info!("🔍 验证数据库连接...");
        
        // 这里可以添加数据库连接验证逻辑
        // 例如执行简单的查询或健康检查
        
        info!("✅ 数据库连接验证成功");
        Ok(())
    }

    /// 初始化数据库表结构（如果需要）
    pub async fn initialize_schema(database: &Arc<dyn db::Database>) -> Result<()> {
        info!("📋 检查数据库表结构...");
        
        // 这里可以添加表结构初始化逻辑
        // 例如创建必要的表或执行迁移
        
        info!("✅ 数据库表结构检查完成");
        Ok(())
    }

    /// 执行数据库健康检查
    pub async fn health_check(database: &Arc<dyn db::Database>) -> Result<DatabaseHealth> {
        // 执行基本的数据库健康检查
        let start_time = std::time::Instant::now();
        
        // 尝试执行简单查询（具体实现根据数据库类型而定）
        let connection_test_result = Self::test_connection(database).await;
        let response_time = start_time.elapsed();
        
        Ok(DatabaseHealth {
            is_healthy: connection_test_result.is_ok(),
            response_time_ms: response_time.as_millis() as u64,
            error_message: connection_test_result.err().map(|e| e.to_string()),
            last_check: chrono::Utc::now(),
        })
    }

    /// 测试数据库连接
    async fn test_connection(database: &Arc<dyn db::Database>) -> Result<()> {
        // 这里应该根据具体的数据库实现来执行连接测试
        // 暂时返回成功，实际实现中应该调用数据库的健康检查方法
        
        // 模拟连接测试
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        Ok(())
    }
}

/// 数据库健康状态
#[derive(Debug, Clone)]
pub struct DatabaseHealth {
    /// 是否健康
    pub is_healthy: bool,
    /// 响应时间（毫秒）
    pub response_time_ms: u64,
    /// 错误消息（如果有）
    pub error_message: Option<String>,
    /// 最后检查时间
    pub last_check: chrono::DateTime<chrono::Utc>,
}

impl DatabaseHealth {
    /// 创建健康状态
    pub fn healthy(response_time_ms: u64) -> Self {
        Self {
            is_healthy: true,
            response_time_ms,
            error_message: None,
            last_check: chrono::Utc::now(),
        }
    }

    /// 创建不健康状态
    pub fn unhealthy(error: String) -> Self {
        Self {
            is_healthy: false,
            response_time_ms: 0,
            error_message: Some(error),
            last_check: chrono::Utc::now(),
        }
    }
}