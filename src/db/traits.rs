use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

/// 预审请求记录
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

/// 预审请求查询过滤条件
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

/// 预审记录
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

/// 预审材料评估结果记录
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

/// 预审规则评估结果记录
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

/// 事项规则配置记录
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

/// 预审状态
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

/// API调用统计
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

/// 预审记录过滤条件
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

/// 预审状态计数
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

/// API统计过滤条件
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

/// 数据库操作trait
#[async_trait]
pub trait Database: Send + Sync {
    /// 类型转换方法 - 用于访问具体实现的方法
    fn as_any(&self) -> &dyn std::any::Any;

    /// 保存预审请求基础信息
    async fn save_preview_request(&self, request: &PreviewRequestRecord) -> Result<()>;

    /// 获取预审请求
    async fn get_preview_request(&self, id: &str) -> Result<Option<PreviewRequestRecord>>;

    /// 根据第三方请求ID查询预审请求
    async fn find_preview_request_by_third_party(
        &self,
        third_party_request_id: &str,
    ) -> Result<Option<PreviewRequestRecord>>;

    /// 更新预审请求的最新结果指针
    async fn update_preview_request_latest(
        &self,
        request_id: &str,
        latest_preview_id: Option<&str>,
        latest_status: Option<PreviewStatus>,
    ) -> Result<()>;

    /// 查询预审请求列表
    async fn list_preview_requests(
        &self,
        filter: &PreviewRequestFilter,
    ) -> Result<Vec<PreviewRequestRecord>>;

    /// 保存预审记录
    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()>;

    /// 获取预审记录
    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>>;

    /// 更新预审状态
    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()>;

    /// 更新预审的evaluation_result字段
    async fn update_preview_evaluation_result(
        &self,
        id: &str,
        evaluation_result: &str,
    ) -> Result<()>;

    /// 标记预审任务进入Processing状态，并记录worker/attempt信息
    async fn mark_preview_processing(
        &self,
        id: &str,
        worker_id: &str,
        attempt_id: &str,
    ) -> Result<()>;

    /// 更新预审记录的文件信息（文件名、访问URL等）
    async fn update_preview_artifacts(
        &self,
        id: &str,
        file_name: &str,
        preview_url: &str,
        preview_view_url: Option<&str>,
        preview_download_url: Option<&str>,
    ) -> Result<()>;

    /// 替换预审材料评估结果
    async fn replace_preview_material_results(
        &self,
        preview_id: &str,
        records: &[PreviewMaterialResultRecord],
    ) -> Result<()>;

    /// 替换预审规则评估结果
    async fn replace_preview_rule_results(
        &self,
        preview_id: &str,
        records: &[PreviewRuleResultRecord],
    ) -> Result<()>;

    /// 更新预审失败上下文信息（失败原因、错误码、慢附件等）
    async fn update_preview_failure_context(&self, update: &PreviewFailureUpdate) -> Result<()>;

    /// 查询预审记录列表
    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>>;

    /// 预审去重：记录指纹并判断是否需要复用已有结果（仅 DM 实现，SQLite 返回允许）
    async fn check_and_update_preview_dedup(
        &self,
        fingerprint: &str,
        preview_id: &str,
        meta: &PreviewDedupMeta,
        limit: i32,
    ) -> Result<PreviewDedupDecision>;

    /// 获取预审状态计数（默认实现基于 list_preview_records，可由具体实现覆盖优化）
    async fn get_preview_status_counts(&self) -> Result<PreviewStatusCounts> {
        let records = self.list_preview_records(&PreviewFilter::default()).await?;
        let mut counts = PreviewStatusCounts::default();
        for rec in records.iter() {
            counts.record(&rec.status);
        }
        Ok(counts)
    }

    /// 根据第三方请求ID和用户ID查找预审记录
    async fn find_preview_by_third_party_id(
        &self,
        third_party_id: &str,
        user_id: &str,
    ) -> Result<Option<PreviewRecord>>;

    /// 保存API调用统计
    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()>;

    /// 查询API统计数据
    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>>;

    /// 获取API调用汇总统计
    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary>;

    /// 健康检查
    async fn health_check(&self) -> Result<bool>;

    /// 初始化数据库（创建表等）
    async fn initialize(&self) -> Result<()>;

    /// 保存用户登录审计记录
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

    /// 写入材料缓存记录
    async fn upsert_cached_material_record(&self, record: &CachedMaterialRecord) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    /// 更新材料缓存状态
    async fn update_cached_material_status(
        &self,
        id: &str,
        status: CachedMaterialStatus,
        oss_key: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    /// 查询材料缓存记录
    async fn list_cached_material_records(
        &self,
        _filter: &CachedMaterialFilter,
    ) -> Result<Vec<CachedMaterialRecord>> {
        Err(anyhow!("cached material persistence not supported"))
    }

    /// 删除单个材料缓存记录
    async fn delete_cached_material_record(&self, _id: &str) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    /// 清理指定预审的缓存记录
    async fn delete_cached_materials_by_preview(&self, _preview_id: &str) -> Result<()> {
        Err(anyhow!("cached material persistence not supported"))
    }

    /// 保存材料文件记录
    async fn save_material_file_record(&self, record: &MaterialFileRecord) -> Result<()>;

    /// 更新材料文件状态
    async fn update_material_file_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()>;

    /// 更新材料文件处理信息
    async fn update_material_file_processing(
        &self,
        id: &str,
        processed_keys_json: Option<&str>,
        ocr_text_key: Option<&str>,
        ocr_text_length: Option<i64>,
    ) -> Result<()>;

    /// 查询材料文件列表
    async fn list_material_files(
        &self,
        filter: &MaterialFileFilter,
    ) -> Result<Vec<MaterialFileRecord>>;

    /// 保存任务payload
    async fn save_task_payload(&self, preview_id: &str, payload: &str) -> Result<()>;

    /// 获取任务payload
    async fn load_task_payload(&self, preview_id: &str) -> Result<Option<String>>;

    /// 删除任务payload
    async fn delete_task_payload(&self, preview_id: &str) -> Result<()>;

    /// 更新预审回调状态
    async fn update_preview_callback_state(&self, update: &PreviewCallbackUpdate) -> Result<()>;

    /// 列出需要回调的预审记录
    async fn list_due_callbacks(&self, limit: u32) -> Result<Vec<PreviewRecord>>;

    /// 写入 Outbox 事件
    async fn enqueue_outbox_event(&self, event: &NewOutboxEvent) -> Result<()>;

    /// 拉取待处理 Outbox 事件
    async fn fetch_pending_outbox_events(&self, limit: u32) -> Result<Vec<OutboxEvent>>;

    /// 标记 Outbox 事件成功
    async fn mark_outbox_event_applied(&self, event_id: &str) -> Result<()>;

    /// 标记 Outbox 事件失败
    async fn mark_outbox_event_failed(&self, event_id: &str, error: &str) -> Result<()>;

    /// 获取事项规则配置
    async fn get_matter_rule_config(
        &self,
        matter_id: &str,
    ) -> Result<Option<MatterRuleConfigRecord>>;

    /// 保存事项规则配置
    async fn upsert_matter_rule_config(&self, config: &MatterRuleConfigRecord) -> Result<()>;

    /// 列出事项规则配置
    async fn list_matter_rule_configs(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<MatterRuleConfigRecord>>;

    // 监控系统相关方法

    /// 根据用户名查找监控用户
    async fn find_monitor_user_by_username(&self, username: &str) -> Result<Option<MonitorUser>> {
        Err(anyhow!("find_monitor_user_by_username not implemented"))
    }

    /// 获取监控用户密码哈希
    async fn get_monitor_user_password_hash(&self, user_id: &str) -> Result<String> {
        Err(anyhow!("get_monitor_user_password_hash not implemented"))
    }

    /// 创建监控会话
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

    /// 根据会话ID查找监控会话
    async fn find_monitor_session_by_id(&self, session_id: &str) -> Result<Option<MonitorSession>> {
        Err(anyhow!("find_monitor_session_by_id not implemented"))
    }

    /// 更新监控用户登录信息
    async fn update_monitor_login_info(&self, user_id: &str, now: &str) -> Result<()> {
        Err(anyhow!("update_monitor_login_info not implemented"))
    }

    /// 更新监控会话活动时间
    async fn update_monitor_session_activity(&self, session_id: &str, now: &str) -> Result<()> {
        Err(anyhow!("update_monitor_session_activity not implemented"))
    }

    /// 删除监控会话
    async fn delete_monitor_session(&self, session_id: &str) -> Result<()> {
        Err(anyhow!("delete_monitor_session not implemented"))
    }

    /// 清理过期监控会话
    async fn cleanup_expired_monitor_sessions(&self, now: &str) -> Result<u64> {
        Err(anyhow!("cleanup_expired_monitor_sessions not implemented"))
    }

    /// 获取活跃监控会话数量
    async fn get_active_monitor_sessions_count(&self, now: &str) -> Result<i64> {
        Err(anyhow!("get_active_monitor_sessions_count not implemented"))
    }

    /// 列出监控用户
    async fn list_monitor_users(&self) -> Result<Vec<MonitorUser>> {
        Err(anyhow!("list_monitor_users not implemented"))
    }

    /// 创建监控用户
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

    /// 更新监控用户角色
    async fn update_monitor_user_role(&self, user_id: &str, role: &str, now: &str) -> Result<()> {
        Err(anyhow!("update_monitor_user_role not implemented"))
    }

    /// 更新监控用户密码
    async fn update_monitor_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
        now: &str,
    ) -> Result<()> {
        Err(anyhow!("update_monitor_user_password not implemented"))
    }

    /// 设置监控用户是否启用
    async fn set_monitor_user_active(
        &self,
        user_id: &str,
        is_active: bool,
        now: &str,
    ) -> Result<()> {
        Err(anyhow!("set_monitor_user_active not implemented"))
    }

    /// 统计活跃管理员数量
    async fn count_active_monitor_admins(&self) -> Result<i64> {
        Err(anyhow!("count_active_monitor_admins not implemented"))
    }

    /// 根据ID查找监控用户
    async fn find_monitor_user_by_id(&self, user_id: &str) -> Result<Option<MonitorUser>> {
        Err(anyhow!("find_monitor_user_by_id not implemented"))
    }

    // Worker结果异步处理队列相关方法

    /// 入队Worker结果
    async fn enqueue_worker_result(&self, preview_id: &str, payload: &str) -> Result<()> {
        Err(anyhow!("enqueue_worker_result not implemented"))
    }

    /// 拉取待处理的Worker结果
    async fn fetch_pending_worker_results(
        &self,
        limit: u32,
    ) -> Result<Vec<WorkerResultQueueRecord>> {
        Err(anyhow!("fetch_pending_worker_results not implemented"))
    }

    /// 更新Worker结果处理状态
    async fn update_worker_result_status(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<()>;

    /// 按 preview_id 查询 Worker 结果队列记录（用于回补 evaluation_result）
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

    // 持久化跨请求去重缓存（按URL→token）
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

    // 外部分享一次性访问token（DM实现，SQLite可忽略）
    async fn create_preview_share_token(
        &self,
        preview_id: &str,
        token: &str,
        format: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        Err(anyhow!("create_preview_share_token not implemented"))
    }

    /// 消费一次性分享token：原子标记已使用并返回token记录
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

/// 外部分享一次性访问token记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewShareTokenRecord {
    pub token: String,
    pub preview_id: String,
    pub format: String,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

/// 监控用户
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorUser {
    pub id: String,
    pub username: String,
    pub role: String,
    pub last_login_at: Option<String>,
    pub login_count: i64,
    pub is_active: bool,
}

/// 监控会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorSession {
    pub id: String,
    pub user_id: String,
    pub user: MonitorUser,
    pub expires_at: String,
    pub ip_address: Option<String>,
}

/// API调用汇总统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSummary {
    pub total_calls: u64,
    pub success_calls: u64,
    pub failed_calls: u64,
    pub avg_response_time_ms: f64,
    pub total_request_size: u64,
    pub total_response_size: u64,
}

/// Outbox事件
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

/// Outbox事件入队结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewOutboxEvent {
    pub table_name: String,
    pub op_type: String,
    pub pk_value: String,
    pub idempotency_key: String,
    pub payload: String,
}

/// 材料文件记录
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

/// 材料文件查询条件
#[derive(Debug, Clone, Default)]
pub struct MaterialFileFilter {
    pub preview_id: Option<String>,
    pub material_code: Option<String>,
}

/// 材料缓存状态
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

/// 材料缓存记录
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

/// 材料缓存查询条件
#[derive(Debug, Clone, Default)]
pub struct CachedMaterialFilter {
    pub preview_id: Option<String>,
    pub status: Option<CachedMaterialStatus>,
    pub limit: Option<u32>,
}

/// 第三方回调状态更新
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

/// Worker结果队列记录
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

/// 材料下载队列记录
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

// 在 Database trait 中添加以下方法
// 注意：由于 Database trait 定义在上面，我们需要修改上面的 Database trait 定义
// 这里我将使用 multi_replace_file_content 来同时修改 trait 定义和添加 struct 定义
