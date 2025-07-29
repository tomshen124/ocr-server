//! 配置管理模块
//! 负责配置文件的加载、验证和环境变量处理

use crate::util::config::Config;
use crate::util::log::{log_init_with_config, cleanup_old_logs};
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use anyhow::Result;
use tracing_appender::non_blocking::WorkerGuard;

/// 配置管理器
pub struct ConfigManager;

impl ConfigManager {
    /// 加载和验证配置
    pub fn load_and_validate() -> Result<(Config, ConfigValidationReport)> {
        info!("📋 开始加载配置文件...");
        
        // 智能查找配置文件
        let config_path = Self::find_config_file_path("config.yaml");
        info!("配置文件路径: {}", config_path.display());
        
        // 加载配置
        let config = match Self::load_config_from_file(&config_path) {
            Ok(config) => config,
            Err(e) => {
                warn!("⚠️ 配置文件读取失败: {} - {}", config_path.display(), e);
                Self::handle_config_load_failure(&config_path)?
            }
        };
        
        // 验证配置
        let validation_report = Self::validate_config(&config)?;
        
        // 应用环境变量覆盖
        let config = Self::apply_environment_overrides(config);
        
        info!("✅ 配置加载完成");
        Ok((config, validation_report))
    }

    /// 初始化日志系统
    pub fn initialize_logging(config: &Config) -> Result<WorkerGuard> {
        info!("📝 初始化日志系统...");
        
        let log_guard = log_init_with_config("logs", "ocr", config.logging.clone())?
            .ok_or_else(|| anyhow::anyhow!("日志系统初始化失败"))?;
        
        // 执行日志清理（如果配置了保留天数）
        if let Some(retention_days) = config.logging.file.retention_days {
            if config.logging.file.enabled {
                let log_path = Path::new(&config.logging.file.directory);
                if let Err(e) = cleanup_old_logs(log_path, retention_days) {
                    warn!("日志清理失败: {}", e);
                } else {
                    info!("✅ 日志清理完成，保留 {} 天", retention_days);
                }
            }
        }
        
        info!("✅ 日志系统初始化完成");
        Ok(log_guard)
    }

    /// 智能查找配置文件路径，适应开发和生产环境
    pub fn find_config_file_path(filename: &str) -> PathBuf {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        
        // 获取可执行文件路径，用于判断我们在生产环境的哪个目录
        let exe_path = std::env::current_exe().ok();
        
        // 情况1：如果当前目录就有config子目录，直接使用（开发环境或生产根目录）
        let config_in_current = current_dir.join("config").join(filename);
        if config_in_current.exists() {
            return config_in_current;
        }
        
        // 情况2：如果我们在bin/目录，尝试上级目录的config/（生产环境）
        if let Some(parent) = current_dir.parent() {
            let config_in_parent = parent.join("config").join(filename);
            if config_in_parent.exists() {
                return config_in_parent;
            }
        }
        
        // 情况3：检查可执行文件同级目录是否有config/
        if let Some(exe_path) = exe_path {
            if let Some(exe_dir) = exe_path.parent() {
                // 如果在bin/目录，检查上级目录
                if exe_dir.file_name() == Some(std::ffi::OsStr::new("bin")) {
                    if let Some(project_root) = exe_dir.parent() {
                        let config_in_root = project_root.join("config").join(filename);
                        if config_in_root.exists() {
                            return config_in_root;
                        }
                    }
                }
            }
        }
        
        // 情况4：开发环境路径 (直接在当前目录)
        let dev_path = current_dir.join(filename);
        if dev_path.exists() {
            return dev_path;
        }
        
        // 如果都不存在，生产环境优先返回上级目录的config/，开发环境返回当前目录
        if current_dir.file_name() == Some(std::ffi::OsStr::new("bin")) {
            if let Some(parent) = current_dir.parent() {
                parent.join("config").join(filename)
            } else {
                current_dir.join(filename)
            }
        } else {
            current_dir.join(filename)
        }
    }

    /// 从文件加载配置
    fn load_config_from_file(config_path: &Path) -> Result<Config> {
        Config::read_yaml(config_path)
    }

    /// 处理配置加载失败
    fn handle_config_load_failure(config_path: &Path) -> Result<Config> {
        if !config_path.exists() {
            info!("📝 创建默认配置文件: {}", config_path.display());
            let config = Config::default();
            if let Err(write_err) = config.write_yaml_to_path(config_path) {
                warn!("❌ 创建默认配置文件失败: {}", write_err);
            }
            Ok(config)
        } else {
            warn!("❌ 配置文件存在但无法解析，请检查语法");
            warn!("🔧 使用默认配置启动，但不会覆盖现有文件");
            Ok(Config::default())
        }
    }

    /// 验证配置
    fn validate_config(config: &Config) -> Result<ConfigValidationReport> {
        let mut report = ConfigValidationReport::new();
        
        // 验证基本配置
        Self::validate_basic_settings(config, &mut report);
        
        // 验证网络配置
        Self::validate_network_settings(config, &mut report);
        
        // 验证数据库配置
        Self::validate_database_settings(config, &mut report);
        
        // 验证存储配置
        Self::validate_storage_settings(config, &mut report);
        
        // 验证第三方配置
        Self::validate_third_party_settings(config, &mut report);
        
        // 验证监控配置
        Self::validate_monitoring_settings(config, &mut report);
        
        if report.has_errors() {
            warn!("⚠️ 配置验证发现错误: {}", report.error_count());
            for error in &report.errors {
                warn!("  - {}: {}", error.field, error.message);
            }
        }
        
        if report.has_warnings() {
            info!("ℹ️ 配置验证发现警告: {}", report.warning_count());
            for warning in &report.warnings {
                info!("  - {}: {}", warning.field, warning.message);
            }
        }
        
        Ok(report)
    }

    /// 验证基本设置
    fn validate_basic_settings(config: &Config, report: &mut ConfigValidationReport) {
        if config.port == 0 {
            report.add_error("port", "端口不能为0");
        }
        
        if config.app_id.is_empty() {
            report.add_warning("app_id", "应用ID为空，可能影响SSO认证");
        }
        
        if config.session_timeout <= 0 {
            report.add_error("session_timeout", "会话超时必须大于0");
        }
    }

    /// 验证网络设置
    fn validate_network_settings(config: &Config, report: &mut ConfigValidationReport) {
        if !config.host.starts_with("http://") && !config.host.starts_with("https://") {
            report.add_error("host", "主机URL必须以http://或https://开头");
        }
        
        if !config.preview_url.is_empty() && 
           !config.preview_url.starts_with("http://") && 
           !config.preview_url.starts_with("https://") {
            report.add_error("preview_url", "预览URL必须以http://或https://开头");
        }
    }

    /// 验证数据库设置
    fn validate_database_settings(config: &Config, report: &mut ConfigValidationReport) {
        if !config.dm_sql.database_host.is_empty() {
            if config.dm_sql.database_port.parse::<u16>().is_err() {
                report.add_error("database_port", "数据库端口格式无效");
            }
            
            if config.dm_sql.database_user.is_empty() {
                report.add_error("database_user", "数据库用户名不能为空");
            }
            
            if config.dm_sql.database_name.is_empty() {
                report.add_error("database_name", "数据库名称不能为空");
            }
        } else {
            report.add_info("database", "使用SQLite作为默认数据库");
        }
    }

    /// 验证存储设置
    fn validate_storage_settings(config: &Config, report: &mut ConfigValidationReport) {
        if !config.oss.access_key.is_empty() {
            if config.oss.bucket.is_empty() {
                report.add_error("oss_bucket", "OSS桶名称不能为空");
            }
            
            if config.oss.server_url.is_empty() {
                report.add_error("oss_server_url", "OSS服务器URL不能为空");
            }
            
            if config.oss.access_key_secret.is_empty() {
                report.add_error("oss_secret", "OSS密钥不能为空");
            }
        } else {
            report.add_info("storage", "使用本地存储作为默认存储");
        }
    }

    /// 验证第三方设置
    fn validate_third_party_settings(config: &Config, report: &mut ConfigValidationReport) {
        if config.third_party_access.enabled {
            if config.third_party_access.clients.is_empty() {
                report.add_warning("third_party_clients", "启用了第三方访问但没有配置客户端");
            }
            
            for (index, client) in config.third_party_access.clients.iter().enumerate() {
                if client.client_id.is_empty() {
                    report.add_error(&format!("client_{}_id", index), "客户端ID不能为空");
                }
                
                if client.secret_key.is_empty() {
                    report.add_error(&format!("client_{}_secret", index), "客户端密钥不能为空");
                }
                
                if client.permissions.is_empty() {
                    report.add_warning(&format!("client_{}_permissions", index), "客户端没有配置权限");
                }
            }
        }
    }

    /// 验证监控设置
    fn validate_monitoring_settings(config: &Config, report: &mut ConfigValidationReport) {
        if config.monitoring.enabled {
            report.add_info("monitoring", "监控功能已启用");
        } else {
            report.add_info("monitoring", "监控功能已禁用");
        }
    }

    /// 应用环境变量覆盖
    fn apply_environment_overrides(mut config: Config) -> Config {
        info!("🔧 应用环境变量覆盖...");
        
        // 主机和端口配置
        if let Ok(host) = std::env::var("OCR_HOST") {
            info!("环境变量覆盖: OCR_HOST = {}", host);
            config.host = host;
        }
        
        if let Ok(port) = std::env::var("OCR_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                info!("环境变量覆盖: OCR_PORT = {}", port_num);
                config.port = port_num;
            }
        }
        
        // 数据库配置
        if let Ok(db_host) = std::env::var("DATABASE_HOST") {
            info!("环境变量覆盖: DATABASE_HOST = {}", db_host);
            config.dm_sql.database_host = db_host;
        }
        
        // OSS配置
        if let Ok(oss_key) = std::env::var("OSS_ACCESS_KEY") {
            info!("环境变量覆盖: OSS_ACCESS_KEY = ****");
            config.oss.access_key = oss_key;
        }
        
        // 调试模式
        if let Ok(debug_enabled) = std::env::var("DEBUG_ENABLED") {
            let enabled = debug_enabled.to_lowercase() == "true";
            info!("环境变量覆盖: DEBUG_ENABLED = {}", enabled);
            config.debug.enabled = enabled;
        }
        
        config
    }
}

/// 配置验证报告
#[derive(Debug, Clone)]
pub struct ConfigValidationReport {
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub info: Vec<ValidationIssue>,
}

/// 验证问题
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
}

impl ConfigValidationReport {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            info: Vec::new(),
        }
    }
    
    pub fn add_error(&mut self, field: &str, message: &str) {
        self.errors.push(ValidationIssue {
            field: field.to_string(),
            message: message.to_string(),
        });
    }
    
    pub fn add_warning(&mut self, field: &str, message: &str) {
        self.warnings.push(ValidationIssue {
            field: field.to_string(),
            message: message.to_string(),
        });
    }
    
    pub fn add_info(&mut self, field: &str, message: &str) {
        self.info.push(ValidationIssue {
            field: field.to_string(),
            message: message.to_string(),
        });
    }
    
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }
    
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
    
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
    
    pub fn warning_count(&self) -> usize {
        self.warnings.len()
    }
    
    pub fn is_valid(&self) -> bool {
        !self.has_errors()
    }
}

impl Default for ConfigValidationReport {
    fn default() -> Self {
        Self::new()
    }
}