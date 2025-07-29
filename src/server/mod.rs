//! 服务器模块
//! 
//! 这个模块提供了完整的服务器设置和管理功能，包括：
//! - 配置管理 (config.rs)
//! - 数据库初始化 (database.rs)
//! - 存储系统初始化 (storage.rs)
//! - HTTP服务器设置 (http.rs)
//! - 监控服务管理 (monitoring.rs)
//! 
//! 使用示例：
//! ```rust
//! use crate::server::ServerBootstrap;
//! 
//! // 启动完整的服务器
//! let server = ServerBootstrap::new().await?;
//! server.start().await?;
//! ```

pub mod config;
pub mod database;
pub mod storage;
pub mod http;
pub mod monitoring;

// 重新导出主要组件
pub use config::{ConfigManager, ConfigValidationReport};
pub use database::{DatabaseInitializer, DatabaseHealth};
pub use storage::{StorageInitializer, StorageHealth, StorageStats};
pub use http::{ServerManager, HttpServer, ServerStatus};
pub use monitoring::{MonitoringManager, MonitoringServices, MonitoringStatus};

use crate::util::config::Config;
use crate::util::system_info::init_start_time;
use crate::AppState;
use std::sync::Arc;
use tracing::info;
use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;

/// 服务器引导程序 - 统一的服务器启动入口
pub struct ServerBootstrap {
    config: Config,
    validation_report: ConfigValidationReport,
    _log_guard: WorkerGuard,
}

impl ServerBootstrap {
    /// 创建新的服务器引导程序
    pub async fn new() -> Result<Self> {
        info!("🚀 开始服务器引导程序...");
        
        // 加载和验证配置
        let (config, validation_report) = ConfigManager::load_and_validate()?;
        
        // 初始化日志系统
        let log_guard = ConfigManager::initialize_logging(&config)?;
        
        // 记录配置验证结果
        if validation_report.has_errors() {
            return Err(anyhow::anyhow!("配置验证失败: {} 个错误", validation_report.error_count()));
        }
        
        info!("✅ 服务器引导程序初始化完成");
        
        Ok(Self {
            config,
            validation_report,
            _log_guard: log_guard,
        })
    }

    /// 启动服务器
    pub async fn start(self) -> Result<()> {
        info!("=== OCR服务启动 ===");
        info!("服务端口: {}", self.config.port);
        
        // 初始化系统启动时间
        init_start_time();
        
        // 创建应用状态
        let app_state = self.create_app_state().await?;
        
        // 启动监控服务
        let monitoring_services = MonitoringManager::start_monitoring_services(&self.config).await?;
        
        // 创建HTTP服务器
        let server = ServerManager::create_server(&self.config, app_state).await?;
        
        // 启动服务器（这会阻塞直到关闭）
        let server_result = ServerManager::start_server(server).await;
        
        // 停止监控服务
        MonitoringManager::stop_monitoring_services(monitoring_services).await?;
        
        server_result
    }

    /// 创建应用状态
    async fn create_app_state(&self) -> Result<AppState> {
        info!("🏗️ 创建应用状态...");
        
        // 初始化数据库
        let database = self.initialize_database().await?;
        
        // 初始化存储系统
        let storage = self.initialize_storage().await?;
        
        // 创建应用状态
        let app_state = AppState {
            database,
            storage,
            config: self.config.clone(),
        };
        
        info!("✅ 应用状态创建完成");
        Ok(app_state)
    }

    /// 初始化数据库
    async fn initialize_database(&self) -> Result<Arc<dyn crate::db::Database>> {
        let database = DatabaseInitializer::create_from_config(&self.config).await?;
        
        // 验证数据库连接
        DatabaseInitializer::validate_connection(&database).await?;
        
        // 初始化数据库架构
        DatabaseInitializer::initialize_schema(&database).await?;
        
        Ok(database)
    }

    /// 初始化存储系统
    async fn initialize_storage(&self) -> Result<Arc<dyn crate::storage::Storage>> {
        let storage = StorageInitializer::create_from_config(&self.config).await?;
        
        // 验证存储系统连接
        StorageInitializer::validate_connection(&storage).await?;
        
        // 初始化存储目录结构
        StorageInitializer::initialize_directories(&storage).await?;
        
        Ok(storage)
    }

    /// 获取配置信息
    pub fn get_config(&self) -> &Config {
        &self.config
    }

    /// 获取验证报告
    pub fn get_validation_report(&self) -> &ConfigValidationReport {
        &self.validation_report
    }

    /// 执行健康检查
    pub async fn health_check(&self) -> Result<SystemHealthReport> {
        info!("🔍 执行系统健康检查...");
        
        // 创建临时的数据库和存储实例进行健康检查
        let database = DatabaseInitializer::create_from_config(&self.config).await?;
        let storage = StorageInitializer::create_from_config(&self.config).await?;
        
        // 执行数据库健康检查
        let database_health = DatabaseInitializer::health_check(&database).await?;
        
        // 执行存储系统健康检查
        let storage_health = StorageInitializer::health_check(&storage).await?;
        
        // 验证服务器配置
        let server_validation = ServerManager::validate_server_config(&self.config)?;
        
        let overall_healthy = database_health.is_healthy && 
                            storage_health.is_healthy && 
                            server_validation.is_valid();
        
        Ok(SystemHealthReport {
            overall_healthy,
            database_health,
            storage_health,
            server_config_valid: server_validation.is_valid(),
            validation_warnings: server_validation.warnings.clone(),
            check_time: chrono::Utc::now(),
        })
    }

    /// 获取系统信息摘要
    pub fn get_system_summary(&self) -> SystemSummary {
        SystemSummary {
            service_name: "OCR智能预审系统".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            host: self.config.host.clone(),
            port: self.config.port,
            debug_mode: self.config.debug.enabled,
            database_type: if self.config.dm_sql.database_host.is_empty() {
                "SQLite".to_string()
            } else {
                "达梦数据库".to_string()
            },
            storage_type: if self.config.oss.access_key.is_empty() {
                "本地存储".to_string()
            } else {
                "OSS存储".to_string()
            },
            monitoring_enabled: self.config.monitoring.enabled,
            third_party_access_enabled: self.config.third_party_access.enabled,
        }
    }
}

/// 系统健康检查报告
#[derive(Debug, Clone)]
pub struct SystemHealthReport {
    pub overall_healthy: bool,
    pub database_health: DatabaseHealth,
    pub storage_health: StorageHealth,
    pub server_config_valid: bool,
    pub validation_warnings: Vec<String>,
    pub check_time: chrono::DateTime<chrono::Utc>,
}

/// 系统信息摘要
#[derive(Debug, Clone)]
pub struct SystemSummary {
    pub service_name: String,
    pub version: String,
    pub host: String,
    pub port: u16,
    pub debug_mode: bool,
    pub database_type: String,
    pub storage_type: String,
    pub monitoring_enabled: bool,
    pub third_party_access_enabled: bool,
}

/// 便捷函数：快速启动服务器
pub async fn start_server() -> Result<()> {
    let bootstrap = ServerBootstrap::new().await?;
    bootstrap.start().await
}

/// 便捷函数：执行健康检查
pub async fn check_system_health() -> Result<SystemHealthReport> {
    let bootstrap = ServerBootstrap::new().await?;
    bootstrap.health_check().await
}