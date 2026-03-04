
use crate::util::config::Config;
use crate::util::log::{cleanup_old_logs, log_init_with_config};
use anyhow::Result;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use tracing_appender::non_blocking::WorkerGuard;

pub struct ConfigManager;

impl ConfigManager {
    pub fn load_and_validate() -> Result<(Config, ConfigValidationReport)> {
        info!("[clipboard] 开始加载配置文件...");

        let config_path = Self::find_config_file_path("config.yaml");
        info!("配置文件路径: {}", config_path.display());

        let config = match Self::load_config_from_file(&config_path) {
            Ok(config) => config,
            Err(e) => {
                warn!("[warn] 配置文件读取失败: {} - {}", config_path.display(), e);
                Self::handle_config_load_failure(&config_path)?
            }
        };

        let validation_report = Self::validate_config(&config)?;

        let config = Self::apply_environment_overrides(config);

        info!("[ok] 配置加载完成");
        Ok((config, validation_report))
    }

    pub fn initialize_logging(config: &Config) -> Result<WorkerGuard> {
        info!("[note] 初始化日志系统...");

        let log_guard = log_init_with_config("logs", "ocr-server", config.logging.clone())?
            .ok_or_else(|| anyhow::anyhow!("日志系统初始化失败"))?;

        if let Some(retention_days) = config.logging.file.retention_days {
            if config.logging.file.enabled {
                let log_path = Path::new(&config.logging.file.directory);
                if let Err(e) = cleanup_old_logs(log_path, retention_days) {
                    warn!("日志清理失败: {}", e);
                } else {
                    info!("[ok] 日志清理完成，保留 {} 天", retention_days);
                }
            }
        }

        info!("[ok] 日志系统初始化完成");
        Ok(log_guard)
    }

    pub fn find_config_file_path(filename: &str) -> PathBuf {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let exe_path = std::env::current_exe().ok();

        let config_in_current = current_dir.join("config").join(filename);
        if config_in_current.exists() {
            return config_in_current;
        }

        if let Some(parent) = current_dir.parent() {
            let config_in_parent = parent.join("config").join(filename);
            if config_in_parent.exists() {
                return config_in_parent;
            }
        }

        if let Some(exe_path) = exe_path {
            if let Some(exe_dir) = exe_path.parent() {
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

        let dev_path = current_dir.join(filename);
        if dev_path.exists() {
            return dev_path;
        }

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

    fn load_config_from_file(config_path: &Path) -> Result<Config> {
        crate::util::config::loader::ConfigLoader::load_with_env_overrides(config_path)
    }

    fn handle_config_load_failure(config_path: &Path) -> Result<Config> {
        if !config_path.exists() {
            info!("[note] 创建默认配置文件: {}", config_path.display());
            let config = Config::default();
            if let Err(write_err) = config.write_yaml_to_path(config_path) {
                warn!("[fail] 创建默认配置文件失败: {}", write_err);
            }
            Ok(config)
        } else {
            warn!("[fail] 配置文件存在但无法解析，请检查语法");
            Err(anyhow::anyhow!(
                "配置文件解析失败: {}",
                config_path.display()
            ))
        }
    }

    fn validate_config(config: &Config) -> Result<ConfigValidationReport> {
        let mut report = ConfigValidationReport::new();

        Self::validate_basic_settings(config, &mut report);

        Self::validate_network_settings(config, &mut report);

        Self::validate_database_settings(config, &mut report);

        Self::validate_storage_settings(config, &mut report);

        Self::validate_third_party_settings(config, &mut report);

        Self::validate_monitoring_settings(config, &mut report);

        if report.has_errors() {
            warn!("[warn] 配置验证发现错误: {}", report.error_count());
            for error in &report.errors {
                warn!("  - {}: {}", error.field, error.message);
            }
        }

        if report.has_warnings() {
            info!("ℹ 配置验证发现警告: {}", report.warning_count());
            for warning in &report.warnings {
                info!("  - {}: {}", warning.field, warning.message);
            }
        }

        Ok(report)
    }

    fn validate_basic_settings(config: &Config, report: &mut ConfigValidationReport) {
        if config.get_port() == 0 {
            report.add_error("port", "端口不能为0");
        }

        if config.app_id.is_empty() {
            report.add_warning("app_id", "应用ID为空，可能影响SSO认证");
        }

        if config.session_timeout <= 0 {
            report.add_error("session_timeout", "会话超时必须大于0");
        }
    }

    fn validate_network_settings(config: &Config, report: &mut ConfigValidationReport) {
        let base_url = config.base_url();

        if base_url.is_empty() {
            report.add_error("server", "服务器配置不能为空");
        } else if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
            report.add_error("server.protocol", "服务器协议必须是http或https");
        }

        if !config.host.is_empty()
            && !config.host.starts_with("http://")
            && !config.host.starts_with("https://")
        {
            report.add_warning("host", "建议使用新的server配置格式，旧的host配置已废弃");
        }

        if !config.preview_url.is_empty()
            && !config.preview_url.starts_with("http://")
            && !config.preview_url.starts_with("https://")
        {
            report.add_warning(
                "preview_url",
                "建议使用新的server配置格式，preview_url将自动生成",
            );
        }
    }

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
                    report.add_warning(
                        &format!("client_{}_permissions", index),
                        "客户端没有配置权限",
                    );
                }
            }
        }
    }

    fn validate_monitoring_settings(config: &Config, report: &mut ConfigValidationReport) {
        if config.monitoring.enabled {
            report.add_info("monitoring", "监控功能已启用");
        } else {
            report.add_info("monitoring", "监控功能已禁用");
        }
    }

    fn apply_environment_overrides(config: Config) -> Config {
        info!("[ok] 环境变量覆盖已由智能配置加载器处理");
        config
    }
}

#[derive(Debug, Clone)]
pub struct ConfigValidationReport {
    pub errors: Vec<ValidationIssue>,
    pub warnings: Vec<ValidationIssue>,
    pub info: Vec<ValidationIssue>,
}

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
