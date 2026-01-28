use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{post, put};
use axum::{extract::Path, Json, Router};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tokio::time::sleep;
use tracing::{error, info, warn};

use chrono::{DateTime, Duration as ChronoDuration, Utc};

use crate::db::traits::{MaterialFileFilter, PreviewFailureUpdate};
use crate::model::evaluation::{AttachmentInfo, PreviewEvaluationResult};
use crate::model::preview::PreviewBody;
use crate::storage::Storage;
use crate::util::config::types::DeploymentRole;
use crate::util::material_cache;
use crate::util::report::PreviewReportGenerator;
use crate::util::rules::matches_ocr_failure;
use crate::util::task_queue::{PreviewTask, PreviewTaskHandler};
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use crate::util::{IntoJson, WebResult};
use crate::AppState;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView};
use ocr_conn::CURRENT_DIR;
use url::Url;
use urlencoding::encode;

use super::preview::{
    notify_third_party_system, sync_preview_request_status_with_hint, LocalPreviewTaskHandler,
};

use base64::{engine::general_purpose::STANDARD as BASE64_ENGINE, Engine as _};
use mime_guess::MimeGuess;
use serde_json::{json, Map as JsonMap, Value as JsonValue};
use sha2::{Digest, Sha256};
use tokio::fs;
use uuid::Uuid;

static WORKER_HEARTBEATS: Lazy<tokio::sync::RwLock<HashMap<String, WorkerHeartbeatState>>> =
    Lazy::new(|| tokio::sync::RwLock::new(HashMap::new()));
static HEARTBEAT_MONITOR_STARTED: AtomicBool = AtomicBool::new(false);
static WORKER_ACTIVITY: Lazy<tokio::sync::RwLock<HashMap<String, WorkerActivity>>> =
    Lazy::new(|| tokio::sync::RwLock::new(HashMap::new()));

const HEARTBEAT_CHECK_INTERVAL_SECS: u64 = 10;
const HEARTBEAT_TIMEOUT_FACTOR: u64 = 3;
const HEARTBEAT_MIN_TIMEOUT_SECS: u64 = 30;
const HEARTBEAT_SUCCESS_LOG_INTERVAL_SECS: u64 = 300;
const OCR_RESTART_BURST_THRESHOLD: u64 = 3;
const OCR_FAILURE_BURST_THRESHOLD: u64 = 10;
const OCR_RESTART_COOLDOWN_SECS: i64 = 180;

struct HeartbeatLogSummary {
    window_start: Instant,
    last_emit: Instant,
    success_count: u64,
    last_interval_secs: u64,
    last_queue_depth: Option<u64>,
    last_running_tasks: usize,
}

static HEARTBEAT_LOG_SUMMARY: Lazy<tokio::sync::RwLock<HashMap<String, HeartbeatLogSummary>>> =
    Lazy::new(|| tokio::sync::RwLock::new(HashMap::new()));

#[derive(Clone)]
struct WorkerActivity {
    last_assignment: DateTime<Utc>,
}

/// Worker 材料下载请求
#[derive(Debug, Deserialize)]
pub struct MaterialFetchRequest {
    pub token: String,
    pub preview_id: Option<String>,
    pub material_code: Option<String>,
}

#[derive(Debug, Deserialize)]

pub struct PresignRequest {
    pub oss_key: String,
    #[serde(default = "default_operation")]
    pub operation: PresignOperation,
    #[serde(default = "default_ttl")]
    pub ttl_seconds: u64,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]

pub enum PresignOperation {
    #[serde(alias = "get")]
    #[serde(alias = "GET")]
    Get,
}

impl Default for PresignOperation {
    fn default() -> Self {
        PresignOperation::Get
    }
}

fn default_operation() -> PresignOperation {
    PresignOperation::Get
}

fn default_ttl() -> u64 {
    600
}

fn parse_optional_datetime(value: Option<String>) -> Option<DateTime<Utc>> {
    value.and_then(|raw| {
        DateTime::parse_from_rfc3339(raw.trim())
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
    })
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkerResultRequest {
    pub status: WorkerJobStatus,
    #[serde(default)]
    pub failure_reason: Option<String>,
    #[serde(default)]
    pub evaluation_result: Option<crate::model::evaluation::PreviewEvaluationResult>,
    #[serde(default)]
    pub web_result: Option<WebResult>,
    #[serde(default)]
    pub metrics: Option<WorkerResultMetrics>,
    #[serde(default)]
    pub attempt_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct WorkerStartRequest {
    pub attempt_id: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]

pub enum WorkerJobStatus {
    #[serde(alias = "completed")]
    #[serde(alias = "COMPLETED")]
    Completed,
    #[serde(alias = "failed")]
    #[serde(alias = "FAILED")]
    Failed,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct WorkerResultMetrics {
    #[serde(default)]
    pub job_duration_ms: Option<u64>,
    #[serde(default)]
    pub ocr_duration_ms: Option<u64>,
    #[serde(default)]
    pub pages: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
pub struct WorkerHeartbeatRequest {
    pub worker_id: String,
    #[serde(default)]
    pub queue_depth: Option<u64>,
    #[serde(default)]
    pub running_tasks: Vec<String>,
    #[serde(default)]
    pub metrics: Option<WorkerHeartbeatMetrics>,
    #[serde(default)]
    pub interval_secs: Option<u64>,
    #[serde(default)]
    pub last_job_started_at: Option<String>,
    #[serde(default)]
    pub last_job_finished_at: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct WorkerHeartbeatMetrics {
    #[serde(default)]
    pub cpu_percent: Option<f64>,
    #[serde(default)]
    pub memory_mb: Option<u64>,
    #[serde(default)]
    pub memory_percent: Option<f64>,
    #[serde(default)]
    pub disk_percent: Option<f64>,
    #[serde(default)]
    pub load_1min: Option<f64>,
    #[serde(default)]
    pub load_5min: Option<f64>,
    #[serde(default)]
    pub load_15min: Option<f64>,
    #[serde(default)]
    pub ocr_pool_capacity: Option<usize>,
    #[serde(default)]
    pub ocr_pool_available: Option<usize>,
    #[serde(default)]
    pub ocr_pool_in_use: Option<usize>,
    #[serde(default)]
    pub ocr_pool_circuit_open: Option<bool>,
    #[serde(default)]
    pub ocr_pool_consecutive_failures: Option<u32>,
    #[serde(default)]
    pub ocr_pool_total_started: Option<u64>,
    #[serde(default)]
    pub ocr_pool_total_restarted: Option<u64>,
    #[serde(default)]
    pub ocr_pool_total_failures: Option<u64>,
}

#[derive(Debug, Clone)]
struct WorkerHeartbeatState {
    last_seen: DateTime<Utc>,
    queue_depth: Option<u64>,
    running_tasks: Vec<String>,
    metrics: Option<WorkerHeartbeatMetrics>,
    interval_secs: u64,
    was_timed_out: bool,
    restart_cooldown_until: Option<DateTime<Utc>>,
    last_job_started_at: Option<DateTime<Utc>>,
    last_job_finished_at: Option<DateTime<Utc>>,
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/internal/worker/materials/fetch",
            post(fetch_material_handler),
        )
        .route("/internal/worker/storage/presign", post(presign_handler))
        .route(
            "/internal/worker/previews/:preview_id/start",
            post(worker_start_handler),
        )
        .route(
            "/internal/worker/previews/:preview_id/result",
            put(worker_result_handler),
        )
        .route("/internal/worker/heartbeat", post(heartbeat_handler))
}

async fn fetch_material_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<MaterialFetchRequest>,
) -> impl IntoResponse {
    let worker_id = match authorize_worker(&headers, &app_state) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if payload.token.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "token 不能为空");
    }

    let download_start = Instant::now();
    info!(
        worker_id = %worker_id,
        token = %payload.token,
        preview_id = ?payload.preview_id,
        material_code = ?payload.material_code,
        "worker 请求材料缓存"
    );

    match material_cache::read_material(&payload.token).await {
        Ok(bytes) => {
            METRICS_COLLECTOR.record_preview_download(
                true,
                download_start.elapsed(),
                "worker_cache",
            );

            let (filename, content_type) = material_cache::get_material_metadata(&payload.token)
                .await
                .unwrap_or_else(|| ("attachment.bin".to_string(), None));

            let mut response = Response::new(bytes.into());
            *response.status_mut() = StatusCode::OK;

            let ct_value = content_type.unwrap_or_else(|| "application/octet-stream".to_string());
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                ct_value.parse().unwrap_or_else(|_| {
                    header::HeaderValue::from_static("application/octet-stream")
                }),
            );

            if let Ok(disposition) =
                header::HeaderValue::from_str(&format!("attachment; filename=\"{}\"", filename))
            {
                response
                    .headers_mut()
                    .insert(header::CONTENT_DISPOSITION, disposition);
            }

            response
        }
        Err(err) => {
            warn!(
                worker_id = %worker_id,
                token = %payload.token,
                error = %err,
                "worker 材料读取失败，尝试回退"
            );

            // 回退：如果提供了 preview_id/material_code，尝试从存储/DB 获取
            if let Some(preview_id) = payload.preview_id.as_deref() {
                let mut filter = MaterialFileFilter::default();
                filter.preview_id = Some(preview_id.to_string());
                filter.material_code = payload.material_code.clone();

                if let Ok(files) = app_state.database.list_material_files(&filter).await {
                    if let Some(record) = files
                        .iter()
                        .find(|r| r.stored_original_key.trim().len() > 0)
                    {
                        match app_state.storage.get(&record.stored_original_key).await {
                            Ok(Some(bytes)) => {
                                // 回填到缓存路径（如果能定位到）
                                if let Some(path) =
                                    material_cache::get_material_path(&payload.token).await
                                {
                                    if let Some(parent) = path.parent() {
                                        let _ = tokio::fs::create_dir_all(parent).await;
                                    }
                                    let _ = tokio::fs::write(&path, &bytes).await;
                                }

                                let mut response = Response::new(bytes.into());
                                *response.status_mut() = StatusCode::OK;
                                let ct_value = record
                                    .mime_type
                                    .clone()
                                    .unwrap_or_else(|| "application/octet-stream".to_string());
                                response.headers_mut().insert(
                                    header::CONTENT_TYPE,
                                    ct_value.parse().unwrap_or_else(|_| {
                                        header::HeaderValue::from_static("application/octet-stream")
                                    }),
                                );
                                if let Some(name) =
                                    record.attachment_name.as_deref().filter(|s| !s.is_empty())
                                {
                                    if let Ok(disposition) = header::HeaderValue::from_str(
                                        &format!("attachment; filename=\"{}\"", name),
                                    ) {
                                        response
                                            .headers_mut()
                                            .insert(header::CONTENT_DISPOSITION, disposition);
                                    }
                                }

                                METRICS_COLLECTOR.record_preview_download(
                                    true,
                                    download_start.elapsed(),
                                    "worker_cache_fallback",
                                );
                                return response;
                            }
                            Ok(None) => {
                                warn!(
                                    worker_id = %worker_id,
                                    preview_id = %preview_id,
                                    material_code = ?payload.material_code,
                                    key = %record.stored_original_key,
                                    "存储中未找到材料对象"
                                );
                            }
                            Err(e) => {
                                warn!(
                                    worker_id = %worker_id,
                                    preview_id = %preview_id,
                                    material_code = ?payload.material_code,
                                    key = %record.stored_original_key,
                                    error = %e,
                                    "从存储读取材料失败"
                                );
                            }
                        }
                    }
                }
            }

            METRICS_COLLECTOR.record_preview_download(
                false,
                download_start.elapsed(),
                "worker_cache",
            );
            error_response(StatusCode::NOT_FOUND, format!("材料未找到: {}", err))
        }
    }
}

pub async fn enrich_preview_attachments(
    database: &Arc<dyn crate::db::Database>,
    storage: &Arc<dyn Storage>,
    preview_id: &str,
    result: &mut PreviewEvaluationResult,
) -> anyhow::Result<()> {
    let filter = MaterialFileFilter {
        preview_id: Some(preview_id.to_string()),
        material_code: None,
    };

    let mut records = database
        .list_material_files(&filter)
        .await
        .unwrap_or_else(|err| {
            warn!(
                preview_id = %preview_id,
                error = %err,
                "查询材料文件记录失败，将继续使用现有链接"
            );
            Vec::new()
        });

    let mut by_material: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_source: HashMap<String, Vec<usize>> = HashMap::new();

    for (idx, record) in records.iter().enumerate() {
        by_material
            .entry(record.material_code.clone())
            .or_default()
            .push(idx);
        if let Some(src) = &record.source_url {
            by_source.entry(src.clone()).or_default().push(idx);
        }
    }

    let mut url_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut data_uri_cache: HashMap<String, Option<String>> = HashMap::new();
    let mut worker_cache_resolution: HashMap<String, Option<WorkerCacheResolution>> =
        HashMap::new();

    for material in &mut result.material_results {
        let material_indices = by_material
            .get(&material.material_code)
            .cloned()
            .unwrap_or_default();

        for attachment in &mut material.attachments {
            let mut extra_map: JsonMap<String, JsonValue> = attachment
                .extra
                .as_ref()
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();

            let mut record_opt =
                select_material_record(&records, &material_indices, &by_source, attachment)
                    .cloned();

            if let Some(record) = record_opt.as_ref() {
                if let Some(size) = record.size_bytes.and_then(|v| (v >= 0).then_some(v as u64)) {
                    if attachment.file_size.is_none() {
                        attachment.file_size = Some(size);
                    }
                }

                if attachment.mime_type.is_none() {
                    attachment.mime_type = record.mime_type.clone();
                }
            }

            if let Some(record) = record_opt.as_ref() {
                apply_record_links(
                    attachment,
                    &mut extra_map,
                    record,
                    storage,
                    &mut url_cache,
                    &mut data_uri_cache,
                )
                .await?;
            }

            normalize_worker_cache_links(
                attachment,
                preview_id,
                &mut extra_map,
                &mut worker_cache_resolution,
            )
            .await?;

            let record_has_stable_keys = record_opt
                .as_ref()
                .map(record_has_stable_keys)
                .unwrap_or(false);

            let has_worker_cache_links = attachment
                .preview_url
                .as_deref()
                .map(|url| url.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME))
                .unwrap_or(false)
                || attachment
                    .thumbnail_url
                    .as_deref()
                    .map(|url| url.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME))
                    .unwrap_or(false)
                || attachment
                    .file_url
                    .starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME);

            let file_missing = attachment.file_url.trim().is_empty();

            let needs_persist = (!record_has_stable_keys
                && (has_worker_cache_links || file_missing))
                || (!record_has_stable_keys && record_opt.is_none());

            if needs_persist {
                let new_record = persist_material_file_from_worker_cache(
                    database,
                    storage,
                    preview_id,
                    &material.material_code,
                    attachment,
                    &mut extra_map,
                )
                .await?;
                let idx = records.len();
                by_material
                    .entry(material.material_code.clone())
                    .or_default()
                    .push(idx);
                if let Some(src) = new_record.source_url.clone() {
                    by_source.entry(src).or_default().push(idx);
                }
                apply_record_links(
                    attachment,
                    &mut extra_map,
                    &new_record,
                    storage,
                    &mut url_cache,
                    &mut data_uri_cache,
                )
                .await?;
                records.push(new_record.clone());
                record_opt = Some(new_record);
            }

            if let Some(record) = record_opt.as_ref() {
                apply_record_links(
                    attachment,
                    &mut extra_map,
                    record,
                    storage,
                    &mut url_cache,
                    &mut data_uri_cache,
                )
                .await?;
            }

            let still_worker_cache = attachment
                .preview_url
                .as_deref()
                .map(|url| url.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME))
                .unwrap_or(false)
                || attachment
                    .thumbnail_url
                    .as_deref()
                    .map(|url| url.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME))
                    .unwrap_or(false)
                || attachment
                    .file_url
                    .starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME);

            if still_worker_cache {
                return Err(anyhow!(
                    "附件尚未持久化到稳定存储: preview_id={} material={} attachment={}",
                    preview_id,
                    material.material_code,
                    attachment.file_name
                ));
            }

            attachment.extra = if extra_map.is_empty() {
                None
            } else {
                Some(JsonValue::Object(extra_map))
            };
        }
    }

    Ok(())
}

async fn apply_record_links(
    attachment: &mut AttachmentInfo,
    extra_map: &mut JsonMap<String, JsonValue>,
    record: &crate::db::traits::MaterialFileRecord,
    storage: &Arc<dyn Storage>,
    url_cache: &mut HashMap<String, Option<String>>,
    data_uri_cache: &mut HashMap<String, Option<String>>,
) -> anyhow::Result<()> {
    if !record.stored_original_key.is_empty() {
        if let Some(original_url) =
            fetch_public_url(storage, &record.stored_original_key, url_cache).await
        {
            extra_map.insert(
                "ossOriginalKey".to_string(),
                JsonValue::String(record.stored_original_key.clone()),
            );
            extra_map.insert(
                "ossOriginalUrl".to_string(),
                JsonValue::String(original_url.clone()),
            );

            if attachment.file_url.is_empty()
                || attachment
                    .file_url
                    .starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME)
            {
                attachment.file_url = original_url.clone();
            }

            if attachment
                .preview_url
                .as_ref()
                .map(|url| url.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME))
                .unwrap_or(false)
            {
                attachment.preview_url = Some(original_url.clone());
            }
        }

        let mime_hint = attachment.mime_type.clone();
        maybe_embed_from_storage(
            attachment,
            &record.stored_original_key,
            storage,
            mime_hint.as_deref(),
            data_uri_cache,
            extra_map,
            "ossOriginalInline",
        )
        .await;
    }

    if let Some(keys_json) = &record.stored_processed_keys {
        if let Ok(keys) = serde_json::from_str::<Vec<String>>(keys_json) {
            if let Some(first_key) = keys.iter().find(|key| !key.is_empty()) {
                if let Some(preview_url) = fetch_public_url(storage, first_key, url_cache).await {
                    extra_map.insert(
                        "ossPreviewKey".to_string(),
                        JsonValue::String(first_key.clone()),
                    );
                    extra_map.insert(
                        "ossPreviewUrl".to_string(),
                        JsonValue::String(preview_url.clone()),
                    );

                    let should_replace_preview = attachment
                        .preview_url
                        .as_ref()
                        .map(|url| {
                            url.is_empty()
                                || url.starts_with('/')
                                || url.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME)
                        })
                        .unwrap_or(true);

                    if should_replace_preview {
                        attachment.preview_url = Some(preview_url.clone());
                    }

                    let should_replace_thumbnail = attachment
                        .thumbnail_url
                        .as_ref()
                        .map(|url| url.is_empty())
                        .unwrap_or(true);

                    if should_replace_thumbnail {
                        attachment.thumbnail_url = Some(preview_url);
                    }
                }

                if let Some(data_uri) = fetch_data_uri(
                    storage,
                    first_key,
                    attachment.mime_type.as_deref(),
                    data_uri_cache,
                )
                .await
                {
                    attachment.preview_url = Some(data_uri.clone());
                    attachment.thumbnail_url = Some(data_uri);
                    extra_map.insert("embeddedPreview".to_string(), JsonValue::Bool(true));
                } else {
                    let mime_hint = attachment.mime_type.clone();
                    maybe_embed_from_storage(
                        attachment,
                        first_key,
                        storage,
                        mime_hint.as_deref(),
                        data_uri_cache,
                        extra_map,
                        "ossPreviewInline",
                    )
                    .await;
                }
            }
        }
    }

    Ok(())
}

fn select_material_record<'a>(
    records: &'a [crate::db::traits::MaterialFileRecord],
    material_indices: &[usize],
    by_source: &HashMap<String, Vec<usize>>,
    attachment: &AttachmentInfo,
) -> Option<&'a crate::db::traits::MaterialFileRecord> {
    if let Some(original_url) = attachment_extra_str(attachment, "originalUrl") {
        if let Some(indices) = by_source.get(original_url) {
            for idx in indices {
                if let Some(record) = records.get(*idx) {
                    return Some(record);
                }
            }
        }
    }

    let normalized = normalize_attachment_name(&attachment.file_name);
    if !normalized.is_empty() {
        for idx in material_indices {
            if let Some(record) = records.get(*idx) {
                if record
                    .attachment_name
                    .as_deref()
                    .map(|name| name.eq_ignore_ascii_case(&normalized))
                    .unwrap_or(false)
                {
                    return Some(record);
                }
            }
        }
    }

    material_indices.first().and_then(|idx| records.get(*idx))
}

fn record_has_stable_keys(record: &crate::db::traits::MaterialFileRecord) -> bool {
    if !record.stored_original_key.trim().is_empty() {
        return true;
    }

    record
        .stored_processed_keys
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .map(|keys| keys.iter().any(|k| !k.trim().is_empty()))
        .unwrap_or(false)
}

fn attachment_extra_str<'a>(attachment: &'a AttachmentInfo, key: &str) -> Option<&'a str> {
    attachment
        .extra
        .as_ref()
        .and_then(|value| value.as_object())
        .and_then(|map| map.get(key))
        .and_then(|value| value.as_str())
}

fn is_worker_cache_link(url: &str) -> bool {
    url.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME)
}

fn count_unstable_attachments(result: &PreviewEvaluationResult) -> usize {
    result
        .material_results
        .iter()
        .flat_map(|m| m.attachments.iter())
        .filter(|att| {
            att.file_url.trim().is_empty()
                || is_worker_cache_link(&att.file_url)
                || att
                    .preview_url
                    .as_deref()
                    .map(is_worker_cache_link)
                    .unwrap_or(false)
                || att
                    .thumbnail_url
                    .as_deref()
                    .map(is_worker_cache_link)
                    .unwrap_or(false)
        })
        .count()
}

fn normalize_attachment_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            out.push(ch);
        } else if ch.is_whitespace() {
            out.push('_');
        }
    }
    out.trim_matches('.').to_string()
}

async fn fetch_public_url(
    storage: &Arc<dyn Storage>,
    key: &str,
    cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    if key.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(key) {
        return cached.clone();
    }

    let fetched = match storage.get_public_url(key).await {
        Ok(url) => Some(resolve_public_url(key, Some(url))),
        Err(err) => {
            warn!(storage_key = %key, error = %err, "获取存储URL失败");
            Some(resolve_public_url(key, None))
        }
    };

    cache.insert(key.to_string(), fetched.clone());
    fetched
}

async fn fetch_data_uri(
    storage: &Arc<dyn Storage>,
    key: &str,
    mime_hint: Option<&str>,
    cache: &mut HashMap<String, Option<String>>,
) -> Option<String> {
    if key.is_empty() {
        return None;
    }

    if let Some(cached) = cache.get(key) {
        return cached.clone();
    }

    let bytes = match storage.get(key).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            cache.insert(key.to_string(), None);
            return None;
        }
        Err(err) => {
            warn!(storage_key = %key, error = %err, "读取存储预览文件失败");
            cache.insert(key.to_string(), None);
            return None;
        }
    };

    if bytes.len() > WORKER_CACHE_MAX_INLINE_BYTES {
        warn!(
            storage_key = %key,
            size_kb = bytes.len() / 1024,
            "预览文件超过内联阈值，跳过内联"
        );
        cache.insert(key.to_string(), None);
        return None;
    }

    let mime = mime_hint
        .map(|s| s.to_string())
        .or_else(|| guess_mime_from_key(key))
        .unwrap_or_else(|| "image/jpeg".to_string());

    let encoded = BASE64_ENGINE.encode(bytes);
    let data_uri = format!("data:{};base64,{}", mime, encoded);
    cache.insert(key.to_string(), Some(data_uri.clone()));
    Some(data_uri)
}

async fn maybe_embed_from_storage(
    attachment: &mut AttachmentInfo,
    key: &str,
    storage: &Arc<dyn Storage>,
    mime_hint: Option<&str>,
    cache: &mut HashMap<String, Option<String>>,
    extra_map: &mut JsonMap<String, JsonValue>,
    flag: &str,
) {
    if key.is_empty() {
        return;
    }

    let already_inline = attachment
        .preview_url
        .as_ref()
        .map(|url| url.starts_with("data:") || url.starts_with("file:"))
        .unwrap_or(false);
    if already_inline {
        return;
    }

    if let Some(data_uri) = fetch_data_uri(storage, key, mime_hint, cache).await {
        attachment.preview_url = Some(data_uri.clone());
        if attachment
            .thumbnail_url
            .as_ref()
            .map(|url| url.is_empty())
            .unwrap_or(true)
        {
            attachment.thumbnail_url = Some(data_uri.clone());
        }
        extra_map.insert("embeddedPreview".to_string(), JsonValue::Bool(true));
        extra_map.insert(flag.to_string(), JsonValue::Bool(true));
    } else {
        extra_map.insert("previewTooLarge".to_string(), JsonValue::Bool(true));
    }
}

fn guess_mime_from_key(key: &str) -> Option<String> {
    MimeGuess::from_path(key)
        .first_raw()
        .map(|mime| mime.to_string())
}

const WORKER_CACHE_MAX_INLINE_BYTES: usize = 1_000_000; // 1MB以内才内联
const WORKER_CACHE_MAX_DISK_BYTES: usize = 20 * 1024 * 1024; // 20MB以内落盘

#[derive(Clone)]
struct WorkerCacheResolution {
    data_uri: Option<String>,
    local_path: Option<String>,
    public_url: Option<String>,
    mime: Option<String>,
    size: usize,
    too_large_for_inline: bool,
    too_large_for_disk: bool,
}

async fn normalize_worker_cache_links(
    attachment: &mut AttachmentInfo,
    preview_id: &str,
    extra_map: &mut JsonMap<String, JsonValue>,
    cache: &mut HashMap<String, Option<WorkerCacheResolution>>,
) -> anyhow::Result<()> {
    let preview_url_before = attachment.preview_url.clone();
    let thumbnail_url_before = attachment.thumbnail_url.clone();
    let file_url_before = attachment.file_url.clone();
    let should_replace_file_url =
        file_url_before.starts_with(crate::util::material_cache::WORKER_CACHE_SCHEME);

    let preview_token = preview_url_before
        .as_deref()
        .and_then(extract_worker_cache_token)
        .map(|s| s.to_string());
    let thumbnail_token = thumbnail_url_before
        .as_deref()
        .and_then(extract_worker_cache_token)
        .map(|s| s.to_string());
    let file_token = extract_worker_cache_token(&file_url_before).map(|s| s.to_string());

    let token = preview_token
        .or(thumbnail_token)
        .or(file_token)
        .unwrap_or_default();

    if token.is_empty() {
        return Ok(());
    }

    let resolution =
        resolve_worker_cache_resolution(&token, preview_id, attachment.mime_type.as_deref(), cache)
            .await;

    let Some(resolution) = resolution else {
        extra_map.insert("embeddedPreview".to_string(), JsonValue::Bool(false));
        return Ok(());
    };

    extra_map.insert(
        "workerCacheOriginalUrl".to_string(),
        JsonValue::String(format!("{}{}", material_cache::WORKER_CACHE_SCHEME, token)),
    );

    if let Some(mime) = resolution.mime.clone() {
        if attachment.mime_type.is_none() {
            attachment.mime_type = Some(mime);
        }
    }

    let mut new_preview_url: Option<String> = None;
    let mut new_thumbnail_url: Option<String> = None;
    let mut new_file_url: Option<String> = None;

    if let Some(data_uri) = resolution.data_uri.clone() {
        new_preview_url = Some(data_uri.clone());
        new_thumbnail_url = Some(data_uri.clone());
        if should_replace_file_url {
            new_file_url = Some(data_uri);
        }
    } else if let Some(local_path) = resolution.local_path.clone() {
        new_preview_url = Some(local_path.clone());
        new_thumbnail_url = Some(local_path.clone());
        if should_replace_file_url {
            new_file_url = Some(local_path.clone());
        }
        extra_map.insert("localImagePath".to_string(), JsonValue::String(local_path));
    }

    if let Some(public_url) = resolution.public_url.clone() {
        extra_map.insert(
            "publicPreviewUrl".to_string(),
            JsonValue::String(public_url.clone()),
        );
        if new_preview_url.is_none() {
            new_preview_url = Some(public_url.clone());
        }
        if new_thumbnail_url.is_none() {
            new_thumbnail_url = Some(public_url.clone());
        }
        if should_replace_file_url && new_file_url.is_none() {
            new_file_url = Some(public_url);
        }
    }

    if resolution.too_large_for_inline {
        extra_map.insert("previewTooLarge".to_string(), JsonValue::Bool(true));
    }
    if resolution.too_large_for_disk {
        extra_map.insert("previewTooLargeForDisk".to_string(), JsonValue::Bool(true));
    }

    if let Some(url) = new_preview_url {
        attachment.preview_url = Some(url);
    }
    if let Some(url) = new_thumbnail_url {
        attachment.thumbnail_url = Some(url);
    }
    if let Some(url) = new_file_url {
        attachment.file_url = url;
    }

    extra_map.insert("embeddedPreview".to_string(), JsonValue::Bool(true));
    extra_map.insert(
        "workerCacheToken".to_string(),
        JsonValue::String(token.clone()),
    );
    extra_map.insert(
        "embeddedPreviewSize".to_string(),
        JsonValue::Number((resolution.size as u64).into()),
    );

    Ok(())
}

async fn resolve_worker_cache_resolution(
    token: &str,
    preview_id: &str,
    mime_hint: Option<&str>,
    cache: &mut HashMap<String, Option<WorkerCacheResolution>>,
) -> Option<WorkerCacheResolution> {
    if let Some(cached) = cache.get(token) {
        return cached.clone();
    }

    let bytes = match material_cache::read_material(token).await {
        Ok(bytes) => bytes,
        Err(err) => {
            warn!(token = %token, error = %err, "读取 worker-cache 文件失败");
            cache.insert(token.to_string(), None);
            return None;
        }
    };

    let meta = material_cache::get_material_metadata(token).await;
    let (filename, meta_mime) = meta.unwrap_or_else(|| (format!("{token}.bin"), None));
    let mime = mime_hint
        .map(|s| s.to_string())
        .or(meta_mime)
        .or_else(|| guess_mime_from_key(&filename))
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let too_large_for_inline = bytes.len() > WORKER_CACHE_MAX_INLINE_BYTES;
    let too_large_for_disk = bytes.len() > WORKER_CACHE_MAX_DISK_BYTES;

    let data_uri = if !too_large_for_inline {
        Some(format!(
            "data:{};base64,{}",
            mime,
            BASE64_ENGINE.encode(&bytes)
        ))
    } else {
        None
    };

    let local_path = if data_uri.is_none() && !too_large_for_disk {
        persist_worker_cache_image(preview_id, token, &filename, &mime, &bytes).await
    } else {
        None
    };

    let resolution = WorkerCacheResolution {
        data_uri,
        local_path,
        public_url: None,
        mime: Some(mime),
        size: bytes.len(),
        too_large_for_inline,
        too_large_for_disk,
    };

    cache.insert(token.to_string(), Some(resolution.clone()));
    Some(resolution)
}

fn extract_worker_cache_token(url: &str) -> Option<&str> {
    url.strip_prefix(material_cache::WORKER_CACHE_SCHEME)
        .and_then(|token| (!token.is_empty()).then_some(token))
}

async fn persist_worker_cache_image(
    preview_id: &str,
    token: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
) -> Option<String> {
    let extension = std::path::Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .or_else(|| guess_extension_from_mime(mime))
        .unwrap_or_else(|| "bin".to_string());

    let safe_preview = sanitize_for_fs(preview_id);
    let safe_token = sanitize_for_fs(token);

    let images_dir = CURRENT_DIR.join("images").join(&safe_preview);
    if let Err(err) = tokio::fs::create_dir_all(&images_dir).await {
        warn!(
            preview_id = %preview_id,
            dir = %images_dir.display(),
            error = %err,
            "创建本地预览图片目录失败"
        );
        return None;
    }

    let file_name = format!("{}_{}.{}", safe_preview, safe_token, extension);
    let file_path = images_dir.join(&file_name);

    if let Err(err) = tokio::fs::write(&file_path, bytes).await {
        warn!(
            preview_id = %preview_id,
            path = %file_path.display(),
            error = %err,
            "写入本地预览图片失败"
        );
        return None;
    }

    Some(format!("file://{}", file_path.display()))
}

fn guess_extension_from_mime(mime: &str) -> Option<String> {
    match mime.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => Some("jpg".to_string()),
        "image/png" => Some("png".to_string()),
        "image/gif" => Some("gif".to_string()),
        "image/webp" => Some("webp".to_string()),
        "image/svg+xml" => Some("svg".to_string()),
        _ => None,
    }
}

fn sanitize_for_fs(input: &str) -> String {
    input
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

fn resolve_public_url(storage_key: &str, _raw_url: Option<String>) -> String {
    let base = crate::CONFIG.base_url();
    format!(
        "{}/api/storage/files/{}",
        base.trim_end_matches('/'),
        encode(storage_key.trim_start_matches('/'))
    )
}

#[allow(dead_code)]
fn normalize_public_url(url: &str) -> Option<String> {
    let mut parsed = Url::parse(url).ok()?;
    if parsed.scheme() == "http" {
        parsed.set_scheme("https").ok()?;
    }
    Some(parsed.to_string())
}

async fn persist_material_file_from_worker_cache(
    database: &Arc<dyn crate::db::Database>,
    storage: &Arc<dyn Storage>,
    preview_id: &str,
    material_code: &str,
    attachment: &mut AttachmentInfo,
    extra_map: &mut JsonMap<String, JsonValue>,
) -> anyhow::Result<crate::db::traits::MaterialFileRecord> {
    let bytes = match load_worker_cache_bytes(attachment, extra_map).await {
        Some(data) => data,
        None => {
            return Err(anyhow!(
                "worker-cache 缓存已失效，无法提取附件 preview_id={} material={}",
                preview_id,
                material_code
            ));
        }
    };

    let mime = attachment
        .mime_type
        .clone()
        .or_else(|| guess_mime_from_filename(&attachment.file_name))
        .unwrap_or_else(|| "application/octet-stream".to_string());

    let sanitized_preview = sanitize_for_fs(preview_id);
    let sanitized_material = sanitize_for_fs(material_code);
    if sanitized_preview.is_empty() || sanitized_material.is_empty() {
        return Err(anyhow!("生成存储路径失败，预审ID或材料编码为空"));
    }
    let normalized_name = {
        let cleaned = normalize_attachment_name(&attachment.file_name);
        if cleaned.is_empty() {
            format!("attachment-{}", chrono::Utc::now().timestamp_millis())
        } else {
            cleaned
        }
    };
    let extension = guess_extension_from_mime(&mime).unwrap_or_else(|| "bin".to_string());
    let file_name = if normalized_name.contains('.') {
        normalized_name.clone()
    } else {
        format!("{normalized_name}.{extension}")
    };

    let storage_key = format!(
        "previews/{}/materials/{}/{}",
        sanitized_preview, sanitized_material, file_name
    );
    if storage_key.trim().is_empty() {
        return Err(anyhow!("生成存储key为空，拒绝上传"));
    }

    if let Err(err) = storage.put(&storage_key, &bytes).await {
        warn!(
            preview_id = %preview_id,
            material = %material_code,
            error = %err,
            "上传附件到存储失败，继续使用缓存链接"
        );
        return Err(anyhow!(err).context("上传附件到存储失败"));
    }

    let mut processed_keys: Vec<String> = Vec::new();
    if let Some((preview_key, preview_bytes_len, preview_url)) = generate_preview_variant(
        storage,
        preview_id,
        material_code,
        &bytes,
        &mime,
        &normalized_name,
    )
    .await
    {
        processed_keys.push(preview_key.clone());
        extra_map.insert(
            "ossPreviewKey".to_string(),
            JsonValue::String(preview_key.clone()),
        );
        extra_map.insert(
            "ossPreviewUrl".to_string(),
            JsonValue::String(preview_url.clone()),
        );
        extra_map.insert(
            "compressedPreviewSize".to_string(),
            JsonValue::Number((preview_bytes_len as u64).into()),
        );
        attachment.preview_url = Some(preview_url.clone());
        attachment.thumbnail_url = Some(preview_url.clone());
    }

    processed_keys.push(storage_key.clone());

    let raw_public_url = storage.get_public_url(&storage_key).await.ok();
    let public_url = resolve_public_url(&storage_key, raw_public_url);

    if attachment.preview_url.is_none() {
        attachment.preview_url = Some(public_url.clone());
    }
    if attachment.thumbnail_url.is_none() {
        attachment.thumbnail_url = Some(public_url.clone());
    }
    attachment.file_url = public_url.clone();
    attachment.mime_type.get_or_insert(mime.clone());

    extra_map.insert(
        "publicPreviewUrl".to_string(),
        JsonValue::String(public_url.clone()),
    );
    extra_map.insert(
        "ossStoredKey".to_string(),
        JsonValue::String(storage_key.clone()),
    );
    extra_map.insert(
        "ossStoredProcessedKeys".to_string(),
        JsonValue::String(serde_json::to_string(&processed_keys).unwrap_or_default()),
    );
    extra_map.insert(
        "embeddedPreviewSize".to_string(),
        JsonValue::Number(bytes.len().into()),
    );

    let checksum = hex::encode(Sha256::digest(&bytes));
    let now = Utc::now();
    let record = crate::db::traits::MaterialFileRecord {
        id: Uuid::new_v4().to_string(),
        preview_id: preview_id.to_string(),
        material_code: material_code.to_string(),
        attachment_name: if attachment.file_name.trim().is_empty() {
            None
        } else {
            Some(attachment.file_name.clone())
        },
        source_url: extra_map
            .get("workerCacheOriginalUrl")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(attachment.file_url.clone())),
        stored_original_key: storage_key.clone(),
        stored_processed_keys: Some(serde_json::json!(processed_keys).to_string()),
        mime_type: Some(mime),
        size_bytes: Some(bytes.len() as i64),
        checksum_sha256: Some(checksum),
        ocr_text_key: None,
        ocr_text_length: None,
        status: "master_synced".to_string(),
        error_message: None,
        created_at: now,
        updated_at: now,
    };

    if let Err(err) = database.save_material_file_record(&record).await {
        warn!(
            preview_id = %preview_id,
            material = %material_code,
            error = %err,
            "保存材料文件记录失败，继续使用缓存链接"
        );
        return Err(anyhow!(err).context("保存材料文件记录失败"));
    }

    Ok(record)
}

#[derive(Debug, Serialize)]
pub struct RepairPreviewResult {
    pub preview_id: String,
    pub repaired: bool,
    pub attachments_before: usize,
    pub attachments_after: usize,
    pub persisted: usize,
}

pub async fn repair_preview_materials(
    database: &Arc<dyn crate::db::Database>,
    storage: &Arc<dyn Storage>,
    preview_id: &str,
    evaluation_json: &str,
) -> anyhow::Result<RepairPreviewResult> {
    let mut evaluation: PreviewEvaluationResult = serde_json::from_str(evaluation_json)
        .map_err(|err| anyhow!("解析 evaluation_result 失败: {}", err))?;

    let before = count_unstable_attachments(&evaluation);

    enrich_preview_attachments(database, storage, preview_id, &mut evaluation).await?;

    let after = count_unstable_attachments(&evaluation);

    let serialized =
        serde_json::to_string(&evaluation).context("序列化修复后的 evaluation_result 失败")?;

    database
        .update_preview_evaluation_result(preview_id, &serialized)
        .await
        .context("写回 evaluation_result 失败")?;

    if let Err(err) = persist_evaluation_breakdown(database, preview_id, &evaluation).await {
        warn!(
            preview_id = %preview_id,
            error = %err,
            "修复后持久化评估明细失败，将忽略"
        );
    }

    Ok(RepairPreviewResult {
        preview_id: preview_id.to_string(),
        repaired: after == 0,
        attachments_before: before,
        attachments_after: after,
        persisted: before.saturating_sub(after),
    })
}

async fn generate_preview_variant(
    storage: &Arc<dyn Storage>,
    preview_id: &str,
    material_code: &str,
    bytes: &[u8],
    mime: &str,
    normalized_name: &str,
) -> Option<(String, usize, String)> {
    if !is_image_mime(mime) {
        return None;
    }

    let img = image::load_from_memory(bytes).ok()?;
    let resized = resize_for_preview(img);

    let rgb = resized.to_rgb8();
    let mut output = Vec::new();
    if JpegEncoder::new_with_quality(&mut output, 75)
        .encode_image(&rgb)
        .is_err()
    {
        return None;
    }
    let preview_key = format!(
        "previews/{}/materials/{}/preview/{}-preview.jpg",
        sanitize_for_fs(preview_id),
        sanitize_for_fs(material_code),
        normalized_name
    );

    if storage.put(&preview_key, &output).await.is_err() {
        return None;
    }

    let raw_public = storage.get_public_url(&preview_key).await.ok();
    let url = resolve_public_url(&preview_key, raw_public);
    Some((preview_key, output.len(), url))
}

fn resize_for_preview(img: DynamicImage) -> DynamicImage {
    let (w, h) = img.dimensions();
    let max_side: u32 = 1600;
    if w <= max_side && h <= max_side {
        return img;
    }
    let ratio = (max_side as f32 / w as f32).min(max_side as f32 / h as f32);
    let new_w = ((w as f32) * ratio).round() as u32;
    let new_h = ((h as f32) * ratio).round() as u32;
    img.resize(new_w.max(1), new_h.max(1), FilterType::Triangle)
}

fn is_image_mime(mime: &str) -> bool {
    matches!(
        mime.to_ascii_lowercase().as_str(),
        "image/jpeg" | "image/jpg" | "image/png" | "image/webp" | "image/bmp" | "image/gif"
    )
}

async fn load_worker_cache_bytes(
    attachment: &AttachmentInfo,
    extra_map: &JsonMap<String, JsonValue>,
) -> Option<Vec<u8>> {
    if let Some(token) = extra_map.get("workerCacheToken").and_then(|v| v.as_str()) {
        if let Ok(bytes) = material_cache::read_material(token).await {
            return Some(bytes);
        }
        warn!(token = %token, "worker-cache 读取失败，尝试其他来源");
    }

    if let Some(local_path) = extra_map
        .get("localImagePath")
        .and_then(|v| v.as_str())
        .and_then(|p| p.strip_prefix("file://"))
    {
        if let Ok(bytes) = fs::read(local_path).await {
            return Some(bytes);
        }
    }

    if let Some(preview_url) = attachment.preview_url.as_deref() {
        if let Some(bytes) = decode_data_uri_bytes(preview_url) {
            return Some(bytes);
        }
    }

    if !attachment.file_url.is_empty() {
        if let Some(bytes) = decode_data_uri_bytes(&attachment.file_url) {
            return Some(bytes);
        }
    }

    None
}

fn decode_data_uri_bytes(data_uri: &str) -> Option<Vec<u8>> {
    let marker = ";base64,";
    let idx = data_uri.find(marker)?;
    let encoded = &data_uri[idx + marker.len()..];
    BASE64_ENGINE.decode(encoded).ok()
}

fn guess_mime_from_filename(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        None
    } else {
        mime_guess::from_path(name).first().map(|m| m.to_string())
    }
}

async fn persist_evaluation_breakdown(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
    evaluation: &PreviewEvaluationResult,
) -> anyhow::Result<()> {
    use crate::db::traits::{PreviewMaterialResultRecord, PreviewRuleResultRecord};

    let now = Utc::now();
    let mut material_records = Vec::new();
    let mut rule_records = Vec::new();

    for material in &evaluation.material_results {
        let status_code = material.rule_evaluation.status_code as i32;
        let status_str = match material.rule_evaluation.status_code {
            200 => "passed",
            201..=299 => "passed",
            300..=399 => "warning",
            400..=499 => "warning",
            _ => "failed",
        };

        let processing_status = match &material.processing_status {
            crate::model::evaluation::ProcessingStatus::Success => Some("success".to_string()),
            crate::model::evaluation::ProcessingStatus::PartialSuccess { .. } => {
                Some("partial_success".to_string())
            }
            crate::model::evaluation::ProcessingStatus::Failed { .. } => Some("failed".to_string()),
        };

        let warnings_count = match &material.processing_status {
            crate::model::evaluation::ProcessingStatus::PartialSuccess { warnings } => {
                warnings.len() as i32
            }
            _ => 0,
        };
        let issues_count = if status_str == "failed" || status_str == "warning" {
            material.rule_evaluation.suggestions.len() as i32
        } else {
            0
        };

        let attachments_summary: Vec<JsonValue> = material
            .attachments
            .iter()
            .map(|attachment| {
                json!({
                    "file_name": attachment.file_name,
                    "preview_url": attachment.preview_url,
                    "thumbnail_url": attachment.thumbnail_url,
                    "mime_type": attachment.mime_type,
                    "is_cloud_share": attachment.is_cloud_share,
                    "extra": attachment.extra
                })
            })
            .collect();
        let attachments_json = serde_json::to_string(&attachments_summary)
            .ok()
            .filter(|s| !s.is_empty());

        let summary_json = serde_json::to_string(&json!({
            "status_code": material.rule_evaluation.status_code,
            "message": material.rule_evaluation.message,
            "description": material.rule_evaluation.description,
            "suggestions": material.rule_evaluation.suggestions,
            "processing_status": processing_status,
            "ocr_content_length": material.ocr_content.len(),
        }))
        .ok();

        let material_record = PreviewMaterialResultRecord {
            id: Uuid::new_v4().to_string(),
            preview_id: preview_id.to_string(),
            material_code: material.material_code.clone(),
            material_name: Some(material.material_name.clone()),
            status: status_str.to_string(),
            status_code,
            processing_status: processing_status.clone(),
            issues_count,
            warnings_count,
            attachments_json,
            summary_json,
            created_at: now,
            updated_at: now,
        };
        let material_result_id = material_record.id.clone();
        material_records.push(material_record);

        let severity = match status_str {
            "failed" => Some("error".to_string()),
            "warning" => Some("warning".to_string()),
            _ => Some("info".to_string()),
        };

        let rule_record = PreviewRuleResultRecord {
            id: Uuid::new_v4().to_string(),
            preview_id: preview_id.to_string(),
            material_result_id: Some(material_result_id),
            material_code: Some(material.material_code.clone()),
            rule_id: None,
            rule_code: None,
            rule_name: Some(material.material_name.clone()),
            engine: Some("summary".to_string()),
            severity,
            status: Some(status_str.to_string()),
            message: Some(material.rule_evaluation.message.clone()),
            suggestions_json: serde_json::to_string(&material.rule_evaluation.suggestions)
                .ok()
                .filter(|s| !s.is_empty()),
            evidence_json: material
                .rule_evaluation
                .rule_details
                .as_ref()
                .and_then(|details| serde_json::to_string(details).ok()),
            extra_json: serde_json::to_string(&json!({
                "description": material.rule_evaluation.description,
                "processing_status": processing_status,
            }))
            .ok(),
            created_at: now,
            updated_at: now,
        };
        rule_records.push(rule_record);
    }

    database
        .replace_preview_material_results(preview_id, &material_records)
        .await?;
    database
        .replace_preview_rule_results(preview_id, &rule_records)
        .await?;

    Ok(())
}
fn enhance_rule_messages(result: &mut PreviewEvaluationResult) {
    for material in &mut result.material_results {
        material.rule_evaluation.message = humanize_rule_text(&material.rule_evaluation.message);
        material.rule_evaluation.description =
            humanize_rule_text(&material.rule_evaluation.description);
        material
            .rule_evaluation
            .suggestions
            .iter_mut()
            .for_each(|suggestion| {
                *suggestion = humanize_rule_text(suggestion);
            });
    }
}

fn humanize_rule_text(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut normalized = text.replace('_', " ");

    const REPLACEMENTS: [(&str, &str); 12] = [
        ("expiryDate", "证件有效期"),
        ("expiry date", "证件有效期"),
        ("legalRepId", "法定代表人身份证"),
        ("legalRepName", "法定代表人姓名"),
        ("legalRepCert", "法定代表人证件"),
        ("legalRep", "法定代表人"),
        ("agentId", "经办人身份证"),
        ("agentName", "经办人姓名"),
        ("agentCert", "经办人证件"),
        ("agent", "经办人"),
        ("businessLicense", "营业执照"),
        ("copy", "复印件"),
    ];

    for (from, to) in REPLACEMENTS {
        normalized = normalized.replace(from, to);
    }

    normalized
}

fn authorize_worker(headers: &HeaderMap, app_state: &AppState) -> Result<String, Response> {
    if !app_state.config.distributed.enabled {
        return Err(error_response(StatusCode::FORBIDDEN, "分布式模式未启用"));
    }

    let worker_id = headers
        .get("X-Worker-Id")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let worker_key = headers
        .get("X-Worker-Key")
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let worker_id = match worker_id {
        Some(id) => id,
        None => {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "缺少 X-Worker-Id 头",
            ))
        }
    };

    let worker_key = match worker_key {
        Some(key) => key,
        None => {
            return Err(error_response(
                StatusCode::UNAUTHORIZED,
                "缺少 X-Worker-Key 头",
            ))
        }
    };

    let config = &app_state.config.worker_proxy;

    if config.workers.is_empty() {
        warn!("worker proxy 未配置任何 worker，拒绝访问");
        return Err(error_response(StatusCode::FORBIDDEN, "worker proxy 未启用"));
    }

    let authorized = config.workers.iter().any(|worker| {
        worker.enabled && worker.worker_id == worker_id && worker.secret == worker_key
    });

    if authorized {
        Ok(worker_id)
    } else {
        Err(error_response(
            StatusCode::UNAUTHORIZED,
            "无效的 worker 凭证",
        ))
    }
}

fn error_response(status: StatusCode, msg: impl ToString) -> Response {
    (status, WebResult::err_custom(msg).into_json()).into_response()
}

#[axum::debug_handler]
async fn presign_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PresignRequest>,
) -> Response {
    let worker_id = match authorize_worker(&headers, &app_state) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if payload.operation != PresignOperation::Get {
        return error_response(StatusCode::BAD_REQUEST, "仅支持 GET 操作预签名");
    }

    if payload.oss_key.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "oss_key 不能为空");
    }

    let storage = app_state.storage.clone();
    let ttl = payload.ttl_seconds.clamp(60, 3600);

    match storage
        .get_presigned_url(&payload.oss_key, Duration::from_secs(ttl))
        .await
    {
        Ok(url) => {
            info!(
                worker_id = %worker_id,
                key = %payload.oss_key,
                ttl = ttl,
                "生成预签名URL"
            );
            Json(WebResult::ok(serde_json::json!({
                "url": url,
                "expires_in": ttl,
            })))
            .into_response()
        }
        Err(err) => {
            warn!(
                worker_id = %worker_id,
                key = %payload.oss_key,
                error = %err,
                "生成预签名URL失败"
            );
            error_response(
                StatusCode::BAD_GATEWAY,
                format!("无法生成预签名URL: {}", err),
            )
        }
    }
}

/// 异步处理 Worker 结果的核心逻辑
/// 异步处理 Worker 结果的核心逻辑
pub async fn process_worker_result_logic(
    app_state: &AppState,
    preview_id: &str,
    payload: WorkerResultRequest,
    worker_id: &str,
) -> anyhow::Result<()> {
    let database = app_state.database.clone();

    let record = match database.get_preview_record(preview_id).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            warn!(preview_id = %preview_id, "预审记录不存在，无法处理 Worker 结果");
            return Ok(()); // 记录不存在，视为处理完成（丢弃）
        }
        Err(err) => {
            return Err(anyhow!("查询预审记录失败: {}", err));
        }
    };

    if let Some(expected_attempt) = record.last_attempt_id.as_deref() {
        if let Some(request_attempt) = payload.attempt_id.as_deref() {
            if request_attempt != expected_attempt {
                warn!(
                    worker_id = %worker_id,
                    preview_id = %preview_id,
                    expected_attempt,
                    request_attempt,
                    "收到过期 attempt_id 的 Worker 结果，将忽略"
                );
                return Ok(());
            }
        } else {
            warn!(
                worker_id = %worker_id,
                preview_id = %preview_id,
                "Worker 结果缺少 attempt_id 字段，将继续处理"
            );
        }
    }

    let success = matches!(payload.status, WorkerJobStatus::Completed);

    let mut evaluation_result_opt = payload.evaluation_result.clone();
    if let Some(result) = evaluation_result_opt.as_mut() {
        enrich_preview_attachments(&database, &app_state.storage, preview_id, result).await?;
        enhance_rule_messages(result);
    }

    if success && payload.evaluation_result.is_none() {
        warn!(preview_id = %preview_id, "status=completed 需要提供 evaluation_result");
        return Err(anyhow!("status=completed 需要提供 evaluation_result"));
    }

    let mut failure_code_override: Option<String> = None;
    let mut failure_context_note: Option<String> = None;

    if !success {
        let fallback_cfg = &app_state.config.master.worker_fallback;
        if fallback_cfg.enabled {
            let failure_reason_text = payload
                .failure_reason
                .clone()
                .or_else(|| payload.web_result.as_ref().map(|res| res.msg.clone()))
                .unwrap_or_default();

            let matches_keyword = if fallback_cfg.trigger_keywords.is_empty() {
                matches_ocr_failure(&failure_reason_text)
            } else {
                reason_matches(&failure_reason_text, &fallback_cfg.trigger_keywords)
            };

            let attempts = record.retry_count.max(0) as u32;
            let exhausted = attempts >= fallback_cfg.max_attempts;
            let marked_failed = record
                .last_error_code
                .as_deref()
                .map(|code| code == "MASTER_FALLBACK_FAILED")
                .unwrap_or(false);

            if matches_keyword && !exhausted && !marked_failed {
                tracing::warn!(
                    preview_id = %preview_id,
                    worker_id = %worker_id,
                    reason = %failure_reason_text,
                    "Worker OCR 失败，触发主节点回退处理"
                );

                let mut update = PreviewFailureUpdate::default();
                update.preview_id = preview_id.to_string();
                update.last_error_code = Some(Some("MASTER_FALLBACK_IN_PROGRESS".to_string()));
                if let Err(err) = database.update_preview_failure_context(&update).await {
                    warn!(
                        preview_id = %preview_id,
                        error = %err,
                        "记录回退处理中状态失败"
                    );
                }

                match attempt_master_fallback(app_state, preview_id).await {
                    Ok(_) => {
                        tracing::info!(
                            preview_id = %preview_id,
                            "主节点回退处理成功"
                        );

                        let mut update = PreviewFailureUpdate::default();
                        update.preview_id = preview_id.to_string();
                        update.failure_reason = Some(None);
                        update.failure_context = Some(None);
                        update.last_error_code = Some(Some("MASTER_FALLBACK_SUCCESS".to_string()));
                        update.ocr_stderr_summary = Some(None);
                        if let Err(err) = database.update_preview_failure_context(&update).await {
                            warn!(
                                preview_id = %preview_id,
                                error = %err,
                                "清理回退状态失败"
                            );
                        }

                        if let Err(err) = database.delete_task_payload(preview_id).await {
                            warn!(preview_id = %preview_id, error = %err, "删除任务payload失败");
                        }

                        if let Err(err) = material_cache::cleanup_preview(preview_id).await {
                            warn!(
                                preview_id = %preview_id,
                                error = %err,
                                "回退完成后清理材料缓存失败"
                            );
                        }
                        if let Err(err) = database
                            .delete_cached_materials_by_preview(preview_id)
                            .await
                        {
                            warn!(
                                preview_id = %preview_id,
                                error = %err,
                                "回退完成后同步清理缓存记录失败"
                            );
                        }

                        // 回退成功，视为处理完成
                        return Ok(());
                    }
                    Err(err) => {
                        tracing::error!(
                            preview_id = %preview_id,
                            error = %err,
                            "主节点回退处理失败"
                        );
                        failure_code_override = Some("MASTER_FALLBACK_FAILED".to_string());
                        failure_context_note = Some(format!(
                            "worker_id={} attempt_id={} fallback=failed",
                            worker_id,
                            payload.attempt_id.as_deref().unwrap_or("unknown")
                        ));
                    }
                }
            }
        }
    }

    if let Some(result) = evaluation_result_opt.as_ref() {
        let json_str = match serde_json::to_string(result) {
            Ok(s) => s,
            Err(err) => {
                warn!(
                    worker_id = %worker_id,
                    preview_id = %preview_id,
                    error = %err,
                    "序列化 evaluation_result 失败"
                );
                return Err(anyhow!("序列化 evaluation_result 失败: {}", err));
            }
        };

        // 写入 evaluation_result，若失败则中断流程，避免状态已完成但结果缺失
        let mut written = false;
        for attempt in 1..=2 {
            match database
                .update_preview_evaluation_result(preview_id, &json_str)
                .await
            {
                Ok(_) => {
                    written = true;
                    break;
                }
                Err(err) => {
                    warn!(
                        worker_id = %worker_id,
                        preview_id = %preview_id,
                        attempt,
                        error = %err,
                        "更新 evaluation_result 失败"
                    );
                }
            }
        }

        if !written {
            return Err(anyhow!(
                "更新 evaluation_result 失败，预防生成空报告: preview_id={}",
                preview_id
            ));
        }

        if let Err(err) = persist_evaluation_breakdown(&database, preview_id, result).await {
            warn!(
                preview_id = %preview_id,
                error = %err,
                "持久化评估结果明细失败"
            );
        }
    }

    if success {
        let generator = PreviewReportGenerator::new(app_state.clone());
        match generator.generate_and_persist_reports(preview_id).await {
            Ok(files) => {
                info!(
                    preview_id = %preview_id,
                    files_count = files.len(),
                    "报告文件生成并持久化成功"
                );

                let mut preview_view_url = None;
                let mut preview_download_url = None;

                for file in &files {
                    if file.file_type == "html" {
                        preview_view_url = Some(file.view_url.clone());
                        // 如果还没有更优的下载链接，则用 HTML
                        if preview_download_url.is_none() {
                            preview_download_url = Some(file.download_url.clone());
                        }
                    }

                    if file.file_type == "pdf" {
                        // 下载优先使用 PDF
                        preview_download_url = Some(file.download_url.clone());
                    }
                }

                if let Err(err) = database
                    .update_preview_artifacts(
                        preview_id,
                        &record.file_name,
                        &record.preview_url,
                        preview_view_url.as_deref(),
                        preview_download_url.as_deref(),
                    )
                    .await
                {
                    warn!(
                        preview_id = %preview_id,
                        error = %err,
                        "更新预审产物链接失败"
                    );
                }
            }
            Err(err) => {
                warn!(
                    preview_id = %preview_id,
                    error = %err,
                    "生成报告文件失败"
                );
            }
        }

        let target_status = crate::db::PreviewStatus::Completed;
        database
            .update_preview_status(preview_id, target_status.clone())
            .await
            .map_err(|err| anyhow!("更新预审状态失败: {}", err))?;
        sync_preview_request_status_with_hint(
            &database,
            preview_id,
            record.third_party_request_id.as_deref(),
            target_status,
        )
        .await;

        if let Err(err) = database.delete_task_payload(preview_id).await {
            warn!(preview_id = %preview_id, error = %err, "删除任务payload失败");
        }

        if let Err(err) = material_cache::cleanup_preview(preview_id).await {
            warn!(
                preview_id = %preview_id,
                error = %err,
                "清理材料缓存失败"
            );
        }
        if let Err(err) = database
            .delete_cached_materials_by_preview(preview_id)
            .await
        {
            warn!(
                preview_id = %preview_id,
                error = %err,
                "同步清理缓存记录失败"
            );
        }
    } else {
        let mut update = PreviewFailureUpdate::default();
        update.preview_id = preview_id.to_string();
        update.failure_reason = Some(Some(
            payload
                .failure_reason
                .clone()
                .or_else(|| payload.web_result.as_ref().map(|res| res.msg.clone()))
                .unwrap_or_else(|| "Unknown worker failure".to_string()),
        ));

        if let Some(code) = failure_code_override {
            update.last_error_code = Some(Some(code));
        }

        if let Some(ctx) = failure_context_note {
            update.failure_context = Some(Some(ctx));
        }

        if let Err(err) = database.update_preview_failure_context(&update).await {
            warn!(
                preview_id = %preview_id,
                error = %err,
                "更新失败上下文信息失败"
            );
        }

        let target_status = crate::db::PreviewStatus::Failed;
        database
            .update_preview_status(preview_id, target_status.clone())
            .await
            .map_err(|err| anyhow!("更新预审状态失败: {}", err))?;
        sync_preview_request_status_with_hint(
            &database,
            preview_id,
            record.third_party_request_id.as_deref(),
            target_status,
        )
        .await;
    }

    Ok(())
}
#[axum::debug_handler]
async fn worker_result_handler(
    State(app_state): State<AppState>,
    Path(preview_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<WorkerResultRequest>,
) -> Response {
    let worker_id = match authorize_worker(&headers, &app_state) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if preview_id.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "preview_id 不能为空");
    }

    let database = app_state.database.clone();

    // 先检查预审记录是否存在，避免无效入队
    let record = match database.get_preview_record(&preview_id).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            return error_response(StatusCode::NOT_FOUND, "预审记录不存在");
        }
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("查询预审记录失败: {}", err),
            );
        }
    };

    // 避免过期 attempt_id 进入队列
    if let Some(expected_attempt) = record.last_attempt_id.as_deref() {
        if let Some(request_attempt) = payload.attempt_id.as_deref() {
            if request_attempt != expected_attempt {
                warn!(
                    worker_id = %worker_id,
                    preview_id = %preview_id,
                    expected_attempt,
                    request_attempt,
                    "收到过期 attempt_id 的 Worker 结果，将忽略"
                );
                return Json(serde_json::json!({
                    "success": false,
                    "preview_id": preview_id,
                    "status": "ignored",
                    "reason": "attempt_id_mismatch",
                }))
                .into_response();
            }
        } else {
            warn!(
                worker_id = %worker_id,
                preview_id = %preview_id,
                "Worker 结果缺少 attempt_id 字段，将继续处理"
            );
        }
    }

    // 优先尝试入队，交给后台异步处理器处理
    let payload_json = match serde_json::to_string(&payload) {
        Ok(json) => json,
        Err(err) => {
            return error_response(StatusCode::BAD_REQUEST, format!("序列化结果失败: {}", err));
        }
    };

    match database
        .enqueue_worker_result(&preview_id, &payload_json)
        .await
    {
        Ok(_) => {
            info!(
                worker_id = %worker_id,
                preview_id = %preview_id,
                "Worker 结果已入队，等待后台异步处理"
            );
            return (
                StatusCode::ACCEPTED,
                Json(serde_json::json!({
                    "preview_id": preview_id,
                    "status": format!("{:?}", payload.status),
                    "worker_id": worker_id,
                    "attempt_id": payload.attempt_id,
                    "queued": true
                })),
            )
                .into_response();
        }
        Err(err) => {
            warn!(
                worker_id = %worker_id,
                preview_id = %preview_id,
                error = %err,
                "Worker 结果入队失败，回退为同步处理"
            );
        }
    }

    // 入队失败则走同步处理逻辑，避免结果丢失
    let sync_result =
        process_worker_result_logic(&app_state, &preview_id, payload, &worker_id).await;

    match sync_result {
        Ok(_) => Json(WebResult::ok(serde_json::json!({
            "preview_id": preview_id,
            "status": "completed",
            "worker_id": worker_id,
        })))
        .into_response(),
        Err(err) => {
            let msg = err.to_string();
            let status = if msg.contains("status=completed 需要提供 evaluation_result") {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            error!(
                worker_id = %worker_id,
                preview_id = %preview_id,
                error = %msg,
                "同步处理 Worker 结果失败"
            );
            error_response(status, format!("处理 Worker 结果失败: {}", msg))
        }
    }
}

async fn attempt_master_fallback(app_state: &AppState, preview_id: &str) -> anyhow::Result<()> {
    let payload_json = app_state
        .database
        .load_task_payload(preview_id)
        .await?
        .ok_or_else(|| anyhow!("任务payload不存在，无法执行主节点回退"))?;
    let task: PreviewTask = serde_json::from_str(&payload_json).context("解析任务payload失败")?;

    let handler =
        LocalPreviewTaskHandler::new(app_state.database.clone(), app_state.storage.clone());
    handler.handle_preview_task(task).await?;
    Ok(())
}

fn reason_matches(reason: &str, keywords: &[String]) -> bool {
    if keywords.is_empty() {
        return matches_ocr_failure(reason);
    }
    let lowered = reason.to_ascii_lowercase();
    keywords.iter().any(|keyword| {
        let key = keyword.trim();
        !key.is_empty() && lowered.contains(&key.to_ascii_lowercase())
    })
}

async fn worker_start_handler(
    State(app_state): State<AppState>,
    Path(preview_id): Path<String>,
    headers: HeaderMap,
    Json(payload): Json<WorkerStartRequest>,
) -> Response {
    let worker_id = match authorize_worker(&headers, &app_state) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    if preview_id.trim().is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "preview_id 不能为空");
    }

    let attempt_id = payload.attempt_id.trim();
    if attempt_id.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "attempt_id 不能为空");
    }

    if let Err(resp) = ensure_worker_capacity(&worker_id).await {
        return resp;
    }

    if let Err(err) = app_state
        .database
        .mark_preview_processing(&preview_id, &worker_id, attempt_id)
        .await
    {
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("标记任务Processing状态失败: {}", err),
        );
    }

    record_worker_assignment(&worker_id).await;

    Json(WebResult::ok(serde_json::json!({
        "preview_id": preview_id,
        "worker_id": worker_id,
        "attempt_id": attempt_id,
        "status": "processing"
    })))
    .into_response()
}

#[axum::debug_handler]
async fn heartbeat_handler(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<WorkerHeartbeatRequest>,
) -> Response {
    let handler_start = Instant::now();

    let worker_id = match authorize_worker(&headers, &app_state) {
        Ok(id) => id,
        Err(resp) => return resp,
    };

    let heartbeat_span = tracing::info_span!("worker_heartbeat", worker_id = %worker_id);
    let _heartbeat_guard = heartbeat_span.enter();

    if payload.worker_id != worker_id {
        return error_response(StatusCode::BAD_REQUEST, "worker_id 与凭证不匹配");
    }

    let WorkerHeartbeatRequest {
        worker_id: _,
        queue_depth,
        running_tasks,
        metrics,
        interval_secs,
        last_job_started_at,
        last_job_finished_at,
    } = payload;

    let parsed_last_job_started = parse_optional_datetime(last_job_started_at);
    let parsed_last_job_finished = parse_optional_datetime(last_job_finished_at);

    let running_task_count = running_tasks.len();

    let previous_interval = {
        let guard = WORKER_HEARTBEATS.read().await;
        guard.get(&worker_id).map(|state| state.interval_secs)
    };

    let computed_interval = interval_secs
        .or(previous_interval)
        .or_else(|| {
            app_state
                .config
                .deployment
                .worker
                .as_ref()
                .and_then(|cfg| cfg.heartbeat_interval_secs)
        })
        .unwrap_or(30);

    let mut guard = WORKER_HEARTBEATS.write().await;
    let previous_state = guard.get(&worker_id).cloned();
    let was_timed_out = previous_state
        .as_ref()
        .map(|state| state.was_timed_out)
        .unwrap_or(false);
    let mut restart_cooldown_until = previous_state
        .as_ref()
        .and_then(|state| state.restart_cooldown_until);

    if let (Some(prev_metrics), Some(curr_metrics)) = (
        previous_state.as_ref().and_then(|s| s.metrics.as_ref()),
        metrics.as_ref(),
    ) {
        if let Some((trigger, delta)) = detect_ocr_restart_burst(prev_metrics, curr_metrics) {
            let cooldown_deadline = Utc::now() + ChronoDuration::seconds(OCR_RESTART_COOLDOWN_SECS);
            if restart_cooldown_until
                .map(|current| cooldown_deadline > current)
                .unwrap_or(true)
            {
                restart_cooldown_until = Some(cooldown_deadline);
            }
            warn!(
                target: "worker.ocr_health",
                worker_id = %worker_id,
                trigger = %trigger,
                delta,
                cooldown_secs = OCR_RESTART_COOLDOWN_SECS,
                "Worker OCR 引擎出现异常突增，进入冷却期"
            );
        }
    }

    guard.insert(
        worker_id.clone(),
        WorkerHeartbeatState {
            last_seen: Utc::now(),
            queue_depth,
            running_tasks,
            metrics,
            interval_secs: computed_interval,
            was_timed_out: false,
            restart_cooldown_until,
            last_job_started_at: parsed_last_job_started,
            last_job_finished_at: parsed_last_job_finished,
        },
    );
    drop(guard);

    if let Some(depth) = queue_depth {
        let queue_label = format!("worker:{}", worker_id);
        METRICS_COLLECTOR.record_queue_depth(&queue_label, depth);
    }

    if was_timed_out {
        info!(worker_id = %worker_id, "Worker 心跳恢复");
    }

    record_heartbeat_success_log(
        &worker_id,
        computed_interval,
        queue_depth,
        running_task_count,
        was_timed_out,
    )
    .await;

    METRICS_COLLECTOR.record_worker_heartbeat_success(&worker_id, handler_start.elapsed());

    Json(WebResult::ok(serde_json::json!({
        "ack": true,
        "timestamp": Utc::now(),
        "interval_secs": computed_interval,
    })))
    .into_response()
}

const WORKER_STALE_MULTIPLIER: i64 = 3;
const WORKER_CPU_LIMIT: f64 = 95.0;
const WORKER_MEM_LIMIT: f64 = 92.0;
const RECENT_ACTIVITY_WINDOW_SECS: i64 = 300;

async fn ensure_worker_capacity(worker_id: &str) -> Result<(), Response> {
    let guard = WORKER_HEARTBEATS.read().await;
    let state = match guard.get(worker_id) {
        Some(state) => state,
        None => {
            return Err(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "worker 尚未上报心跳，请稍后再试",
            ))
        }
    };

    let recently_active = worker_recently_active(worker_id).await;

    let now = Utc::now();
    let elapsed = (now - state.last_seen).num_seconds();
    let stale_budget = (state.interval_secs.max(5) as i64) * WORKER_STALE_MULTIPLIER;
    if elapsed > stale_budget && !recently_active {
        warn!(
            worker_id = %worker_id,
            elapsed_secs = elapsed,
            budget_secs = stale_budget,
            "阻止 worker 派单：心跳过旧"
        );
        return Err(error_response(
            StatusCode::SERVICE_UNAVAILABLE,
            "worker 心跳过旧，已暂停派发任务",
        ));
    }

    if let Some(until) = state.restart_cooldown_until {
        if until > now {
            warn!(
                worker_id = %worker_id,
                cooldown_until = %until,
                "阻止 worker 派单：OCR 引擎频繁重启，冷却中"
            );
            return Err(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "worker OCR 引擎频繁异常，冷却中",
            ));
        }
    }

    if let Some(metrics) = &state.metrics {
        if metrics.ocr_pool_circuit_open.unwrap_or(false) {
            warn!(
                worker_id = %worker_id,
                "阻止 worker 派单：OCR 引擎池熔断"
            );
            return Err(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "worker OCR 引擎池熔断，任务暂时回退",
            ));
        }

        if let (Some(capacity), Some(available)) =
            (metrics.ocr_pool_capacity, metrics.ocr_pool_available)
        {
            if capacity > 0 && available == 0 {
                warn!(
                    worker_id = %worker_id,
                    capacity,
                    available,
                    "阻止 worker 派单：OCR 引擎池无可用槽位"
                );
                return Err(error_response(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "worker OCR 引擎池无可用槽位，任务暂时回退",
                ));
            }
        }

        if metrics.cpu_percent.unwrap_or(0.0) > WORKER_CPU_LIMIT {
            warn!(
                worker_id = %worker_id,
                cpu = metrics.cpu_percent.unwrap_or(0.0),
                "阻止 worker 派单：CPU 过载"
            );
            return Err(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "worker CPU 使用率过高，暂不派发任务",
            ));
        }

        if metrics.memory_percent.unwrap_or(0.0) > WORKER_MEM_LIMIT {
            warn!(
                worker_id = %worker_id,
                memory = metrics.memory_percent.unwrap_or(0.0),
                "阻止 worker 派单：内存占用过高"
            );
            return Err(error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "worker 内存占用过高，暂不派发任务",
            ));
        }
    }

    Ok(())
}

async fn record_worker_assignment(worker_id: &str) {
    let mut guard = WORKER_ACTIVITY.write().await;

    guard.insert(
        worker_id.to_string(),
        WorkerActivity {
            last_assignment: Utc::now(),
        },
    );
}

async fn worker_recently_active(worker_id: &str) -> bool {
    let guard = WORKER_ACTIVITY.read().await;
    guard
        .get(worker_id)
        .map(|activity| {
            (Utc::now() - activity.last_assignment).num_seconds() <= RECENT_ACTIVITY_WINDOW_SECS
        })
        .unwrap_or(false)
}

/// 启动 worker 心跳监控后台任务（仅 Master 节点）
pub fn spawn_heartbeat_watchdog(app_state: &AppState) {
    if app_state.config.deployment.role != DeploymentRole::Master {
        return;
    }

    if HEARTBEAT_MONITOR_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let expected_workers: Vec<String> = app_state
        .config
        .worker_proxy
        .workers
        .iter()
        .filter(|w| w.enabled)
        .map(|w| w.worker_id.clone())
        .collect();

    tokio::spawn(async move {
        heartbeat_watchdog_loop(expected_workers).await;
    });
}

async fn heartbeat_watchdog_loop(expected_workers: Vec<String>) {
    let mut missing_warned: HashSet<String> = HashSet::new();

    loop {
        sleep(Duration::from_secs(HEARTBEAT_CHECK_INTERVAL_SECS)).await;

        let now = Utc::now();
        let mut active_ids: Vec<String> = Vec::new();

        {
            let mut guard = WORKER_HEARTBEATS.write().await;

            for (worker_id, state) in guard.iter_mut() {
                active_ids.push(worker_id.clone());

                let interval = state.interval_secs.max(5);
                let timeout_secs = interval
                    .saturating_mul(HEARTBEAT_TIMEOUT_FACTOR)
                    .max(HEARTBEAT_MIN_TIMEOUT_SECS);
                let elapsed = (now - state.last_seen).num_seconds();

                if elapsed > timeout_secs as i64 {
                    if !state.was_timed_out {
                        state.was_timed_out = true;
                        warn!(
                            worker_id = %worker_id,
                            last_seen = %state.last_seen,
                            elapsed_secs = elapsed,
                            timeout_secs,
                            "Worker 心跳超时"
                        );
                        METRICS_COLLECTOR.record_worker_heartbeat_timeout(worker_id);
                    }
                } else if state.was_timed_out {
                    state.was_timed_out = false;
                    info!(worker_id = %worker_id, elapsed_secs = elapsed, "Worker 心跳恢复");
                }
            }
        }

        let active_set: HashSet<String> = active_ids.into_iter().collect();

        for worker_id in expected_workers.iter() {
            if active_set.contains(worker_id) {
                missing_warned.remove(worker_id);
            } else if !missing_warned.contains(worker_id) {
                warn!(worker_id = %worker_id, "预期的 Worker 尚未上报心跳");
                missing_warned.insert(worker_id.clone());
            }
        }
    }
}

async fn record_heartbeat_success_log(
    worker_id: &str,
    interval_secs: u64,
    queue_depth: Option<u64>,
    running_task_count: usize,
    was_timed_out: bool,
) {
    let now = Instant::now();
    let mut guard = HEARTBEAT_LOG_SUMMARY.write().await;
    let entry = guard
        .entry(worker_id.to_string())
        .or_insert_with(|| HeartbeatLogSummary {
            window_start: now,
            last_emit: now,
            success_count: 0,
            last_interval_secs: interval_secs,
            last_queue_depth: queue_depth,
            last_running_tasks: running_task_count,
        });

    if was_timed_out {
        entry.window_start = now;
        entry.last_emit = now;
        entry.success_count = 0;
    }

    entry.success_count = entry.success_count.saturating_add(1);
    entry.last_interval_secs = interval_secs;
    entry.last_queue_depth = queue_depth;
    entry.last_running_tasks = running_task_count;

    let mut emit_payload: Option<(u64, u64, u64, Option<u64>, usize)> = None;
    if now.duration_since(entry.last_emit)
        >= Duration::from_secs(HEARTBEAT_SUCCESS_LOG_INTERVAL_SECS)
    {
        let window_secs = now.duration_since(entry.window_start).as_secs().max(1);
        let success_count = entry.success_count;
        let interval_for_log = entry.last_interval_secs;
        let queue_depth_for_log = entry.last_queue_depth;
        let running_tasks_for_log = entry.last_running_tasks;

        emit_payload = Some((
            window_secs,
            success_count,
            interval_for_log,
            queue_depth_for_log,
            running_tasks_for_log,
        ));

        entry.window_start = now;
        entry.last_emit = now;
        entry.success_count = 0;
    }
    drop(guard);

    if let Some((
        window_secs,
        success_count,
        interval_for_log,
        queue_depth_for_log,
        running_tasks_for_log,
    )) = emit_payload
    {
        info!(
            worker_id = %worker_id,
            window_secs,
            success_count,
            interval_secs = interval_for_log,
            queue_depth = ?queue_depth_for_log,
            running_tasks = running_tasks_for_log,
            "Worker 心跳稳定"
        );
    }
}

/// 心跳状态快照（用于监控接口）
#[derive(Debug, Clone, Serialize)]
pub struct WorkerHeartbeatSnapshot {
    pub worker_id: String,
    pub last_seen: DateTime<Utc>,
    pub seconds_since: i64,
    pub interval_secs: u64,
    pub queue_depth: Option<u64>,
    pub running_tasks: Vec<String>,
    pub metrics: Option<WorkerHeartbeatMetrics>,
    pub timed_out: bool,
    pub restart_cooldown_until: Option<DateTime<Utc>>,
}

pub async fn collect_worker_heartbeat_snapshot() -> Vec<WorkerHeartbeatSnapshot> {
    let now = Utc::now();
    let guard = WORKER_HEARTBEATS.read().await;
    guard
        .iter()
        .map(|(worker_id, state)| WorkerHeartbeatSnapshot {
            worker_id: worker_id.clone(),
            last_seen: state.last_seen,
            seconds_since: (now - state.last_seen).num_seconds(),
            interval_secs: state.interval_secs,
            queue_depth: state.queue_depth,
            running_tasks: state.running_tasks.clone(),
            metrics: state.metrics.clone(),
            timed_out: state.was_timed_out,
            restart_cooldown_until: state.restart_cooldown_until,
        })
        .collect()
}

fn detect_ocr_restart_burst(
    previous: &WorkerHeartbeatMetrics,
    current: &WorkerHeartbeatMetrics,
) -> Option<(&'static str, u64)> {
    let prev_restarts = previous.ocr_pool_total_restarted.unwrap_or(0);
    let curr_restarts = current.ocr_pool_total_restarted.unwrap_or(prev_restarts);
    let restart_delta = curr_restarts.saturating_sub(prev_restarts);
    if restart_delta >= OCR_RESTART_BURST_THRESHOLD {
        return Some(("restart", restart_delta));
    }

    let prev_failures = previous.ocr_pool_total_failures.unwrap_or(0);
    let curr_failures = current.ocr_pool_total_failures.unwrap_or(prev_failures);
    let failure_delta = curr_failures.saturating_sub(prev_failures);
    if failure_delta >= OCR_FAILURE_BURST_THRESHOLD {
        return Some(("failure", failure_delta));
    }

    None
}

/// Worker 心跳的最新信息，供 watchdog / 监控等内部逻辑使用
#[derive(Debug, Clone)]
pub struct WorkerHeartbeatInfo {
    pub worker_id: String,
    pub last_seen: DateTime<Utc>,
    pub seconds_since: i64,
    pub interval_secs: u64,
    pub timed_out: bool,
    pub restart_cooldown_until: Option<DateTime<Utc>>,
}

/// 查询 Worker 心跳信息，如果尚未上报则返回 None
pub async fn get_worker_heartbeat_info(worker_id: &str) -> Option<WorkerHeartbeatInfo> {
    let now = Utc::now();
    let guard = WORKER_HEARTBEATS.read().await;
    guard.get(worker_id).map(|state| WorkerHeartbeatInfo {
        worker_id: worker_id.to_string(),
        last_seen: state.last_seen,
        seconds_since: (now - state.last_seen).num_seconds(),
        interval_secs: state.interval_secs,
        timed_out: state.was_timed_out,
        restart_cooldown_until: state.restart_cooldown_until,
    })
}

fn truncate_str(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() > max_chars {
        trimmed.chars().take(max_chars).collect()
    } else {
        trimmed.to_string()
    }
}
