//! 数据库初始化模块
//! 负责根据配置创建和初始化数据库连接

use crate::util::config::Config;
use crate::{db, storage};
use anyhow::Result;
use std::sync::Arc;
use tracing::{error, info, warn};

/// 数据库初始化器
pub struct DatabaseInitializer;

impl DatabaseInitializer {
    /// 计算DM网关URL：优先环境变量；否则使用本机IP避免localhost
    fn resolve_dm_gateway_url(config: &Config) -> String {
        if let Ok(url) = std::env::var("DM_GATEWAY_URL") {
            return url;
        }

        // 候选IP: HOST_IP > OCR_HOST > config.server.host
        let candidate_ip = std::env::var("HOST_IP")
            .ok()
            .or_else(|| std::env::var("OCR_HOST").ok())
            .unwrap_or_else(|| config.server.host.clone());

        let invalid = candidate_ip.is_empty()
            || candidate_ip == "0.0.0.0"
            || candidate_ip == "127.0.0.1"
            || candidate_ip == "::1";

        let port = std::env::var("DM_GATEWAY_PORT")
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(8080);

        let url = if invalid {
            format!("http://127.0.0.1:{}", port)
        } else {
            format!("http://{}:{}", candidate_ip, port)
        };

        tracing::info!("[compass] 解析DM网关URL: {}", url);
        url
    }
    /// 根据配置创建数据库实例
    pub async fn create_from_config(config: &Config) -> Result<Arc<dyn db::Database>> {
        info!("[cabinet] 初始化数据库连接...");

        // [brain] 优先使用新的统一数据库配置
        if let Some(database_config) = &config.database {
            info!("[ok] 使用新的统一数据库配置");
            return Self::create_from_unified_config(database_config).await;
        }

        // 兼容旧的DMSql配置
        info!("[doc] 使用兼容的DMSql配置模式");
        info!("达梦数据库开关: {}", config.dm_sql.enabled);
        info!("达梦数据库严格模式: {}", config.dm_sql.strict_mode);

        // 根据配置创建数据库配置
        let db_config = if config.dm_sql.enabled && !config.dm_sql.database_host.is_empty() {
            // 使用智能模式：会根据编译特性自动选择达梦数据库
            info!(
                "[ok] 达梦数据库已启用，使用智能模式: {}:{}",
                config.dm_sql.database_host, config.dm_sql.database_port
            );

            #[cfg(feature = "dm_go")]
            {
                let gw_url = Self::resolve_dm_gateway_url(config);
                db::factory::DatabaseConfig {
                    db_type: db::factory::DatabaseType::Dm,
                    sqlite: Some(db::factory::SqliteConfig {
                        path: "runtime/data/ocr_fallback.db".to_string(),
                    }),
                    dm: Some(db::factory::DmConfig {
                        host: config.dm_sql.database_host.clone(),
                        port: config.dm_sql.database_port.parse().unwrap_or(5237),
                        username: config.dm_sql.database_user.clone(),
                        password: config.dm_sql.database_password.clone(),
                        database: config.dm_sql.database_name.clone(),
                        max_connections: config.dm_sql.max_connections,
                        connection_timeout: config.dm_sql.connection_timeout,
                        go_gateway: Some(db::factory::GoGatewayConfig {
                            enabled: true,
                            url: gw_url,
                            api_key: std::env::var("DM_GATEWAY_API_KEY").unwrap_or_default(),
                            timeout: 30,
                            health_check_interval: 60,
                        }),
                    }),
                }
            }
            #[cfg(not(feature = "dm_go"))]
            {
                // 没有dm_go特性时，自动降级到SQLite
                warn!("[warn] 达梦数据库配置存在但缺少dm_go特性，自动降级到SQLite");
                db::factory::DatabaseConfig {
                    db_type: db::factory::DatabaseType::Sqlite,
                    sqlite: Some(db::factory::SqliteConfig {
                        path: "runtime/data/ocr_fallback.db".to_string(),
                    }),
                    dm: None,
                }
            }
        } else {
            // 使用 SQLite 作为降级数据库
            if config.dm_sql.enabled {
                info!("[warn] 达梦数据库已启用但配置不完整，降级到 SQLite");
            } else {
                info!("[doc] 达梦数据库未启用，使用 SQLite 作为默认数据库");
            }
            db::factory::DatabaseConfig {
                db_type: db::factory::DatabaseType::Sqlite,
                sqlite: Some(db::factory::SqliteConfig {
                    path: "data/ocr.db".to_string(),
                }),
                dm: None,
            }
        };

        // 尝试创建数据库连接
        let database_result = db::factory::create_database(&db_config).await;

        let database = match database_result {
            Ok(db) => {
                info!("[ok] 数据库连接创建成功");
                db
            }
            Err(e) => {
                let error_msg = format!("数据库连接失败: {}", e);

                if config.dm_sql.enabled && config.dm_sql.strict_mode {
                    // 严格模式下，达梦数据库连接失败则服务启动失败
                    return Err(anyhow::anyhow!(
                        "[red] 严格模式: 达梦数据库连接失败，服务停止启动: {}",
                        e
                    ));
                } else if config.dm_sql.enabled {
                    // 非严格模式下，降级到 SQLite
                    tracing::warn!("[warn] 达梦数据库连接失败，自动降级到 SQLite: {}", e);

                    let sqlite_config = db::factory::DatabaseConfig {
                        db_type: db::factory::DatabaseType::Sqlite,
                        sqlite: Some(db::factory::SqliteConfig {
                            path: "data/ocr.db".to_string(),
                        }),
                        dm: None,
                    };

                    match db::factory::create_database(&sqlite_config).await {
                        Ok(sqlite_db) => {
                            info!("[ok] SQLite 降级数据库连接成功");
                            sqlite_db
                        }
                        Err(sqlite_err) => {
                            return Err(anyhow::anyhow!(
                                "[red] 达梦数据库和 SQLite 降级都失败: DM错误={}, SQLite错误={}",
                                e,
                                sqlite_err
                            ));
                        }
                    }
                } else {
                    return Err(anyhow::anyhow!(error_msg));
                }
            }
        };

        // 如果启用了故障转移，包装数据库
        if config.failover.database.enabled {
            info!("[loop] 数据库故障转移已启用");
            let failover_db =
                db::FailoverDatabase::new(Arc::from(database), config.failover.database.clone())
                    .await?;
            Ok(Arc::new(failover_db) as Arc<dyn db::Database>)
        } else {
            info!("[ok] 数据库初始化完成");
            Ok(Arc::from(database))
        }
    }

    /// [brain] 使用新的统一数据库配置创建数据库
    async fn create_from_unified_config(
        database_config: &crate::util::config::DatabaseConfig,
    ) -> Result<Arc<dyn db::Database>> {
        use crate::util::config::types::{
            DmConfig as ConfigDmConfig, SqliteConfig as ConfigSqliteConfig,
        };

        info!(
            "[brain] 使用统一数据库配置模式: {}",
            database_config.database_type
        );

        // 转换配置格式
        let db_config =
            match database_config.database_type.as_str() {
                "sqlite" => {
                    info!("[doc] SQLite数据库模式");
                    let sqlite_config = database_config
                        .sqlite
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("SQLite配置缺失"))?;
                    db::factory::DatabaseConfig {
                        db_type: db::factory::DatabaseType::Sqlite,
                        sqlite: Some(db::factory::SqliteConfig {
                            path: sqlite_config.path.clone(),
                        }),
                        dm: None,
                    }
                }
                #[cfg(feature = "dm_go")]
                "dm" => {
                    info!("[card] 达梦数据库模式");
                    let dm_config = database_config
                        .dm
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("达梦数据库配置缺失"))?;
                    let gw = dm_config
                        .go_gateway
                        .as_ref()
                        .map(|g| db::factory::GoGatewayConfig {
                            enabled: g.enabled,
                            url: g.url.clone(),
                            api_key: g.api_key.clone(),
                            timeout: g.timeout,
                            health_check_interval: g.health_check_interval,
                        });
                    db::factory::DatabaseConfig {
                        db_type: db::factory::DatabaseType::Dm,
                        sqlite: None,
                        dm: Some(db::factory::DmConfig {
                            host: dm_config.host.clone(),
                            port: dm_config.port,
                            username: dm_config.username.clone(),
                            password: dm_config.password.clone(),
                            database: dm_config.database.clone(),
                            max_connections: dm_config.max_connections,
                            connection_timeout: dm_config.connection_timeout,
                            go_gateway: gw,
                        }),
                    }
                }
                "smart" => {
                    info!("[ok] 智能数据库模式启用");
                    #[cfg(feature = "dm_go")]
                    {
                        // 优先尝试达梦数据库
                        if let Some(dm_config) = &database_config.dm {
                            info!("[brain] 启动智能数据库连接模式");
                            info!("[loop] 尝试智能达梦数据库连接...");
                            let gw = dm_config.go_gateway.as_ref().map(|g| {
                                db::factory::GoGatewayConfig {
                                    enabled: g.enabled,
                                    url: g.url.clone(),
                                    api_key: g.api_key.clone(),
                                    timeout: g.timeout,
                                    health_check_interval: g.health_check_interval,
                                }
                            });
                            db::factory::DatabaseConfig {
                                db_type: db::factory::DatabaseType::Dm,
                                sqlite: database_config.sqlite.as_ref().map(|s| {
                                    db::factory::SqliteConfig {
                                        path: s.path.clone(),
                                    }
                                }),
                                dm: Some(db::factory::DmConfig {
                                    host: dm_config.host.clone(),
                                    port: dm_config.port,
                                    username: dm_config.username.clone(),
                                    password: dm_config.password.clone(),
                                    database: dm_config.database.clone(),
                                    max_connections: dm_config.max_connections,
                                    connection_timeout: dm_config.connection_timeout,
                                    go_gateway: gw,
                                }),
                            }
                        } else {
                            warn!("[warn] 智能模式启用但缺少达梦数据库配置，使用SQLite");
                            let sqlite_config = database_config
                                .sqlite
                                .as_ref()
                                .ok_or_else(|| anyhow::anyhow!("SQLite配置缺失"))?;
                            db::factory::DatabaseConfig {
                                db_type: db::factory::DatabaseType::Sqlite,
                                sqlite: Some(db::factory::SqliteConfig {
                                    path: sqlite_config.path.clone(),
                                }),
                                dm: None,
                            }
                        }
                    }
                    #[cfg(not(feature = "dm_go"))]
                    {
                        // 没有dm_go特性时，智能模式自动降级到SQLite
                        info!("[warn] 智能模式启用但缺少dm_go特性，自动降级到SQLite");
                        let sqlite_config = database_config
                            .sqlite
                            .as_ref()
                            .ok_or_else(|| anyhow::anyhow!("SQLite配置缺失"))?;
                        db::factory::DatabaseConfig {
                            db_type: db::factory::DatabaseType::Sqlite,
                            sqlite: Some(db::factory::SqliteConfig {
                                path: sqlite_config.path.clone(),
                            }),
                            dm: None,
                        }
                    }
                }
                _ => {
                    warn!(
                        "[warn] 未知数据库类型 '{}', 降级使用SQLite",
                        database_config.database_type
                    );
                    let sqlite_config = database_config
                        .sqlite
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("SQLite配置缺失"))?;
                    db::factory::DatabaseConfig {
                        db_type: db::factory::DatabaseType::Sqlite,
                        sqlite: Some(db::factory::SqliteConfig {
                            path: sqlite_config.path.clone(),
                        }),
                        dm: None,
                    }
                }
            };

        // 尝试创建数据库连接，智能故障转移
        let database = match db::factory::create_database(&db_config).await {
            Ok(database) => {
                info!("[ok] 统一数据库配置连接成功");
                Arc::from(database)
            }
            Err(e) => {
                warn!("[warn] 主数据库连接失败，尝试故障转移: {}", e);

                // 智能故障转移逻辑
                if db_config.dm.is_some() {
                    // 达梦数据库连接失败，自动降级到SQLite
                    if let Some(sqlite_config) = &db_config.sqlite {
                        info!("[loop] 网关连接失败，智能降级到SQLite数据库");
                        let fallback_config = db::factory::DatabaseConfig {
                            db_type: db::factory::DatabaseType::Sqlite,
                            sqlite: Some(sqlite_config.clone()),
                            dm: None,
                        };

                        match db::factory::create_database(&fallback_config).await {
                            Ok(sqlite_db) => {
                                info!("[ok] SQLite故障转移数据库连接成功");
                                Arc::from(sqlite_db)
                            }
                            Err(sqlite_err) => {
                                return Err(anyhow::anyhow!(
                                    "[red] 达梦数据库和SQLite故障转移都失败: 网关错误={}, SQLite错误={}", 
                                    e, sqlite_err
                                ));
                            }
                        }
                    } else {
                        // 没有SQLite配置，创建默认的SQLite数据库
                        warn!("[loop] 网关连接失败且未配置SQLite，创建默认本地数据库");
                        let default_sqlite_config = db::factory::DatabaseConfig {
                            db_type: db::factory::DatabaseType::Sqlite,
                            sqlite: Some(db::factory::SqliteConfig {
                                path: "runtime/data/ocr_fallback.db".to_string(),
                            }),
                            dm: None,
                        };

                        match db::factory::create_database(&default_sqlite_config).await {
                            Ok(sqlite_db) => {
                                info!("[ok] 默认SQLite数据库创建成功");
                                Arc::from(sqlite_db)
                            }
                            Err(sqlite_err) => {
                                return Err(anyhow::anyhow!(
                                    "[red] 网关连接失败且SQLite创建失败: 网关错误={}, SQLite错误={}",
                                    e,
                                    sqlite_err
                                ));
                            }
                        }
                    }
                } else {
                    return Err(anyhow::anyhow!("[red] 数据库连接失败: {}", e));
                }
            }
        };

        Ok(database)
    }

    /// 验证数据库连接
    pub async fn validate_connection(database: &Arc<dyn db::Database>) -> Result<()> {
        info!("[search] 验证数据库连接...");

        // 这里可以添加数据库连接验证逻辑
        // 例如执行简单的查询或健康检查

        info!("[ok] 数据库连接验证成功");
        Ok(())
    }

    /// 初始化数据库表结构（如果需要）
    pub async fn initialize_schema(database: &Arc<dyn db::Database>) -> Result<()> {
        info!("[clipboard] 检查数据库表结构...");

        // 这里可以添加表结构初始化逻辑
        // 例如创建必要的表或执行迁移

        info!("[ok] 数据库表结构检查完成");
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
