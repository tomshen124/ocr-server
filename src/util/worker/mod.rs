pub mod result_processor;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use once_cell::sync::{Lazy, OnceCell};
use parking_lot::Mutex;
use serde::Serialize;
use tracing::{error, info, warn};

use crate::util::logging::standards::events;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;

use crate::model::evaluation::PreviewEvaluationResult;
use crate::util::material_cache::{self, WORKER_CACHE_SCHEME};
use crate::util::{system_info, WebResult};
use ocr_conn::{ocr, pdf_page_count};

#[cfg(feature = "reqwest")]
use reqwest::{Client, StatusCode};

/// Worker 端上下文（凭证 + HTTP 客户端）
struct WorkerContext {
    client: Arc<WorkerProxyClient>,
    worker_id: String,
}

static WORKER_CONTEXT: OnceCell<WorkerContext> = OnceCell::new();

static WORKER_ACTIVITY: Lazy<Mutex<WorkerActivityState>> =
    Lazy::new(|| Mutex::new(WorkerActivityState::default()));

#[derive(Default, Clone)]
struct WorkerActivityState {
    running_tasks: Vec<String>,
    last_job_started_at: Option<DateTime<Utc>>,
    last_job_finished_at: Option<DateTime<Utc>>,
}

#[derive(Default)]
struct WorkerActivitySnapshot {
    running_tasks: Vec<String>,
    last_job_started_at: Option<String>,
    last_job_finished_at: Option<String>,
}

fn record_worker_job_start(preview_id: &str) {
    let mut state = WORKER_ACTIVITY.lock();
    if !state.running_tasks.iter().any(|task| task == preview_id) {
        state.running_tasks.push(preview_id.to_string());
    }
    state.last_job_started_at = Some(Utc::now());
}

fn record_worker_job_finish(preview_id: &str) {
    let mut state = WORKER_ACTIVITY.lock();
    state.running_tasks.retain(|task| task != preview_id);
    state.last_job_finished_at = Some(Utc::now());
}

fn snapshot_worker_activity() -> WorkerActivitySnapshot {
    let state = WORKER_ACTIVITY.lock();
    WorkerActivitySnapshot {
        running_tasks: state.running_tasks.clone(),
        last_job_started_at: state.last_job_started_at.map(|dt| dt.to_rfc3339()),
        last_job_finished_at: state.last_job_finished_at.map(|dt| dt.to_rfc3339()),
    }
}

pub(crate) struct WorkerJobActivityGuard {
    preview_id: String,
    active: bool,
}

impl WorkerJobActivityGuard {
    pub(crate) fn new(preview_id: String) -> Self {
        record_worker_job_start(&preview_id);
        Self {
            preview_id,
            active: true,
        }
    }
}

impl Drop for WorkerJobActivityGuard {
    fn drop(&mut self) {
        if self.active {
            record_worker_job_finish(&self.preview_id);
        }
    }
}

/// 初始化 Worker 上下文
pub fn init_worker_context(
    worker_id: impl Into<String>,
    worker_secret: impl Into<String>,
    base_url: impl Into<String>,
) -> Result<Arc<WorkerProxyClient>> {
    let worker_id = worker_id.into();
    let worker_secret = worker_secret.into();
    let base_url = base_url.into();

    if worker_id.trim().is_empty() {
        return Err(anyhow!("worker_id 不能为空"));
    }
    if worker_secret.trim().is_empty() {
        return Err(anyhow!("worker_secret 不能为空"));
    }
    if base_url.trim().is_empty() {
        return Err(anyhow!("worker master base_url 不能为空"));
    }

    if WORKER_CONTEXT.get().is_some() {
        return WORKER_CONTEXT
            .get()
            .map(|ctx| Arc::clone(&ctx.client))
            .ok_or_else(|| anyhow!("worker context 初始化失败"));
    }

    let client = Arc::new(WorkerProxyClient::new(
        base_url,
        worker_id.clone(),
        worker_secret,
    ));

    WORKER_CONTEXT
        .set(WorkerContext {
            client: Arc::clone(&client),
            worker_id,
        })
        .map_err(|_| anyhow!("WorkerContext 已初始化"))?;

    Ok(client)
}

/// 当前是否处于 Worker 节点
pub fn is_worker() -> bool {
    WORKER_CONTEXT.get().is_some()
}

/// 获取 Worker ID
pub fn worker_id() -> Option<&'static str> {
    WORKER_CONTEXT.get().map(|ctx| ctx.worker_id.as_str())
}

/// 获取 Worker Proxy 客户端
pub fn client() -> Option<Arc<WorkerProxyClient>> {
    WORKER_CONTEXT.get().map(|ctx| Arc::clone(&ctx.client))
}

/// 启动周期性心跳任务
pub fn spawn_heartbeat_task(interval_seconds: u64) {
    let ctx = match WORKER_CONTEXT.get() {
        Some(ctx) => ctx,
        None => {
            warn!("尝试启动 worker 心跳任务，但上下文未初始化");
            return;
        }
    };

    let client = Arc::clone(&ctx.client);
    let worker_id = ctx.worker_id.clone();
    let heartbeat_interval = interval_seconds.max(5);

    tokio::spawn(async move {
        const HEARTBEAT_FAILURE_THRESHOLD: u32 = 5;
        const HEARTBEAT_MAX_BACKOFF_SECS: u64 = 60;

        let mut consecutive_failures: u32 = 0;
        let mut next_delay = Duration::from_secs(0);
        let mut failure_alert_emitted = false;

        loop {
            tokio::time::sleep(next_delay).await;

            let metrics = collect_worker_metrics();
            let activity = snapshot_worker_activity();
            let payload = WorkerHeartbeatPayload {
                worker_id: worker_id.clone(),
                queue_depth: None,
                running_tasks: activity.running_tasks,
                metrics: Some(metrics),
                interval_secs: Some(heartbeat_interval as u64),
                last_job_started_at: activity.last_job_started_at,
                last_job_finished_at: activity.last_job_finished_at,
            };

            let send_started = Instant::now();
            match client.send_heartbeat(&payload).await {
                Ok(_) => {
                    let elapsed = send_started.elapsed();
                    METRICS_COLLECTOR.record_worker_heartbeat_success(&worker_id, elapsed);

                    if consecutive_failures > 0 {
                        info!(
                            worker_id = %worker_id,
                            elapsed_ms = elapsed.as_millis(),
                            recover_after = %consecutive_failures,
                            "Worker 心跳恢复正常"
                        );
                    }

                    consecutive_failures = 0;
                    next_delay = Duration::from_secs(heartbeat_interval as u64);
                    failure_alert_emitted = false;
                }
                Err(err) => {
                    let elapsed = send_started.elapsed();
                    consecutive_failures = consecutive_failures.saturating_add(1);
                    let failure_reason = err.to_string();
                    METRICS_COLLECTOR.record_worker_heartbeat_failure(
                        &worker_id,
                        &failure_reason,
                        elapsed,
                    );

                    let shift = consecutive_failures.min(5).saturating_sub(1);
                    let exp_backoff_factor = 1u64.checked_shl(shift as u32).unwrap_or(u64::MAX);
                    let mut backoff_secs =
                        (heartbeat_interval as u64).saturating_mul(exp_backoff_factor);
                    backoff_secs =
                        backoff_secs.clamp(heartbeat_interval as u64, HEARTBEAT_MAX_BACKOFF_SECS);
                    next_delay = Duration::from_secs(backoff_secs);

                    warn!(
                        worker_id = %worker_id,
                        failure_count = consecutive_failures,
                        backoff_secs,
                        error = %failure_reason,
                        "Worker 心跳发送失败，将按指数退避重试"
                    );

                    if consecutive_failures >= HEARTBEAT_FAILURE_THRESHOLD && !failure_alert_emitted
                    {
                        error!(
                            worker_id = %worker_id,
                            failure_count = consecutive_failures,
                            "连续心跳失败已达阈值，将保持退避并持续重试"
                        );
                        failure_alert_emitted = true;
                    }
                }
            }
        }
    });
}

/// 通过主节点代理下载材料并返回本地路径（如果可用）
pub async fn fetch_material_path(
    url: &str,
    preview_id: Option<&str>,
    material_code: Option<&str>,
    filename: Option<&str>,
) -> Option<Result<std::path::PathBuf>> {
    if !url.starts_with(WORKER_CACHE_SCHEME) {
        return None;
    }

    let token = url.trim_start_matches(WORKER_CACHE_SCHEME);
    if token.is_empty() {
        return Some(Err(anyhow!("材料令牌不能为空")));
    }

    // 1. 尝试直接从本地缓存获取路径
    if let Some(path) = material_cache::get_material_path(token).await {
        if path.exists() {
            return Some(Ok(path));
        }
    }

    // 2. 如果本地没有，尝试通过代理下载并保存
    let preview_label = preview_id.unwrap_or("unknown");
    let material_label = material_code.unwrap_or("unknown");
    let token_prefix = &token[..token.len().min(8)];

    if let Some(client) = client() {
        let request = FetchMaterialRequest {
            token: token.to_string(),
            preview_id: preview_id.map(|s| s.to_string()),
            material_code: material_code.map(|s| s.to_string()),
        };
        info!(
            target: "worker.material",
            event = events::WORKER_FETCH_MATERIAL,
            preview_id = %preview_label,
            material_code = %material_label,
            token_prefix = %token_prefix,
            channel = "master_proxy_save"
        );

        let result = client.fetch_material(&request).await;
        match result {
            Ok(bytes) => {
                log_material_fetch(preview_id, material_code, filename, &bytes);
                // 保存到本地缓存
                let filename_str = filename.unwrap_or("downloaded_file");
                match material_cache::store_material(
                    preview_label,
                    material_label,
                    filename_str,
                    &bytes,
                    None,
                )
                .await
                {
                    Ok(stored_token) => {
                        // 修正：使用 store_material_with_token 直接保存并注册 token
                        match material_cache::store_material_with_token(
                            token,
                            preview_id.unwrap_or("unknown"),
                            material_code.unwrap_or("unknown"),
                            filename_str,
                            &bytes,
                            None,
                        )
                        .await
                        {
                            Ok(path) => return Some(Ok(path)),
                            Err(e) => return Some(Err(anyhow!("保存缓存文件失败: {}", e))),
                        }
                    }
                    Err(e) => Some(Err(anyhow!("保存材料失败: {}", e))),
                }
            }
            Err(err) => {
                warn!(
                    target: "worker.material",
                    event = events::WORKER_FETCH_FAILURE,
                    preview_id = %preview_label,
                    material_code = %material_label,
                    token_prefix = %token_prefix,
                    channel = "master_proxy_save",
                    error = %err
                );
                Some(Err(err))
            }
        }
    } else {
        // 本地模式但文件不存在？可能是被清理了
        Some(Err(anyhow!("本地缓存文件不存在且无主节点代理")))
    }
}

/// 通过主节点代理下载材料（如果可用）
pub async fn fetch_material_via_proxy(
    url: &str,
    preview_id: Option<&str>,
    material_code: Option<&str>,
    filename: Option<&str>,
) -> Option<Result<Vec<u8>>> {
    if !url.starts_with(WORKER_CACHE_SCHEME) {
        return None;
    }

    let token = url.trim_start_matches(WORKER_CACHE_SCHEME);
    if token.is_empty() {
        return Some(Err(anyhow!("材料令牌不能为空")));
    }

    let preview_label = preview_id.unwrap_or("unknown");
    let material_label = material_code.unwrap_or("unknown");
    let token_prefix = &token[..token.len().min(8)];

    if let Some(client) = client() {
        let request = FetchMaterialRequest {
            token: token.to_string(),
            preview_id: preview_id.map(|s| s.to_string()),
            material_code: material_code.map(|s| s.to_string()),
        };
        info!(
            target: "worker.material",
            event = events::WORKER_FETCH_MATERIAL,
            preview_id = %preview_label,
            material_code = %material_label,
            token_prefix = %token_prefix,
            channel = "master_proxy"
        );
        let result = client.fetch_material(&request).await;
        Some(match result {
            Ok(bytes) => {
                log_material_fetch(preview_id, material_code, filename, &bytes);
                Ok(bytes)
            }
            Err(err) => {
                warn!(
                    target: "worker.material",
                    event = events::WORKER_FETCH_FAILURE,
                    preview_id = %preview_label,
                    material_code = %material_label,
                    token_prefix = %token_prefix,
                    channel = "master_proxy",
                    error = %err
                );
                Err(err)
            }
        })
    } else {
        info!(
            target: "worker.material",
            event = events::WORKER_FETCH_MATERIAL,
            preview_id = %preview_label,
            material_code = %material_label,
            token_prefix = %token_prefix,
            channel = "local_cache"
        );
        Some(
            match material_cache::read_material(token).await.map(|bytes| {
                log_material_fetch(preview_id, material_code, filename, &bytes);
                bytes
            }) {
                Ok(bytes) => Ok(bytes),
                Err(err) => {
                    warn!(
                        target: "worker.material",
                        event = events::WORKER_FETCH_FAILURE,
                        preview_id = %preview_label,
                        material_code = %material_label,
                        token_prefix = %token_prefix,
                        channel = "local_cache",
                        error = %err
                    );
                    Err(err)
                }
            },
        )
    }
}

/// Worker Proxy HTTP 客户端
#[derive(Clone)]
pub struct WorkerProxyClient {
    base_url: String,
    worker_id: String,
    worker_secret: String,
    #[cfg(feature = "reqwest")]
    http: Client,
}

impl WorkerProxyClient {
    pub fn new(base_url: String, worker_id: String, worker_secret: String) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();

        #[cfg(feature = "reqwest")]
        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .build()
            .expect("failed to build reqwest client");

        Self {
            base_url,
            worker_id,
            worker_secret,
            #[cfg(feature = "reqwest")]
            http,
        }
    }

    pub fn worker_id(&self) -> &str {
        &self.worker_id
    }

    #[cfg(feature = "reqwest")]
    fn build_request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{}", self.base_url, path))
            .header("X-Worker-Id", &self.worker_id)
            .header("X-Worker-Key", &self.worker_secret)
    }

    pub async fn fetch_material(&self, payload: &FetchMaterialRequest) -> Result<Vec<u8>> {
        #[cfg(not(feature = "reqwest"))]
        {
            let _ = payload;
            return Err(anyhow!("reqwest 功能未启用，无法通过主节点下载材料"));
        }

        #[cfg(feature = "reqwest")]
        {
            let response = self
                .build_request(reqwest::Method::POST, "/internal/worker/materials/fetch")
                .json(payload)
                .send()
                .await
                .context("调用材料下载接口失败")?;

            if response.status().is_success() {
                let bytes = response.bytes().await.context("读取材料内容失败")?;
                Ok(bytes.to_vec())
            } else {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<no-body>".to_string());
                Err(anyhow!("材料下载失败: status={} body={}", status, body))
            }
        }
    }

    pub async fn submit_result(
        &self,
        preview_id: &str,
        payload: &WorkerResultPayload,
    ) -> Result<()> {
        #[cfg(not(feature = "reqwest"))]
        {
            let _ = (preview_id, payload);
            return Err(anyhow!("reqwest 功能未启用，无法上报处理结果"));
        }

        #[cfg(feature = "reqwest")]
        {
            const MAX_RETRIES: u32 = 3;
            let mut retry_delay = std::time::Duration::from_secs(1);

            for attempt in 1..=MAX_RETRIES {
                let response_result = self
                    .build_request(
                        reqwest::Method::PUT,
                        &format!("/internal/worker/previews/{}/result", preview_id),
                    )
                    .json(payload)
                    .send()
                    .await;

                match response_result {
                    Ok(response) => {
                        if response.status().is_success() {
                            return Ok(());
                        } else {
                            let status = response.status();
                            // 如果是 5xx 错误，或者是 429 Too Many Requests，则重试
                            if status.is_server_error()
                                || status == reqwest::StatusCode::TOO_MANY_REQUESTS
                            {
                                let body = response
                                    .text()
                                    .await
                                    .unwrap_or_else(|_| "<no-body>".to_string());
                                tracing::warn!(
                                    preview_id = %preview_id,
                                    attempt = attempt,
                                    status = %status,
                                    body = %body,
                                    "上报结果遇到服务端错误，准备重试"
                                );
                            } else {
                                // 其他 4xx 错误（如 400 Bad Request, 401 Unauthorized）通常不重试
                                let body = response
                                    .text()
                                    .await
                                    .unwrap_or_else(|_| "<no-body>".to_string());
                                return Err(anyhow!(
                                    "上报处理结果失败: status={} body={}",
                                    status,
                                    body
                                ));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            preview_id = %preview_id,
                            attempt = attempt,
                            error = %e,
                            "上报结果请求失败，准备重试"
                        );
                    }
                }

                if attempt < MAX_RETRIES {
                    tokio::time::sleep(retry_delay).await;
                    retry_delay *= 2;
                }
            }

            Err(anyhow!("上报处理结果失败，已重试 {} 次", MAX_RETRIES))
        }
    }

    pub async fn send_heartbeat(&self, payload: &WorkerHeartbeatPayload) -> Result<()> {
        #[cfg(not(feature = "reqwest"))]
        {
            let _ = payload;
            return Err(anyhow!("reqwest 功能未启用，无法发送 worker 心跳"));
        }

        #[cfg(feature = "reqwest")]
        {
            let response = self
                .build_request(reqwest::Method::POST, "/internal/worker/heartbeat")
                .json(payload)
                .send()
                .await
                .context("发送 worker 心跳失败")?;

            match response.status() {
                StatusCode::OK => Ok(()),
                status => {
                    let body = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "<no-body>".to_string());
                    Err(anyhow!(
                        "worker 心跳被拒绝: status={} body={}",
                        status,
                        body
                    ))
                }
            }
        }
    }

    pub async fn notify_job_started(&self, preview_id: &str, attempt_id: &str) -> Result<()> {
        #[cfg(not(feature = "reqwest"))]
        {
            let _ = (preview_id, attempt_id);
            return Err(anyhow!("reqwest 功能未启用，无法上报任务开始"));
        }

        #[cfg(feature = "reqwest")]
        {
            let response = self
                .build_request(
                    reqwest::Method::POST,
                    &format!("/internal/worker/previews/{}/start", preview_id),
                )
                .json(&WorkerStartPayload {
                    attempt_id: attempt_id.to_string(),
                })
                .send()
                .await
                .context("上报任务开始请求失败")?;

            if response.status().is_success() {
                Ok(())
            } else {
                let status = response.status();
                let body = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "<no-body>".to_string());
                Err(anyhow!("上报任务开始失败: status={} body={}", status, body))
            }
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FetchMaterialRequest {
    pub token: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material_code: Option<String>,
}

#[derive(Debug, Serialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum WorkerJobStatus {
    Completed,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct WorkerResultPayload {
    pub status: WorkerJobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluation_result: Option<PreviewEvaluationResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web_result: Option<WebResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<WorkerResultMetricsPayload>,
    pub attempt_id: String,
}

#[derive(Debug, Serialize)]
struct WorkerStartPayload {
    pub attempt_id: String,
}

#[derive(Debug, Serialize, Default)]
pub struct WorkerResultMetricsPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pages: Option<u32>,
}

#[derive(Debug, Serialize, Clone)]
pub struct WorkerHeartbeatPayload {
    pub worker_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue_depth: Option<u64>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub running_tasks: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<WorkerHeartbeatMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_job_started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_job_finished_at: Option<String>,
}

#[derive(Debug, Serialize, Clone, Default)]
pub struct WorkerHeartbeatMetrics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cpu_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disk_percent: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_1min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_5min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_15min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_capacity: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_available: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_in_use: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_circuit_open: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_consecutive_failures: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_total_started: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_total_restarted: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_pool_total_failures: Option<u64>,
}

/// 上报 worker 结果的便捷构造器
pub fn build_result_payload(
    status: WorkerJobStatus,
    failure_reason: Option<String>,
    evaluation_result: Option<PreviewEvaluationResult>,
    web_result: Option<WebResult>,
    job_duration: Duration,
    attempt_id: String,
) -> WorkerResultPayload {
    WorkerResultPayload {
        status,
        failure_reason,
        evaluation_result,
        web_result,
        metrics: Some(WorkerResultMetricsPayload {
            job_duration_ms: Some(job_duration.as_millis() as u64),
            ocr_duration_ms: None,
            pages: None,
        }),
        attempt_id,
    }
}

fn collect_worker_metrics() -> WorkerHeartbeatMetrics {
    let cpu = system_info::get_cpu_usage();
    let memory = system_info::get_memory_usage();
    let disk = system_info::get_disk_usage();
    let load = system_info::get_load_average();
    let pool_stats = ocr::ocr_pool_stats();

    WorkerHeartbeatMetrics {
        cpu_percent: Some(cpu.usage_percent as f64),
        memory_mb: Some(memory.used_mb),
        memory_percent: Some(memory.usage_percent as f64),
        disk_percent: Some(disk.usage_percent as f64),
        load_1min: Some(load.one),
        load_5min: Some(load.five),
        load_15min: Some(load.fifteen),
        ocr_pool_capacity: Some(pool_stats.capacity),
        ocr_pool_available: Some(pool_stats.available),
        ocr_pool_in_use: Some(pool_stats.in_use),
        ocr_pool_circuit_open: Some(pool_stats.circuit_open),
        ocr_pool_consecutive_failures: Some(pool_stats.consecutive_failures),
        ocr_pool_total_started: Some(pool_stats.total_started),
        ocr_pool_total_restarted: Some(pool_stats.total_restarted),
        ocr_pool_total_failures: Some(pool_stats.total_failures),
    }
}

/// 记录 Worker 启动日志
pub fn log_worker_startup(role: &str) {
    if let Some(id) = worker_id() {
        info!(worker_id = %id, role = %role, "Worker 节点初始化完成");
    } else {
        info!(role = %role, "Worker 节点启动，但未注册 worker_id");
    }
}

fn log_material_fetch(
    preview_id: Option<&str>,
    material_code: Option<&str>,
    filename: Option<&str>,
    bytes: &[u8],
) {
    let size_bytes = bytes.len();
    let is_pdf = filename
        .map(|name| name.to_ascii_lowercase().ends_with(".pdf"))
        .unwrap_or(false)
        || bytes.starts_with(b"%PDF");

    let pdf_pages = if is_pdf {
        match pdf_page_count(bytes) {
            Ok(pages) => Some(pages),
            Err(err) => {
                warn!(
                    preview_id = preview_id.unwrap_or(""),
                    material_code = material_code.unwrap_or(""),
                    filename = filename.unwrap_or(""),
                    error = %err,
                    "PDF 材料页数统计失败"
                );
                None
            }
        }
    } else {
        None
    };

    info!(
        target: "worker.material_fetch",
        preview_id = preview_id.unwrap_or(""),
        material_code = material_code.unwrap_or(""),
        filename = filename.unwrap_or(""),
        size_bytes,
        pdf_pages = pdf_pages.unwrap_or(0),
        is_pdf,
        "材料下载完成"
    );
}
