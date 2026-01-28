//! 配置结构定义模块
//! 包含系统配置的所有数据结构

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;

static BASE_URL_PLACEHOLDER_WARNED: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_internal_host(host: &str) -> bool {
    if matches!(host, "0.0.0.0" | "127.0.0.1" | "localhost" | "::1") {
        return true;
    }

    host.parse::<IpAddr>()
        .map(|ip| match ip {
            IpAddr::V4(v4) => {
                v4.is_private() || v4.is_loopback() || v4.is_link_local() || v4.is_unspecified()
            }
            IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unspecified(),
        })
        .unwrap_or(false)
}

/// 主配置结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // 兼容旧配置字段，但标记为deprecated
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub preview_url: String,
    #[serde(default)]
    pub callback_url: String,
    #[serde(default)]
    pub third_party_callback_url: Option<String>,
    #[serde(default)]
    pub public_base_url: Option<String>,

    // 新的服务器配置
    #[serde(default)]
    pub server: ServerConfig,

    pub login: Login,
    pub app_id: String,
    pub session_timeout: i64,
    #[serde(rename = "zhzwdt-oss")]
    pub oss: Oss,
    #[serde(rename = "DMSql")]
    #[serde(default)]
    pub dm_sql: DmSql,

    // [brain] 新增：智能数据库配置 (v2024.12)
    #[serde(default)]
    pub database: Option<DatabaseConfig>,
    pub approve: Approve,
    pub runtime_mode: RuntimeModeConfig,
    pub debug: Debug,
    pub test_mode: Option<TestModeConfig>,
    pub logging: LoggingConfig,
    pub monitoring: MonitoringConfig,
    pub third_party_access: ThirdPartyAccessConfig,
    pub failover: FailoverConfig,
    pub api_enhancement: ApiEnhancementConfig, // 新增：API增强功能开关
    #[serde(default)]
    pub concurrency: Option<ConcurrencyConfig>, // 新增：并发控制配置
    #[serde(default)]
    pub business_metrics: Option<BusinessMetricsConfig>, // 新增：业务指标配置
    #[serde(default)]
    pub user_data_encryption: UserDataEncryptionConfig, // NEW 用户数据加密配置
    #[serde(default)]
    pub api_call_tracking: Option<ApiCallTrackingConfig>, // NEW API调用记录配置
    #[serde(default)]
    pub report_export: ReportExportConfig, // NEW 报表导出开关
    #[serde(default)]
    pub distributed_tracing: Option<DistributedTracingConfig>, // [search] 分布式链路追踪配置
    #[serde(default)]
    pub download_limits: DownloadLimitsConfig, // NEW 下载/转换限制配置
    #[serde(default)]
    pub ocr_engine: Option<OcrEngineConfig>, // NEW 本地OCR引擎配置（可选）
    #[serde(default)]
    pub ocr_tuning: OcrTuningConfig, // NEW OCR质量调优（阈值/重试策略/日志）
    #[serde(default)]
    pub ocr_pool: OcrPoolConfig, // NEW OCR池配置
    #[serde(default)]
    pub task_queue: TaskQueueConfig, // NEW 任务队列配置
    #[serde(default)]
    pub worker_proxy: WorkerProxyConfig, // NEW Worker代理配置
    #[serde(default)]
    pub distributed: DistributedConfig, // NEW 分布式配置
    #[serde(default)]
    pub deployment: DeploymentConfig, // NEW 节点部署角色配置
    #[serde(default)]
    pub master: MasterNodeConfig, // NEW 主节点专用配置
    #[serde(default)]
    pub dynamic_worker: Option<crate::util::dynamic_worker::DynamicWorkerConfig>, // [target] 动态Worker管理配置
    #[serde(default)]
    pub outbox: OutboxConfig, // NEW Outbox事件队列配置
    #[serde(default)]
    pub service_watchdog: ServiceWatchdogConfig, // NEW 服务看门狗配置

    #[serde(default)]
    pub adaptive_concurrency: Option<AdaptiveConcurrencyConfig>, // NEW 自适应并发配置
}

/// 节点部署模式配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentConfig {
    #[serde(default = "default_deployment_role")]
    pub role: DeploymentRole,
    #[serde(default = "default_node_id")]
    pub node_id: String,
    #[serde(default)]
    pub cluster: Option<DeploymentClusterConfig>,
    #[serde(default)]
    pub worker: Option<WorkerDeploymentConfig>,
}

impl Default for DeploymentConfig {
    fn default() -> Self {
        Self {
            role: DeploymentRole::Standalone,
            node_id: default_node_id(),
            cluster: None,
            worker: None,
        }
    }
}

fn default_deployment_role() -> DeploymentRole {
    DeploymentRole::Standalone
}

fn default_node_id() -> String {
    "node-01".to_string()
}

/// 节点角色定义
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum DeploymentRole {
    Standalone,
    Master,
    Worker,
    Hybrid,
}

impl Default for DeploymentRole {
    fn default() -> Self {
        DeploymentRole::Standalone
    }
}

/// Outbox 事件队列配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxConfig {
    #[serde(default = "default_outbox_enabled")]
    pub enabled: bool,
    #[serde(default = "default_outbox_poll_interval")]
    pub poll_interval_secs: u64,
    #[serde(default = "default_outbox_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_outbox_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_outbox_error_len")]
    pub max_error_len: usize,
}

impl Default for OutboxConfig {
    fn default() -> Self {
        Self {
            enabled: default_outbox_enabled(),
            poll_interval_secs: default_outbox_poll_interval(),
            batch_size: default_outbox_batch_size(),
            max_retries: default_outbox_max_retries(),
            max_error_len: default_outbox_error_len(),
        }
    }
}

fn default_outbox_enabled() -> bool {
    true
}

fn default_outbox_poll_interval() -> u64 {
    5
}

fn default_outbox_batch_size() -> u32 {
    50
}

fn default_outbox_max_retries() -> u32 {
    10
}

fn default_outbox_error_len() -> usize {
    512
}

/// 集群配置（预留扩展）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentClusterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_cluster_discovery")]
    pub discovery: String,
    #[serde(default)]
    pub static_nodes: Vec<String>,
}

impl Default for DeploymentClusterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            discovery: default_cluster_discovery(),
            static_nodes: Vec::new(),
        }
    }
}

fn default_cluster_discovery() -> String {
    "static".to_string()
}

/// Worker 专属配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerDeploymentConfig {
    pub id: String,
    pub secret: String,
    pub master_url: String,
    #[serde(default)]
    pub capabilities: Option<WorkerCapabilitiesConfig>,
    #[serde(default)]
    pub heartbeat_interval_secs: Option<u64>,
    #[serde(default)]
    pub rule_cache: WorkerRuleCacheConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCapabilitiesConfig {
    #[serde(default = "default_worker_concurrency")]
    pub max_concurrent: u32,
    #[serde(default = "default_worker_file_size")]
    pub max_file_size_mb: u32,
}

impl Default for WorkerCapabilitiesConfig {
    fn default() -> Self {
        Self {
            max_concurrent: default_worker_concurrency(),
            max_file_size_mb: default_worker_file_size(),
        }
    }
}

fn default_worker_concurrency() -> u32 {
    4
}

fn default_worker_file_size() -> u32 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRuleCacheConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_worker_rule_cache_ttl")]
    pub ttl_secs: u64,
    #[serde(default = "default_worker_rule_cache_capacity")]
    pub max_entries: usize,
}

impl Default for WorkerRuleCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ttl_secs: default_worker_rule_cache_ttl(),
            max_entries: default_worker_rule_cache_capacity(),
        }
    }
}

fn default_worker_rule_cache_ttl() -> u64 {
    900
}

fn default_worker_rule_cache_capacity() -> usize {
    256
}

/// 任务队列驱动类型
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskQueueDriver {
    /// 使用本地内存通道，适合单机部署或开发环境
    Local,
    /// 使用 NATS JetStream 作为任务队列，支持多节点分布式处理
    Nats,
    /// 使用数据库表模拟任务队列（预留，尚未实现）
    Database,
}

impl Default for TaskQueueDriver {
    fn default() -> Self {
        TaskQueueDriver::Local
    }
}

fn default_task_queue_driver() -> TaskQueueDriver {
    TaskQueueDriver::Local
}

/// 任务队列配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQueueConfig {
    /// 队列驱动类型，默认本地内存实现
    #[serde(default = "default_task_queue_driver")]
    pub driver: TaskQueueDriver,
    /// 本地队列配置
    #[serde(default)]
    pub local: LocalQueueConfig,
    /// NATS JetStream 队列配置
    #[serde(default)]
    pub nats: Option<NatsQueueConfig>,
    /// 数据库队列配置（预留）
    #[serde(default)]
    pub database: Option<DatabaseQueueConfig>,
}

impl Default for TaskQueueConfig {
    fn default() -> Self {
        Self {
            driver: TaskQueueDriver::Local,
            local: LocalQueueConfig::default(),
            nats: None,
            database: None,
        }
    }
}

/// 本地任务队列配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalQueueConfig {
    /// Tokio 通道容量，默认 128
    #[serde(default = "default_local_channel_capacity")]
    pub channel_capacity: usize,
}

fn default_local_channel_capacity() -> usize {
    128
}

impl Default for LocalQueueConfig {
    fn default() -> Self {
        Self {
            channel_capacity: default_local_channel_capacity(),
        }
    }
}

/// NATS JetStream 队列配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsQueueConfig {
    /// NATS 服务器地址，例如 nats://127.0.0.1:4222
    pub server_url: String,
    /// 发布/订阅科目（subject），默认 ocr.preview
    #[serde(default = "default_nats_subject")]
    pub subject: String,
    /// JetStream Stream 名称
    #[serde(default = "default_nats_stream")]
    pub stream: String,
    /// JetStream 耐久消费者名称
    #[serde(default = "default_nats_durable")]
    pub durable_consumer: String,
    /// 单条消息最大投递次数，超过后进入死信
    #[serde(default = "default_nats_max_deliver")]
    pub max_deliver: i32,
    /// Ack 等待时长（毫秒）
    #[serde(default = "default_nats_ack_wait_ms")]
    pub ack_wait_ms: u64,
    /// 每次批量拉取的最大消息数量
    #[serde(default = "default_nats_max_batch")]
    pub max_batch: usize,
    /// 拉取请求的等待时间（毫秒）
    #[serde(default = "default_nats_pull_wait_ms")]
    pub pull_wait_ms: u64,
    /// 是否在主节点进程内同时运行 worker（仅用于开发/回放）
    #[serde(default)]
    pub inline_worker: bool,
    /// TLS 配置
    #[serde(default)]
    pub tls: Option<NatsTlsConfig>,
    /// Stream 最大消息数，None 表示不限制
    #[serde(default = "default_nats_max_messages")]
    pub max_messages: Option<i64>,
    /// Stream 最大存储字节数，None 表示不限制
    #[serde(default = "default_nats_max_bytes")]
    pub max_bytes: Option<i64>,
    /// 消息最大保留时长（秒），None 表示不限制
    #[serde(default = "default_nats_max_age_seconds")]
    pub max_age_seconds: Option<u64>,
}

fn default_nats_subject() -> String {
    "ocr.preview".to_string()
}

fn default_nats_stream() -> String {
    "OCR_PREVIEW".to_string()
}

fn default_nats_durable() -> String {
    "ocr-preview-workers".to_string()
}

fn default_nats_max_deliver() -> i32 {
    5
}

fn default_nats_ack_wait_ms() -> u64 {
    60_000
}

fn default_nats_max_batch() -> usize {
    8
}

fn default_nats_pull_wait_ms() -> u64 {
    1_000
}

fn default_nats_max_messages() -> Option<i64> {
    Some(10_000)
}

fn default_nats_max_bytes() -> Option<i64> {
    Some(1_073_741_824) // 1 GiB
}

fn default_nats_max_age_seconds() -> Option<u64> {
    Some(3_600) // 1 hour
}

impl Default for NatsQueueConfig {
    fn default() -> Self {
        Self {
            server_url: "nats://127.0.0.1:4222".to_string(),
            subject: default_nats_subject(),
            stream: default_nats_stream(),
            durable_consumer: default_nats_durable(),
            max_deliver: default_nats_max_deliver(),
            ack_wait_ms: default_nats_ack_wait_ms(),
            max_batch: default_nats_max_batch(),
            pull_wait_ms: default_nats_pull_wait_ms(),
            inline_worker: false,
            tls: None,
            max_messages: default_nats_max_messages(),
            max_bytes: default_nats_max_bytes(),
            max_age_seconds: default_nats_max_age_seconds(),
        }
    }
}

/// NATS TLS 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsTlsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub ca_file: Option<String>,
    #[serde(default)]
    pub client_cert: Option<String>,
    #[serde(default)]
    pub client_key: Option<String>,
    #[serde(default = "default_true")]
    pub require_tls: bool,
}

impl Default for NatsTlsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ca_file: None,
            client_cert: None,
            client_key: None,
            require_tls: true,
        }
    }
}

/// 数据库任务队列配置（预留）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseQueueConfig {
    /// 是否启用数据库队列
    pub enabled: bool,
    /// 队列表名称
    #[serde(default = "default_db_queue_table")]
    pub table_name: String,
    /// 轮询间隔（毫秒）
    #[serde(default = "default_db_queue_poll_interval_ms")]
    pub poll_interval_ms: u64,
}

/// Worker代理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerProxyConfig {
    /// 允许的worker列表
    #[serde(default)]
    pub workers: Vec<WorkerCredentialConfig>,
    /// 允许的最大时间漂移（秒）
    #[serde(default = "default_worker_clock_skew")]
    pub max_clock_skew_seconds: i64,
}

impl Default for WorkerProxyConfig {
    fn default() -> Self {
        Self {
            workers: Vec::new(),
            max_clock_skew_seconds: default_worker_clock_skew(),
        }
    }
}

fn default_worker_clock_skew() -> i64 {
    60
}

/// Worker凭证配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCredentialConfig {
    pub worker_id: String,
    pub secret: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// 分布式运行配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedConfig {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for DistributedConfig {
    fn default() -> Self {
        Self { enabled: false }
    }
}

fn default_db_queue_table() -> String {
    "preview_queue".to_string()
}

fn default_db_queue_poll_interval_ms() -> u64 {
    1_000
}

/// 主节点专用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterNodeConfig {
    #[serde(default = "default_material_cache_dir")]
    pub material_cache_dir: String,
    #[serde(default = "default_temp_pdf_dir")]
    pub temp_pdf_dir: String,
    #[serde(default = "default_temp_pdf_ttl_hours")]
    pub temp_pdf_ttl_hours: u64,
    #[serde(default = "default_material_token_ttl")]
    pub material_token_ttl_secs: u64,
    #[serde(default)]
    pub processing_watchdog: ProcessingWatchdogConfig,
    #[serde(default)]
    pub worker_fallback: WorkerFallbackConfig,
    #[serde(default)]
    pub adaptive_limits: AdaptiveConcurrencyConfig,
    #[serde(default)]
    pub background_processing: BackgroundProcessingConfig,
}

impl Default for MasterNodeConfig {
    fn default() -> Self {
        Self {
            material_cache_dir: default_material_cache_dir(),
            temp_pdf_dir: default_temp_pdf_dir(),
            temp_pdf_ttl_hours: default_temp_pdf_ttl_hours(),
            material_token_ttl_secs: default_material_token_ttl(),
            processing_watchdog: ProcessingWatchdogConfig::default(),
            worker_fallback: WorkerFallbackConfig::default(),
            adaptive_limits: AdaptiveConcurrencyConfig::default(),
            background_processing: BackgroundProcessingConfig::default(),
        }
    }
}

fn default_material_cache_dir() -> String {
    "runtime/cache/materials".to_string()
}

fn default_temp_pdf_dir() -> String {
    "runtime/temp_pdfs".to_string()
}

fn default_temp_pdf_ttl_hours() -> u64 {
    24
}

fn default_material_token_ttl() -> u64 {
    3600
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerFallbackConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_worker_fallback_keywords")]
    pub trigger_keywords: Vec<String>,
    #[serde(default = "default_worker_fallback_max_attempts")]
    pub max_attempts: u32,
}

impl Default for WorkerFallbackConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_keywords: default_worker_fallback_keywords(),
            max_attempts: default_worker_fallback_max_attempts(),
        }
    }
}

fn default_worker_fallback_keywords() -> Vec<String> {
    vec![
        "ocr引擎".to_string(),
        "ocr失败".to_string(),
        "ocr pool circuit open".to_string(),
        "ocr引擎无响应".to_string(),
    ]
}

fn default_worker_fallback_max_attempts() -> u32 {
    1
}

/// 后台处理任务（结果处理/材料下载）配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundProcessingConfig {
    #[serde(default)]
    pub result_processor: BackgroundWorkerConfig,
    #[serde(default)]
    pub material_downloader: BackgroundWorkerConfig,
}

impl Default for BackgroundProcessingConfig {
    fn default() -> Self {
        Self {
            result_processor: BackgroundWorkerConfig::default(),
            material_downloader: BackgroundWorkerConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundWorkerConfig {
    #[serde(default = "default_background_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_background_concurrency")]
    pub max_concurrency: u32,
    #[serde(default = "default_background_idle_backoff_ms")]
    pub idle_backoff_ms: u64,
    #[serde(default = "default_background_max_backoff_ms")]
    pub max_backoff_ms: u64,
    #[serde(default = "default_background_max_attempts")]
    pub max_attempts: u32,
}

impl Default for BackgroundWorkerConfig {
    fn default() -> Self {
        Self {
            batch_size: default_background_batch_size(),
            max_concurrency: default_background_concurrency(),
            idle_backoff_ms: default_background_idle_backoff_ms(),
            max_backoff_ms: default_background_max_backoff_ms(),
            max_attempts: default_background_max_attempts(),
        }
    }
}

fn default_background_batch_size() -> u32 {
    10
}

fn default_background_concurrency() -> u32 {
    10
}

fn default_background_idle_backoff_ms() -> u64 {
    100
}

fn default_background_max_backoff_ms() -> u64 {
    1_000
}

fn default_background_max_attempts() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingWatchdogConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_watchdog_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_watchdog_timeout")]
    pub timeout_secs: u64,
    #[serde(default = "default_watchdog_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_watchdog_worker_grace")]
    pub worker_grace_secs: u64,
}

impl Default for ProcessingWatchdogConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_watchdog_interval(),
            timeout_secs: default_watchdog_timeout(),
            max_retries: default_watchdog_max_retries(),
            worker_grace_secs: default_watchdog_worker_grace(),
        }
    }
}

fn default_watchdog_interval() -> u64 {
    60
}

fn default_watchdog_timeout() -> u64 {
    900
}

fn default_watchdog_max_retries() -> u32 {
    3
}

fn default_watchdog_worker_grace() -> u64 {
    180
}

/// 服务看门狗配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceWatchdogConfig {
    #[serde(default = "default_service_watchdog_enabled")]
    pub enabled: bool,
    #[serde(default = "default_service_watchdog_interval")]
    pub interval_secs: u64,
    #[serde(default = "default_service_watchdog_cpu")]
    pub cpu_threshold_percent: f64,
    #[serde(default = "default_service_watchdog_memory")]
    pub memory_threshold_percent: f64,
    #[serde(default = "default_service_watchdog_disk")]
    pub disk_threshold_percent: f64,
    #[serde(default = "default_service_watchdog_auto_restart")]
    pub auto_restart_on_violation: bool,
    #[serde(default = "default_service_watchdog_violation_limit")]
    pub max_consecutive_violations: u32,
    #[serde(default = "default_service_watchdog_restart_cooldown")]
    pub restart_cooldown_secs: u64,
    #[serde(default = "default_service_watchdog_exit_code")]
    pub restart_exit_code: i32,
}

impl Default for ServiceWatchdogConfig {
    fn default() -> Self {
        Self {
            enabled: default_service_watchdog_enabled(),
            interval_secs: default_service_watchdog_interval(),
            cpu_threshold_percent: default_service_watchdog_cpu(),
            memory_threshold_percent: default_service_watchdog_memory(),
            disk_threshold_percent: default_service_watchdog_disk(),
            auto_restart_on_violation: default_service_watchdog_auto_restart(),
            max_consecutive_violations: default_service_watchdog_violation_limit(),
            restart_cooldown_secs: default_service_watchdog_restart_cooldown(),
            restart_exit_code: default_service_watchdog_exit_code(),
        }
    }
}

fn default_service_watchdog_enabled() -> bool {
    true
}

fn default_service_watchdog_interval() -> u64 {
    30
}

fn default_service_watchdog_cpu() -> f64 {
    90.0
}

fn default_service_watchdog_memory() -> f64 {
    90.0
}

fn default_service_watchdog_disk() -> f64 {
    90.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdaptiveConcurrencyConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_adaptive_check_interval")]
    pub check_interval_secs: u64,
    #[serde(default = "default_adaptive_cpu_high")]
    pub cpu_high_percent: f64,
    #[serde(default = "default_adaptive_memory_high")]
    pub memory_high_percent: f64,
    #[serde(default = "default_adaptive_load_high")]
    pub load_high_threshold: f64,
    #[serde(default = "default_adaptive_min_submission")]
    pub min_submission_permits: usize,
    #[serde(default = "default_adaptive_min_download")]
    pub min_download_permits: usize,
    #[serde(default = "default_adaptive_min_ocr")]
    pub min_ocr_permits: usize,
}

impl Default for AdaptiveConcurrencyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            check_interval_secs: default_adaptive_check_interval(),
            cpu_high_percent: default_adaptive_cpu_high(),
            memory_high_percent: default_adaptive_memory_high(),
            load_high_threshold: default_adaptive_load_high(),
            min_submission_permits: default_adaptive_min_submission(),
            min_download_permits: default_adaptive_min_download(),
            min_ocr_permits: default_adaptive_min_ocr(),
        }
    }
}

fn default_adaptive_check_interval() -> u64 {
    5
}

fn default_adaptive_cpu_high() -> f64 {
    85.0
}

fn default_adaptive_memory_high() -> f64 {
    85.0
}

fn default_adaptive_load_high() -> f64 {
    6.0
}

fn default_adaptive_min_submission() -> usize {
    8
}

fn default_adaptive_min_download() -> usize {
    8
}

fn default_adaptive_min_ocr() -> usize {
    2
}

fn default_service_watchdog_auto_restart() -> bool {
    true
}

fn default_service_watchdog_violation_limit() -> u32 {
    5
}

fn default_service_watchdog_restart_cooldown() -> u64 {
    300
}

fn default_service_watchdog_exit_code() -> i32 {
    16
}

impl Default for DatabaseQueueConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            table_name: default_db_queue_table(),
            poll_interval_ms: default_db_queue_poll_interval_ms(),
        }
    }
}

/// 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8964,
            protocol: "http".to_string(),
        }
    }
}

fn default_protocol() -> String {
    "http".to_string()
}

/// 下载与转换限制配置
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DownloadLimitsConfig {
    /// 通用文件最大大小（MB）
    #[serde(default = "default_max_file_mb")]
    pub max_file_mb: u64,
    /// PDF 文件最大大小（MB），优先于通用限制
    #[serde(default = "default_max_pdf_mb")]
    pub max_pdf_mb: u64,
    /// PDF 最大页数（超过后按策略处理）
    #[serde(default = "default_pdf_max_pages")]
    pub pdf_max_pages: u32,
    /// 超限策略（已废弃，默认reject）
    #[serde(default = "default_oversize_action")]
    pub oversize_action: String,
    /// PDF 渲染 DPI（影响清晰度与内存占用）
    #[serde(default = "default_pdf_render_dpi")]
    pub pdf_render_dpi: u32,
    /// PDF 渲染 JPEG 质量（1-100）
    #[serde(default = "default_pdf_jpeg_quality")]
    pub pdf_jpeg_quality: u8,
}

fn default_max_file_mb() -> u64 {
    40
}
fn default_max_pdf_mb() -> u64 {
    40
}
fn default_pdf_max_pages() -> u32 {
    100
}
fn default_oversize_action() -> String {
    "reject".to_string()
}
fn default_pdf_render_dpi() -> u32 {
    150
}
fn default_pdf_jpeg_quality() -> u8 {
    85
}

/// 本地OCR引擎配置（可选）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrEngineConfig {
    /// 引擎工作的目录（包含 PaddleOCR-json, lib/, models/）
    pub work_dir: Option<String>,
    /// 二进制路径（默认 work_dir/PaddleOCR-json）
    pub binary: Option<String>,
    /// 依赖库目录（默认 work_dir/lib）
    pub lib_path: Option<String>,
    /// 超时时间（秒），默认 10
    pub timeout_secs: Option<u64>,
}

impl Config {
    /// 获取基础URL
    pub fn base_url(&self) -> String {
        if let Some(public) = self
            .public_base_url
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return public.trim_end_matches('/').to_string();
        }
        // 如果新配置存在且有效，使用新配置
        let fallback = if !self.server.host.is_empty() && self.server.port > 0 {
            format!(
                "{}://{}:{}",
                self.server.protocol, self.server.host, self.server.port
            )
        } else {
            // 兼容旧配置
            format!("{}:{}", self.host.trim_end_matches('/'), self.port)
        };

        let warn_needed = Url::parse(&fallback)
            .ok()
            .and_then(|url| url.host_str().map(is_internal_host))
            .unwrap_or(false);

        if warn_needed
            && BASE_URL_PLACEHOLDER_WARNED
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            tracing::warn!(
                target: "config",
                base_url = %fallback,
                "public_base_url 未配置或不可用，基础URL仍为内网地址，外部访问可能失败"
            );
        }

        fallback
    }

    /// 获取回调URL
    pub fn callback_url(&self) -> String {
        // 如果旧配置中有callback_url且不为空，使用旧配置（兼容性）
        if !self.callback_url.is_empty() {
            self.callback_url.clone()
        } else {
            format!("{}/api/sso/callback", self.base_url())
        }
    }

    /// 获取第三方结果回调URL（如果已配置）
    pub fn third_party_callback_url(&self) -> Option<String> {
        self.third_party_callback_url
            .as_ref()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty())
    }

    /// 获取预审视图URL
    pub fn preview_view_url(&self, preview_id: &str) -> String {
        format!("{}/api/preview/view/{}", self.base_url(), preview_id)
    }

    /// 获取服务器监听地址
    pub fn server_address(&self) -> String {
        if !self.server.host.is_empty() && self.server.port > 0 {
            format!("{}:{}", self.server.host, self.server.port)
        } else {
            format!(
                "{}:{}",
                self.host.split("://").last().unwrap_or(&self.host),
                self.port
            )
        }
    }

    /// 获取服务器端口
    pub fn get_port(&self) -> u16 {
        if self.server.port > 0 {
            self.server.port
        } else {
            self.port
        }
    }
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

/// [brain] 智能数据库配置 (v2024.12)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// 数据库类型: "sqlite", "dm", "smart"
    #[serde(rename = "type")]
    pub database_type: String,

    /// 达梦数据库配置
    #[serde(default)]
    pub dm: Option<DmConfig>,

    /// SQLite数据库配置  
    #[serde(default)]
    pub sqlite: Option<SqliteConfig>,

    /// Go网关配置（用于达梦数据库）
    #[serde(default)]
    pub go_gateway: Option<GoGatewayConfig>,
}

/// 达梦数据库连接配置 (简化版)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,

    /// 连接池配置
    #[serde(default = "default_dm_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_dm_connection_timeout")]
    pub connection_timeout: u64,

    /// Go网关配置
    #[serde(default)]
    pub go_gateway: Option<GoGatewayConfig>,
}

/// Go网关配置
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// SQLite数据库配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConfig {
    pub path: String,
}

// 默认值函数
fn default_dm_max_connections() -> u32 {
    10
}
fn default_dm_connection_timeout() -> u64 {
    30
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
    /// 达梦数据库总开关（false时使用SQLite降级）
    #[serde(default)]
    pub enabled: bool,
    /// 严格模式：true时连接失败则服务启动失败  
    #[serde(default)]
    pub strict_mode: bool,

    // 基础连接配置
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

    // 连接超时和重试配置
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_delay: u64,

    // 连接池配置
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,

    // 健康检查配置
    #[serde(default)]
    pub health_check: Option<DmHealthCheckConfig>,
}

impl Default for DmSql {
    fn default() -> Self {
        Self {
            enabled: false,
            strict_mode: false,
            database_host: "".to_string(),
            database_port: "5237".to_string(),
            database_user: "SYSDBA".to_string(),
            database_password: "SYSDBA".to_string(),
            database_name: "OCR_DB".to_string(),
            connection_timeout: default_connection_timeout(),
            max_retries: default_max_retries(),
            retry_delay: default_retry_delay(),
            max_connections: default_max_connections(),
            min_connections: default_min_connections(),
            idle_timeout: default_idle_timeout(),
            health_check: None,
        }
    }
}

/// 达梦数据库健康检查配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmHealthCheckConfig {
    pub enabled: bool,
    pub interval: u64,
    pub timeout: u64,
    pub failure_threshold: u32,
}

// 默认值函数
fn default_connection_timeout() -> u64 {
    30
}
fn default_max_retries() -> u32 {
    3
}
fn default_retry_delay() -> u64 {
    1000
}
fn default_max_connections() -> u32 {
    10
}
fn default_min_connections() -> u32 {
    2
}
fn default_idle_timeout() -> u64 {
    600
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

/// 调试配置 - 简化版本，移除多余mock选项
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debug {
    pub enabled: bool,
    // 移除enable_mock_login和mock_login_warning - 由debug ticket代替
    pub tools_enabled: DebugToolsConfig,
}

/// 调试工具配置 - 简化版本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugToolsConfig {
    pub api_test: bool,
    // 移除mock_login - 由debug ticket代替
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
    pub structured: Option<bool>, // 新增：是否启用结构化日志
    #[serde(default)]
    pub business_logging: Option<BusinessLoggingConfig>,
    #[serde(default)]
    pub level_config: Option<LevelConfig>,
    #[serde(default)]
    pub attachment_logging: AttachmentLoggingConfig,
    #[serde(default)]
    pub enable_debug_file: bool,
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
    #[serde(default)]
    pub performance: Option<PerformanceConfig>,
    #[serde(default)]
    pub business_metrics: Option<BusinessMetricsMonitorConfig>,
}

/// 第三方访问配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThirdPartyAccessConfig {
    pub enabled: bool,
    pub clients: Vec<ThirdPartyClient>,
    pub signature: SignatureConfig,
    pub rate_limiting: RateLimitingConfig,
}

/// 第三方客户端配置 - 简化版本
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThirdPartyClient {
    pub client_id: String,
    pub secret_key: String,
    pub name: String,
    #[serde(default = "default_source_type")]
    pub source_type: String, // "platform_gateway" | "direct_api"
    pub enabled: bool,
    #[serde(default)]
    pub permissions: Vec<String>, // 权限字段保留但设为可选，向后兼容
}

/// 签名配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureConfig {
    pub required: bool,
    pub timestamp_tolerance: u64,
}

fn default_source_type() -> String {
    "direct_api".to_string()
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

/// API功能增强配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEnhancementConfig {
    pub enhanced_error_handling: bool, // 启用增强错误处理
    pub trace_id_enabled: bool,        // 启用请求追踪
    pub structured_response: bool,     // 启用结构化响应
}

// ============= 新增配置结构体 =============

/// 性能监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub enabled: bool,
    pub metrics_interval: u64,
    pub history_retention: u64,
    pub alert_thresholds: AlertThresholdsConfig,
}

/// 告警阈值配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThresholdsConfig {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub ocr_queue_length: u32,
    pub response_time_ms: u64,
}

/// 业务指标监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessMetricsMonitorConfig {
    pub enabled: bool,
    pub track_user_activity: bool,
    pub track_preview_lifecycle: bool,
    pub track_third_party_calls: bool,
    pub error_rate_threshold: f64,
}

/// 业务日志配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessLoggingConfig {
    pub trace_id_enabled: bool,
    pub user_actions: bool,
    pub preview_lifecycle: bool,
    pub third_party_interactions: bool,
    pub performance_metrics: bool,
    pub error_context: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentLoggingConfig {
    #[serde(default = "AttachmentLoggingConfig::default_enabled")]
    pub enabled: bool,
    #[serde(default = "AttachmentLoggingConfig::default_sampling_rate")]
    pub sampling_rate: u32,
    #[serde(default = "AttachmentLoggingConfig::default_slow_threshold")]
    pub slow_threshold_ms: u64,
}

impl AttachmentLoggingConfig {
    const fn default_enabled() -> bool {
        true
    }

    const fn default_sampling_rate() -> u32 {
        5
    }

    const fn default_slow_threshold() -> u64 {
        5_000
    }
}

impl Default for AttachmentLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: Self::default_enabled(),
            sampling_rate: Self::default_sampling_rate(),
            slow_threshold_ms: Self::default_slow_threshold(),
        }
    }
}

/// 日志级别配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LevelConfig {
    #[serde(default)]
    pub api: Option<String>,
    #[serde(default)]
    pub business: Option<String>,
    #[serde(default)]
    pub system: Option<String>,
    #[serde(default)]
    pub security: Option<String>,
    #[serde(default)]
    pub overrides: HashMap<String, String>,
}

/// 并发控制配置 - 扩展为多阶段支持
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    pub ocr_processing: OcrProcessingConfig,
    pub queue_monitoring: QueueMonitoringConfig,
    pub resource_limits: ResourceLimitsConfig,
    /// 多阶段并发控制配置
    #[serde(default)]
    pub multi_stage: Option<MultiStageConcurrencyConfig>,
}

/// 多阶段并发控制配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiStageConcurrencyConfig {
    /// 是否启用多阶段并发控制
    pub enabled: bool,
    /// 下载阶段并发数
    pub download_concurrency: u32,
    /// PDF转换阶段并发数
    pub pdf_conversion_concurrency: u32,
    /// PDF转换阶段最小并发（资源紧张时降至该值）
    #[serde(default = "MultiStageConcurrencyConfig::default_pdf_min_concurrency")]
    pub pdf_conversion_min_concurrency: u32,
    /// 提升并发所需的最小可用内存（MB）
    #[serde(default = "MultiStageConcurrencyConfig::default_pdf_min_free_mem_mb")]
    pub pdf_min_free_mem_mb: u32,
    /// 降档阈值的最大1分钟load（大于则降档）
    #[serde(default = "MultiStageConcurrencyConfig::default_pdf_max_load_one")]
    pub pdf_max_load_one: f64,
    /// OCR处理阶段并发数
    pub ocr_processing_concurrency: u32,
    /// 存储阶段并发数
    pub storage_concurrency: u32,
    /// 资源预测器配置
    #[serde(default)]
    pub resource_predictor: ResourcePredictorConfig,
}

impl Default for MultiStageConcurrencyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            download_concurrency: 12,
            pdf_conversion_concurrency: 3,
            pdf_conversion_min_concurrency: 1,
            pdf_min_free_mem_mb: 2048,
            pdf_max_load_one: 1.5,
            ocr_processing_concurrency: 6, // 与全局OCR信号量保持一致
            storage_concurrency: 10,
            resource_predictor: ResourcePredictorConfig::default(),
        }
    }
}

impl MultiStageConcurrencyConfig {
    fn default_pdf_min_concurrency() -> u32 {
        1
    }
    fn default_pdf_min_free_mem_mb() -> u32 {
        2048
    }
    fn default_pdf_max_load_one() -> f64 {
        1.5
    }
}

/// 资源预测器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePredictorConfig {
    /// 是否启用智能资源预测
    pub enabled: bool,
    /// PDF转换内存基数（每MB文件使用的内存，MB）
    pub pdf_memory_base_mb: f64,
    /// PDF转换内存乘数（每页使用的内存，MB）
    pub pdf_memory_per_page_mb: f64,
    /// OCR处理内存基数（每MB文件使用的内存，MB）
    pub ocr_memory_base_mb: f64,
    /// OCR处理内存乘数（每页使用的内存，MB）
    pub ocr_memory_per_page_mb: f64,
    /// 高风险任务内存阈值（MB）
    pub high_risk_memory_threshold_mb: f64,
    /// 临界风险任务内存阈值（MB）
    pub critical_risk_memory_threshold_mb: f64,
}

impl Default for ResourcePredictorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            pdf_memory_base_mb: 500.0,
            pdf_memory_per_page_mb: 100.0,
            ocr_memory_base_mb: 200.0,
            ocr_memory_per_page_mb: 80.0,
            high_risk_memory_threshold_mb: 4000.0,     // 4GB
            critical_risk_memory_threshold_mb: 6000.0, // 6GB
        }
    }
}

/// OCR处理配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrProcessingConfig {
    pub max_concurrent_tasks: u32,
    pub queue_enabled: bool,
    pub queue_timeout: u64,
    pub retry_on_failure: bool,
    pub max_retries: u32,
}

/// 队列监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMonitoringConfig {
    pub enabled: bool,
    pub status_check_interval: u64,
    pub alert_on_overflow: bool,
    pub max_queue_length: u32,
}

/// 资源限制配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimitsConfig {
    pub max_memory_per_task: u64,
    pub task_timeout: u64,
    pub cleanup_interval: u64,
}

/// 业务指标配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessMetricsConfig {
    pub preview_metrics: PreviewMetricsConfig,
    pub system_metrics: SystemMetricsConfig,
    pub integration_metrics: IntegrationMetricsConfig,
}

/// 预审指标配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewMetricsConfig {
    pub track_processing_time: bool,
    pub track_success_rate: bool,
    pub track_user_patterns: bool,
    pub track_theme_usage: bool,
}

/// 系统指标配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetricsConfig {
    pub cpu_monitoring: bool,
    pub memory_monitoring: bool,
    pub disk_monitoring: bool,
    pub network_monitoring: bool,
}

/// 集成指标配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationMetricsConfig {
    pub sso_performance: bool,
    pub callback_success_rate: bool,
    pub external_api_latency: bool,
}

/// [lock] 用户数据加密配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDataEncryptionConfig {
    /// 是否启用用户数据加密
    pub enabled: bool,

    /// 加密密钥 (32字节hex字符串，或密钥文件路径)
    pub encryption_key: String,

    /// 加密密钥文件路径 (优先级高于encryption_key)
    pub key_file_path: Option<String>,

    /// 密钥版本标识 (用于密钥轮换)
    pub key_version: String,

    /// 加密算法标识
    pub algorithm: String,

    /// 强制加密的字段列表
    pub force_encrypt_fields: Vec<String>,
}

impl Default for UserDataEncryptionConfig {
    fn default() -> Self {
        Self {
            enabled: true,                  // [locked] 默认启用加密
            encryption_key: "".to_string(), // 需要在配置中指定
            key_file_path: None,
            key_version: "v1".to_string(),
            algorithm: "AES-256-GCM".to_string(),
            force_encrypt_fields: vec![
                "user_name".to_string(),
                "certificate_number".to_string(),
                "phone_number".to_string(),
                "email".to_string(),
            ],
        }
    }
}

/// [lab] OCR质量调优配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrTuningConfig {
    /// 是否启用详细日志（记录每批次/页面的关键参数）
    #[serde(default = "default_true")]
    pub logging_detail: bool,
    /// 低置信度阈值（平均score低于该值将提示调高DPI或重试）
    #[serde(default = "default_low_conf_threshold")]
    pub low_confidence_threshold: f64,
    /// 最小字符数阈值（过低提示质量不足）
    #[serde(default = "default_min_char_threshold")]
    pub min_char_threshold: usize,
    /// 是否启用低质页面的二次尝试（按步进提升DPI）
    #[serde(default)]
    pub retry_enabled: bool,
    /// 单请求允许重试的最大页面数
    #[serde(default = "default_retry_pages_limit")]
    pub retry_pages_limit: usize,
    /// 每次重试提升的DPI步进
    #[serde(default = "default_retry_dpi_step")]
    pub retry_dpi_step: u32,
    /// 最大允许的DPI（不超过该值）
    #[serde(default = "default_max_dpi")]
    pub max_dpi: u32,
    /// 预热引擎数量（服务启动时预创建），0 表示不预热
    #[serde(default = "default_prewarm_engines")]
    pub prewarm_engines: u32,
}

fn default_true() -> bool {
    true
}
fn default_low_conf_threshold() -> f64 {
    0.60
}
fn default_min_char_threshold() -> usize {
    16
}
fn default_retry_pages_limit() -> usize {
    3
}
fn default_retry_dpi_step() -> u32 {
    60
}
fn default_max_dpi() -> u32 {
    300
}
fn default_prewarm_engines() -> u32 {
    2
}

impl Default for OcrTuningConfig {
    fn default() -> Self {
        Self {
            logging_detail: true,
            low_confidence_threshold: default_low_conf_threshold(),
            min_char_threshold: default_min_char_threshold(),
            retry_enabled: false,
            retry_pages_limit: default_retry_pages_limit(),
            retry_dpi_step: default_retry_dpi_step(),
            max_dpi: default_max_dpi(),
            prewarm_engines: default_prewarm_engines(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrPoolConfig {
    #[serde(default = "default_ocr_pool_max_engines")]
    pub max_engines: usize,
}

impl Default for OcrPoolConfig {
    fn default() -> Self {
        Self {
            max_engines: default_ocr_pool_max_engines(),
        }
    }
}

const fn default_ocr_pool_max_engines() -> usize {
    6
}

/// NEW API调用记录配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCallTrackingConfig {
    /// 是否启用API调用记录
    pub enabled: bool,

    /// 是否记录详细请求头
    pub record_headers: bool,

    /// 是否记录到数据库
    pub save_to_database: bool,

    /// 内存中保留的记录数量
    pub memory_retention: usize,

    /// 需要记录的API路径模式
    pub tracked_paths: Vec<String>,
}

impl Default for ApiCallTrackingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            record_headers: true,
            save_to_database: false, // 默认不存数据库
            memory_retention: 1000,
            tracked_paths: vec!["/api/preview".to_string()],
        }
    }
}

/// 报表导出/回传配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportExportConfig {
    #[serde(default)]
    pub enable_approve_pdf: bool,
}

impl Default for ReportExportConfig {
    fn default() -> Self {
        Self {
            enable_approve_pdf: false,
        }
    }
}

/// [search] 分布式链路追踪配置 - 扩展版
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedTracingConfig {
    /// 是否启用分布式追踪
    pub enabled: bool,

    /// 采样率 (0.0-1.0)
    pub sampling_rate: f64,

    /// 最大span数量
    pub max_spans: usize,

    /// 追踪数据保留时间（秒）
    pub retention_seconds: u64,

    /// 是否启用详细日志
    pub verbose_logging: bool,

    /// 是否启用HTTP中间件追踪
    pub http_middleware_enabled: bool,

    /// 是否追踪数据库操作
    pub trace_database_operations: bool,

    /// 是否追踪存储操作
    pub trace_storage_operations: bool,

    /// 是否追踪OCR处理过程
    pub trace_ocr_processing: bool,

    /// 指标收集配置
    #[serde(default)]
    pub metrics_collection: Option<MetricsCollectionConfig>,

    /// 慢操作阈值配置
    #[serde(default)]
    pub slow_operation_thresholds: Option<SlowOperationThresholdsConfig>,
}

/// 指标收集配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsCollectionConfig {
    /// 是否启用指标收集
    pub enabled: bool,
    /// 是否收集HTTP请求指标
    pub collect_http_metrics: bool,
    /// 是否收集OCR处理指标
    pub collect_ocr_metrics: bool,
    /// 是否收集系统资源指标
    pub collect_system_metrics: bool,
    /// 是否收集业务指标
    pub collect_business_metrics: bool,
    /// 指标聚合间隔（秒）
    pub aggregation_interval: u64,
    /// 内存中保留的最大指标数量
    pub max_metrics_in_memory: usize,
}

impl Default for MetricsCollectionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collect_http_metrics: true,
            collect_ocr_metrics: true,
            collect_system_metrics: true,
            collect_business_metrics: true,
            aggregation_interval: 60,
            max_metrics_in_memory: 10000,
        }
    }
}

/// 慢操作阈值配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowOperationThresholdsConfig {
    /// HTTP请求慢操作阈值（毫秒）
    pub http_request_slow_threshold_ms: u64,
    /// 文件下载慢操作阈值（毫秒）
    pub file_download_slow_threshold_ms: u64,
    /// PDF转换慢操作阈值（毫秒）
    pub pdf_conversion_slow_threshold_ms: u64,
    /// OCR处理慢操作阈值（毫秒）
    pub ocr_processing_slow_threshold_ms: u64,
    /// 数据库操作慢操作阈值（毫秒）
    pub database_operation_slow_threshold_ms: u64,
}

impl Default for SlowOperationThresholdsConfig {
    fn default() -> Self {
        Self {
            http_request_slow_threshold_ms: 5000,       // 5秒
            file_download_slow_threshold_ms: 10000,     // 10秒
            pdf_conversion_slow_threshold_ms: 30000,    // 30秒
            ocr_processing_slow_threshold_ms: 60000,    // 60秒
            database_operation_slow_threshold_ms: 1000, // 1秒
        }
    }
}

impl Default for DistributedTracingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sampling_rate: 1.0, // 开发环境100%采样
            max_spans: 10000,
            retention_seconds: 3600, // 1小时
            verbose_logging: false,
            http_middleware_enabled: true,
            trace_database_operations: true,
            trace_storage_operations: true,
            trace_ocr_processing: true,
            metrics_collection: Some(MetricsCollectionConfig::default()),
            slow_operation_thresholds: Some(SlowOperationThresholdsConfig::default()),
        }
    }
}
