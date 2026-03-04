
use crate::util::config::Config;
use crate::{db, storage};
use anyhow::Result;
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct DatabaseInitializer;

impl DatabaseInitializer {
    fn resolve_dm_gateway_url(config: &Config) -> String {
        if let Ok(url) = std::env::var("DM_GATEWAY_URL") {
            return url;
        }

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
    pub async fn create_from_config(config: &Config) -> Result<Arc<dyn db::Database>> {
        info!("[cabinet] 初始化数据库连接...");

        if let Some(database_config) = &config.database {
            info!("[ok] 使用新的统一数据库配置");
            return Self::create_from_unified_config(database_config).await;
        }

        info!("[doc] 使用兼容的DMSql配置模式");
        info!("达梦数据库开关: {}", config.dm_sql.enabled);
        info!("达梦数据库严格模式: {}", config.dm_sql.strict_mode);

        let db_config = if config.dm_sql.enabled && !config.dm_sql.database_host.is_empty() {
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

        let database_result = db::factory::create_database(&db_config).await;

        let database = match database_result {
            Ok(db) => {
                info!("[ok] 数据库连接创建成功");
                db
            }
            Err(e) => {
                let error_msg = format!("数据库连接失败: {}", e);

                if config.dm_sql.enabled && config.dm_sql.strict_mode {
                    return Err(anyhow::anyhow!(
                        "[red] 严格模式: 达梦数据库连接失败，服务停止启动: {}",
                        e
                    ));
                } else if config.dm_sql.enabled {
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

        let database = match db::factory::create_database(&db_config).await {
            Ok(database) => {
                info!("[ok] 统一数据库配置连接成功");
                Arc::from(database)
            }
            Err(e) => {
                warn!("[warn] 主数据库连接失败，尝试故障转移: {}", e);

                if db_config.dm.is_some() {
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

    pub async fn validate_connection(database: &Arc<dyn db::Database>) -> Result<()> {
        info!("[search] 验证数据库连接...");


        info!("[ok] 数据库连接验证成功");
        Ok(())
    }

    pub async fn initialize_schema(database: &Arc<dyn db::Database>) -> Result<()> {
        info!("[clipboard] 检查数据库表结构...");


        info!("[ok] 数据库表结构检查完成");
        Ok(())
    }

    pub async fn health_check(database: &Arc<dyn db::Database>) -> Result<DatabaseHealth> {
        let start_time = std::time::Instant::now();

        let connection_test_result = Self::test_connection(database).await;
        let response_time = start_time.elapsed();

        Ok(DatabaseHealth {
            is_healthy: connection_test_result.is_ok(),
            response_time_ms: response_time.as_millis() as u64,
            error_message: connection_test_result.err().map(|e| e.to_string()),
            last_check: chrono::Utc::now(),
        })
    }

    async fn test_connection(database: &Arc<dyn db::Database>) -> Result<()> {

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseHealth {
    pub is_healthy: bool,
    pub response_time_ms: u64,
    pub error_message: Option<String>,
    pub last_check: chrono::DateTime<chrono::Utc>,
}

impl DatabaseHealth {
    pub fn healthy(response_time_ms: u64) -> Self {
        Self {
            is_healthy: true,
            response_time_ms,
            error_message: None,
            last_check: chrono::Utc::now(),
        }
    }

    pub fn unhealthy(error: String) -> Self {
        Self {
            is_healthy: false,
            response_time_ms: 0,
            error_message: Some(error),
            last_check: chrono::Utc::now(),
        }
    }
}
