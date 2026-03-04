use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRequestRecord {
    pub id: String,
    pub third_party_request_id: Option<String>,
    pub user_id: String,
    pub user_info_json: Option<String>,
    pub matter_id: String,
    pub matter_type: String,
    pub matter_name: String,
    pub channel: String,
    pub sequence_no: String,
    pub agent_info_json: Option<String>,
    pub subject_info_json: Option<String>,
    pub form_data_json: Option<String>,
    pub scene_data_json: Option<String>,
    pub material_data_json: Option<String>,
    pub latest_preview_id: Option<String>,
    pub latest_status: Option<PreviewStatus>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct PreviewRequestFilter {
    pub user_id: Option<String>,
    pub matter_id: Option<String>,
    pub channel: Option<String>,
    pub sequence_no: Option<String>,
    pub third_party_request_id: Option<String>,
    pub latest_status: Option<PreviewStatus>,
    pub created_from: Option<DateTime<Utc>>,
    pub created_to: Option<DateTime<Utc>>,
    pub search: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRecord {
    pub id: String,
    pub user_id: String,
    pub user_info_json: Option<String>,
    pub file_name: String,
    pub ocr_text: String,
    pub theme_id: Option<String>,
    pub evaluation_result: Option<String>,
    pub preview_url: String,
    pub preview_view_url: Option<String>,
    pub preview_download_url: Option<String>,
    pub status: PreviewStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub third_party_request_id: Option<String>,
    pub queued_at: Option<DateTime<Utc>>,
    pub processing_started_at: Option<DateTime<Utc>>,
    pub retry_count: i32,
    pub last_worker_id: Option<String>,
    pub last_attempt_id: Option<String>,
    pub failure_reason: Option<String>,
    pub ocr_stderr_summary: Option<String>,
    pub failure_context: Option<String>,
    pub last_error_code: Option<String>,
    pub slow_attachment_info_json: Option<String>,
    pub callback_url: Option<String>,
    pub callback_status: Option<String>,
    pub callback_attempts: i32,
    pub callback_successes: i32,
    pub callback_failures: i32,
    pub last_callback_at: Option<DateTime<Utc>>,
    pub last_callback_status_code: Option<i32>,
    pub last_callback_response: Option<String>,
    pub last_callback_error: Option<String>,
    pub callback_payload: Option<String>,
    pub next_callback_after: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewMaterialResultRecord {
    pub id: String,
    pub preview_id: String,
    pub material_code: String,
    pub material_name: Option<String>,
    pub status: String,
    pub status_code: i32,
    pub processing_status: Option<String>,
    pub issues_count: i32,
    pub warnings_count: i32,
    pub attachments_json: Option<String>,
    pub summary_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRuleResultRecord {
    pub id: String,
    pub preview_id: String,
    pub material_result_id: Option<String>,
    pub material_code: Option<String>,
    pub rule_id: Option<String>,
    pub rule_code: Option<String>,
    pub rule_name: Option<String>,
    pub engine: Option<String>,
    pub severity: Option<String>,
    pub status: Option<String>,
    pub message: Option<String>,
    pub suggestions_json: Option<String>,
    pub evidence_json: Option<String>,
    pub extra_json: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatterRuleConfigRecord {
    pub id: String,
    pub matter_id: String,
    pub matter_name: Option<String>,
    pub spec_version: String,
    pub mode: String,
    pub rule_payload: String,
    pub status: String,
    pub description: Option<String>,
    pub checksum: Option<String>,
    pub updated_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PreviewStatus {
    Pending,
    Queued,
    Processing,
    Completed,
    Failed,
}

impl fmt::Display for PreviewStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreviewStatus::Pending => write!(f, "pending"),
            PreviewStatus::Queued => write!(f, "queued"),
            PreviewStatus::Processing => write!(f, "processing"),
            PreviewStatus::Completed => write!(f, "completed"),
            PreviewStatus::Failed => write!(f, "failed"),
        }
    }
}

impl PreviewStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PreviewStatus::Pending => "pending",
            PreviewStatus::Queued => "queued",
            PreviewStatus::Processing => "processing",
            PreviewStatus::Completed => "completed",
            PreviewStatus::Failed => "failed",
        }
    }
}

impl FromStr for PreviewStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "pending" => Ok(PreviewStatus::Pending),
            "queued" => Ok(PreviewStatus::Queued),
            "processing" => Ok(PreviewStatus::Processing),
            "completed" => Ok(PreviewStatus::Completed),
            "failed" => Ok(PreviewStatus::Failed),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiStats {
    pub id: String,
    pub endpoint: String,
    pub method: String,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub status_code: u16,
    pub response_time_ms: u32,
    pub request_size: u32,
    pub response_size: u32,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct PreviewFilter {
    pub user_id: Option<String>,
    pub status: Option<PreviewStatus>,
    pub theme_id: Option<String>,
    pub third_party_request_id: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct PreviewStatusCounts {
    pub total: u64,
    pub completed: u64,
    pub processing: u64,
    pub failed: u64,
    pub pending: u64,
    pub queued: u64,
}

impl PreviewStatusCounts {
    pub fn record(&mut self, status: &PreviewStatus) {
        self.total += 1;
        match status {
            PreviewStatus::Completed => self.completed += 1,
            PreviewStatus::Processing => self.processing += 1,
            PreviewStatus::Failed => self.failed += 1,
            PreviewStatus::Pending => self.pending += 1,
            PreviewStatus::Queued => self.queued += 1,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct StatsFilter {
    pub endpoint: Option<String>,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[async_trait]
pub trait Database: Send + Sync {
    fn as_any(&self) -> &dyn std::any::Any;

    async fn save_preview_request(&self, request: &PreviewRequestRecord) -> Result<()>;

    async fn get_preview_request(&self, id: &str) -> Result<Option<PreviewRequestRecord>>;

    async fn find_preview_request_by_third_party(
        &self,
        third_party_request_id: &str,
    ) -> Result<Option<PreviewRequestRecord>>;

    async fn update_preview_request_latest(
        &self,
        request_id: &str,
        latest_preview_id: Option<&str>,
        latest_status: Option<PreviewStatus>,
    ) -> Result<()>;

    async fn list_preview_requests(
        &self,
        filter: &PreviewRequestFilter,
    ) -> Result<Vec<PreviewRequestRecord>>;

    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()>;

    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>>;

    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()>;

    async fn update_preview_evaluation_result(
        &self,
        id: &str,
        evaluation_result: &str,
    ) -> Result<()>;

    async fn mark_preview_processing(
        &self,
        id: &str,
        worker_id: &str,
        attempt_id: &str,
    ) -> Result<()>;

    async fn update_preview_artifacts(
        &self,
        id: &str,
        file_name: &str,
        preview_url: &str,
        preview_view_url: Option<&str>,
        preview_download_url: Option<&str>,
    ) -> Result<()>;

    async fn replace_preview_material_results(
        &self,
        preview_id: &str,
        records: &[PreviewMaterialResultRecord],
    ) -> Result<()>;

    async fn replace_preview_rule_results(
        &self,
        preview_id: &str,
        records: &[PreviewRuleResultRecord],
    ) -> Result<()>;

    async fn update_preview_failure_context(&self, update: &PreviewFailureUpdate) -> Result<()>;

    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>>;

    async fn check_and_update_preview_dedup(
        &self,
        fingerprint: &str,
        preview_id: &str,
        meta: &PreviewDedupMeta,
        limit: i32,
    ) -> Result<PreviewDedupDecision>;

    async fn get_preview_status_counts(&self) -> Result<PreviewStatusCounts> {
        let records = self.list_preview_records(&PreviewFilter::default()).await?;
        let mut counts = PreviewStatusCounts::default();
        for rec in records.iter() {
            counts.record(&rec.status);
        }
        Ok(counts)
    }

    async fn find_preview_by_third_party_id(
        &self,
        third_party_id: &str,
        user_id: &str,
    ) -> Result<Option<PreviewRecord>>;

    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()>;

    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>>;

    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary>;

    async fn health_check(&self) -> Result<bool>;

    async fn initialize(&self) -> Result<()>;

    #[allow(clippy::too_many_arguments)]
    async fn save_user_login_record(
        &self,
        user_id: &str,
        user_name: Option<&str>,
        certificate_type: &str,
        certificate_number: Option<&str>,
        phone_number: Option<&str>,
        email: Option<&str>,
        organization_name: Option<&str>,
        organization_code: Option<&str>,
        login_type: &str,
        login_time: &str,
        client_ip: &str,
        user_agent: &str,
        referer: &str,
        cookie_info: &str,
        raw_data: &str,
    ) -> Result<()>;

    async fn upsert_cached_material_record(&self, record: &CachedMaterialRecord) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    async fn update_cached_material_status(
        &self,
        id: &str,
        status: CachedMaterialStatus,
        oss_key: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    async fn list_cached_material_records(
        &self,
        _filter: &CachedMaterialFilter,
    ) -> Result<Vec<CachedMaterialRecord>> {
        Err(anyhow!("cached material persistence not supported"))
    }

    async fn delete_cached_material_record(&self, _id: &str) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    async fn delete_cached_materials_by_preview(&self, _preview_id: &str) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    async fn save_material_file_record(&self, record: &MaterialFileRecord) -> Result<()>;

    async fn update_material_file_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()>;

    async fn update_material_file_processing(
        &self,
        id: &str,
        processed_keys_json: Option<&str>,
        ocr_text_key: Option<&str>,
        ocr_text_length: Option<i64>,
    ) -> Result<()>;

    async fn list_material_files(
        &self,
        filter: &MaterialFileFilter,
    ) -> Result<Vec<MaterialFileRecord>>;

    async fn save_task_payload(&self, preview_id: &str, payload: &str) -> Result<()>;

    async fn load_task_payload(&self, preview_id: &str) -> Result<Option<String>>;

    async fn delete_task_payload(&self, preview_id: &str) -> Result<()>;

    async fn update_preview_callback_state(&self, update: &PreviewCallbackUpdate) -> Result<()>;

    async fn list_due_callbacks(&self, limit: u32) -> Result<Vec<PreviewRecord>>;

    async fn enqueue_outbox_event(&self, event: &NewOutboxEvent) -> Result<()>;

    async fn fetch_pending_outbox_events(&self, limit: u32) -> Result<Vec<OutboxEvent>>;

    async fn mark_outbox_event_applied(&self, event_id: &str) -> Result<()>;

    async fn mark_outbox_event_failed(&self, event_id: &str, error: &str) -> Result<()>;

    async fn get_matter_rule_config(
        &self,
        matter_id: &str,
    ) -> Result<Option<MatterRuleConfigRecord>>;

    async fn upsert_matter_rule_config(&self, config: &MatterRuleConfigRecord) -> Result<()>;

    async fn list_matter_rule_configs(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<MatterRuleConfigRecord>>;


    async fn find_monitor_user_by_username(&self, username: &str) -> Result<Option<MonitorUser>> {
        Err(anyhow!("find_monitor_user_by_username not implemented"))
    }

    async fn get_monitor_user_password_hash(&self, user_id: &str) -> Result<String> {
        Err(anyhow!("get_monitor_user_password_hash not implemented"))
    }

    async fn create_monitor_session(
        &self,
        session_id: &str,
        user_id: &str,
        ip: &str,
        user_agent: &str,
        created_at: &str,
        expires_at: &str,
    ) -> Result<()> {
        Err(anyhow!("create_monitor_session not implemented"))
    }

    async fn find_monitor_session_by_id(&self, session_id: &str) -> Result<Option<MonitorSession>> {
        Err(anyhow!("find_monitor_session_by_id not implemented"))
    }

    async fn update_monitor_login_info(&self, user_id: &str, now: &str) -> Result<()> {
        Err(anyhow!("update_monitor_login_info not implemented"))
    }

    async fn update_monitor_session_activity(&self, session_id: &str, now: &str) -> Result<()> {
        Err(anyhow!("update_monitor_session_activity not implemented"))
    }

    async fn delete_monitor_session(&self, session_id: &str) -> Result<()> {
        Err(anyhow!("delete_monitor_session not implemented"))
    }

    async fn cleanup_expired_monitor_sessions(&self, now: &str) -> Result<u64> {
        Err(anyhow!("cleanup_expired_monitor_sessions not implemented"))
    }

    async fn get_active_monitor_sessions_count(&self, now: &str) -> Result<i64> {
        Err(anyhow!("get_active_monitor_sessions_count not implemented"))
    }

    async fn list_monitor_users(&self) -> Result<Vec<MonitorUser>> {
        Err(anyhow!("list_monitor_users not implemented"))
    }

    async fn create_monitor_user(
        &self,
        id: &str,
        username: &str,
        password_hash: &str,
        role: &str,
        now: &str,
    ) -> Result<()> {
        Err(anyhow!("create_monitor_user not implemented"))
    }

    async fn update_monitor_user_role(&self, user_id: &str, role: &str, now: &str) -> Result<()> {
        Err(anyhow!("update_monitor_user_role not implemented"))
    }

    async fn update_monitor_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
        now: &str,
    ) -> Result<()> {
        Err(anyhow!("update_monitor_user_password not implemented"))
    }

    async fn set_monitor_user_active(
        &self,
        user_id: &str,
        is_active: bool,
        now: &str,
    ) -> Result<()> {
        Err(anyhow!("set_monitor_user_active not implemented"))
    }

    async fn count_active_monitor_admins(&self) -> Result<i64> {
        Err(anyhow!("count_active_monitor_admins not implemented"))
    }

    async fn find_monitor_user_by_id(&self, user_id: &str) -> Result<Option<MonitorUser>> {
        Err(anyhow!("find_monitor_user_by_id not implemented"))
    }


    async fn enqueue_worker_result(&self, preview_id: &str, payload: &str) -> Result<()> {
        Err(anyhow!("enqueue_worker_result not implemented"))
    }

    async fn fetch_pending_worker_results(
        &self,
        limit: u32,
    ) -> Result<Vec<WorkerResultQueueRecord>> {
        Err(anyhow!("fetch_pending_worker_results not implemented"))
    }

    async fn update_worker_result_status(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<()>;

    async fn get_worker_result_by_preview_id(
        &self,
        preview_id: &str,
    ) -> Result<Option<WorkerResultQueueRecord>> {
        Err(anyhow!("get_worker_result_by_preview_id not implemented"))
    }

    // New methods for material download queue:
    async fn enqueue_material_download(&self, preview_id: &str, payload: &str) -> Result<()>;
    async fn fetch_pending_material_downloads(
        &self,
        limit: u32,
    ) -> Result<Vec<MaterialDownloadQueueRecord>>;
    async fn update_material_download_status(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<()>;
    async fn update_material_download_payload(&self, id: &str, payload: &str) -> Result<()>;

    async fn get_download_cache_token(
        &self,
        url: &str,
    ) -> Result<Option<MaterialDownloadCacheEntry>>;
    async fn upsert_download_cache_token(
        &self,
        url: &str,
        token: &str,
        ttl_secs: i64,
    ) -> Result<()>;

    async fn create_preview_share_token(
        &self,
        preview_id: &str,
        token: &str,
        format: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        Err(anyhow!("create_preview_share_token not implemented"))
    }

    async fn consume_preview_share_token(
        &self,
        token: &str,
    ) -> Result<Option<PreviewShareTokenRecord>> {
        Err(anyhow!("consume_preview_share_token not implemented"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialDownloadCacheEntry {
    pub url: String,
    pub token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewShareTokenRecord {
    pub token: String,
    pub preview_id: String,
    pub format: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorUser {
    pub id: String,
    pub username: String,
    pub role: String,
    pub last_login_at: Option<String>,
    pub login_count: i64,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorSession {
    pub id: String,
    pub user_id: String,
    pub user: MonitorUser,
    pub expires_at: String,
    pub ip_address: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSummary {
    pub total_calls: u64,
    pub success_calls: u64,
    pub failed_calls: u64,
    pub avg_response_time_ms: f64,
    pub total_request_size: u64,
    pub total_response_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEvent {
    pub id: String,
    pub table_name: String,
    pub op_type: String,
    pub pk_value: String,
    pub idempotency_key: String,
    pub payload: String,
    pub created_at: DateTime<Utc>,
    pub applied_at: Option<DateTime<Utc>>,
    pub retries: i32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOutboxEvent {
    pub table_name: String,
    pub op_type: String,
    pub pk_value: String,
    pub idempotency_key: String,
    pub payload: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialFileRecord {
    pub id: String,
    pub preview_id: String,
    pub material_code: String,
    pub attachment_name: Option<String>,
    pub source_url: Option<String>,
    pub stored_original_key: String,
    pub stored_processed_keys: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub ocr_text_key: Option<String>,
    pub ocr_text_length: Option<i64>,
    pub status: String,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct MaterialFileFilter {
    pub preview_id: Option<String>,
    pub material_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CachedMaterialStatus {
    Downloaded,
    Uploading,
    Uploaded,
    Cleaned,
    Failed,
}

impl CachedMaterialStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CachedMaterialStatus::Downloaded => "downloaded",
            CachedMaterialStatus::Uploading => "uploading",
            CachedMaterialStatus::Uploaded => "uploaded",
            CachedMaterialStatus::Cleaned => "cleaned",
            CachedMaterialStatus::Failed => "failed",
        }
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "uploading" => CachedMaterialStatus::Uploading,
            "uploaded" => CachedMaterialStatus::Uploaded,
            "cleaned" => CachedMaterialStatus::Cleaned,
            "failed" => CachedMaterialStatus::Failed,
            _ => CachedMaterialStatus::Downloaded,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedMaterialRecord {
    pub id: String,
    pub preview_id: String,
    pub material_code: String,
    pub attachment_index: i32,
    pub token: String,
    pub local_path: String,
    pub upload_status: CachedMaterialStatus,
    pub oss_key: Option<String>,
    pub last_error: Option<String>,
    pub file_size: Option<i64>,
    pub checksum_sha256: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct CachedMaterialFilter {
    pub preview_id: Option<String>,
    pub status: Option<CachedMaterialStatus>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct PreviewCallbackUpdate {
    pub preview_id: String,
    pub callback_url: Option<Option<String>>,
    pub callback_status: Option<Option<String>>,
    pub callback_attempts: Option<i32>,
    pub callback_successes: Option<i32>,
    pub callback_failures: Option<i32>,
    pub last_callback_at: Option<Option<DateTime<Utc>>>,
    pub last_callback_status_code: Option<Option<i32>>,
    pub last_callback_response: Option<Option<String>>,
    pub last_callback_error: Option<Option<String>>,
    pub callback_payload: Option<Option<String>>,
    pub next_callback_after: Option<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, Default)]
pub struct PreviewFailureUpdate {
    pub preview_id: String,
    pub failure_reason: Option<Option<String>>,
    pub failure_context: Option<Option<String>>,
    pub last_error_code: Option<Option<String>>,
    pub slow_attachment_info_json: Option<Option<String>>,
    pub ocr_stderr_summary: Option<Option<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResultQueueRecord {
    pub id: String,
    pub preview_id: String,
    pub payload: String,
    pub status: String, // e.g., "pending", "processing", "completed", "failed"
    pub attempts: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialDownloadQueueRecord {
    pub id: String,
    pub preview_id: String,
    pub payload: String,
    pub status: String, // e.g., "pending", "processing", "completed", "failed"
    pub attempts: i32,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewDedupMeta {
    pub user_id: String,
    pub matter_id: String,
    pub third_party_request_id: Option<String>,
    pub payload_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PreviewDedupDecision {
    Allowed {
        repeat_count: i32,
    },
    ReuseExisting {
        preview_id: String,
        repeat_count: i32,
    },
}

