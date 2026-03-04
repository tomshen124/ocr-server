
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
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
    pub api_enhancement: ApiEnhancementConfig,
    #[serde(default)]
    pub concurrency: Option<ConcurrencyConfig>,
    #[serde(default)]
    pub business_metrics: Option<BusinessMetricsConfig>,
    #[serde(default)]
    pub user_data_encryption: UserDataEncryptionConfig,
    #[serde(default)]
    pub api_call_tracking: Option<ApiCallTrackingConfig>,
    #[serde(default)]
    pub report_export: ReportExportConfig,
    #[serde(default)]
    pub distributed_tracing: Option<DistributedTracingConfig>,
    #[serde(default)]
    pub download_limits: DownloadLimitsConfig,
    #[serde(default)]
    pub ocr_engine: Option<OcrEngineConfig>,
    #[serde(default)]
    pub ocr_tuning: OcrTuningConfig,
    #[serde(default)]
    pub ocr_pool: OcrPoolConfig,
    #[serde(default)]
    pub task_queue: TaskQueueConfig,
    #[serde(default)]
    pub worker_proxy: WorkerProxyConfig,
    #[serde(default)]
    pub distributed: DistributedConfig,
    #[serde(default)]
    pub deployment: DeploymentConfig,
    #[serde(default)]
    pub master: MasterNodeConfig,
    #[serde(default)]
    pub dynamic_worker: Option<crate::util::dynamic_worker::DynamicWorkerConfig>,
    #[serde(default)]
    pub outbox: OutboxConfig,
    #[serde(default)]
    pub service_watchdog: ServiceWatchdogConfig,

    #[serde(default)]
    pub adaptive_concurrency: Option<AdaptiveConcurrencyConfig>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TaskQueueDriver {
    Local,
    Nats,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskQueueConfig {
    #[serde(default = "default_task_queue_driver")]
    pub driver: TaskQueueDriver,
    #[serde(default)]
    pub local: LocalQueueConfig,
    #[serde(default)]
    pub nats: Option<NatsQueueConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalQueueConfig {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatsQueueConfig {
    pub server_url: String,
    #[serde(default = "default_nats_subject")]
    pub subject: String,
    #[serde(default = "default_nats_stream")]
    pub stream: String,
    #[serde(default = "default_nats_durable")]
    pub durable_consumer: String,
    #[serde(default = "default_nats_max_deliver")]
    pub max_deliver: i32,
    #[serde(default = "default_nats_ack_wait_ms")]
    pub ack_wait_ms: u64,
    #[serde(default = "default_nats_max_batch")]
    pub max_batch: usize,
    #[serde(default = "default_nats_pull_wait_ms")]
    pub pull_wait_ms: u64,
    #[serde(default)]
    pub inline_worker: bool,
    #[serde(default)]
    pub tls: Option<NatsTlsConfig>,
    #[serde(default = "default_nats_max_messages")]
    pub max_messages: Option<i64>,
    #[serde(default = "default_nats_max_bytes")]
    pub max_bytes: Option<i64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseQueueConfig {
    pub enabled: bool,
    #[serde(default = "default_db_queue_table")]
    pub table_name: String,
    #[serde(default = "default_db_queue_poll_interval_ms")]
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerProxyConfig {
    #[serde(default)]
    pub workers: Vec<WorkerCredentialConfig>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerCredentialConfig {
    pub worker_id: String,
    pub secret: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DownloadLimitsConfig {
    #[serde(default = "default_max_file_mb")]
    pub max_file_mb: u64,
    #[serde(default = "default_max_pdf_mb")]
    pub max_pdf_mb: u64,
    #[serde(default = "default_pdf_max_pages")]
    pub pdf_max_pages: u32,
    #[serde(default = "default_oversize_action")]
    pub oversize_action: String,
    #[serde(default = "default_pdf_render_dpi")]
    pub pdf_render_dpi: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrEngineConfig {
    pub work_dir: Option<String>,
    pub binary: Option<String>,
    pub lib_path: Option<String>,
    pub timeout_secs: Option<u64>,
}

impl Config {
    pub fn base_url(&self) -> String {
        if let Some(public) = self
            .public_base_url
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return public.trim_end_matches('/').to_string();
        }
        let fallback = if !self.server.host.is_empty() && self.server.port > 0 {
            format!(
                "{}://{}:{}",
                self.server.protocol, self.server.host, self.server.port
            )
        } else {
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

    pub fn callback_url(&self) -> String {
        if !self.callback_url.is_empty() {
            self.callback_url.clone()
        } else {
            format!("{}/api/sso/callback", self.base_url())
        }
    }

    pub fn third_party_callback_url(&self) -> Option<String> {
        self.third_party_callback_url
            .as_ref()
            .map(|url| url.trim().to_string())
            .filter(|url| !url.is_empty())
    }

    pub fn preview_view_url(&self, preview_id: &str) -> String {
        format!("{}/api/preview/view/{}", self.base_url(), preview_id)
    }

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

    pub fn get_port(&self) -> u16 {
        if self.server.port > 0 {
            self.server.port
        } else {
            self.port
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Login {
    pub sso_login_url: String,
    pub access_token_url: String,
    pub get_user_info_url: String,
    pub access_key: String,
    pub secret_key: String,
    pub use_callback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    #[serde(rename = "type")]
    pub database_type: String,

    #[serde(default)]
    pub dm: Option<DmConfig>,

    #[serde(default)]
    pub sqlite: Option<SqliteConfig>,

    #[serde(default)]
    pub go_gateway: Option<GoGatewayConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmConfig {
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,

    #[serde(default = "default_dm_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_dm_connection_timeout")]
    pub connection_timeout: u64,

    #[serde(default)]
    pub go_gateway: Option<GoGatewayConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoGatewayConfig {
    #[serde(default = "default_go_gateway_enabled")]
    pub enabled: bool,
    #[serde(default = "default_go_gateway_url")]
    pub url: String,
    pub api_key: String,
    #[serde(default = "default_go_gateway_timeout")]
    pub timeout: u64,
    #[serde(default = "default_go_gateway_health_check_interval")]
    pub health_check_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SqliteConfig {
    pub path: String,
}

fn default_dm_max_connections() -> u32 {
    10
}
fn default_dm_connection_timeout() -> u64 {
    30
}

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
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub strict_mode: bool,

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

    #[serde(default = "default_connection_timeout")]
    pub connection_timeout: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_retry_delay")]
    pub retry_delay: u64,

    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    #[serde(default = "default_idle_timeout")]
    pub idle_timeout: u64,

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmHealthCheckConfig {
    pub enabled: bool,
    pub interval: u64,
    pub timeout: u64,
    pub failure_threshold: u32,
}

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
pub struct RuntimeModeConfig {
    pub mode: String,
    pub development: DevelopmentConfig,
    pub testing: TestingConfig,
    pub production: ProductionConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopmentConfig {
    pub debug_enabled: bool,
    pub mock_login: bool,
    pub mock_ocr: bool,
    pub test_tools: bool,
    pub auto_login: bool,
    pub detailed_logs: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestingConfig {
    pub mock_data: bool,
    pub mock_delay: u64,
    pub test_scenarios: bool,
    pub performance_test: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProductionConfig {
    pub debug_enabled: bool,
    pub mock_login: bool,
    pub mock_ocr: bool,
    pub test_tools: bool,
    pub security_strict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Debug {
    pub enabled: bool,
    pub tools_enabled: DebugToolsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebugToolsConfig {
    pub api_test: bool,
    pub preview_demo: bool,
    pub flow_test: bool,
    pub system_monitor: bool,
    pub data_manager: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestModeConfig {
    pub enabled: bool,
    pub auto_login: bool,
    pub mock_ocr: bool,
    pub mock_delay: u64,
    pub test_user: TestUserConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestUserConfig {
    pub id: String,
    pub username: String,
    pub email: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file: LogFileConfig,
    pub structured: Option<bool>,
    #[serde(default)]
    pub business_logging: Option<BusinessLoggingConfig>,
    #[serde(default)]
    pub level_config: Option<LevelConfig>,
    #[serde(default)]
    pub attachment_logging: AttachmentLoggingConfig,
    #[serde(default)]
    pub enable_debug_file: bool,
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
    #[serde(default)]
    pub performance: Option<PerformanceConfig>,
    #[serde(default)]
    pub business_metrics: Option<BusinessMetricsMonitorConfig>,
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
    #[serde(default = "default_source_type")]
    pub source_type: String, // "platform_gateway" | "direct_api"
    pub enabled: bool,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureConfig {
    pub required: bool,
    pub timestamp_tolerance: u64,
}

fn default_source_type() -> String {
    "direct_api".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitingConfig {
    pub enabled: bool,
    pub requests_per_minute: u32,
    pub requests_per_hour: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailoverConfig {
    pub database: DatabaseFailoverConfig,
    pub storage: StorageFailoverConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseFailoverConfig {
    pub enabled: bool,
    pub health_check_interval: u64,
    pub max_retries: u32,
    pub retry_delay: u64,
    pub fallback_to_local: bool,
    pub local_data_dir: String,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiEnhancementConfig {
    pub enhanced_error_handling: bool,
    pub trace_id_enabled: bool,
    pub structured_response: bool,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    pub enabled: bool,
    pub metrics_interval: u64,
    pub history_retention: u64,
    pub alert_thresholds: AlertThresholdsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertThresholdsConfig {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub ocr_queue_length: u32,
    pub response_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessMetricsMonitorConfig {
    pub enabled: bool,
    pub track_user_activity: bool,
    pub track_preview_lifecycle: bool,
    pub track_third_party_calls: bool,
    pub error_rate_threshold: f64,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    pub ocr_processing: OcrProcessingConfig,
    pub queue_monitoring: QueueMonitoringConfig,
    pub resource_limits: ResourceLimitsConfig,
    #[serde(default)]
    pub multi_stage: Option<MultiStageConcurrencyConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiStageConcurrencyConfig {
    pub enabled: bool,
    pub download_concurrency: u32,
    pub pdf_conversion_concurrency: u32,
    #[serde(default = "MultiStageConcurrencyConfig::default_pdf_min_concurrency")]
    pub pdf_conversion_min_concurrency: u32,
    #[serde(default = "MultiStageConcurrencyConfig::default_pdf_min_free_mem_mb")]
    pub pdf_min_free_mem_mb: u32,
    #[serde(default = "MultiStageConcurrencyConfig::default_pdf_max_load_one")]
    pub pdf_max_load_one: f64,
    pub ocr_processing_concurrency: u32,
    pub storage_concurrency: u32,
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
            ocr_processing_concurrency: 6,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcePredictorConfig {
    pub enabled: bool,
    pub pdf_memory_base_mb: f64,
    pub pdf_memory_per_page_mb: f64,
    pub ocr_memory_base_mb: f64,
    pub ocr_memory_per_page_mb: f64,
    pub high_risk_memory_threshold_mb: f64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrProcessingConfig {
    pub max_concurrent_tasks: u32,
    pub queue_enabled: bool,
    pub queue_timeout: u64,
    pub retry_on_failure: bool,
    pub max_retries: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMonitoringConfig {
    pub enabled: bool,
    pub status_check_interval: u64,
    pub alert_on_overflow: bool,
    pub max_queue_length: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimitsConfig {
    pub max_memory_per_task: u64,
    pub task_timeout: u64,
    pub cleanup_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessMetricsConfig {
    pub preview_metrics: PreviewMetricsConfig,
    pub system_metrics: SystemMetricsConfig,
    pub integration_metrics: IntegrationMetricsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewMetricsConfig {
    pub track_processing_time: bool,
    pub track_success_rate: bool,
    pub track_user_patterns: bool,
    pub track_theme_usage: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetricsConfig {
    pub cpu_monitoring: bool,
    pub memory_monitoring: bool,
    pub disk_monitoring: bool,
    pub network_monitoring: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationMetricsConfig {
    pub sso_performance: bool,
    pub callback_success_rate: bool,
    pub external_api_latency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDataEncryptionConfig {
    pub enabled: bool,

    pub encryption_key: String,

    pub key_file_path: Option<String>,

    pub key_version: String,

    pub algorithm: String,

    pub force_encrypt_fields: Vec<String>,
}

impl Default for UserDataEncryptionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            encryption_key: "".to_string(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrTuningConfig {
    #[serde(default = "default_true")]
    pub logging_detail: bool,
    #[serde(default = "default_low_conf_threshold")]
    pub low_confidence_threshold: f64,
    #[serde(default = "default_min_char_threshold")]
    pub min_char_threshold: usize,
    #[serde(default)]
    pub retry_enabled: bool,
    #[serde(default = "default_retry_pages_limit")]
    pub retry_pages_limit: usize,
    #[serde(default = "default_retry_dpi_step")]
    pub retry_dpi_step: u32,
    #[serde(default = "default_max_dpi")]
    pub max_dpi: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiCallTrackingConfig {
    pub enabled: bool,

    pub record_headers: bool,

    pub save_to_database: bool,

    pub memory_retention: usize,

    pub tracked_paths: Vec<String>,
}

impl Default for ApiCallTrackingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            record_headers: true,
            save_to_database: false,
            memory_retention: 1000,
            tracked_paths: vec!["/api/preview".to_string()],
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedTracingConfig {
    pub enabled: bool,

    pub sampling_rate: f64,

    pub max_spans: usize,

    pub retention_seconds: u64,

    pub verbose_logging: bool,

    pub http_middleware_enabled: bool,

    pub trace_database_operations: bool,

    pub trace_storage_operations: bool,

    pub trace_ocr_processing: bool,

    #[serde(default)]
    pub metrics_collection: Option<MetricsCollectionConfig>,

    #[serde(default)]
    pub slow_operation_thresholds: Option<SlowOperationThresholdsConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsCollectionConfig {
    pub enabled: bool,
    pub collect_http_metrics: bool,
    pub collect_ocr_metrics: bool,
    pub collect_system_metrics: bool,
    pub collect_business_metrics: bool,
    pub aggregation_interval: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlowOperationThresholdsConfig {
    pub http_request_slow_threshold_ms: u64,
    pub file_download_slow_threshold_ms: u64,
    pub pdf_conversion_slow_threshold_ms: u64,
    pub ocr_processing_slow_threshold_ms: u64,
    pub database_operation_slow_threshold_ms: u64,
}

impl Default for SlowOperationThresholdsConfig {
    fn default() -> Self {
        Self {
            http_request_slow_threshold_ms: 5000,
            file_download_slow_threshold_ms: 10000,
            pdf_conversion_slow_threshold_ms: 30000,
            ocr_processing_slow_threshold_ms: 60000,
            database_operation_slow_threshold_ms: 1000,
        }
    }
}

impl Default for DistributedTracingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sampling_rate: 1.0,
            max_spans: 10000,
            retention_seconds: 3600,
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
