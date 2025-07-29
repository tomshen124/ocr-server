//! 配置结构定义模块
//! 包含系统配置的所有数据结构

use serde::{Deserialize, Serialize};

/// 主配置结构
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
    pub runtime_mode: RuntimeModeConfig,
    pub debug: Debug,
    pub test_mode: Option<TestModeConfig>,
    pub logging: LoggingConfig,
    pub monitoring: MonitoringConfig,
    pub third_party_access: ThirdPartyAccessConfig,
    pub failover: FailoverConfig,
}

/// 登录配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub sso_login_url: String,
    pub access_token_url: String,
    pub get_user_info_url: String,
    pub access_key: String,
    pub secret_key: String,
    pub use_callback: bool,
}

/// OSS存储配置
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

/// 达梦数据库配置
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

/// 审批配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Approve {
    #[serde(rename = "submit-url")]
    pub submit_url: String,
    #[serde(rename = "access-key")]
    pub access_key: String,
    #[serde(rename = "secret-key")]
    pub secret_key: String,
}

/// 运行时模式配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeModeConfig {
    pub mode: String,
    pub development: DevelopmentConfig,
    pub testing: TestingConfig,
    pub production: ProductionConfig,
}

/// 开发环境配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopmentConfig {
    pub debug_enabled: bool,
    pub mock_login: bool,
    pub mock_ocr: bool,
    pub test_tools: bool,
    pub auto_login: bool,
    pub detailed_logs: bool,
}

/// 测试环境配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestingConfig {
    pub mock_data: bool,
    pub mock_delay: u64,
    pub test_scenarios: bool,
    pub performance_test: bool,
}

/// 生产环境配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionConfig {
    pub debug_enabled: bool,
    pub mock_login: bool,
    pub mock_ocr: bool,
    pub test_tools: bool,
    pub security_strict: bool,
}

/// 调试配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debug {
    pub enabled: bool,
    pub enable_mock_login: bool,
    pub mock_login_warning: bool,
    pub tools_enabled: DebugToolsConfig,
}

/// 调试工具配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugToolsConfig {
    pub api_test: bool,
    pub mock_login: bool,
    pub preview_demo: bool,
    pub flow_test: bool,
    pub system_monitor: bool,
    pub data_manager: bool,
}

/// 测试模式配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestModeConfig {
    pub enabled: bool,
    pub auto_login: bool,
    pub mock_ocr: bool,
    pub mock_delay: u64,
    pub test_user: TestUserConfig,
}

/// 测试用户配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestUserConfig {
    pub id: String,
    pub username: String,
    pub email: String,
    pub role: String,
}

/// 日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file: LogFileConfig,
}

/// 日志文件配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFileConfig {
    pub enabled: bool,
    pub directory: String,
    pub retention_days: Option<u32>,
}

/// 监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringConfig {
    pub enabled: bool,
}

/// 第三方访问配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThirdPartyAccessConfig {
    pub enabled: bool,
    pub clients: Vec<ThirdPartyClient>,
    pub signature: SignatureConfig,
    pub rate_limiting: RateLimitingConfig,
}

/// 第三方客户端配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThirdPartyClient {
    pub client_id: String,
    pub secret_key: String,
    pub name: String,
    pub enabled: bool,
    pub permissions: Vec<String>,
}

/// 签名配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureConfig {
    pub required: bool,
    pub timestamp_tolerance: u64,
}

/// 限流配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub requests_per_hour: u32,
}

/// 故障转移配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    pub database: DatabaseFailoverConfig,
    pub storage: StorageFailoverConfig,
}

/// 数据库故障转移配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseFailoverConfig {
    pub enabled: bool,
    pub health_check_interval: u64,
    pub max_retries: u32,
    pub retry_delay: u64,
    pub fallback_to_local: bool,
    pub local_data_dir: String,
}

/// 存储故障转移配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageFailoverConfig {
    pub enabled: bool,
    pub health_check_interval: u64,
    pub max_retries: u32,
    pub retry_delay: u64,
    pub auto_switch_to_local: bool,
    pub sync_when_recovered: bool,
    pub local_fallback_dir: String,
}