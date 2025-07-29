//! 配置验证模块
//! 提供配置的验证、检查和诊断功能

use super::types::*;
use anyhow::Result;
use std::path::Path;

/// 配置验证器
pub struct ConfigValidator;

impl ConfigValidator {
    /// 全面验证配置
    pub fn validate_all(config: &Config) -> Result<ValidationReport> {
        let mut report = ValidationReport::new();
        
        // 验证基础配置
        Self::validate_basic_config(config, &mut report);
        
        // 验证网络配置
        Self::validate_network_config(config, &mut report);
        
        // 验证数据库配置
        Self::validate_database_config(&config.dm_sql, &mut report);
        
        // 验证存储配置
        Self::validate_storage_config(&config.oss, &mut report);
        
        // 验证第三方配置
        Self::validate_third_party_config(&config.third_party_access, &mut report);
        
        // 验证日志配置
        Self::validate_logging_config(&config.logging, &mut report);
        
        // 验证监控配置
        Self::validate_monitoring_config(&config.monitoring, &mut report);
        
        // 验证故障转移配置
        Self::validate_failover_config(&config.failover, &mut report);
        
        Ok(report)
    }
    
    /// 验证基础配置
    fn validate_basic_config(config: &Config, report: &mut ValidationReport) {
        // 验证端口
        if config.port == 0 || config.port > 65535 {
            report.add_error("port", &format!("无效的端口号: {}", config.port));
        }
        
        // 验证会话超时
        if config.session_timeout <= 0 {
            report.add_error("session_timeout", "会话超时必须大于0");
        }
        
        // 验证应用ID
        if config.app_id.is_empty() {
            report.add_warning("app_id", "应用ID为空，可能影响SSO认证");
        }
    }
    
    /// 验证网络配置
    fn validate_network_config(config: &Config, report: &mut ValidationReport) {
        // 验证主机URL格式
        if !config.host.starts_with("http://") && !config.host.starts_with("https://") {
            report.add_error("host", &format!("无效的主机URL格式: {}", config.host));
        }
        
        // 验证预览URL
        if !config.preview_url.is_empty() {
            if !config.preview_url.starts_with("http://") && !config.preview_url.starts_with("https://") {
                report.add_error("preview_url", &format!("无效的预览URL格式: {}", config.preview_url));
            }
        }
        
        // 验证回调URL
        if !config.callback_url.is_empty() {
            if !config.callback_url.starts_with("http://") && !config.callback_url.starts_with("https://") {
                report.add_error("callback_url", &format!("无效的回调URL格式: {}", config.callback_url));
            }
        }
    }
    
    /// 验证数据库配置
    fn validate_database_config(db_config: &DmSql, report: &mut ValidationReport) {
        // 如果启用数据库
        if !db_config.database_host.is_empty() {
            if db_config.database_port.parse::<u16>().is_err() {
                report.add_error("database_port", "无效的数据库端口号");
            }
            
            if db_config.database_user.is_empty() {
                report.add_error("database_user", "数据库用户名不能为空");
            }
            
            if db_config.database_name.is_empty() {
                report.add_error("database_name", "数据库名称不能为空");
            }
        } else {
            report.add_info("database", "使用SQLite作为默认数据库");
        }
    }
    
    /// 验证存储配置
    fn validate_storage_config(oss_config: &Oss, report: &mut ValidationReport) {
        if !oss_config.access_key.is_empty() {
            // OSS配置验证
            if oss_config.bucket.is_empty() {
                report.add_error("oss_bucket", "OSS桶名称不能为空");
            }
            
            if oss_config.server_url.is_empty() {
                report.add_error("oss_server_url", "OSS服务器URL不能为空");
            }
            
            if oss_config.access_key_secret.is_empty() {
                report.add_error("oss_secret", "OSS密钥不能为空");
            }
            
            if !oss_config.server_url.starts_with("http://") && !oss_config.server_url.starts_with("https://") {
                report.add_error("oss_server_url", "无效的OSS服务器URL格式");
            }
        } else {
            report.add_info("storage", "使用本地存储作为默认存储");
        }
    }
    
    /// 验证第三方配置
    fn validate_third_party_config(config: &ThirdPartyAccessConfig, report: &mut ValidationReport) {
        if config.enabled {
            if config.clients.is_empty() {
                report.add_warning("third_party_clients", "启用了第三方访问但没有配置客户端");
            }
            
            for (index, client) in config.clients.iter().enumerate() {
                let prefix = format!("client_{}", index);
                
                if client.client_id.is_empty() {
                    report.add_error(&format!("{}_id", prefix), "客户端ID不能为空");
                }
                
                if client.secret_key.is_empty() {
                    report.add_error(&format!("{}_secret_key", prefix), "客户端密钥不能为空");
                }
            }
            
            // 验证限流配置
            if config.rate_limiting.requests_per_minute == 0 {
                report.add_warning("rate_limit", "限流设置为0可能导致服务过载");
            }
        }
    }
    
    /// 验证日志配置
    fn validate_logging_config(config: &LoggingConfig, report: &mut ValidationReport) {
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&config.level.as_str()) {
            report.add_error("log_level", &format!("无效的日志级别: {}", config.level));
        }
        
        if config.file.enabled {
            if config.file.retention_days.is_some() && config.file.retention_days.unwrap() == 0 {
                report.add_warning("log_retention", "日志保留天数为0，日志将不会被清理");
            }
        }
    }
    
    /// 验证监控配置
    fn validate_monitoring_config(config: &MonitoringConfig, report: &mut ValidationReport) {
        if config.enabled {
            // 对于简化的监控配置，只验证是否启用
            report.add_info("monitoring", "监控已启用");
        }
    }
    
    /// 验证故障转移配置
    fn validate_failover_config(config: &FailoverConfig, report: &mut ValidationReport) {
        if config.database.enabled && config.database.health_check_interval == 0 {
            report.add_error("db_failover_interval", "数据库故障转移检查间隔不能为0");
        }
        
        if config.storage.enabled {
            let fallback_path = Path::new(&config.storage.local_fallback_dir);
            if !fallback_path.exists() {
                report.add_warning("storage_fallback_path", "存储故障转移目录不存在，将自动创建");
            }
        }
    }
    
    /// 检查配置兼容性
    pub fn check_compatibility(config: &Config) -> Result<CompatibilityReport> {
        let mut report = CompatibilityReport::new();
        
        // 检查调试模式和生产环境的冲突
        if config.runtime_mode.mode == "production" && config.debug.enabled {
            report.add_warning("生产环境启用了调试模式，可能存在安全风险");
        }
        
        // 检查模拟登录在生产环境的使用
        if config.runtime_mode.mode == "production" && config.debug.enable_mock_login {
            report.add_error("生产环境不应启用模拟登录功能");
        }
        
        // 检查第三方访问安全性
        if config.third_party_access.enabled && !config.third_party_access.signature.required {
            report.add_warning("第三方访问未要求签名验证，存在安全风险");
        }
        
        Ok(report)
    }
}

/// 验证报告
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub info: Vec<ValidationIssue>,
}

impl ValidationReport {
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
    
    pub fn is_valid(&self) -> bool {
        !self.has_errors()
    }
}

/// 验证问题
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
}

/// 兼容性报告
#[derive(Debug, Clone)]
pub struct CompatibilityReport {
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
}

impl CompatibilityReport {
    pub fn new() -> Self {
        Self {
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }
    
    pub fn add_warning(&mut self, message: &str) {
        self.warnings.push(message.to_string());
    }
    
    pub fn add_error(&mut self, message: &str) {
        self.errors.push(message.to_string());
    }
    
    pub fn has_issues(&self) -> bool {
        !self.warnings.is_empty() || !self.errors.is_empty()
    }
}