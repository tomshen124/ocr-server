use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub login: Login,
    pub app_id: String,
    pub preview_url: String,
    pub session_timeout: i64,
    pub callback_url: String,
    #[serde(rename = "zhzwdt-oss")]
    pub oss: Oss,
    #[serde(rename = "DMSql")]
    pub dm_sql: DmSql,
    pub approve: Approve,
    pub debug: Debug,
    pub logging: LoggingConfig,
    pub monitoring: MonitoringConfig,
    pub third_party_access: ThirdPartyAccessConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debug {
    pub enabled: bool,
    pub enable_mock_login: bool,
    pub mock_login_warning: bool,
    pub tools_enabled: DebugToolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugToolsConfig {
    pub api_test: bool,
    pub mock_login: bool,
    pub preview_demo: bool,
    pub flow_test: bool,
    pub system_monitor: bool,
    pub data_manager: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approve {
    #[serde(rename = "submit-url")]
    pub submit_url: String,
    #[serde(rename = "access-key")]
    pub access_key: String,
    #[serde(rename = "secret-key")]
    pub secret_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Oss {
    pub root: String,
    pub bucket: String,
    pub server_url: String,
    #[serde(rename = "AccessKey")]
    pub access_key: String,
    #[serde(rename = "AccessKey Secret")]
    pub access_key_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmSql {
    #[serde(rename = "DATABASE_HOST")]
    pub database_host: String,
    #[serde(rename = "DATABASE_PORT")]
    pub database_port: String,
    #[serde(rename = "DATABASE_USER")]
    pub database_user: String,
    #[serde(rename = "DATABASE_PASSWORD")]
    pub database_password: String,
    #[serde(rename = "DATABASE_NAME")]
    pub database_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub access_token_url: String,
    pub get_user_info_url: String,
    pub access_key: String,
    pub secret_key: String,
    #[serde(default = "default_use_callback")]
    pub use_callback: bool,
}

fn default_use_callback() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file: LogFileConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFileConfig {
    pub enabled: bool,
    pub directory: String,
    pub retention_days: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThirdPartyAccessConfig {
    pub enabled: bool,
    pub clients: Vec<ThirdPartyClient>,
    pub signature: SignatureConfig,
    pub rate_limiting: RateLimitingConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThirdPartyClient {
    pub client_id: String,
    pub secret_key: String,
    pub name: String,
    pub enabled: bool,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureConfig {
    pub required: bool,
    pub timestamp_tolerance: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub requests_per_hour: u32,
}

impl Config {
    pub fn read_yaml(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let config_str = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&config_str)?;
        Ok(config)
    }

    pub fn write_yaml(&self, path: &std::path::Path) {
        let config_yaml = serde_yaml::to_string(self).unwrap_or_default();
        fs::write(path, config_yaml).ok();
    }
    
    pub fn write_yaml_to_path(&self, path: &Path) -> anyhow::Result<()> {
        // 确保目录存在
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        
        let yaml_content = serde_yaml::to_string(self)?;
        std::fs::write(path, yaml_content)?;
        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            host: "http://127.0.0.1".to_string(),
            port: 31101,
            login: Login {
                access_token_url: "".to_string(),
                get_user_info_url: "".to_string(),
                access_key: "".to_string(),
                secret_key: "".to_string(),
                use_callback: true,
            },
            app_id: "".to_string(),
            preview_url: "".to_string(),
            session_timeout: 86400,
            callback_url: "".to_string(),
            oss: Oss {
                root: "".to_string(),
                bucket: "".to_string(),
                server_url: "".to_string(),
                access_key: "".to_string(),
                access_key_secret: "".to_string(),
            },
            dm_sql: DmSql {
                database_host: "".to_string(),
                database_port: "".to_string(),
                database_user: "".to_string(),
                database_password: "".to_string(),
                database_name: "".to_string(),
            },
            approve: Approve {
                submit_url: "".to_string(),
                access_key: "".to_string(),
                secret_key: "".to_string(),
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
        }
    }
} 