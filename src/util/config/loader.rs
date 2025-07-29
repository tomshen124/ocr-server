//! 配置加载和管理模块
//! 处理配置文件的读取、写入、验证和默认值生成

use super::types::*;
use anyhow::Result;
use std::fs;
use std::path::Path;

/// 配置加载器
pub struct ConfigLoader;

impl ConfigLoader {
    /// 从YAML文件读取配置
    pub fn read_yaml(path: impl AsRef<Path>) -> Result<Config> {
        let config_str = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&config_str)?;
        Ok(config)
    }

    /// 从环境变量读取配置覆盖
    pub fn apply_env_overrides(mut config: Config) -> Config {
        // 主机和端口配置
        if let Ok(host) = std::env::var("OCR_HOST") {
            config.host = host;
        }
        if let Ok(port) = std::env::var("OCR_PORT") {
            if let Ok(port_num) = port.parse::<u16>() {
                config.port = port_num;
            }
        }

        // 数据库配置
        if let Ok(db_host) = std::env::var("DATABASE_HOST") {
            config.dm_sql.database_host = db_host;
        }
        if let Ok(db_port) = std::env::var("DATABASE_PORT") {
            config.dm_sql.database_port = db_port;
        }
        if let Ok(db_user) = std::env::var("DATABASE_USER") {
            config.dm_sql.database_user = db_user;
        }
        if let Ok(db_password) = std::env::var("DATABASE_PASSWORD") {
            config.dm_sql.database_password = db_password;
        }

        // OSS配置
        if let Ok(oss_key) = std::env::var("OSS_ACCESS_KEY") {
            config.oss.access_key = oss_key;
        }
        if let Ok(oss_secret) = std::env::var("OSS_ACCESS_KEY_SECRET") {
            config.oss.access_key_secret = oss_secret;
        }
        if let Ok(oss_bucket) = std::env::var("OSS_BUCKET") {
            config.oss.bucket = oss_bucket;
        }

        // 调试模式
        if let Ok(debug_enabled) = std::env::var("DEBUG_ENABLED") {
            config.debug.enabled = debug_enabled.to_lowercase() == "true";
        }

        config
    }

    /// 验证配置的有效性
    pub fn validate_config(config: &Config) -> Result<()> {
        // 验证端口范围
        if config.port == 0 || config.port > 65535 {
            return Err(anyhow::anyhow!("无效的端口号: {}", config.port));
        }

        // 验证URL格式
        if !config.host.starts_with("http://") && !config.host.starts_with("https://") {
            return Err(anyhow::anyhow!("无效的主机URL格式: {}", config.host));
        }

        // 验证会话超时
        if config.session_timeout <= 0 {
            return Err(anyhow::anyhow!("会话超时必须大于0"));
        }

        // 验证日志级别
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&config.logging.level.as_str()) {
            return Err(anyhow::anyhow!("无效的日志级别: {}", config.logging.level));
        }

        Ok(())
    }

    /// 生成配置模板
    pub fn generate_template() -> Config {
        Config::default()
    }
}

/// 配置写入器
pub struct ConfigWriter;

impl ConfigWriter {
    /// 将配置写入YAML文件
    pub fn write_yaml(config: &Config, path: impl AsRef<Path>) -> Result<()> {
        let yaml_content = serde_yaml::to_string(config)?;
        fs::write(path, yaml_content)?;
        Ok(())
    }

    /// 写入配置到指定路径，确保目录存在
    pub fn write_yaml_with_dir(config: &Config, path: &Path) -> Result<()> {
        // 确保目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let yaml_content = serde_yaml::to_string(config)?;
        std::fs::write(path, yaml_content)?;
        Ok(())
    }

    /// 生成配置示例文件
    pub fn generate_example_config(path: &Path) -> Result<()> {
        let example_config = Self::create_example_config();
        Self::write_yaml_with_dir(&example_config, path)?;
        Ok(())
    }

    /// 生成配置模板
    pub fn generate_template() -> Config {
        Self::create_example_config()
    }

    /// 创建示例配置
    fn create_example_config() -> Config {
        Config {
            host: "http://127.0.0.1".to_string(),
            port: 31101,
            app_id: "your_app_id".to_string(),
            preview_url: "http://127.0.0.1:31101".to_string(),
            session_timeout: 86400,
            callback_url: "http://127.0.0.1:31101/api/sso/callback".to_string(),
            login: Login {
                sso_login_url: "https://your-sso-provider.com/login".to_string(),
                access_token_url: "https://your-sso-provider.com/token".to_string(),
                get_user_info_url: "https://your-sso-provider.com/userinfo".to_string(),
                access_key: "your_access_key".to_string(),
                secret_key: "your_secret_key".to_string(),
                use_callback: true,
            },
            oss: Oss {
                root: "ocr-files".to_string(),
                bucket: "your-bucket".to_string(),
                server_url: "https://your-oss-endpoint.com".to_string(),
                access_key: "".to_string(), // 空值使用本地存储
                access_key_secret: "".to_string(),
            },
            dm_sql: DmSql {
                database_host: "".to_string(), // 空值使用SQLite
                database_port: "5236".to_string(),
                database_user: "SYSDBA".to_string(),
                database_password: "SYSDBA".to_string(),
                database_name: "OCR_DB".to_string(),
            },
            approve: Approve {
                submit_url: "".to_string(),
                access_key: "".to_string(),
                secret_key: "".to_string(),
            },
            runtime_mode: RuntimeModeConfig {
                mode: "development".to_string(),
                development: DevelopmentConfig {
                    debug_enabled: true,
                    mock_login: true,
                    mock_ocr: false,
                    test_tools: true,
                    auto_login: false,
                    detailed_logs: true,
                },
                testing: TestingConfig {
                    mock_data: true,
                    mock_delay: 100,
                    test_scenarios: true,
                    performance_test: false,
                },
                production: ProductionConfig {
                    debug_enabled: false,
                    mock_login: false,
                    mock_ocr: false,
                    test_tools: false,
                    security_strict: true,
                },
            },
            debug: Debug {
                enabled: true,
                enable_mock_login: true,
                mock_login_warning: true,
                tools_enabled: DebugToolsConfig {
                    api_test: true,
                    mock_login: true,
                    preview_demo: true,
                    flow_test: true,
                    system_monitor: true,
                    data_manager: true,
                },
            },
            test_mode: Some(TestModeConfig {
                enabled: true,
                auto_login: true,
                mock_ocr: false,
                mock_delay: 100,
                test_user: TestUserConfig {
                    id: "test_user_001".to_string(),
                    username: "测试用户".to_string(),
                    email: "test@example.com".to_string(),
                    role: "tester".to_string(),
                },
            }),
            logging: LoggingConfig {
                level: "info".to_string(),
                file: LogFileConfig {
                    enabled: true,
                    directory: "runtime/logs".to_string(),
                    retention_days: Some(7),
                },
            },
            monitoring: MonitoringConfig {
                enabled: false,
            },
            third_party_access: ThirdPartyAccessConfig {
                enabled: false,
                clients: vec![
                    ThirdPartyClient {
                        client_id: "demo_client".to_string(),
                        secret_key: "demo_secret_key_change_in_production".to_string(),
                        name: "演示客户端".to_string(),
                        enabled: false,
                        permissions: vec!["preview".to_string(), "query".to_string()],
                    }
                ],
                signature: SignatureConfig {
                    required: true,
                    timestamp_tolerance: 300, // 5分钟
                },
                rate_limiting: RateLimitingConfig {
                    enabled: true,
                    requests_per_minute: 100,
                    requests_per_hour: 1000,
                },
            },
            failover: FailoverConfig {
                database: DatabaseFailoverConfig {
                    enabled: true,
                    health_check_interval: 30,
                    max_retries: 3,
                    retry_delay: 1000,
                    fallback_to_local: true,
                    local_data_dir: "runtime/fallback/db".to_string(),
                },
                storage: StorageFailoverConfig {
                    enabled: true,
                    health_check_interval: 30,
                    max_retries: 3,
                    retry_delay: 1000,
                    auto_switch_to_local: true,
                    sync_when_recovered: true,
                    local_fallback_dir: "runtime/fallback/storage".to_string(),
                },
            },
        }
    }
}

impl Config {
    /// 从文件读取配置，支持环境变量覆盖
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let mut config = ConfigLoader::read_yaml(path)?;
        config = ConfigLoader::apply_env_overrides(config);
        ConfigLoader::validate_config(&config)?;
        Ok(config)
    }

    /// 保存配置到文件
    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        ConfigWriter::write_yaml(self, path)
    }

    /// 获取当前运行模式配置
    pub fn get_current_mode_config(&self) -> RuntimeModeInfo {
        match self.runtime_mode.mode.as_str() {
            "development" => RuntimeModeInfo {
                name: "development".to_string(),
                debug_enabled: self.runtime_mode.development.debug_enabled,
                mock_login: self.runtime_mode.development.mock_login,
                test_tools: self.runtime_mode.development.test_tools,
            },
            "testing" => RuntimeModeInfo {
                name: "testing".to_string(),
                debug_enabled: true,
                mock_login: true,
                test_tools: true,
            },
            "production" => RuntimeModeInfo {
                name: "production".to_string(),
                debug_enabled: self.runtime_mode.production.debug_enabled,
                mock_login: self.runtime_mode.production.mock_login,
                test_tools: self.runtime_mode.production.test_tools,
            },
            _ => RuntimeModeInfo {
                name: "unknown".to_string(),
                debug_enabled: false,
                mock_login: false,
                test_tools: false,
            },
        }
    }

    /// 检查是否启用调试模式
    pub fn is_debug_enabled(&self) -> bool {
        self.debug.enabled || self.get_current_mode_config().debug_enabled
    }

    /// 检查是否启用模拟登录
    pub fn is_mock_login_enabled(&self) -> bool {
        self.debug.enable_mock_login || self.get_current_mode_config().mock_login
    }
}

/// 运行时模式信息
#[derive(Debug, Clone)]
pub struct RuntimeModeInfo {
    pub name: String,
    pub debug_enabled: bool,
    pub mock_login: bool,
    pub test_tools: bool,
}