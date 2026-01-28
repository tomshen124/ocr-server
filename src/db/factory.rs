use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tracing::{error, info, warn};

#[cfg(feature = "dm_go")]
use super::dm::DmDatabase;
use super::sqlite::SqliteDatabase;
use super::traits::Database;

/// 数据库类型
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DatabaseType {
    Sqlite,
    #[cfg(feature = "dm_go")]
    Dm, // 达梦数据库（Go网关） - 仅在dm_go特性启用时可用
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
    pub max_connections: u32,
    pub connection_timeout: u64,

    /// Go网关配置
    pub go_gateway: Option<GoGatewayConfig>,
}

/// Go网关配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GoGatewayConfig {
    #[serde(default = "default_go_gateway_enabled")]
    pub enabled: bool,
    #[serde(default = "default_go_gateway_url")]
    pub url: String,
    /// X-API-Key 认证密钥
    pub api_key: String,
    #[serde(default = "default_go_gateway_timeout")]
    pub timeout: u64,
    #[serde(default = "default_go_gateway_health_check_interval")]
    pub health_check_interval: u64,
}

// Go网关默认值函数
fn default_go_gateway_enabled() -> bool {
    true
}
fn default_go_gateway_url() -> String {
    "http://localhost:8080".to_string()
}
fn default_go_gateway_timeout() -> u64 {
    30
}
fn default_go_gateway_health_check_interval() -> u64 {
    60
}

/// 故障转移配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct FailoverConfig {
    pub enabled: bool,
    pub health_check_interval: u64,
    pub max_retries: u32,
    pub retry_delay: u64,
    pub fallback_to_local: bool,
    pub local_data_dir: String,
    pub auto_recovery: AutoRecoveryConfig,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            health_check_interval: 60,
            max_retries: 3,
            retry_delay: 5000,
            fallback_to_local: true,
            local_data_dir: "runtime/fallback/db".to_string(),
            auto_recovery: AutoRecoveryConfig::default(),
        }
    }
}

/// 自动恢复配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AutoRecoveryConfig {
    pub enabled: bool,
    pub check_interval: u64,
    pub consecutive_success: u32,
}

impl Default for AutoRecoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval: 300,
            consecutive_success: 3,
        }
    }
}

/// 数据库状态
#[derive(Debug, Clone, PartialEq)]
enum DatabaseState {
    Primary,    // 使用主数据库
    Fallback,   // 使用备用数据库
    Recovering, // 恢复中
}

/// 智能数据库管理器
pub struct SmartDatabaseManager {
    state: Arc<RwLock<DatabaseState>>,
    primary_db: Option<Arc<Box<dyn Database>>>,
    fallback_db: Arc<Box<dyn Database>>,
    config: FailoverConfig,
    last_health_check: Arc<RwLock<SystemTime>>,
    consecutive_success_count: Arc<RwLock<u32>>,
}

impl SmartDatabaseManager {
    /// 创建智能数据库管理器
    pub async fn new(
        primary_config: Option<&DmConfig>,
        fallback_config: &SqliteConfig,
        failover_config: FailoverConfig,
    ) -> Result<Self> {
        info!("[tool] 初始化智能数据库管理器...");

        // 创建备用SQLite数据库
        let fallback_db = {
            let db = SqliteDatabase::new(&fallback_config.path).await?;
            db.initialize().await?;
            info!("[ok] 备用SQLite数据库初始化完成: {}", fallback_config.path);
            Arc::new(Box::new(db) as Box<dyn Database>)
        };

        // 尝试创建主数据库
        let (primary_db, initial_state) = if let Some(dm_config) = primary_config {
            #[cfg(feature = "dm_go")]
            match Self::try_create_dm_database(dm_config, &failover_config).await {
                Ok(db) => {
                    info!("[ok] 主达梦数据库连接成功");
                    (Some(Arc::new(db)), DatabaseState::Primary)
                }
                Err(e) => {
                    if failover_config.enabled && failover_config.fallback_to_local {
                        warn!("[warn] 主数据库连接失败，自动切换到备用数据库: {}", e);
                        (None, DatabaseState::Fallback)
                    } else {
                        error!("[fail] 主数据库连接失败且故障转移被禁用: {}", e);
                        return Err(e);
                    }
                }
            }

            #[cfg(not(feature = "dm_go"))]
            {
                info!("ℹ DM数据库功能未编译，跳过主数据库连接");
                (None, DatabaseState::Fallback)
            }
        } else {
            info!("ℹ 未配置主数据库，使用SQLite数据库");
            (None, DatabaseState::Fallback)
        };

        let manager = Self {
            state: Arc::new(RwLock::new(initial_state)),
            primary_db,
            fallback_db,
            config: failover_config,
            last_health_check: Arc::new(RwLock::new(SystemTime::now())),
            consecutive_success_count: Arc::new(RwLock::new(0)),
        };

        info!(
            "[target] 智能数据库管理器初始化完成，当前状态: {:?}",
            manager.state.read()
        );
        Ok(manager)
    }

    /// 尝试创建达梦数据库连接
    #[cfg(feature = "dm_go")]
    async fn try_create_dm_database(
        config: &DmConfig,
        failover_config: &FailoverConfig,
    ) -> Result<Box<dyn Database>> {
        let mut attempts = 0;
        let max_attempts = failover_config.max_retries + 1;

        while attempts < max_attempts {
            info!(
                "[loop] 尝试连接达梦数据库... (第{}/{}次)",
                attempts + 1,
                max_attempts
            );

            match DmDatabase::new(config).await {
                Ok(db) => {
                    info!("[clipboard] 检查达梦数据库表结构并初始化...");
                    match db.initialize().await {
                        Ok(()) => {
                            info!("[ok] 达梦数据库连接和初始化成功");
                            return Ok(Box::new(db));
                        }
                        Err(e) => {
                            warn!("[warn] 达梦数据库初始化失败: {}", e);
                            attempts += 1;
                        }
                    }
                }
                Err(e) => {
                    warn!("[warn] 达梦数据库连接失败: {}", e);
                    attempts += 1;
                }
            }

            if attempts < max_attempts {
                info!("[hourglass] {}毫秒后重试...", failover_config.retry_delay);
                tokio::time::sleep(Duration::from_millis(failover_config.retry_delay)).await;
            }
        }

        Err(anyhow!("达梦数据库连接失败，已重试{}次", max_attempts))
    }

    /// 获取当前活跃的数据库
    pub fn get_active_database(&self) -> Arc<Box<dyn Database>> {
        match *self.state.read() {
            DatabaseState::Primary => {
                if let Some(ref primary_db) = self.primary_db {
                    primary_db.clone()
                } else {
                    // 如果主数据库不可用，降级到备用数据库
                    warn!("[warn] 主数据库不可用，自动切换到备用数据库");
                    let mut state = self.state.write();
                    *state = DatabaseState::Fallback;
                    self.fallback_db.clone()
                }
            }
            DatabaseState::Fallback | DatabaseState::Recovering => self.fallback_db.clone(),
        }
    }

    /// 健康检查和自动恢复
    pub async fn health_check_and_recovery(&self) {
        if !self.config.enabled {
            return;
        }

        let now = SystemTime::now();
        let last_check = *self.last_health_check.read();

        if now.duration_since(last_check).unwrap_or_default().as_secs()
            < self.config.health_check_interval
        {
            return; // 还未到检查时间
        }

        *self.last_health_check.write() = now;

        match *self.state.read() {
            DatabaseState::Fallback => {
                // 如果当前使用备用数据库，尝试恢复主数据库
                if self.config.auto_recovery.enabled {
                    self.try_recovery_primary().await;
                }
            }
            DatabaseState::Primary => {
                // 检查主数据库健康状态
                if let Some(ref primary_db) = self.primary_db {
                    match primary_db.health_check().await {
                        Ok(true) => {
                            *self.consecutive_success_count.write() += 1;
                        }
                        Ok(false) | Err(_) => {
                            warn!("[warn] 主数据库健康检查失败，切换到备用数据库");
                            let mut state = self.state.write();
                            *state = DatabaseState::Fallback;
                            *self.consecutive_success_count.write() = 0;
                        }
                    }
                }
            }
            DatabaseState::Recovering => {
                // 恢复过程中，继续尝试
                self.try_recovery_primary().await;
            }
        }
    }

    /// 尝试恢复到主数据库
    async fn try_recovery_primary(&self) {
        if let Some(ref primary_db) = self.primary_db {
            match primary_db.health_check().await {
                Ok(true) => {
                    let mut count = self.consecutive_success_count.write();
                    *count += 1;

                    if *count >= self.config.auto_recovery.consecutive_success {
                        info!("[celebrate] 主数据库恢复成功，切换回主数据库");
                        let mut state = self.state.write();
                        *state = DatabaseState::Primary;
                        *count = 0;
                    } else {
                        info!(
                            "[loop] 主数据库恢复中... ({}/{})",
                            *count, self.config.auto_recovery.consecutive_success
                        );
                        let mut state = self.state.write();
                        *state = DatabaseState::Recovering;
                    }
                }
                Ok(false) | Err(_) => {
                    *self.consecutive_success_count.write() = 0;
                    let mut state = self.state.write();
                    *state = DatabaseState::Fallback;
                }
            }
        }
    }

    /// 获取当前状态信息
    pub fn get_status(&self) -> (String, bool, bool) {
        let state = self.state.read();
        let state_str = match *state {
            DatabaseState::Primary => "主数据库",
            DatabaseState::Fallback => "备用数据库",
            DatabaseState::Recovering => "恢复中",
        };

        let is_primary = matches!(*state, DatabaseState::Primary);
        let has_primary = self.primary_db.is_some();

        (state_str.to_string(), is_primary, has_primary)
    }
}

/// 创建数据库实例
pub async fn create_database(config: &DatabaseConfig) -> Result<Box<dyn Database>> {
    match config.db_type {
        #[cfg(feature = "dm_go")]
        DatabaseType::Dm => {
            // 达梦数据库（Go网关）
            let dm_config = config
                .dm
                .as_ref()
                .ok_or_else(|| anyhow!("达梦数据库配置缺失"))?;

            info!(
                "[link] 连接达梦数据库（Go网关）: {}:{}/{}",
                dm_config.host, dm_config.port, dm_config.database
            );

            let db = DmDatabase::new(dm_config).await?;
            db.initialize().await?;
            info!("[ok] 达梦数据库连接和初始化成功");
            Ok(Box::new(db))
        }

        DatabaseType::Sqlite => {
            // SQLite数据库
            let sqlite_config = config
                .sqlite
                .as_ref()
                .ok_or_else(|| anyhow!("SQLite数据库配置缺失"))?;

            info!("[card] 连接SQLite数据库: {}", sqlite_config.path);
            let db = SqliteDatabase::new(&sqlite_config.path).await?;
            db.initialize().await?;
            info!("[ok] SQLite数据库连接和初始化成功");
            Ok(Box::new(db))
        }

        // 在没有dm_go特性时，任何非SQLite请求都降级到SQLite
        #[cfg(not(feature = "dm_go"))]
        _ => {
            warn!("[warn] DM数据库功能未启用，自动降级到SQLite");
            let sqlite_config = config
                .sqlite
                .as_ref()
                .ok_or_else(|| anyhow!("SQLite数据库配置缺失"))?;

            info!("[card] 连接SQLite数据库: {}", sqlite_config.path);
            let db = SqliteDatabase::new(&sqlite_config.path).await?;
            db.initialize().await?;
            info!("[ok] SQLite数据库连接和初始化成功");
            Ok(Box::new(db))
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
