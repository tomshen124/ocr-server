//! 预审处理模块
//! 处理预审请求、状态查询、结果展示等核心业务逻辑

use crate::api::enhancement::{maybe_enhance_error, RequestContext};
use crate::db::traits::{
    CachedMaterialRecord, CachedMaterialStatus, PreviewFailureUpdate, PreviewRequestRecord,
};
use crate::db::traits::{PreviewDedupDecision, PreviewDedupMeta};
use crate::db::PreviewStatus;
use crate::model::preview::{
    Attachment, MaterialValue, Preview, PreviewBody, SceneValue, UserInfo,
};
use crate::model::{Goto, SessionUser};
use crate::util::logging::standards::events;
use crate::util::material_cache::{self, WORKER_CACHE_SCHEME};
use crate::util::rules::{RuleRepository, WorkerRuleCache};
use crate::util::task_queue::{PreviewTask, PreviewTaskHandler, TaskQueue, PREVIEW_QUEUE_NAME};
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use crate::util::worker::{
    build_result_payload, WorkerJobActivityGuard, WorkerJobStatus, WorkerProxyClient,
};
use crate::util::IntoJson;
use crate::{AppState, CONFIG};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use futures::stream::FuturesUnordered;
use futures::StreamExt;
use image::{GenericImageView, ImageFormat};
use mime_guess::get_mime_extensions;
use mime_guess::mime::Mime;
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::sleep;
use url::Url;
use uuid::Uuid;

const PREVIEW_REQUEST_TIMEOUT_SECS: u64 = 30;
const MATERIAL_PREPARE_TIMEOUT_SECS: u64 = 30;
const MATERIAL_ATTACHMENT_TIMEOUT_SECS: u64 = 60;
const MATERIAL_PREPARE_WATCHDOG_SECS: u64 = 120;
const MATERIAL_SLOW_ATTACHMENT_THRESHOLD_MS: u128 = 5_000;
const DUPLICATE_REQUEST_LIMIT: usize = 5;

#[derive(Debug, Clone, Serialize)]
struct SlowAttachmentRecord {
    material_code: String,
    attachment_index: usize,
    source_url: String,
    elapsed_ms: u128,
    outcome: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct DownloadTask {
    material_index: usize,
    attachment_index: usize,
    material_code: String,
    attachment: Attachment,
    original_url: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum MaterialPreparationErrorCode {
    AttachmentTimeout,
    AttachmentDownloadFailed,
    WatchdogTimeout,
}

impl MaterialPreparationErrorCode {
    fn as_str(&self) -> &'static str {
        match self {
            MaterialPreparationErrorCode::AttachmentTimeout => "ATTACHMENT_TIMEOUT",
            MaterialPreparationErrorCode::AttachmentDownloadFailed => "ATTACHMENT_DOWNLOAD_FAILED",
            MaterialPreparationErrorCode::WatchdogTimeout => "WATCHDOG_TIMEOUT",
        }
    }
}

#[derive(Debug)]
struct MaterialPreparationFailure {
    code: MaterialPreparationErrorCode,
    message: String,
    slow_attachments: Vec<SlowAttachmentRecord>,
}

impl MaterialPreparationFailure {
    fn new(
        code: MaterialPreparationErrorCode,
        message: impl Into<String>,
        slow_attachments: Vec<SlowAttachmentRecord>,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            slow_attachments,
        }
    }

    fn code(&self) -> MaterialPreparationErrorCode {
        self.code
    }
}

impl std::fmt::Display for MaterialPreparationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for MaterialPreparationFailure {}

impl From<anyhow::Error> for MaterialPreparationFailure {
    fn from(err: anyhow::Error) -> Self {
        MaterialPreparationFailure::new(
            MaterialPreparationErrorCode::AttachmentDownloadFailed,
            err.to_string(),
            Vec::new(),
        )
    }
}

#[derive(Debug)]
struct MaterialAttachmentError {
    code: MaterialPreparationErrorCode,
    message: String,
    record: SlowAttachmentRecord,
}

impl MaterialAttachmentError {
    fn timeout(task: &DownloadTask, elapsed_ms: u128) -> Self {
        Self {
            code: MaterialPreparationErrorCode::AttachmentTimeout,
            message: format!(
                "附件下载超时 (material={}, attachment_index={})",
                task.material_code, task.attachment_index
            ),
            record: SlowAttachmentRecord {
                material_code: task.material_code.clone(),
                attachment_index: task.attachment_index,
                source_url: task.original_url.clone(),
                elapsed_ms,
                outcome: "timeout",
                error: Some("timeout".to_string()),
            },
        }
    }

    fn download_failed(task: &DownloadTask, elapsed_ms: u128, err: anyhow::Error) -> Self {
        let reason = summarize_download_error(&err);
        Self {
            code: MaterialPreparationErrorCode::AttachmentDownloadFailed,
            message: format!(
                "附件下载失败: {} (material={}, attachment_index={})",
                reason, task.material_code, task.attachment_index
            ),
            record: SlowAttachmentRecord {
                material_code: task.material_code.clone(),
                attachment_index: task.attachment_index,
                source_url: task.original_url.clone(),
                elapsed_ms,
                outcome: "failed",
                error: Some(err.to_string()),
            },
        }
    }

    fn into_failure(self, mut notes: Vec<SlowAttachmentRecord>) -> MaterialPreparationFailure {
        notes.push(self.record);
        MaterialPreparationFailure::new(self.code, self.message, notes)
    }
}

const SEMAPHORE_ACQUIRE_TIMEOUT_SECS: u64 = 600;
const OCR_PROCESS_TIMEOUT_SECS: u64 = 600;
const SUBMISSION_ACQUIRE_TIMEOUT_SECS: u64 = 5;

fn summarize_download_error(err: &anyhow::Error) -> String {
    let msg = err.to_string();
    if let Some(limit) = msg
        .strip_prefix("文件过大: ")
        .and_then(|rest| rest.split("超过上限").nth(1))
    {
        let trimmed = limit.trim();
        return format!("文件大小超过上限{}", trimmed);
    }
    if let Some(limit) = msg
        .strip_prefix("本地文件过大: ")
        .and_then(|rest| rest.split("超过上限").nth(1))
    {
        let trimmed = limit.trim();
        return format!("本地文件大小超过上限{}", trimmed);
    }
    if let Some(rest) = msg.strip_prefix("PDF页数超限: ") {
        let mut parts = rest.split('>');
        if let (Some(actual), Some(limit)) = (parts.next(), parts.next()) {
            return format!(
                "PDF页数超限 (实际 {} 页 / 上限 {} 页)",
                actual.trim(),
                limit.trim()
            );
        }
    }
    if msg.contains("timeout") || msg.contains("超时") {
        return "下载超时，请稍后重试".to_string();
    }
    if let Some(code) = msg
        .strip_prefix("HTTP请求失败: ")
        .and_then(|v| v.split_whitespace().next())
    {
        return format!("远端返回错误状态 {}", code);
    }
    msg
}

/// 主预审处理函数
pub async fn preview(
    State(app_state): State<AppState>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let request_start = Instant::now();
    let timeout_duration = Duration::from_secs(PREVIEW_REQUEST_TIMEOUT_SECS);

    match tokio::time::timeout(
        timeout_duration,
        preview_internal(app_state, req, request_start),
    )
    .await
    {
        Ok(response) => response,
        Err(_) => {
            tracing::error!(
                timeout_secs = PREVIEW_REQUEST_TIMEOUT_SECS,
                "预审请求处理超时"
            );
            let response = Json(serde_json::json!({
                "success": false,
                "errorCode": 504,
                "errorMsg": "请求处理超时，请稍后重试",
                "data": null
            }))
            .into_response();
            finalize_preview_response(response, "request_timeout", request_start)
        }
    }
}

async fn preview_internal(
    app_state: AppState,
    req: axum::extract::Request,
    request_start: Instant,
) -> Response {
    // 提取请求体并保存headers和extensions以便后续使用
    let (parts, body) = req.into_parts();
    let headers = &parts.headers;

    // 获取请求上下文（包含trace_id和功能开关）
    let request_ctx = parts
        .extensions
        .get::<RequestContext>()
        .cloned()
        .unwrap_or_else(|| RequestContext {
            trace_id: "legacy".to_string(),
            enhanced_features: false,
        });

    // 从认证中间件获取SessionUser
    let session_user = parts.extensions.get::<SessionUser>().cloned();
    let mut resolved_user = session_user.clone();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!(
                trace_id = %request_ctx.trace_id,
                error = %e,
                "读取请求体失败"
            );
            let response = maybe_enhance_error("无效的请求体", &request_ctx);
            return finalize_preview_response(response, "read_body_failed", request_start);
        }
    };

    //  开放格式解析：先尝试解析为通用JSON，然后容错处理
    let mut preview_body: PreviewBody = match serde_json::from_slice::<Value>(&bytes) {
        Ok(json_value) => {
            // 策略1：尝试标准格式
            if let Ok(standard_body) = serde_json::from_value::<PreviewBody>(json_value.clone()) {
                tracing::debug!(" 解析为标准PreviewBody格式成功");
                standard_body
            }
            // 策略2：尝试生产环境格式
            else if let Ok(prod_request) = serde_json::from_value::<
                crate::model::preview::ProductionPreviewRequest,
            >(json_value.clone())
            {
                tracing::debug!(" 解析为生产环境格式成功，正在转换...");
                prod_request.to_preview_body()
            }
            // 策略3：创建最小化的兼容结构
            else {
                tracing::warn!(" 无法解析为已知格式，创建兼容结构...");
                create_fallback_preview_body(&json_value)
            }
        }
        Err(e) => {
            tracing::error!("无法解析JSON: {}", e);
            return finalize_preview_response(
                bad_request_response("无效的JSON格式"),
                "invalid_json",
                request_start,
            );
        }
    };

    if let Err(errors) = validate_preview_body(&preview_body) {
        tracing::warn!(
            trace_id = %request_ctx.trace_id,
            errors = ?errors,
            "预审请求参数校验失败"
        );
        return finalize_preview_response(
            bad_request_response(errors.join("; ")),
            "invalid_payload",
            request_start,
        );
    }

    log_raw_preview_request(
        &request_ctx.trace_id,
        &preview_body.preview.request_id,
        &bytes,
    )
    .await;

    //  用户身份验证 - 根据环境模式决定严格程度
    let debug_mode = CONFIG.debug.enabled
        || CONFIG.runtime_mode.mode == "development"
        || std::env::var("ENABLE_AUTH_BYPASS").unwrap_or("false".to_string()) == "true";

    //  记录API调用来源信息，用于审计和统计
    let api_source = headers
        .get("X-API-Source")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let api_version = headers
        .get("X-API-Version")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("v1");

    //  增强日志：记录用户状态传递相关信息
    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let referer = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("none");
    let cookie_header = headers.get("cookie").and_then(|v| v.to_str().ok());
    let x_forwarded_for = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok());

    let third_party_request_id_original = preview_body.preview.request_id.clone();
    let session_user_masked = session_user
        .as_ref()
        .map(|u| mask_identifier(&u.user_id))
        .unwrap_or_else(|| "anonymous".to_string());
    let content_type = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let preview_span = tracing::info_span!(
        "preview_request",
        preview_id = tracing::field::Empty,
        third_party_request_id = %third_party_request_id_original,
        user_id = %mask_identifier(&preview_body.user_id),
        session_user = %session_user_masked,
        request_bytes = bytes.len(),
        content_type = %content_type,
        client_ip = %client_ip,
        api_source = %api_source,
        api_version = %api_version,
    );
    let _preview_span_guard = preview_span.enter();

    let cookie_summary = summarize_header(cookie_header);
    let xff_summary = summarize_header(x_forwarded_for);
    let cookie_present = cookie_header.is_some();
    let cookie_contains_session = cookie_header
        .map(|value| value.contains("session") || value.contains("axum"))
        .unwrap_or(false);

    tracing::info!(
        event = events::PREVIEW_RECEIVED,
        trace_id = %request_ctx.trace_id,
        api_source = %api_source,
        api_version = %api_version,
        request_bytes = bytes.len(),
        user_agent = %user_agent,
        referer = %referer,
        cookie_summary = %cookie_summary,
        xff_summary = %xff_summary,
        request_user = %mask_identifier(&preview_body.user_id)
    );

    tracing::debug!(
        event = "preview.session_state",
        session_user_present = resolved_user.is_some(),
        cookie_present,
        cookie_contains_session
    );

    if let Some(ref session_user) = resolved_user {
        let ids_match = preview_body.user_id == session_user.user_id;
        tracing::debug!(
            event = "preview.session_auth",
            request_user = %mask_identifier(&preview_body.user_id),
            session_user = %mask_identifier(&session_user.user_id),
            ids_match,
            debug_mode
        );

        if !ids_match {
            tracing::warn!(
                event = events::AUTH_FAILURE,
                reason = "user_id_mismatch",
                request_user = %mask_identifier(&preview_body.user_id),
                session_user = %mask_identifier(&session_user.user_id),
                mode = if debug_mode { "debug" } else { "strict" }
            );

            if !debug_mode {
                let response = crate::util::WebResult::err_custom("用户身份验证失败")
                    .into_json()
                    .into_response();
                return finalize_preview_response(response, "user_mismatch", request_start);
            }
        } else {
            tracing::info!(
                event = events::AUTH_SUCCESS,
                source = "session",
                request_user = %mask_identifier(&preview_body.user_id)
            );
        }
    } else {
        tracing::debug!(
            event = events::AUTH_CHECK,
            source = "auto_login",
            session_user_present = false,
            cookie_present,
            cookie_contains_session
        );

        match try_auto_login_and_get_user(headers, &request_ctx).await {
            Ok(auto_login_user) => {
                tracing::info!(
                    event = events::PREVIEW_AUTO_LOGIN,
                    user_id = %mask_identifier(&auto_login_user.user_id),
                    organization = %auto_login_user
                        .organization_name
                        .as_deref()
                        .unwrap_or("未提供"),
                    certificate_type = %auto_login_user.certificate_type,
                    certificate_number_present = auto_login_user.certificate_number.is_some(),
                    phone_present = auto_login_user.phone_number.is_some(),
                    email_present = auto_login_user.email.is_some()
                );

                if let Err(error) = save_user_login_record(
                    &app_state.database,
                    &auto_login_user,
                    "auto_login",
                    headers,
                )
                .await
                {
                    tracing::error!(
                        event = events::PREVIEW_AUTO_LOGIN_FAILED,
                        user_id = %mask_identifier(&auto_login_user.user_id),
                        reason = "persist_failed",
                        error = %error
                    );
                } else {
                    tracing::debug!(
                        event = events::PREVIEW_AUTO_LOGIN,
                        user_id = %mask_identifier(&auto_login_user.user_id),
                        action = "audit_record_saved"
                    );
                }

                preview_body.user_id = auto_login_user.user_id.clone();
                resolved_user = Some(auto_login_user);
            }
            Err(error) => {
                tracing::warn!(
                    event = events::PREVIEW_AUTO_LOGIN_FAILED,
                    reason = "not_detected",
                    error = %error,
                    client_ip = %client_ip,
                    user_agent = %user_agent,
                    referer = %referer
                );

                if debug_mode {
                    tracing::debug!(
                        event = "preview.auto_login_skipped",
                        mode = "debug",
                        request_user = %mask_identifier(&preview_body.user_id)
                    );
                } else {
                    let response = Json(serde_json::json!({
                        "success": false,
                        "need_auth": true,
                        "error_code": 401,
                        "error_msg": "需要用户认证",
                        "sso_url": crate::api::auth::build_sso_login_url(None, Some("person")),
                        "data": null
                    }))
                    .into_response();
                    return finalize_preview_response(response, "auth_required", request_start);
                }
            }
        }
    }

    // 保留第三方系统的原始requestId
    let third_party_request_id = preview_body.preview.request_id.clone();

    // 服务端生成我们自己的安全previewId
    let our_preview_id = crate::api::utils::generate_secure_preview_id();
    tracing::Span::current().record("preview_id", &tracing::field::display(&our_preview_id));
    tracing::debug!(
        event = "preview.id_assigned",
        third_party_request_id = %third_party_request_id,
        preview_id = %our_preview_id
    );

    // 使用我们的previewId作为文件名（确保安全）
    preview_body.preview.request_id = our_preview_id.clone();

    //  安全改进：验证第三方提供的用户ID格式
    if preview_body.user_id.is_empty() || preview_body.user_id.len() > 50 {
        tracing::warn!(
            " 无效的用户ID格式: {}",
            mask_identifier(&preview_body.user_id)
        );
        let response = crate::util::WebResult::err_custom("无效的用户ID")
            .into_json()
            .into_response();
        return finalize_preview_response(response, "invalid_user_id", request_start);
    }

    // 计算材料指纹并对相同数据进行限流/折叠（持久化于 DM）
    // 🔧 增加重复限制到 999999，允许更多重复请求处理
    let materials_hash = compute_materials_hash(&preview_body.preview.material_data);
    let fingerprint = build_dedup_fingerprint(
        &preview_body.user_id,
        &preview_body.preview.matter_id,
        &materials_hash,
    );
    let payload_hash = hex::encode(Sha256::digest(&bytes));

    match app_state
        .database
        .check_and_update_preview_dedup(
            &fingerprint,
            &our_preview_id,
            &PreviewDedupMeta {
                user_id: preview_body.user_id.clone(),
                matter_id: preview_body.preview.matter_id.clone(),
                third_party_request_id: Some(third_party_request_id.clone()),
                payload_hash,
            },
            DUPLICATE_REQUEST_LIMIT as i32,
        )
        .await
    {
        Ok(PreviewDedupDecision::ReuseExisting {
            preview_id: reused_preview_id,
            repeat_count,
        }) => {
            let view_url = CONFIG.preview_view_url(&reused_preview_id);

            tracing::warn!(
                event = "preview.duplicate_rejected",
                preview_id = %our_preview_id,
                reused_preview_id = %reused_preview_id,
                repeat_count = repeat_count,
                materials_hash = %materials_hash,
                "检测到重复材料请求，返回最近结果"
            );

            let response = Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "相同材料请求已完成，返回最近结果",
                "data": {
                    "previewId": reused_preview_id,
                    "thirdPartyRequestId": third_party_request_id,
                    "status": "completed",
                    "message": "重复请求已折叠，复用最近一次的预审结果",
                    "previewUrl": view_url
                }
            }))
            .into_response();

            return finalize_preview_response(response, "duplicate_skipped", request_start);
        }
        Ok(PreviewDedupDecision::Allowed { repeat_count }) => {
            tracing::info!(
                event = "preview.duplicate_count",
                preview_id = %our_preview_id,
                repeat_count = repeat_count,
                materials_hash = %materials_hash,
                "重复材料计数"
            );
        }
        Err(e) => {
            tracing::warn!(
                event = "preview.duplicate_check_failed",
                preview_id = %our_preview_id,
                error = %e,
                "去重检查失败，继续处理"
            );
        }
    }

    // 建立ID映射关系（使用数据库替代文件操作）
    if let Err(e) = save_id_mapping_to_database(
        &app_state.database,
        &our_preview_id,
        &third_party_request_id,
        &preview_body.user_id,
        resolved_user.as_ref(),
    )
    .await
    {
        tracing::error!("保存ID映射失败: {}", e);
        METRICS_COLLECTOR.record_preview_persistence_failure("save_id_mapping");
        let response = crate::util::WebResult::err_custom("系统错误")
            .into_json()
            .into_response();
        return finalize_preview_response(response, "id_mapping_failed", request_start);
    }

    //  NEW: 保存原始请求数据和提取的材料信息
    let original_request_body = String::from_utf8_lossy(&bytes).to_string();
    if let Err(e) = save_original_request_to_database(
        &app_state.database,
        &our_preview_id,
        &original_request_body,
        &preview_body.preview.material_data,
    )
    .await
    {
        tracing::warn!("保存原始请求数据失败: {}", e);
        METRICS_COLLECTOR.record_preview_persistence_failure("save_original_request");
        // 不阻断流程，继续处理
    }

    // 将任务推入材料下载队列
    if let Err(e) = app_state
        .database
        .enqueue_material_download(&our_preview_id, &original_request_body)
        .await
    {
        tracing::error!("入队材料下载任务失败: {}", e);
        let response = crate::util::WebResult::err_custom("系统内部错误: 任务入队失败")
            .into_json()
            .into_response();
        return finalize_preview_response(response, "enqueue_failed", request_start);
    }

    tracing::info!(preview_id = %our_preview_id, "预审任务已入队(材料下载队列)");

    // 立即返回预审访问URL，不等待预审完成
    let view_url = format!("{}/api/preview/view/{}", CONFIG.host, our_preview_id);

    tracing::debug!("立即返回预审访问URL: {}", view_url);

    // 构建响应数据 - 第三方系统只需要知道提交成功
    let response_data = serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "previewId": our_preview_id,
            "thirdPartyRequestId": third_party_request_id,
            "status": "submitted",
        "message": "预审任务已提交，正在后台处理"
    }
    });

    // 预审访问URL是给用户的，不是给第三方系统的
    // 用户会从政务系统跳转到: /api/preview/view/{previewId}
    tracing::debug!("用户预审访问URL: {}", view_url);

    let response = Json(response_data).into_response();
    finalize_preview_response(response, "accepted", request_start)
}

fn bad_request_response(msg: impl Into<String>) -> Response {
    let payload = crate::util::WebResult {
        success: false,
        code: 400,
        msg: msg.into(),
        data: Value::Null,
    };
    (StatusCode::BAD_REQUEST, Json(payload)).into_response()
}

fn validate_preview_body(body: &PreviewBody) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    check_required("userId", &body.user_id, &mut errors);

    let preview = &body.preview;
    check_required("preview.matterId", &preview.matter_id, &mut errors);
    check_required("preview.matterType", &preview.matter_type, &mut errors);
    check_required("preview.matterName", &preview.matter_name, &mut errors);
    check_required("preview.channel", &preview.channel, &mut errors);
    check_required("preview.requestId", &preview.request_id, &mut errors);
    check_required("preview.sequenceNo", &preview.sequence_no, &mut errors);
    check_required(
        "preview.agentInfo.userId",
        &preview.agent_info.user_id,
        &mut errors,
    );
    check_required(
        "preview.subjectInfo.userId",
        &preview.subject_info.user_id,
        &mut errors,
    );

    if preview.material_data.is_empty() {
        errors.push("preview.materialData 不能为空".to_string());
    }

    for (index, material) in preview.material_data.iter().enumerate() {
        check_required(
            &format!("preview.materialData[{index}].code"),
            &material.code,
            &mut errors,
        );
        if material.attachment_list.is_empty() {
            errors.push(format!(
                "preview.materialData[{index}].attachmentList 不能为空"
            ));
        }
        for (a_index, attachment) in material.attachment_list.iter().enumerate() {
            check_required(
                &format!("preview.materialData[{index}].attachmentList[{a_index}].attaName"),
                &attachment.attach_name,
                &mut errors,
            );
            check_required(
                &format!("preview.materialData[{index}].attachmentList[{a_index}].attaUrl"),
                &attachment.attach_url,
                &mut errors,
            );
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn check_required(field: &str, value: &str, errors: &mut Vec<String>) {
    if value.trim().is_empty() {
        errors.push(format!("{} 不能为空", field));
    }
}

fn summarize_header(value: Option<&str>) -> String {
    match value {
        None => "absent".to_string(),
        Some(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                return "present (0 chars)".to_string();
            }
            let separator = if trimmed.contains(',') {
                ','
            } else if trimmed.contains(';') {
                ';'
            } else {
                ' '
            };
            let entry_count = if separator == ' ' {
                1
            } else {
                trimmed
                    .split(separator)
                    .filter(|chunk| !chunk.trim().is_empty())
                    .count()
                    .max(1)
            };
            format!("present ({} chars, {} entries)", trimmed.len(), entry_count)
        }
    }
}

fn mask_identifier(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "<empty>".to_string();
    }
    if trimmed.len() <= 2 {
        "***".to_string()
    } else {
        let head = &trimmed[..2];
        let tail = &trimmed[trimmed.len().saturating_sub(2)..];
        format!("{}***{}", head, tail)
    }
}

fn compute_materials_hash(materials: &[MaterialValue]) -> String {
    let mut normalized = Vec::with_capacity(materials.len());

    for material in materials {
        let mut attachments: Vec<(String, String, bool)> = material
            .attachment_list
            .iter()
            .map(|a| {
                (
                    a.attach_name.clone(),
                    a.attach_url.clone(),
                    a.is_cloud_share,
                )
            })
            .collect();

        attachments.sort_by(|a, b| {
            a.1.cmp(&b.1)
                .then_with(|| a.0.cmp(&b.0))
                .then_with(|| a.2.cmp(&b.2))
        });

        normalized.push((
            material.code.clone(),
            material.name.clone().unwrap_or_default(),
            attachments,
        ));
    }

    normalized.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.len().cmp(&b.2.len()))
    });

    let mut hasher = Sha256::new();
    for (code, name, attachments) in normalized {
        hasher.update(code.as_bytes());
        hasher.update(name.as_bytes());
        for (attach_name, attach_url, is_cloud_share) in attachments {
            hasher.update(attach_name.as_bytes());
            hasher.update(attach_url.as_bytes());
            hasher.update([is_cloud_share as u8]);
        }
    }

    hex::encode(hasher.finalize())
}

fn build_dedup_fingerprint(user_id: &str, matter_id: &str, materials_hash: &str) -> String {
    format!("{user_id}|{matter_id}|{materials_hash}")
}

fn finalize_preview_response(response: Response, reason: &str, start: Instant) -> Response {
    METRICS_COLLECTOR.record_preview_request(response.status().as_u16(), reason, start.elapsed());
    response
}

async fn process_preview_submission_async(
    database: Arc<dyn crate::db::Database>,
    task_queue: Arc<dyn TaskQueue>,
    mut preview_body: PreviewBody,
    preview_id: String,
    third_party_request_id: String,
    download_semaphore: Arc<Semaphore>,
    _submission_permit: OwnedSemaphorePermit,
    session_user: Option<SessionUser>,
) -> Result<()> {
    let third_party_request_id_ref = optional_non_empty(&third_party_request_id);

    if let Err(err) = persist_preview_request_record(
        &database,
        &preview_body,
        &preview_id,
        third_party_request_id_ref,
        session_user.as_ref(),
    )
    .await
    {
        tracing::warn!(
            preview_id = %preview_id,
            error = %err,
            "记录预审请求摘要失败，将继续执行后续流程"
        );
    }

    let slow_attachment_notes = match prepare_material_tokens(
        &mut preview_body,
        &preview_id,
        download_semaphore,
        database.clone(),
    )
    .await
    {
        Ok(notes) => notes,
        Err(failure) => {
            tracing::error!(
                preview_id = %preview_id,
                error_code = %failure.code().as_str(),
                message = %failure,
                "材料预处理失败（后台）"
            );

            let slow_json = serde_json::to_string(&failure.slow_attachments).ok();
            let failure_message = failure.to_string();
            let failure_update = PreviewFailureUpdate {
                preview_id: preview_id.clone(),
                failure_reason: Some(Some(failure_message.clone())),
                failure_context: Some(Some("material_prepare".to_string())),
                last_error_code: Some(Some(failure.code().as_str().to_string())),
                slow_attachment_info_json: Some(slow_json.clone()),
                ocr_stderr_summary: None,
            };

            if let Err(ctx_err) = database
                .update_preview_failure_context(&failure_update)
                .await
            {
                tracing::warn!(
                    preview_id = %preview_id,
                    error = %ctx_err,
                    "记录材料预处理失败上下文信息失败"
                );
            }

            METRICS_COLLECTOR.record_preview_persistence_failure("material_prefetch_failed");
            let status = PreviewStatus::Failed;
            match database
                .update_preview_status(&preview_id, status.clone())
                .await
            {
                Ok(_) => {
                    sync_preview_request_status_inner(
                        &database,
                        &preview_id,
                        third_party_request_id_ref,
                        status,
                    )
                    .await;
                }
                Err(update_err) => {
                    tracing::error!(
                        preview_id = %preview_id,
                        error = %update_err,
                        "材料预处理失败后更新预审状态失败"
                    );
                    METRICS_COLLECTOR
                        .record_preview_persistence_failure("update_status_prefetch_failed");
                }
            }
            tracing::info!(preview_id = %preview_id, "保留材料缓存以便失败排查");
            if !third_party_request_id.is_empty() {
                if let Err(cb_err) = notify_third_party_system(
                    &preview_id,
                    &third_party_request_id,
                    "failed",
                    None,
                    Some(failure.code().as_str()),
                )
                .await
                {
                    tracing::warn!(
                        preview_id = %preview_id,
                        error = %cb_err,
                        "材料预处理失败后通知第三方失败"
                    );
                }
            }
            return Err(anyhow!(failure_message));
        }
    };

    if !slow_attachment_notes.is_empty() {
        if let Ok(json) = serde_json::to_string(&slow_attachment_notes) {
            let mut update = PreviewFailureUpdate::default();
            update.preview_id = preview_id.clone();
            update.slow_attachment_info_json = Some(Some(json));
            if let Err(err) = database.update_preview_failure_context(&update).await {
                tracing::warn!(
                    preview_id = %preview_id,
                    error = %err,
                    "写入慢附件信息失败"
                );
            }
        }

        for note in &slow_attachment_notes {
            tracing::warn!(
                preview_id = %preview_id,
                material_code = %note.material_code,
                attachment_index = note.attachment_index,
                elapsed_ms = note.elapsed_ms,
                "材料下载耗时较长"
            );
        }
    }

    if preview_body.rule_definition.is_none() {
        let repo = RuleRepository::new(Arc::clone(&database));
        match repo.fetch(&preview_body.preview.matter_id).await {
            Ok(Some(config)) => {
                match serde_json::from_str::<serde_json::Value>(&config.record.rule_payload) {
                    Ok(payload) => {
                        tracing::info!(
                            matter_id = %preview_body.preview.matter_id,
                            mode = %config.mode.as_str(),
                            "已将事项规则JSON打包进预审任务载荷"
                        );
                        preview_body.rule_definition = Some(payload);
                    }
                    Err(parse_err) => {
                        tracing::warn!(
                            matter_id = %preview_body.preview.matter_id,
                            error = %parse_err,
                            "事项规则JSON解析失败，任务将回退到默认规则"
                        );
                    }
                }
            }
            Ok(None) => {
                tracing::info!(
                    matter_id = %preview_body.preview.matter_id,
                    "事项未配置规则记录，沿用默认规则"
                );
            }
            Err(err) => {
                tracing::warn!(
                    matter_id = %preview_body.preview.matter_id,
                    error = %err,
                    "加载事项规则配置失败，任务将回退到默认规则"
                );
            }
        }
    }

    let material_total = preview_body.preview.material_data.len();
    let attachment_total: usize = preview_body
        .preview
        .material_data
        .iter()
        .map(|m| m.attachment_list.len())
        .sum();

    let preview_task = PreviewTask::new(
        preview_body,
        preview_id.clone(),
        third_party_request_id.clone(),
    );

    if let Ok(payload_json) = serde_json::to_string(&preview_task) {
        if let Err(err) = database.save_task_payload(&preview_id, &payload_json).await {
            tracing::warn!(
                preview_id = %preview_id,
                error = %err,
                "保存任务payload失败"
            );
            METRICS_COLLECTOR.record_preview_persistence_failure("save_task_payload_failed");
        }
    }

    if let Err(err) = task_queue.enqueue(preview_task).await {
        tracing::error!(
            preview_id = %preview_id,
            error = %err,
            "预审任务入队失败（后台）"
        );
        METRICS_COLLECTOR.record_preview_persistence_failure("queue_enqueue_failed");
        let status = PreviewStatus::Failed;
        match database
            .update_preview_status(&preview_id, status.clone())
            .await
        {
            Ok(_) => {
                sync_preview_request_status_inner(
                    &database,
                    &preview_id,
                    third_party_request_id_ref,
                    status,
                )
                .await;
            }
            Err(db_err) => {
                tracing::error!(
                    preview_id = %preview_id,
                    error = %db_err,
                    "队列入队失败后更新预审状态失败"
                );
                METRICS_COLLECTOR.record_preview_persistence_failure("update_status_queue_failed");
            }
        }
        let _ = database.delete_task_payload(&preview_id).await;
        tracing::info!(preview_id = %preview_id, "入队失败，保留材料缓存以便排查");
        return Err(err);
    }

    let queued_status = PreviewStatus::Queued;
    match database
        .update_preview_status(&preview_id, queued_status.clone())
        .await
    {
        Ok(_) => {
            sync_preview_request_status_inner(
                &database,
                &preview_id,
                third_party_request_id_ref,
                queued_status,
            )
            .await;
        }
        Err(err) => {
            tracing::warn!(
                preview_id = %preview_id,
                error = %err,
                "更新预审状态为 queued 失败"
            );
            METRICS_COLLECTOR.record_preview_persistence_failure("update_status_queued");
        }
    }

    tracing::info!(
        target: "queue.producer",
        event = events::QUEUE_ENQUEUE,
        preview_id = %preview_id,
        third_party_request_id = %third_party_request_id,
        queue = PREVIEW_QUEUE_NAME,
        material_total = material_total as u32,
        attachment_total = attachment_total as u32
    );

    Ok(())
}

async fn prepare_material_tokens(
    preview_body: &mut PreviewBody,
    preview_id: &str,
    download_semaphore: Arc<Semaphore>,
    database: Arc<dyn crate::db::Database>,
) -> Result<Vec<SlowAttachmentRecord>, MaterialPreparationFailure> {
    struct DownloadedAttachment {
        material_index: usize,
        attachment_index: usize,
        original_url: String,
        bytes: Vec<u8>,
        content_type: Option<String>,
        preferred_filename: String,
        elapsed_ms: u128,
    }

    struct DownloadOutcome {
        attachment: DownloadedAttachment,
        slow_note: Option<SlowAttachmentRecord>,
    }

    let mut tasks = Vec::new();

    for (material_index, material) in preview_body.preview.material_data.iter().enumerate() {
        for (attachment_index, attachment) in material.attachment_list.iter().enumerate() {
            let original_url = attachment.attach_url.clone();

            if original_url.starts_with(WORKER_CACHE_SCHEME) {
                continue;
            }

            let cloned_attachment = Attachment {
                attach_name: attachment.attach_name.clone(),
                attach_url: original_url.clone(),
                is_cloud_share: attachment.is_cloud_share,
                extra: attachment.extra.clone(),
            };

            tasks.push(DownloadTask {
                material_index,
                attachment_index,
                material_code: material.code.clone(),
                attachment: cloned_attachment,
                original_url,
            });
        }
    }

    if tasks.is_empty() {
        return Ok(Vec::new());
    }

    let mut download_tasks = FuturesUnordered::new();
    let per_attachment_timeout = Duration::from_secs(MATERIAL_ATTACHMENT_TIMEOUT_SECS);

    for task in tasks.into_iter() {
        let semaphore = download_semaphore.clone();
        download_tasks.push(async move {
            let permit = match semaphore.acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    return Err(MaterialAttachmentError::download_failed(
                        &task,
                        0,
                        anyhow!("下载并发控制信号量不可用"),
                    ));
                }
            };

            let started = Instant::now();
            let timeout_future = tokio::time::timeout(
                per_attachment_timeout,
                fetch_attachment_bytes(
                    &task.material_code,
                    &task.attachment,
                    task.attachment_index,
                ),
            )
            .await;

            drop(permit);

            let elapsed_ms = started.elapsed().as_millis();
            match timeout_future {
                Ok(Ok((bytes, content_type, preferred_filename))) => {
                    let slow_note = if elapsed_ms >= MATERIAL_SLOW_ATTACHMENT_THRESHOLD_MS {
                        Some(SlowAttachmentRecord {
                            material_code: task.material_code.clone(),
                            attachment_index: task.attachment_index,
                            source_url: task.original_url.clone(),
                            elapsed_ms,
                            outcome: "slow",
                            error: None,
                        })
                    } else {
                        None
                    };

                    Ok(DownloadOutcome {
                        attachment: DownloadedAttachment {
                            material_index: task.material_index,
                            attachment_index: task.attachment_index,
                            original_url: task.original_url.clone(),
                            bytes,
                            content_type,
                            preferred_filename,
                            elapsed_ms,
                        },
                        slow_note,
                    })
                }
                Ok(Err(err)) => Err(MaterialAttachmentError::download_failed(
                    &task, elapsed_ms, err,
                )),
                Err(_) => Err(MaterialAttachmentError::timeout(&task, elapsed_ms)),
            }
        });
    }

    let watchdog_timeout = Duration::from_secs(MATERIAL_PREPARE_WATCHDOG_SECS);
    let (downloads, mut slow_notes) = match tokio::time::timeout(watchdog_timeout, async {
        let mut collected = Vec::new();
        let mut slow_notes = Vec::new();

        while let Some(item) = download_tasks.next().await {
            match item {
                Ok(outcome) => {
                    if let Some(note) = outcome.slow_note {
                        slow_notes.push(note);
                    }
                    collected.push(outcome.attachment);
                }
                Err(err) => {
                    return Err(err.into_failure(slow_notes));
                }
            }
        }

        Ok((collected, slow_notes))
    })
    .await
    {
        Ok(Ok(result)) => result,
        Ok(Err(failure)) => return Err(failure),
        Err(_) => {
            return Err(MaterialPreparationFailure::new(
                MaterialPreparationErrorCode::WatchdogTimeout,
                format!("材料预处理超过 {} 秒未完成", MATERIAL_PREPARE_WATCHDOG_SECS),
                Vec::new(),
            ));
        }
    };

    for item in downloads {
        let material = preview_body
            .preview
            .material_data
            .get_mut(item.material_index)
            .ok_or_else(|| anyhow!("材料索引越界: {}", item.material_index))?;

        let attachment = material
            .attachment_list
            .get_mut(item.attachment_index)
            .ok_or_else(|| anyhow!("附件索引越界: {}", item.attachment_index))?;

        let size_bytes = item.bytes.len();
        let (is_pdf, page_count) = analyze_attachment_upload(&item.bytes);
        tracing::info!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_UPLOAD_PROFILE,
            preview_id = %preview_id,
            material_code = %material.code,
            material_name = %material.name.as_deref().unwrap_or_default(),
            attachment_index = item.attachment_index,
            attachment_name = %attachment.attach_name,
            size_bytes = size_bytes as u64,
            size_kb = size_bytes / 1024,
            is_pdf,
            pages = page_count.unwrap_or(0),
            download_ms = item.elapsed_ms as u64
        );

        let token = material_cache::store_material(
            preview_id,
            &material.code,
            &item.preferred_filename,
            &item.bytes,
            item.content_type.clone(),
        )
        .await
        .map_err(|err| {
            let mut notes_clone = slow_notes.clone();
            notes_clone.push(SlowAttachmentRecord {
                material_code: material.code.clone(),
                attachment_index: item.attachment_index,
                source_url: item.original_url.clone(),
                elapsed_ms: item.elapsed_ms,
                outcome: "failed",
                error: Some(err.to_string()),
            });
            MaterialPreparationFailure::new(
                MaterialPreparationErrorCode::AttachmentDownloadFailed,
                format!("缓存材料失败: {}", err),
                notes_clone,
            )
        })?;

        if let Some(local_path) = material_cache::get_material_path(&token.token).await {
            let record_id =
                material_cache::cached_record_id(preview_id, &material.code, item.attachment_index);
            let cache_record = CachedMaterialRecord {
                id: record_id,
                preview_id: preview_id.to_string(),
                material_code: material.code.clone(),
                attachment_index: item.attachment_index as i32,
                token: token.token.clone(),
                local_path: local_path.display().to_string(),
                upload_status: CachedMaterialStatus::Downloaded,
                oss_key: None,
                last_error: None,
                file_size: Some(item.bytes.len() as i64),
                checksum_sha256: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            };

            if let Err(err) = database.upsert_cached_material_record(&cache_record).await {
                tracing::warn!(
                    preview_id = %preview_id,
                    material_code = %material.code,
                    attachment_index = item.attachment_index,
                    error = %err,
                    "记录材料缓存状态失败"
                );
            } else {
                tracing::info!(
                    preview_id = %preview_id,
                    material_code = %material.code,
                    attachment_index = item.attachment_index,
                    token = %token.token,
                    "材料缓存已落盘"
                );
            }
        } else {
            tracing::warn!(
                preview_id = %preview_id,
                material_code = %material.code,
                attachment_index = item.attachment_index,
                "无法获取材料缓存路径，跳过状态记录"
            );
        }

        attachment.attach_url = format!("{}{}", WORKER_CACHE_SCHEME, token.token);
        if attachment.attach_name.trim().is_empty() {
            attachment.attach_name = token.filename.clone();
        }

        attachment
            .extra
            .insert("originalUrl".to_string(), Value::String(item.original_url));
        attachment.extra.insert(
            "materialToken".to_string(),
            Value::String(token.token.clone()),
        );
        attachment.extra.insert(
            "cachedFileName".to_string(),
            Value::String(token.filename.clone()),
        );
        if let Some(ct) = item.content_type {
            attachment
                .extra
                .insert("contentType".to_string(), Value::String(ct));
        }
    }

    Ok(slow_notes)
}
async fn fetch_attachment_bytes(
    material_code: &str,
    attachment: &crate::model::preview::Attachment,
    index: usize,
) -> Result<(Vec<u8>, Option<String>, String)> {
    let url = attachment.attach_url.trim();
    if url.is_empty() {
        return Err(anyhow!("附件URL为空"));
    }

    if url.starts_with("data:") {
        let (bytes, content_type) = decode_data_url(url)?;
        let filename = derive_filename(
            attachment,
            material_code,
            index,
            content_type.as_deref(),
            url,
        );
        return Ok((bytes, content_type, filename));
    }

    let mut bytes = crate::util::zen::downloader::download_file_content(url)
        .await
        .map_err(|e| anyhow!("下载附件失败: {} (原因: {})", url, e))?;

    let mut content_type = mime_guess::from_path(url)
        .first_raw()
        .map(|s| s.to_string());
    if content_type.is_none() {
        if let Ok(parsed) = Url::parse(url) {
            if let Some(segment) = parsed
                .path_segments()
                .and_then(|segments| segments.last())
                .filter(|segment| segment.contains('.'))
            {
                if let Some(ext) = segment.rsplit('.').next() {
                    content_type = mime_guess::from_ext(ext).first_raw().map(|s| s.to_string());
                }
            }
        }
    }

    let mut filename = derive_filename(
        attachment,
        material_code,
        index,
        content_type.as_deref(),
        url,
    );
    if should_convert_docx(&filename, content_type.as_deref()) {
        match crate::util::converter::docx_to_pdf_bytes(bytes.clone()).await {
            Ok(pdf_bytes) => {
                tracing::debug!(
                    "DOCX 附件转换为 PDF 成功: material={} attachment_index={}",
                    material_code,
                    index
                );
                bytes = pdf_bytes;
                content_type = Some("application/pdf".to_string());
                filename = ensure_pdf_extension(&filename, "pdf");
            }
            Err(e) => {
                tracing::warn!(
                    "DOCX 附件转换为 PDF 失败: material={} attachment_index={} error={}",
                    material_code,
                    index,
                    e
                );
            }
        }
    }

    coerce_to_supported_media(bytes, content_type, filename, url, material_code, index)
}

fn decode_data_url(data_url: &str) -> Result<(Vec<u8>, Option<String>)> {
    let without_prefix = data_url
        .strip_prefix("data:")
        .ok_or_else(|| anyhow!("无效的 data URL"))?;

    let (meta, data) = without_prefix
        .split_once(',')
        .ok_or_else(|| anyhow!("无效的 data URL"))?;

    if !meta.contains(";base64") {
        return Err(anyhow!("仅支持 base64 data URL"));
    }

    let mime_type = meta
        .split(';')
        .next()
        .filter(|part| !part.is_empty())
        .map(|s| s.to_string());

    let decoded = general_purpose::STANDARD
        .decode(data.trim())
        .map_err(|e| anyhow!("Base64解码失败: {}", e))?;

    Ok((decoded, mime_type))
}

fn derive_filename(
    attachment: &crate::model::preview::Attachment,
    material_code: &str,
    index: usize,
    content_type: Option<&str>,
    url: &str,
) -> String {
    let trimmed = attachment.attach_name.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }

    if let Ok(parsed) = Url::parse(url) {
        if let Some(seg) = parsed
            .path_segments()
            .and_then(|segments| segments.last())
            .filter(|segment| !segment.is_empty())
        {
            return seg.to_string();
        }
    }

    if let Some(ct) = content_type {
        if let Ok(parsed_mime) = ct.parse::<Mime>() {
            if let Some(exts) = get_mime_extensions(&parsed_mime) {
                if let Some(ext) = exts.first() {
                    return format!("{}_{}.{ext}", material_code, index + 1);
                }
            }
        }
    }

    format!("{}_{}.bin", material_code, index + 1)
}

fn should_convert_docx(filename: &str, content_type: Option<&str>) -> bool {
    let lower = filename.to_ascii_lowercase();
    if lower.ends_with(".docx") {
        return true;
    }
    matches!(
        content_type,
        Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document")
    )
}

fn ensure_pdf_extension(filename: &str, ext: &str) -> String {
    let sanitized_ext = ext.trim_start_matches('.');
    let mut path = PathBuf::from(filename);
    path.set_extension(sanitized_ext);
    let candidate = path.to_string_lossy().into_owned();
    if candidate.trim().is_empty() {
        format!("{}.{}", filename, sanitized_ext)
    } else {
        candidate
    }
}

fn ensure_image_extension(filename: &str, ext: &str) -> String {
    let sanitized_ext = ext.trim_start_matches('.');
    let mut path = PathBuf::from(filename);
    path.set_extension(sanitized_ext);
    let candidate = path.to_string_lossy().into_owned();
    if candidate.trim().is_empty() {
        format!("{}.{}", filename, sanitized_ext)
    } else {
        candidate
    }
}

fn coerce_to_supported_media(
    bytes: Vec<u8>,
    content_type: Option<String>,
    filename: String,
    url: &str,
    material_code: &str,
    attachment_index: usize,
) -> Result<(Vec<u8>, Option<String>, String)> {
    let lower_ct = content_type.as_deref().map(|ct| ct.to_ascii_lowercase());
    let file_lower = filename.to_ascii_lowercase();
    let is_pdf = lower_ct
        .as_deref()
        .map(|ct| ct == "application/pdf" || ct == "application/x-pdf")
        .unwrap_or(false)
        || file_lower.ends_with(".pdf");

    if is_pdf {
        return Ok((
            bytes,
            Some("application/pdf".to_string()),
            ensure_pdf_extension(&filename, "pdf"),
        ));
    }

    match image::load_from_memory(&bytes) {
        Ok(img) => {
            let mut buf = Vec::new();
            let (w, h) = img.dimensions();
            img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                .map_err(|e| anyhow!("图片重编码失败: {}", e))?;
            tracing::info!(
                material_code = %material_code,
                attachment_index,
                url = %url,
                bytes = buf.len(),
                mime = "image/png",
                width = w,
                height = h,
                "附件标准化为PNG"
            );
            Ok((
                buf,
                Some("image/png".to_string()),
                ensure_image_extension(&filename, "png"),
            ))
        }
        Err(e) => Err(anyhow!(
            "[DATA_ERR:UNSUPPORTED_MEDIA] 附件无法解码为图片或PDF: {} (url={})",
            e,
            url
        )),
    }
}

fn analyze_attachment_upload(bytes: &[u8]) -> (bool, Option<u32>) {
    if bytes.len() > 4 && &bytes[0..4] == b"%PDF" {
        (true, quick_pdf_page_scan(bytes))
    } else {
        (false, Some(1))
    }
}

fn quick_pdf_page_scan(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 8 {
        return None;
    }
    let scan_len = bytes.len().min(4 * 1024 * 1024);
    let haystack = std::str::from_utf8(&bytes[..scan_len]).ok()?;
    let count = haystack.matches("/Type /Page").count();
    Some(count.max(1) as u32)
}

pub struct LocalPreviewTaskHandler {
    database: Arc<dyn crate::db::Database>,
    storage: Arc<dyn crate::storage::Storage>,
}

impl LocalPreviewTaskHandler {
    pub fn new(
        database: Arc<dyn crate::db::Database>,
        storage: Arc<dyn crate::storage::Storage>,
    ) -> Self {
        Self { database, storage }
    }
}

#[async_trait]
impl PreviewTaskHandler for LocalPreviewTaskHandler {
    async fn handle_preview_task(&self, task: PreviewTask) -> Result<()> {
        let mut preview_body = task.preview_body;
        let preview_id = task.preview_id;
        let third_party_request_id = task.third_party_request_id;
        let database_clone = self.database.clone();
        let storage_clone = self.storage.clone();
        let job_start = Instant::now();

        let job_span = tracing::info_span!(
            "preview_job",
            preview_id = %preview_id,
            third_party_request_id = %third_party_request_id,
            worker = "master"
        );
        let _job_guard = job_span.enter();

        let third_party_request_id_ref = optional_non_empty(&third_party_request_id);

        let mut permit = match crate::OCR_SEMAPHORE.try_acquire() {
            Ok(permit) => {
                tracing::debug!(
                    preview_id = %preview_id,
                    available_permits = crate::OCR_SEMAPHORE.available_permits(),
                    "获取OCR处理许可成功"
                );
                Some(permit)
            }
            Err(_) => {
                tracing::warn!(preview_id = %preview_id, "系统繁忙，OCR任务排队等待");
                match tokio::time::timeout(
                    Duration::from_secs(SEMAPHORE_ACQUIRE_TIMEOUT_SECS),
                    crate::OCR_SEMAPHORE.acquire(),
                )
                .await
                {
                    Ok(result) => match result {
                        Ok(permit) => {
                            tracing::debug!(
                                preview_id = %preview_id,
                                "等待后获取OCR处理许可成功"
                            );
                            Some(permit)
                        }
                        Err(e) => {
                            tracing::error!(
                                preview_id = %preview_id,
                                error = %e,
                                "获取OCR处理许可失败"
                            );
                            let status = PreviewStatus::Failed;
                            match database_clone
                                .update_preview_status(&preview_id, status.clone())
                                .await
                            {
                                Ok(_) => {
                                    sync_preview_request_status_inner(
                                        &database_clone,
                                        &preview_id,
                                        third_party_request_id_ref,
                                        status,
                                    )
                                    .await;
                                }
                                Err(db_err) => {
                                    tracing::error!(
                                        preview_id = %preview_id,
                                        error = %db_err,
                                        "更新预审状态失败"
                                    );
                                    METRICS_COLLECTOR.record_preview_persistence_failure(
                                        "update_status_permit_fail",
                                    );
                                }
                            }
                            METRICS_COLLECTOR.record_preview_job(false, job_start.elapsed());
                            return Ok(());
                        }
                    },
                    Err(_) => {
                        tracing::error!(
                            preview_id = %preview_id,
                            timeout_secs = SEMAPHORE_ACQUIRE_TIMEOUT_SECS,
                            "等待OCR处理许可超时"
                        );
                        let status = PreviewStatus::Failed;
                        match database_clone
                            .update_preview_status(&preview_id, status.clone())
                            .await
                        {
                            Ok(_) => {
                                sync_preview_request_status_inner(
                                    &database_clone,
                                    &preview_id,
                                    third_party_request_id_ref,
                                    status,
                                )
                                .await;
                            }
                            Err(db_err) => {
                                tracing::error!(
                                    preview_id = %preview_id,
                                    error = %db_err,
                                    "更新预审状态失败"
                                );
                                METRICS_COLLECTOR.record_preview_persistence_failure(
                                    "update_status_permit_timeout",
                                );
                            }
                        }
                        METRICS_COLLECTOR
                            .record_preview_persistence_failure("semaphore_acquire_timeout");
                        METRICS_COLLECTOR.record_preview_job(false, job_start.elapsed());
                        return Ok(());
                    }
                }
            }
        };

        tracing::debug!(
            preview_id = %preview_id,
            third_party_request_id = %third_party_request_id,
            available_permits = crate::OCR_SEMAPHORE.available_permits(),
            "开始自动预审任务（并发控制）"
        );

        // 规则配置已改为事项级别，与主题无关，沿用主题字段记录事项ID
        if preview_body.preview.theme_id.is_none() {
            preview_body.preview.theme_id = Some(preview_body.preview.matter_id.clone());
        }
        tracing::debug!(
            "预审任务使用事项规则: {} ({})",
            preview_body.preview.theme_id.as_deref().unwrap_or(""),
            preview_body.preview.matter_name
        );

        let attempt_id = Uuid::new_v4().to_string();
        if let Err(e) = database_clone
            .mark_preview_processing(&preview_id, "master", &attempt_id)
            .await
        {
            tracing::error!(preview_id = %preview_id, error = %e, "标记预审任务Processing状态失败");
            METRICS_COLLECTOR.record_preview_persistence_failure("mark_processing");
        } else {
            tracing::debug!(
                preview_id = %preview_id,
                attempt_id = %attempt_id,
                "记录Processing状态"
            );
            sync_preview_request_status_inner(
                &database_clone,
                &preview_id,
                third_party_request_id_ref,
                PreviewStatus::Processing,
            )
            .await;
        }

        let execution_future =
            preview_body.execute_preview(Some(storage_clone.clone()), Some(database_clone.clone()));
        let execution_result = match tokio::time::timeout(
            Duration::from_secs(OCR_PROCESS_TIMEOUT_SECS),
            execution_future,
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                tracing::error!(
                    preview_id = %preview_id,
                    timeout_secs = OCR_PROCESS_TIMEOUT_SECS,
                    "OCR 处理超时，终止任务"
                );
                METRICS_COLLECTOR.record_preview_ocr_timeout(&preview_id);
                METRICS_COLLECTOR.record_preview_job(false, job_start.elapsed());
                let status = PreviewStatus::Failed;
                match database_clone
                    .update_preview_status(&preview_id, status.clone())
                    .await
                {
                    Ok(_) => {
                        sync_preview_request_status_inner(
                            &database_clone,
                            &preview_id,
                            third_party_request_id_ref,
                            status,
                        )
                        .await;
                    }
                    Err(err) => {
                        tracing::error!(
                            preview_id = %preview_id,
                            error = %err,
                            "OCR 超时后更新预审状态失败"
                        );
                        METRICS_COLLECTOR
                            .record_preview_persistence_failure("update_status_ocr_timeout");
                    }
                }
                METRICS_COLLECTOR.record_preview_persistence_failure("ocr_processing_timeout");
                if let Err(err) = notify_third_party_system(
                    &preview_id,
                    &third_party_request_id,
                    "failed",
                    None,
                    Some("WORKER_JOB_TIMEOUT"),
                )
                .await
                {
                    tracing::warn!(
                        preview_id = %preview_id,
                        error = %err,
                        "OCR 超时后通知第三方失败"
                    );
                }
                return Ok(());
            }
        };

        let success = execution_result
            .as_ref()
            .map(|output| output.evaluation_result.is_some())
            .unwrap_or(false);

        let worker_error_code = if success {
            None
        } else {
            Some("WORKER_JOB_FAILED")
        };

        let status = if success {
            PreviewStatus::Completed
        } else {
            PreviewStatus::Failed
        };

        match database_clone
            .update_preview_status(&preview_id, status.clone())
            .await
        {
            Ok(_) => {
                sync_preview_request_status_inner(
                    &database_clone,
                    &preview_id,
                    third_party_request_id_ref,
                    status.clone(),
                )
                .await;
            }
            Err(e) => {
                tracing::error!(preview_id = %preview_id, error = %e, "更新最终预审状态失败");
                METRICS_COLLECTOR.record_preview_persistence_failure("update_status_final");
            }
        }

        METRICS_COLLECTOR.record_preview_job(success, job_start.elapsed());

        match execution_result {
            Ok(output) if success => {
                tracing::info!(
                    event = events::PREVIEW_COMPLETE,
                    preview_id = %preview_id,
                    duration_ms = job_start.elapsed().as_millis() as u64
                );
                tracing::debug!(preview_id = %preview_id, ?output.web_result, "预审结果");

                if let Some(report) = output.generated_report.as_ref() {
                    if let Some(file_name) = report.html_path.file_name().and_then(|n| n.to_str()) {
                        let canonical_view_url = CONFIG.preview_view_url(&preview_id);
                        let stored_preview_url = report
                            .remote_html_url
                            .clone()
                            .unwrap_or_else(|| report.preview_url.clone());
                        let mut preferred_download_url = report
                            .remote_pdf_url
                            .clone()
                            .or(report.remote_html_url.clone());
                        let fallback_download_url =
                            crate::util::callbacks::build_default_download_url(&preview_id);
                        if preferred_download_url.is_none() {
                            preferred_download_url = fallback_download_url.clone();
                        }
                        if let Err(err) = database_clone
                            .update_preview_artifacts(
                                &preview_id,
                                file_name,
                                &stored_preview_url,
                                Some(canonical_view_url.as_str()),
                                preferred_download_url.as_deref(),
                            )
                            .await
                        {
                            tracing::warn!(
                                preview_id = %preview_id,
                                error = %err,
                                "更新预审报表信息失败"
                            );
                            METRICS_COLLECTOR
                                .record_preview_persistence_failure("update_preview_artifacts");
                        }
                    }
                }

                if let Err(e) = notify_third_party_system(
                    &preview_id,
                    &third_party_request_id,
                    "completed",
                    Some(&output.web_result),
                    None,
                )
                .await
                {
                    tracing::warn!(preview_id = %preview_id, error = %e, "通知第三方系统失败");
                }
            }
            Ok(output) => {
                tracing::error!(
                    preview_id = %preview_id,
                    "预审任务失败: 缺少评估结果，无法确认处理成功"
                );
                if let Err(notify_err) = notify_third_party_system(
                    &preview_id,
                    &third_party_request_id,
                    "failed",
                    Some(&output.web_result),
                    worker_error_code,
                )
                .await
                {
                    tracing::warn!(
                        preview_id = %preview_id,
                        error = %notify_err,
                        "通知第三方系统失败"
                    );
                }
            }
            Err(e) => {
                tracing::error!(preview_id = %preview_id, error = %e, "预审任务失败");
                if let Err(notify_err) = notify_third_party_system(
                    &preview_id,
                    &third_party_request_id,
                    "failed",
                    None,
                    worker_error_code,
                )
                .await
                {
                    tracing::warn!(
                        preview_id = %preview_id,
                        error = %notify_err,
                        "通知第三方系统失败"
                    );
                }
            }
        }

        if success {
            if let Err(err) = material_cache::cleanup_preview(&preview_id).await {
                tracing::warn!(preview_id = %preview_id, error = %err, "预审材料缓存清理失败");
            }
            if let Err(err) = database_clone
                .delete_cached_materials_by_preview(&preview_id)
                .await
            {
                tracing::warn!(
                    preview_id = %preview_id,
                    error = %err,
                    "清理材料缓存记录失败"
                );
            }
        } else {
            tracing::info!(preview_id = %preview_id, "任务失败，保留材料缓存供排查");
        }

        if let Some(_permit) = permit {
            tracing::debug!(
                preview_id = %preview_id,
                available_permits = crate::OCR_SEMAPHORE.available_permits() + 1,
                "释放OCR处理许可"
            );
        }

        tracing::debug!(preview_id = %preview_id, "自动预审任务结束（并发控制）");

        Ok(())
    }
}

pub struct RemotePreviewTaskHandler {
    client: Arc<WorkerProxyClient>,
}

impl RemotePreviewTaskHandler {
    pub fn new(client: Arc<WorkerProxyClient>) -> Self {
        Self { client }
    }
}

#[async_trait]
impl PreviewTaskHandler for RemotePreviewTaskHandler {
    async fn handle_preview_task(&self, task: PreviewTask) -> Result<()> {
        let mut preview_body = task.preview_body;
        let preview_id = task.preview_id;
        let third_party_request_id = task.third_party_request_id;
        let job_start = Instant::now();

        let matter_id = preview_body.preview.matter_id.clone();
        let rule_cache = WorkerRuleCache::global();

        if let Some(rule_value) = preview_body.rule_definition.clone() {
            match rule_cache.remember(&matter_id, rule_value).await {
                Ok(handle) => {
                    preview_body.rule_definition = Some((*handle.json).clone());
                    preview_body.parsed_rule_definition = Some(handle.definition.clone());
                    tracing::debug!(
                        preview_id = %preview_id,
                        matter_id = %matter_id,
                        fingerprint = %handle.fingerprint,
                        "Worker 缓存事项规则定义"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        preview_id = %preview_id,
                        matter_id = %matter_id,
                        error = %err,
                        "Worker 解析事项规则失败，将继续执行"
                    );
                }
            }
        } else if let Some(handle) = rule_cache.get(&matter_id).await {
            preview_body.rule_definition = Some((*handle.json).clone());
            preview_body.parsed_rule_definition = Some(handle.definition.clone());
            tracing::info!(
                preview_id = %preview_id,
                matter_id = %matter_id,
                fingerprint = %handle.fingerprint,
                "Worker 命中规则缓存，使用本地定义"
            );
        }

        let worker_span = tracing::info_span!(
            "preview_job_worker",
            preview_id = %preview_id,
            worker_id = %self.client.worker_id(),
            third_party_request_id = %third_party_request_id
        );
        let _worker_guard = worker_span.enter();

        let permit = match crate::OCR_SEMAPHORE.try_acquire() {
            Ok(permit) => {
                tracing::debug!(
                    preview_id = %preview_id,
                    available_permits = crate::OCR_SEMAPHORE.available_permits(),
                    "Worker 获取OCR处理许可成功"
                );
                Some(permit)
            }
            Err(_) => {
                tracing::warn!(preview_id = %preview_id, "Worker 系统繁忙，OCR任务排队等待");
                match crate::OCR_SEMAPHORE.acquire().await {
                    Ok(permit) => {
                        tracing::debug!(
                            preview_id = %preview_id,
                            "Worker 等待后获取OCR处理许可成功"
                        );
                        Some(permit)
                    }
                    Err(e) => {
                        tracing::error!(
                            preview_id = %preview_id,
                            error = %e,
                            "Worker 获取OCR处理许可失败"
                        );
                        METRICS_COLLECTOR.record_preview_job(false, job_start.elapsed());
                        return Ok(());
                    }
                }
            }
        };

        let _activity_guard = WorkerJobActivityGuard::new(preview_id.clone());

        tracing::debug!(
            preview_id = %preview_id,
            third_party_request_id = %third_party_request_id,
            "Worker 预审任务开始"
        );

        let attempt_id = Uuid::new_v4().to_string();
        if let Err(err) = self
            .client
            .notify_job_started(&preview_id, &attempt_id)
            .await
        {
            tracing::error!(
                preview_id = %preview_id,
                attempt_id = %attempt_id,
                error = %err,
                "Worker 上报任务开始失败"
            );
            METRICS_COLLECTOR.record_preview_job(false, job_start.elapsed());
            return Err(err);
        }

        if preview_body.preview.theme_id.is_none() {
            preview_body.preview.theme_id = Some(preview_body.preview.matter_id.clone());
        }
        tracing::debug!(
            preview_id = %preview_id,
            theme = %preview_body.preview.theme_id.as_deref().unwrap_or(""),
            matter = %preview_body.preview.matter_name,
            "Worker 预审任务采用事项规则"
        );

        let execution_result = preview_body
            .execute_preview_with_options(None, None, false)
            .await;

        let mut failure_reason: Option<String> = None;
        let mut evaluation_result = None;
        let mut web_result = None;

        match execution_result {
            Ok(ref output) => {
                web_result = Some(output.web_result.clone());
                if let Some(eval) = output.evaluation_result.clone() {
                    evaluation_result = Some(eval);
                } else {
                    failure_reason = Some("evaluation_result_missing".to_string());
                }
            }
            Err(ref err) => {
                tracing::error!(preview_id = %preview_id, error = %err, "Worker 预审任务失败");
                failure_reason = Some(err.to_string());
            }
        }

        let success = evaluation_result.is_some() && failure_reason.is_none();

        METRICS_COLLECTOR.record_preview_job(success, job_start.elapsed());

        let payload = build_result_payload(
            if success {
                WorkerJobStatus::Completed
            } else {
                WorkerJobStatus::Failed
            },
            failure_reason.clone(),
            evaluation_result.clone(),
            web_result.clone(),
            job_start.elapsed(),
            attempt_id.clone(),
        );

        tracing::info!(
            target: "worker.pipeline",
            event = events::WORKER_RESULT_SUBMIT,
            preview_id = %preview_id,
            attempt_id = %attempt_id,
            status = if success { "completed" } else { "failed" },
            worker_id = %self.client.worker_id()
        );

        if let Err(err) = self.client.submit_result(&preview_id, &payload).await {
            tracing::error!(preview_id = %preview_id, attempt_id = %attempt_id, error = %err, "Worker 上报结果失败");
            METRICS_COLLECTOR.record_preview_persistence_failure("worker_submit_result");
            return Err(err);
        }

        if success {
            tracing::info!(
                event = events::PREVIEW_COMPLETE,
                preview_id = %preview_id,
                attempt_id = %attempt_id,
                worker = %self.client.worker_id(),
                duration_ms = job_start.elapsed().as_millis() as u64
            );
        }

        if let Some(_permit) = permit {
            tracing::debug!(
                preview_id = %preview_id,
                available_permits = crate::OCR_SEMAPHORE.available_permits() + 1,
                "Worker 释放OCR处理许可"
            );
        }

        tracing::debug!(preview_id = %preview_id, "Worker 预审任务结束");

        Ok(())
    }
}

pub(crate) fn truncate_str(value: &str, max_chars: usize) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() > max_chars {
        trimmed.chars().take(max_chars).collect()
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn derive_request_record_id(third_party_id: Option<&str>, preview_id: &str) -> String {
    third_party_id
        .and_then(|id| {
            let trimmed = id.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(truncate_str(trimmed, 100))
            }
        })
        .unwrap_or_else(|| truncate_str(preview_id, 100))
}

fn optional_non_empty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

pub(crate) async fn persist_preview_request_record(
    database: &Arc<dyn crate::db::Database>,
    preview_body: &PreviewBody,
    preview_id: &str,
    third_party_request_id: Option<&str>,
    user_info: Option<&SessionUser>,
) -> Result<()> {
    let request_id = derive_request_record_id(third_party_request_id, preview_id);
    let third_party_id = third_party_request_id
        .map(|s| truncate_str(s, 200))
        .filter(|s| !s.is_empty());

    let agent_info_json = serde_json::to_string(&preview_body.preview.agent_info).ok();
    let subject_info_json = serde_json::to_string(&preview_body.preview.subject_info).ok();
    let form_data_json = serde_json::to_string(&preview_body.preview.form_data).ok();
    let scene_data_json = preview_body
        .preview
        .scene_data
        .as_ref()
        .and_then(|data| serde_json::to_string(data).ok());
    let material_data_json = serde_json::to_string(&preview_body.preview.material_data).ok();

    let user_info_json = user_info.and_then(|info| serde_json::to_string(info).ok());

    let record = PreviewRequestRecord {
        id: request_id,
        third_party_request_id: third_party_id,
        user_id: preview_body.user_id.clone(),
        user_info_json,
        matter_id: preview_body.preview.matter_id.clone(),
        matter_type: preview_body.preview.matter_type.clone(),
        matter_name: preview_body.preview.matter_name.clone(),
        channel: preview_body.preview.channel.clone(),
        sequence_no: preview_body.preview.sequence_no.clone(),
        agent_info_json,
        subject_info_json,
        form_data_json,
        scene_data_json,
        material_data_json,
        latest_preview_id: Some(preview_id.to_string()),
        latest_status: Some(PreviewStatus::Pending),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    const MAX_RETRIES: usize = 3;
    let mut attempt = 0;
    let mut last_error = None;

    while attempt < MAX_RETRIES {
        attempt += 1;
        match database.save_preview_request(&record).await {
            Ok(_) => {
                if attempt > 1 {
                    tracing::info!(
                        preview_id = %preview_id,
                        attempt,
                        "预审请求摘要在重试后保存成功"
                    );
                }
                return Ok(());
            }
            Err(err) => {
                tracing::warn!(
                    preview_id = %preview_id,
                    attempt,
                    error = %err,
                    "保存预审请求摘要失败"
                );
                last_error = Some(err);
                if attempt < MAX_RETRIES {
                    sleep(Duration::from_millis(100 * attempt as u64)).await;
                }
            }
        }
    }

    METRICS_COLLECTOR.record_preview_persistence_failure("save_preview_request");
    Err(last_error.unwrap_or_else(|| anyhow!("未知错误：save_preview_request 失败")))
}

async fn sync_preview_request_status_inner(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
    third_party_request_id: Option<&str>,
    status: PreviewStatus,
) {
    let request_id = third_party_request_id.and_then(|id| {
        let trimmed = id.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(truncate_str(trimmed, 100))
        }
    });

    let request_id = if let Some(id) = request_id {
        Some(id)
    } else {
        match database.get_preview_record(preview_id).await {
            Ok(Some(record)) => Some(derive_request_record_id(
                record.third_party_request_id.as_deref(),
                &record.id,
            )),
            Ok(None) => {
                tracing::warn!(
                    preview_id = %preview_id,
                    "无法同步预审请求状态：数据库中找不到预审记录"
                );
                None
            }
            Err(err) => {
                tracing::warn!(
                    preview_id = %preview_id,
                    error = %err,
                    "同步预审请求状态时读取预审记录失败"
                );
                None
            }
        }
    };

    if let Some(request_id) = request_id {
        if let Err(err) = database
            .update_preview_request_latest(&request_id, Some(preview_id), Some(status.clone()))
            .await
        {
            tracing::warn!(
                preview_id = %preview_id,
                request_id = %request_id,
                error = %err,
                "更新预审请求最新状态失败"
            );
        }
    }
}

pub(crate) async fn sync_preview_request_status(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
    status: PreviewStatus,
) {
    sync_preview_request_status_inner(database, preview_id, None, status).await;
}

pub(crate) async fn sync_preview_request_status_with_hint(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
    third_party_request_id: Option<&str>,
    status: PreviewStatus,
) {
    sync_preview_request_status_inner(database, preview_id, third_party_request_id, status).await;
}

// 使用数据库保存ID映射关系（替代文件操作）
pub async fn save_id_mapping_to_database(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
    third_party_request_id: &str,
    user_id: &str,
    user_info: Option<&SessionUser>,
) -> anyhow::Result<()> {
    use crate::db::{PreviewRecord, PreviewStatus};

    tracing::debug!(
        "保存ID映射到数据库: {} -> {}",
        preview_id,
        third_party_request_id
    );

    let user_info_json = user_info.and_then(|info| serde_json::to_string(info).ok());

    let record = PreviewRecord {
        id: preview_id.to_string(),
        user_id: user_id.to_string(),
        user_info_json,
        file_name: format!("{}.html", preview_id),
        ocr_text: "".to_string(), // 将在后续处理中填充
        theme_id: None,
        evaluation_result: None,
        preview_url: CONFIG.preview_view_url(preview_id),
        preview_view_url: Some(CONFIG.preview_view_url(preview_id)),
        preview_download_url: crate::util::callbacks::build_default_download_url(preview_id),
        status: PreviewStatus::Pending,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        third_party_request_id: Some(third_party_request_id.to_string()),
        queued_at: None,
        processing_started_at: None,
        retry_count: 0,
        last_worker_id: None,
        last_attempt_id: None,
        failure_reason: None,
        ocr_stderr_summary: None,
        failure_context: None,
        last_error_code: None,
        slow_attachment_info_json: None,
        callback_url: CONFIG.third_party_callback_url(),
        callback_status: Some("pending".to_string()),
        callback_attempts: 0,
        callback_successes: 0,
        callback_failures: 0,
        last_callback_at: None,
        last_callback_status_code: None,
        last_callback_response: None,
        last_callback_error: None,
        callback_payload: None,
        next_callback_after: None,
    };

    database.save_preview_record(&record).await?;

    tracing::debug!(" ID映射已保存到数据库");
    Ok(())
}

///  增强版：保存完整的原始请求数据到数据库
pub async fn save_original_request_to_database(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
    original_request_body: &str,
    extracted_materials: &[MaterialValue],
) -> anyhow::Result<()> {
    tracing::debug!(" 保存原始请求数据到数据库: {}", preview_id);

    // 创建材料数据摘要
    let materials_summary: Vec<String> = extracted_materials
        .iter()
        .map(|m| format!("{} ({}个附件)", m.code, m.attachment_list.len()))
        .collect();

    tracing::debug!(
        " 原始请求摘要: 长度={} bytes, 材料数={}, 材料列表={:?}",
        original_request_body.len(),
        extracted_materials.len(),
        materials_summary
    );

    // 尝试更新记录，添加原始请求数据
    // 注意：这里需要扩展数据库schema支持原始请求数据存储
    // 暂时通过OCR文本字段存储摘要信息
    let summary_text = format!(
        "原始请求长度: {} bytes\n材料数量: {}\n材料列表: {}\n提取时间: {}",
        original_request_body.len(),
        extracted_materials.len(),
        materials_summary.join(", "),
        Utc::now().to_rfc3339()
    );

    tracing::debug!(
        preview_id = %preview_id,
        summary = %summary_text,
        "原始请求摘要已生成，等待后续持久化扩展"
    );

    Ok(())
}

// 从数据库获取ID映射信息（替代文件操作）
pub async fn get_id_mapping_from_database(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
) -> anyhow::Result<Option<crate::db::PreviewRecord>> {
    database.get_preview_record(preview_id).await
}

/// 提供固定路径的最新预审结果下载（PDF优先，HTML兜底）
pub async fn download_latest_pdf(Path(preview_id): Path<String>) -> Response {
    let pdf_path = ocr_conn::CURRENT_DIR
        .join("preview")
        .join(format!("{}.pdf", preview_id));
    let pdf_goto = Goto {
        goto: pdf_path.to_string_lossy().to_string(),
    };

    match PreviewBody::download_local(pdf_goto).await {
        Ok(response) => response,
        Err(pdf_err) => {
            tracing::debug!(
                preview_id = %preview_id,
                error = %pdf_err,
                "PDF 文件不可用，尝试 HTML 结果"
            );
            let html_path = ocr_conn::CURRENT_DIR
                .join("preview")
                .join(format!("{}.html", preview_id));
            let html_goto = Goto {
                goto: html_path.to_string_lossy().to_string(),
            };
            match PreviewBody::download_local(html_goto).await {
                Ok(response) => response,
                Err(html_err) => {
                    tracing::warn!(
                        preview_id = %preview_id,
                        error = %html_err,
                        "无法找到预审结果文件"
                    );
                    StatusCode::NOT_FOUND.into_response()
                }
            }
        }
    }
}

/// 通知第三方系统预审结果
pub async fn notify_third_party_system(
    preview_id: &str,
    third_party_request_id: &str,
    status: &str,
    result: Option<&crate::util::WebResult>,
    error_code: Option<&str>,
) -> anyhow::Result<()> {
    tracing::debug!(
        preview_id = %preview_id,
        third_party_request_id = %third_party_request_id,
        status = %status,
        "准备调度第三方结果回调"
    );

    if third_party_request_id.trim().is_empty() {
        tracing::debug!(preview_id = %preview_id, "第三方请求ID为空，跳过回调");
        return Ok(());
    }

    let callback_url = match CONFIG.third_party_callback_url() {
        Some(url) => url,
        None => {
            tracing::debug!("未配置 third_party_callback_url，跳过回调");
            return Ok(());
        }
    };

    let mut payload = serde_json::json!({
        "previewId": third_party_request_id,
        "thirdPartyRequestId": third_party_request_id,
        "previewInternalId": preview_id,
        "status": status,
        "timestamp": Utc::now().to_rfc3339(),
        "callbackType": "preview_result"
    });

    if let Some(code) = error_code {
        payload["errorCode"] = serde_json::json!(code);
    }

    let view_url = CONFIG.preview_view_url(preview_id);
    payload["viewUrl"] = serde_json::json!(view_url);

    let download_url = crate::util::callbacks::build_default_download_url(preview_id);
    if let Some(url) = download_url {
        payload["downloadUrl"] = serde_json::json!(url);
    }

    match status {
        "completed" => {
            if let Some(web_result) = result {
                payload["result"] = serde_json::json!({
                    "success": web_result.success,
                    "data": web_result.data,
                    "message": web_result.msg,
                });
            }
        }
        "failed" => {
            payload["result"] = serde_json::json!({
                "success": false,
                "message": result
                    .map(|r| r.msg.clone())
                    .unwrap_or_else(|| "预审处理失败".to_string()),
            });
        }
        _ => {}
    }

    crate::util::outbox::enqueue_third_party_callback_event(
        preview_id,
        &callback_url,
        payload,
        true,
    )
    .await
}

// preview_submit测试占位符函数已删除 - 使用主要的preview函数

///  灵活的JSON解析器：支持任意格式的JSON请求体
/// 根据"开放所有请求体"的设计原则，智能提取和构造PreviewBody结构
pub fn parse_flexible_json_to_preview_body(bytes: &[u8]) -> anyhow::Result<PreviewBody> {
    // 先尝试解析为通用JSON对象
    let json_value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(e) => {
            tracing::error!("无法解析为有效JSON: {}", e);
            return Err(anyhow::anyhow!("无效的JSON格式: {}", e));
        }
    };

    tracing::debug!(" 收到JSON请求，开始智能解析...");
    tracing::debug!(
        "原始JSON结构: {}",
        serde_json::to_string_pretty(&json_value).unwrap_or("无法序列化".to_string())
    );

    //  尝试三种解析策略，按优先级从高到低

    // 策略1：标准PreviewBody格式（包含userId和preview字段）
    if let Ok(standard_body) = serde_json::from_value::<PreviewBody>(json_value.clone()) {
        tracing::debug!(" 策略1成功：标准PreviewBody格式");
        log_scene_data(&standard_body.preview.scene_data);
        return Ok(standard_body);
    }

    // 策略2：生产环境格式（直接包含所有字段，无包装层）
    if let Ok(prod_request) = serde_json::from_value::<
        crate::model::preview::ProductionPreviewRequest,
    >(json_value.clone())
    {
        tracing::debug!(" 策略2成功：生产环境格式，正在转换...");
        let converted_body = prod_request.to_preview_body();
        log_scene_data(&converted_body.preview.scene_data);
        return Ok(converted_body);
    }

    // 策略3：通用智能解析（最灵活的方案）
    tracing::debug!(" 策略3：智能解析任意JSON结构...");

    let preview_body = build_preview_body_from_any_json(&json_value)?;
    log_scene_data(&preview_body.preview.scene_data);

    tracing::debug!(" 策略3成功：智能构造PreviewBody完成");
    Ok(preview_body)
}

/// 记录sceneData接收情况的辅助函数
fn log_scene_data(scene_data: &Option<Vec<SceneValue>>) {
    if let Some(ref scene_data) = scene_data {
        tracing::debug!(" 收到sceneData: {} 个情形数据", scene_data.len());
        for (i, scene) in scene_data.iter().enumerate() {
            tracing::debug!(
                "  情形 {}: questionCode={}, optionList={:?}",
                i + 1,
                scene.question_code,
                scene.option_list
            );
        }
    } else {
        tracing::debug!(" 未收到sceneData（字段为空或不存在）");
    }
}

///  从任意JSON结构智能构造PreviewBody
/// 这是最灵活的解析方案，能够处理各种未知格式
fn build_preview_body_from_any_json(json: &Value) -> anyhow::Result<PreviewBody> {
    tracing::debug!(" 开始智能分析JSON结构...");

    // 智能提取用户ID
    let user_id = extract_user_id(json)?;
    tracing::debug!(" 提取到用户ID: {}", user_id);

    // 智能构造Preview对象
    let preview = build_preview_from_json(json)?;
    tracing::debug!(" 构造Preview对象完成");

    let preview_body = PreviewBody {
        user_id,
        preview,
        rule_definition: None,
        parsed_rule_definition: None,
    };

    Ok(preview_body)
}

/// 智能提取用户ID，支持多种命名约定
fn extract_user_id(json: &Value) -> anyhow::Result<String> {
    // 尝试多种可能的字段名
    let possible_user_id_fields = [
        "userId",
        "user_id",
        "userid",
        "userID",
        "agentInfo.userId",
        "agentInfo.user_id",
        "agentInfo.userID",
        "subjectInfo.userId",
        "subjectInfo.user_id",
        "subjectInfo.userID",
        "agent.userId",
        "agent.user_id",
        "agent.userID",
        "user.id",
        "user.userId",
        "user.user_id",
    ];

    for field_path in &possible_user_id_fields {
        if let Some(user_id) = extract_field_by_path(json, field_path) {
            if let Some(user_id_str) = user_id.as_str() {
                if !user_id_str.is_empty() {
                    tracing::debug!("通过字段路径 '{}' 找到用户ID: {}", field_path, user_id_str);
                    return Ok(user_id_str.to_string());
                }
            }
        }
    }

    // 如果找不到有效的用户ID，生成一个临时ID
    let temp_user_id = format!("temp_user_{}", chrono::Utc::now().timestamp());
    tracing::warn!(" 无法提取用户ID，使用临时ID: {}", temp_user_id);
    Ok(temp_user_id)
}

/// 智能构造Preview对象
fn build_preview_from_json(json: &Value) -> anyhow::Result<Preview> {
    // 智能提取各个字段，如果找不到则使用默认值

    let matter_id = extract_string_field(json, &["matterId", "matter_id", "matterID"])
        .unwrap_or_else(|| format!("matter_{}", chrono::Utc::now().timestamp()));

    let matter_name = extract_string_field(json, &["matterName", "matter_name", "matterNAME"])
        .unwrap_or_else(|| "智能预审事项".to_string());

    let matter_type = extract_string_field(json, &["matterType", "matter_type", "matterTYPE"])
        .unwrap_or_else(|| "default".to_string());

    let request_id = extract_string_field(json, &["requestId", "request_id", "requestID", "id"])
        .unwrap_or_else(|| format!("req_{}", chrono::Utc::now().timestamp()));

    let sequence_no = extract_string_field(
        json,
        &["sequenceNo", "sequence_no", "sequenceNO", "seqNo", "seq"],
    )
    .unwrap_or_else(|| "1".to_string());

    let channel = extract_string_field(json, &["channel", "channelType", "source"])
        .unwrap_or_else(|| "api".to_string());

    let copy = extract_bool_field(json, &["copy", "isCopy"]).unwrap_or(false);

    // 智能提取数组类型字段
    let form_data =
        extract_array_field(json, &["formData", "form_data", "forms"]).unwrap_or_else(Vec::new);

    let material_data = extract_material_data(json)?;

    // 智能提取用户信息
    let agent_info = extract_user_info(json, &["agentInfo", "agent_info", "agent"])?;
    let subject_info = extract_user_info(json, &["subjectInfo", "subject_info", "subject"])
        .unwrap_or_else(|_| agent_info.clone());

    // 智能提取sceneData
    let scene_data = extract_scene_data(json);

    Ok(Preview {
        matter_id,
        matter_type,
        matter_name,
        copy,
        channel,
        request_id,
        sequence_no,
        form_data,
        material_data,
        agent_info,
        subject_info,
        theme_id: None, // 将在后续处理中设置
        scene_data,
    })
}

/// 按路径提取JSON字段（支持嵌套路径，如 "agentInfo.userId"）
fn extract_field_by_path<'a>(json: &'a Value, path: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = json;

    for part in parts {
        current = current.get(part)?;
    }

    Some(current)
}

/// 智能提取字符串字段
fn extract_string_field(json: &Value, field_names: &[&str]) -> Option<String> {
    for field_name in field_names {
        if let Some(value) = extract_field_by_path(json, field_name) {
            if let Some(s) = value.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// 智能提取布尔字段
fn extract_bool_field(json: &Value, field_names: &[&str]) -> Option<bool> {
    for field_name in field_names {
        if let Some(value) = extract_field_by_path(json, field_name) {
            if let Some(b) = value.as_bool() {
                return Some(b);
            }
        }
    }
    None
}

/// 智能提取数组字段
fn extract_array_field(json: &Value, field_names: &[&str]) -> Option<Vec<Value>> {
    for field_name in field_names {
        if let Some(value) = extract_field_by_path(json, field_name) {
            if let Some(arr) = value.as_array() {
                return Some(arr.clone());
            }
        }
    }
    None
}

/// 智能提取材料数据
fn extract_material_data(json: &Value) -> anyhow::Result<Vec<MaterialValue>> {
    let field_names = ["materialData", "material_data", "materials", "attachments"];

    for field_name in &field_names {
        if let Some(value) = extract_field_by_path(json, field_name) {
            if let Some(arr) = value.as_array() {
                // 尝试直接解析为MaterialValue数组
                if let Ok(materials) =
                    serde_json::from_value::<Vec<MaterialValue>>(Value::Array(arr.clone()))
                {
                    tracing::debug!(
                        "通过字段 '{}' 解析到 {} 个材料",
                        field_name,
                        materials.len()
                    );
                    return Ok(materials);
                }

                // 如果直接解析失败，尝试智能构造
                let mut materials = Vec::new();
                for (index, item) in arr.iter().enumerate() {
                    if let Ok(material) = build_material_from_json(item, index) {
                        materials.push(material);
                    }
                }

                if !materials.is_empty() {
                    tracing::debug!(
                        "通过字段 '{}' 智能构造到 {} 个材料",
                        field_name,
                        materials.len()
                    );
                    return Ok(materials);
                }
            }
        }
    }

    // 如果找不到材料数据，返回空数组
    tracing::debug!("未找到材料数据，返回空数组");
    Ok(Vec::new())
}

/// 从JSON智能构造单个MaterialValue
fn build_material_from_json(json: &Value, index: usize) -> anyhow::Result<MaterialValue> {
    let code = extract_string_field(json, &["code", "materialCode", "material_code", "type"])
        .unwrap_or_else(|| format!("material_{}", index));

    let name = extract_string_field(
        json,
        &[
            "name",
            "materialName",
            "material_name",
            "displayName",
            "label",
            "title",
        ],
    );

    let attachment_list = if let Some(attachments_value) =
        extract_field_by_path(json, "attachmentList")
            .or_else(|| extract_field_by_path(json, "attachment_list"))
            .or_else(|| extract_field_by_path(json, "attachments"))
    {
        if let Some(arr) = attachments_value.as_array() {
            arr.iter()
                .enumerate()
                .filter_map(|(i, att_json)| build_attachment_from_json(att_json, i).ok())
                .collect()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(MaterialValue {
        code,
        name,
        attachment_list,
        extra: HashMap::new(),
    })
}

/// 从JSON智能构造单个Attachment
fn build_attachment_from_json(
    json: &Value,
    index: usize,
) -> anyhow::Result<crate::model::preview::Attachment> {
    let attach_name = extract_string_field(
        json,
        &["attaName", "attachName", "name", "fileName", "file_name"],
    )
    .unwrap_or_else(|| format!("attachment_{}", index));

    let attach_url = extract_string_field(
        json,
        &["attaUrl", "attachUrl", "url", "fileUrl", "file_url"],
    )
    .unwrap_or_else(|| "".to_string());

    let is_cloud_share =
        extract_bool_field(json, &["isCloudShare", "is_cloud_share", "cloudShare"])
            .unwrap_or(false);

    Ok(crate::model::preview::Attachment {
        attach_name,
        attach_url,
        is_cloud_share,
        extra: HashMap::new(),
    })
}

/// 智能提取用户信息
fn extract_user_info(json: &Value, field_names: &[&str]) -> anyhow::Result<UserInfo> {
    for field_name in field_names {
        if let Some(value) = extract_field_by_path(json, field_name) {
            // 尝试直接解析
            if let Ok(user_info) = serde_json::from_value::<UserInfo>(value.clone()) {
                tracing::debug!("通过字段 '{}' 解析到用户信息", field_name);
                return Ok(user_info);
            }

            // 智能构造用户信息
            if let Ok(user_info) = build_user_info_from_json(value) {
                tracing::debug!("通过字段 '{}' 智能构造用户信息", field_name);
                return Ok(user_info);
            }
        }
    }

    // 如果找不到用户信息，创建默认用户信息
    tracing::warn!("未找到用户信息，创建默认用户信息");
    Ok(create_default_user_info())
}

/// 从JSON智能构造UserInfo
fn build_user_info_from_json(json: &Value) -> anyhow::Result<UserInfo> {
    let user_id = extract_string_field(json, &["userId", "user_id", "userID", "id"])
        .unwrap_or_else(|| format!("user_{}", chrono::Utc::now().timestamp()));

    let certificate_type = extract_string_field(
        json,
        &["certificateType", "certificate_type", "certType", "idType"],
    )
    .unwrap_or_else(|| "身份证".to_string());

    let user_name = extract_string_field(json, &["userName", "user_name", "name", "fullName"]);
    let nick_name = extract_string_field(json, &["nickName", "nickname"]);
    let certificate_number = extract_string_field(
        json,
        &[
            "certificateNumber",
            "certificate_number",
            "idNumber",
            "certNumber",
        ],
    );
    let phone_number =
        extract_string_field(json, &["phoneNumber", "phone_number", "mobile", "phone"]);
    let email = extract_string_field(json, &["email", "emailAddress", "email_address"]);
    let organization_name = extract_string_field(
        json,
        &[
            "organizationName",
            "organization_name",
            "orgName",
            "company",
            "companyName",
        ],
    );
    let organization_code = extract_string_field(
        json,
        &[
            "organizationCode",
            "organization_code",
            "creditCode",
            "credit_code",
            "organizationNumber",
        ],
    );
    let address = extract_string_field(json, &["address", "companyAddress"]);
    let auth_level = extract_string_field(json, &["authLevel", "auth_level"]);
    let user_type = extract_string_field(json, &["userType", "user_type"]);
    let login_type = extract_string_field(json, &["loginType", "login_type"]);
    let ext_infos = json.get("extInfos").cloned();

    Ok(UserInfo {
        user_id,
        certificate_type,
        user_name,
        nick_name,
        certificate_number,
        phone_number,
        email,
        organization_name,
        organization_code,
        address,
        auth_level,
        user_type,
        login_type,
        ext_infos,
        extra: HashMap::new(),
    })
}

/// 智能提取用户信息的默认值版本
fn create_default_user_info() -> UserInfo {
    let default_user_id = format!("user_{}", chrono::Utc::now().timestamp());
    UserInfo {
        user_id: default_user_id,
        certificate_type: "身份证".to_string(),
        user_name: None,
        nick_name: None,
        certificate_number: None,
        phone_number: None,
        email: None,
        organization_name: None,
        organization_code: None,
        address: None,
        auth_level: None,
        user_type: None,
        login_type: None,
        ext_infos: None,
        extra: HashMap::new(),
    }
}

/// 智能提取场景数据
fn extract_scene_data(json: &Value) -> Option<Vec<SceneValue>> {
    let field_names = ["sceneData", "scene_data", "scenes"];

    for field_name in &field_names {
        if let Some(value) = extract_field_by_path(json, field_name) {
            if let Some(arr) = value.as_array() {
                // 尝试直接解析为SceneValue数组
                if let Ok(scenes) =
                    serde_json::from_value::<Vec<SceneValue>>(Value::Array(arr.clone()))
                {
                    tracing::debug!(
                        "通过字段 '{}' 解析到 {} 个场景数据",
                        field_name,
                        scenes.len()
                    );
                    return Some(scenes);
                }
            }
        }
    }

    None
}

///  创建兼容的PreviewBody结构，确保任何JSON都不会报错
fn create_fallback_preview_body(json: &Value) -> PreviewBody {
    tracing::debug!(" 容错模式：为未知JSON格式创建兼容结构");

    // 尽力提取用户ID，找不到就生成一个
    let user_id = json
        .get("userId")
        .or_else(|| json.get("user_id"))
        .or_else(|| json.get("agentInfo").and_then(|v| v.get("userId")))
        .or_else(|| json.get("agent_info").and_then(|v| v.get("userId")))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown_user")
        .to_string();

    // 如果是空字符串，生成一个临时ID
    let user_id = if user_id.is_empty() || user_id == "unknown_user" {
        format!("fallback_user_{}", chrono::Utc::now().timestamp())
    } else {
        user_id
    };

    // 尽力提取基本字段，找不到就用默认值
    let matter_id = json
        .get("matterId")
        .or_else(|| json.get("matter_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown_matter")
        .to_string();

    let matter_name = json
        .get("matterName")
        .or_else(|| json.get("matter_name"))
        .and_then(|v| v.as_str())
        .unwrap_or("智能预审事项")
        .to_string();

    let request_id = json
        .get("requestId")
        .or_else(|| json.get("request_id"))
        .or_else(|| json.get("sequenceNo"))
        .or_else(|| json.get("sequence_no"))
        .or_else(|| json.get("id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("fallback_req_{}", chrono::Utc::now().timestamp()));

    //  关键修复：智能提取材料数据，不能直接设为空！
    let material_data = extract_material_data_fallback(json);
    tracing::debug!(" 容错模式提取到 {} 个材料数据", material_data.len());

    tracing::debug!(
        " 容错解析结果: user_id={}, matter_id={}, request_id={}, materials_count={}",
        user_id,
        matter_id,
        request_id,
        material_data.len()
    );

    // 创建一个最小但有效的UserInfo
    let default_user_info = UserInfo {
        user_id: user_id.clone(),
        certificate_type: "身份证".to_string(),
        user_name: None,
        nick_name: None,
        certificate_number: None,
        phone_number: None,
        email: None,
        organization_name: None,
        organization_code: None,
        address: None,
        auth_level: None,
        user_type: None,
        login_type: None,
        ext_infos: None,
        extra: HashMap::new(),
    };

    // 创建兼容的Preview结构
    let preview = Preview {
        matter_id,
        matter_type: "default".to_string(),
        matter_name,
        copy: false,
        channel: "api".to_string(),
        request_id,
        sequence_no: "1".to_string(),
        form_data: Vec::new(), // 空的form数据
        material_data,         //  使用提取到的材料数据，而不是空数组
        agent_info: default_user_info.clone(),
        subject_info: default_user_info,
        theme_id: None,
        scene_data: None,
    };

    PreviewBody {
        user_id,
        preview,
        rule_definition: None,
        parsed_rule_definition: None,
    }
}

///  容错模式的材料数据提取 - 确保OCR功能不被丢失
fn extract_material_data_fallback(json: &Value) -> Vec<MaterialValue> {
    tracing::debug!(" 开始容错模式材料数据提取...");

    // 尝试多种可能的字段名和结构
    let possible_material_fields = [
        "materialData",
        "material_data",
        "materials",
        "attachments",
        "attachmentList",
        "attachment_list",
        "files",
        "documents",
    ];

    // 首先在根级别查找
    for field_name in &possible_material_fields {
        tracing::debug!(" 尝试字段: {}", field_name);
        if let Some(materials) = try_extract_materials_from_value(json, field_name) {
            return materials;
        }
    }

    // 然后在preview子对象中查找（针对测试数据格式）
    if let Some(preview_obj) = json.get("preview") {
        tracing::debug!(" 在preview子对象中查找材料数据...");
        for field_name in &possible_material_fields {
            tracing::debug!(" 尝试preview.{}", field_name);
            if let Some(materials) = try_extract_materials_from_value(preview_obj, field_name) {
                return materials;
            }
        }
    }

    // 最后尝试在根级别查找文件URL
    let possible_url_fields = ["fileUrl", "file_url", "url", "link", "attachUrl", "attaUrl"];
    for url_field in &possible_url_fields {
        if let Some(url_value) = json.get(url_field) {
            if let Some(url_str) = url_value.as_str() {
                if !url_str.is_empty() {
                    tracing::debug!(" 在根级别找到文件URL: {}", url_str);
                    let material = MaterialValue {
                        code: "root_material".to_string(),
                        name: Some("root_material".to_string()),
                        attachment_list: vec![crate::model::preview::Attachment {
                            attach_name: "extracted_file".to_string(),
                            attach_url: url_str.to_string(),
                            is_cloud_share: true,
                            extra: HashMap::new(),
                        }],
                        extra: HashMap::new(),
                    };
                    return vec![material];
                }
            }
        }
    }

    tracing::warn!(" 容错模式未能提取到任何材料数据");
    Vec::new()
}

/// 尝试从JSON值中提取材料数据的辅助函数
fn try_extract_materials_from_value(json: &Value, field_name: &str) -> Option<Vec<MaterialValue>> {
    if let Some(value) = json.get(field_name) {
        if let Some(arr) = value.as_array() {
            tracing::debug!(" 找到材料数组字段: {}, 长度: {}", field_name, arr.len());

            // 尝试直接解析
            if let Ok(materials) =
                serde_json::from_value::<Vec<MaterialValue>>(Value::Array(arr.clone()))
            {
                tracing::debug!(" 直接解析成功，提取到 {} 个材料", materials.len());
                return Some(materials);
            }

            // 尝试智能构造 - 支持不同的数据结构
            let mut materials = Vec::new();
            for (index, item) in arr.iter().enumerate() {
                if let Ok(material) = build_material_from_json_fallback(item, index) {
                    tracing::debug!(" 构造材料 {}: {}", index, material.code);
                    materials.push(material);
                } else if let Ok(material) = build_material_from_test_format(item, index) {
                    // 尝试测试数据格式
                    tracing::debug!(" 从测试格式构造材料 {}: {}", index, material.code);
                    materials.push(material);
                }
            }

            if !materials.is_empty() {
                tracing::debug!(" 智能构造成功，提取到 {} 个材料", materials.len());
                return Some(materials);
            }
        } else if value.is_object() {
            // 单个对象的情况
            if let Ok(material) = build_material_from_json_fallback(value, 0) {
                tracing::debug!(" 单个材料对象解析成功: {}", material.code);
                return Some(vec![material]);
            } else if let Ok(material) = build_material_from_test_format(value, 0) {
                tracing::debug!(" 从测试格式解析单个材料对象: {}", material.code);
                return Some(vec![material]);
            }
        }
    }
    None
}

/// 容错模式的单个材料构造
fn build_material_from_json_fallback(json: &Value, index: usize) -> anyhow::Result<MaterialValue> {
    let code = json
        .get("code")
        .or_else(|| json.get("type"))
        .or_else(|| json.get("materialCode"))
        .or_else(|| json.get("material_code"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("fallback_material_{}", index));

    let name = json
        .get("name")
        .or_else(|| json.get("materialName"))
        .or_else(|| json.get("material_name"))
        .or_else(|| json.get("displayName"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut attachment_list = Vec::new();

    // 尝试提取附件列表
    if let Some(attachments) = json
        .get("attachmentList")
        .or_else(|| json.get("attachment_list"))
        .or_else(|| json.get("attachments"))
        .or_else(|| json.get("files"))
    {
        if let Some(arr) = attachments.as_array() {
            for (att_index, att_json) in arr.iter().enumerate() {
                if let Ok(attachment) = build_attachment_from_json_fallback(att_json, att_index) {
                    attachment_list.push(attachment);
                }
            }
        }
    }

    // 如果没有找到附件列表，尝试直接提取URL
    if attachment_list.is_empty() {
        if let Some(url) = json
            .get("url")
            .or_else(|| json.get("fileUrl"))
            .or_else(|| json.get("file_url"))
            .or_else(|| json.get("attachUrl"))
            .or_else(|| json.get("attaUrl"))
            .and_then(|v| v.as_str())
        {
            if !url.is_empty() {
                let name = json
                    .get("name")
                    .or_else(|| json.get("fileName"))
                    .or_else(|| json.get("file_name"))
                    .or_else(|| json.get("attaName"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("extracted_file");

                attachment_list.push(crate::model::preview::Attachment {
                    attach_name: name.to_string(),
                    attach_url: url.to_string(),
                    is_cloud_share: true,
                    extra: HashMap::new(),
                });

                tracing::debug!(" 从对象直接提取文件: {} -> {}", name, url);
            }
        }
    }

    Ok(MaterialValue {
        code,
        name,
        attachment_list,
        extra: HashMap::new(),
    })
}

/// 从测试数据格式构造材料对象
/// 支持格式: {"materialId": "...", "materialName": "...", "fileData": "data:...", "fileType": "..."}
fn build_material_from_test_format(json: &Value, index: usize) -> anyhow::Result<MaterialValue> {
    // 提取材料ID作为code
    let code = json
        .get("materialId")
        .or_else(|| json.get("material_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("test_material_{}", index));

    let name = json
        .get("materialName")
        .or_else(|| json.get("material_name"))
        .or_else(|| json.get("name"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let mut attachment_list = Vec::new();

    // 从fileData和fileType构造附件
    if let Some(file_data) = json.get("fileData").and_then(|v| v.as_str()) {
        if !file_data.is_empty() {
            let material_name = json
                .get("materialName")
                .or_else(|| json.get("material_name"))
                .and_then(|v| v.as_str())
                .unwrap_or("test_file");

            let file_type = json
                .get("fileType")
                .or_else(|| json.get("file_type"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            // 构造文件名
            let file_name = if material_name.contains(".") {
                material_name.to_string()
            } else {
                format!("{}.{}", material_name, file_type)
            };

            attachment_list.push(crate::model::preview::Attachment {
                attach_name: file_name,
                attach_url: file_data.to_string(), // Base64数据直接作为URL
                is_cloud_share: false,             // Base64数据不是云分享
                extra: HashMap::new(),
            });

            tracing::debug!(
                " 从测试格式提取材料: {} -> {} ({})",
                code,
                material_name,
                file_type
            );
        }
    }

    if attachment_list.is_empty() {
        return Err(anyhow::anyhow!("测试格式材料没有有效的文件数据"));
    }

    Ok(MaterialValue {
        code,
        name,
        attachment_list,
        extra: HashMap::new(),
    })
}

/// 容错模式的附件构造
fn build_attachment_from_json_fallback(
    json: &Value,
    index: usize,
) -> anyhow::Result<crate::model::preview::Attachment> {
    let fallback_name = format!("fallback_attachment_{}", index);

    let attach_name = json
        .get("attaName")
        .or_else(|| json.get("name"))
        .or_else(|| json.get("fileName"))
        .or_else(|| json.get("file_name"))
        .or_else(|| json.get("attachName"))
        .and_then(|v| v.as_str())
        .unwrap_or(&fallback_name);

    let attach_url = json
        .get("attaUrl")
        .or_else(|| json.get("url"))
        .or_else(|| json.get("fileUrl"))
        .or_else(|| json.get("file_url"))
        .or_else(|| json.get("attachUrl"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let is_cloud_share = json
        .get("isCloudShare")
        .or_else(|| json.get("is_cloud_share"))
        .or_else(|| json.get("cloudShare"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true); // 容错模式默认为云共享

    if !attach_url.is_empty() {
        tracing::debug!(" 构造附件: {} -> {}", attach_name, attach_url);
    }

    Ok(crate::model::preview::Attachment {
        attach_name: attach_name.to_string(),
        attach_url: attach_url.to_string(),
        is_cloud_share,
        extra: HashMap::new(),
    })
}

/// 尝试通过浙江政务网免登录获取用户信息
///
/// 这个函数模拟了浙江政务网免登录检测的过程：
/// 1. 检查请求是否来自已登录的政务网用户
/// 2. 如果是，尝试获取用户的ticketId
/// 3. 使用ticketId换取完整的用户信息
async fn try_auto_login_and_get_user(
    headers: &axum::http::HeaderMap,
    request_ctx: &RequestContext,
) -> anyhow::Result<SessionUser> {
    tracing::debug!(" 开始免登录检测流程...");

    // 检查请求头中是否有浙江政务网的特征标识
    let zjzw_indicators = [
        ("X-ZJZW-User", "浙江政务网用户标识"),
        ("X-Portal-User", "门户用户标识"),
        ("X-SSO-Token", "SSO令牌"),
        ("Authorization", "认证头"),
    ];

    for (header_name, description) in &zjzw_indicators {
        if let Some(header_value) = headers.get(*header_name) {
            if let Ok(value_str) = header_value.to_str() {
                tracing::debug!(
                    " 发现可能的免登录标识 {}: {}",
                    description,
                    if value_str.len() > 50 {
                        format!("{}...[长度:{}]", &value_str[..50], value_str.len())
                    } else {
                        value_str.to_string()
                    }
                );

                // 这里可以根据实际的浙江政务网接口规范来处理
                // 目前先作为演示实现
                if !value_str.is_empty() && value_str != "none" {
                    return try_get_user_from_zjzw_token(value_str, request_ctx).await;
                }
            }
        }
    }

    // 检查Cookie中是否有政务网会话信息
    if let Some(cookie_header) = headers.get("cookie") {
        if let Ok(cookie_str) = cookie_header.to_str() {
            tracing::debug!(" 检查Cookie中的政务网会话标识");

            // 查找可能的政务网会话Cookie
            let zjzw_cookie_patterns = [
                "ZJZW_SESSION",
                "PORTAL_SESSION",
                "SSO_SESSION",
                "JSESSIONID", // 常见的J2EE会话ID
            ];

            for pattern in &zjzw_cookie_patterns {
                if cookie_str.contains(pattern) {
                    tracing::debug!(" 发现可能的政务网会话Cookie: {}", pattern);

                    // 尝试解析Cookie值
                    if let Some(session_value) = extract_cookie_value(cookie_str, pattern) {
                        return try_get_user_from_zjzw_session(&session_value, request_ctx).await;
                    }
                }
            }
        }
    }

    // 检查Referer是否来自政务网域名
    if let Some(referer) = headers.get("referer") {
        if let Ok(referer_str) = referer.to_str() {
            let zjzw_domains = ["portal.zjzwfw.gov.cn", "zjzwfw.gov.cn", ".zj.gov.cn"];

            for domain in &zjzw_domains {
                if referer_str.contains(domain) {
                    tracing::debug!(" 检测到来自政务网域名的请求: {}", referer_str);
                    // 可能是从政务网跳转过来的，尝试更深层的检测
                    return try_get_user_from_zjzw_referer(referer_str, request_ctx).await;
                }
            }
        }
    }

    tracing::warn!(" 未检测到浙江政务网免登录标识");
    Err(anyhow::anyhow!("无免登录标识"))
}

/// 从浙江政务网令牌获取用户信息
async fn try_get_user_from_zjzw_token(
    token: &str,
    request_ctx: &RequestContext,
) -> anyhow::Result<SessionUser> {
    tracing::debug!(" 尝试从政务网令牌获取用户信息");
    tracing::debug!(" 检测到可能的政务网令牌，长度: {}", token.len());

    // 检查token基本格式
    if token.len() < 10 {
        tracing::warn!(" 令牌长度过短，不符合政务网标准");
        return Err(anyhow::anyhow!("令牌格式无效"));
    }

    //  实际实现：使用token作为ticketId调用真实的SSO接口
    // 政务网免登录流程：
    // 1. 前端检测到门户已登录状态
    // 2. 门户会在请求头中携带ticketId或token信息
    // 3. 后端使用这个ticketId调用SSO接口获取用户信息

    tracing::debug!(
        " 调用真实SSO接口获取用户信息，ticketId: {}...",
        if token.len() > 10 {
            &token[..10]
        } else {
            token
        }
    );

    // 直接使用现有的SSO接口，token就是ticketId
    match crate::api::auth::get_user_info_from_sso_with_retry(token).await {
        Ok(user) => {
            tracing::debug!(
                " 政务网免登录成功，用户ID: {}, 姓名: {}",
                user.user_id,
                user.user_name.as_deref().unwrap_or("未提供")
            );
            tracing::debug!(
                " 用户详细信息: 证件类型={}, 手机={}, 邮箱={}, 组织={}",
                user.certificate_type,
                user.phone_number.as_deref().unwrap_or("未提供"),
                user.email.as_deref().unwrap_or("未提供"),
                user.organization_name.as_deref().unwrap_or("未提供")
            );
            Ok(user)
        }
        Err(e) => {
            tracing::error!(" 政务网免登录失败: {}", e);
            tracing::warn!(
                " 可能原因: 1)令牌已过期 2)用户未在门户登录 3)网络连接问题 4)API接口异常"
            );
            Err(anyhow::anyhow!("政务网令牌验证失败: {}", e))
        }
    }
}

/// 从政务网会话获取用户信息
async fn try_get_user_from_zjzw_session(
    session_id: &str,
    request_ctx: &RequestContext,
) -> anyhow::Result<SessionUser> {
    tracing::debug!(" 尝试从政务网会话获取用户信息");
    tracing::debug!(" 检测到会话ID，长度: {}", session_id.len());

    // 检查会话ID基本格式
    if session_id.len() < 16 {
        tracing::warn!(" 会话ID长度过短，不符合政务网标准");
        return Err(anyhow::anyhow!("会话ID格式无效"));
    }

    //  实际实现方案：
    // 政务网会话模式下，通常session_id可能包含或关联ticketId
    // 如果session_id本身不是ticketId，需要调用政务网会话验证接口获取ticketId
    // 目前我们先尝试直接作为ticketId使用，如果失败再考虑其他方案

    tracing::debug!(" 尝试将会话ID作为ticketId调用SSO接口");

    match crate::api::auth::get_user_info_from_sso_with_retry(session_id).await {
        Ok(user) => {
            tracing::debug!(" 会话ID验证成功，用户ID: {}", user.user_id);
            Ok(user)
        }
        Err(e) => {
            tracing::warn!(" 会话ID作为ticketId验证失败: {}", e);
            tracing::warn!(" 需要实现专门的政务网会话验证接口");

            // 这里可以扩展实现专门的会话验证API调用
            // 例如：调用政务网的会话验证接口，获取真正的ticketId
            // let ticket_id = verify_zjzw_session(session_id).await?;
            // crate::api::auth::get_user_info_from_sso_with_retry(&ticket_id).await

            Err(anyhow::anyhow!("政务网会话验证失败: {}", e))
        }
    }
}

/// 从政务网来源获取用户信息
async fn try_get_user_from_zjzw_referer(
    referer: &str,
    request_ctx: &RequestContext,
) -> anyhow::Result<SessionUser> {
    tracing::debug!(" 尝试从政务网来源获取用户信息");
    tracing::debug!(" 分析来源URL: {}", referer);

    // 解析referer URL参数，查找ticketId
    if let Ok(url) = url::Url::parse(referer) {
        if let Some(query) = url.query() {
            tracing::debug!(" 解析来源URL参数: {}", query);

            // 解析查询参数
            let query_pairs: std::collections::HashMap<String, String> = url
                .query_pairs()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();

            // 按优先级查找可能的用户标识参数
            let user_param_priorities = [
                "ticketId", // 最高优先级，直接可用的ticketId
                "token",    // SSO令牌
                "userId",   // 用户ID（可能需要转换）
                "userCode", // 用户代码（可能需要转换）
            ];

            for param in &user_param_priorities {
                if let Some(param_value) = query_pairs.get(*param) {
                    tracing::debug!(
                        " 发现用户参数 {}: {}...",
                        param,
                        if param_value.len() > 10 {
                            &param_value[..10]
                        } else {
                            param_value
                        }
                    );

                    // 如果是ticketId或token，直接调用SSO接口
                    if *param == "ticketId" || *param == "token" {
                        tracing::debug!(" 使用{}调用SSO接口获取用户信息", param);

                        match crate::api::auth::get_user_info_from_sso_with_retry(param_value).await
                        {
                            Ok(user) => {
                                tracing::debug!(
                                    " 通过来源URL中的{}获取用户信息成功，用户ID: {}",
                                    param,
                                    user.user_id
                                );
                                return Ok(user);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    " 通过{}获取用户信息失败: {}，尝试下一个参数",
                                    param,
                                    e
                                );
                                continue;
                            }
                        }
                    }

                    // 其他参数类型可以在这里扩展处理
                    // 例如：userId可能需要额外的转换步骤
                    tracing::debug!(" 参数{}需要进一步处理逻辑", param);
                }
            }
        }
    } else {
        tracing::warn!(" 无法解析来源URL: {}", referer);
    }

    Err(anyhow::anyhow!("无法从来源URL中提取有效的用户标识"))
}

/// 从Cookie字符串中提取指定名称的Cookie值
fn extract_cookie_value(cookie_str: &str, cookie_name: &str) -> Option<String> {
    for cookie_pair in cookie_str.split(';') {
        let cookie_pair = cookie_pair.trim();
        if let Some((name, value)) = cookie_pair.split_once('=') {
            if name.trim() == cookie_name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

/// 保存用户登录记录到数据库
///
/// 记录用户登录信息，包括免登录、SSO登录、调试登录等不同方式
/// 用于审计跟踪和用户行为分析
pub(crate) async fn save_user_login_record(
    database: &std::sync::Arc<dyn crate::db::Database>,
    user: &SessionUser,
    login_type: &str, // "auto_login", "sso_login", "debug_login"
    headers: &axum::http::HeaderMap,
) -> anyhow::Result<()> {
    tracing::info!(
        " 开始保存用户登录记录: 用户={}, 类型={}",
        user.user_id,
        login_type
    );

    // 提取请求信息
    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let user_agent = headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");

    let referer = headers
        .get("referer")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("none");

    let cookie_info = headers
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .map(|c| {
            if c.len() > 100 {
                format!("{}...[长度:{}]", &c[..100], c.len())
            } else {
                c.to_string()
            }
        })
        .unwrap_or("none".to_string());

    // 构建登录记录数据
    let login_record = serde_json::json!({
        "user_id": user.user_id,
        "user_name": user.user_name,
        "certificate_type": user.certificate_type,
        "certificate_number": user.certificate_number,
        "phone_number": user.phone_number,
        "email": user.email,
        "organization_name": user.organization_name,
        "organization_code": user.organization_code,
        "login_type": login_type,
        "login_time": user.login_time,
        "client_ip": client_ip,
        "user_agent": user_agent,
        "referer": referer,
        "cookie_info": cookie_info,
        "created_at": chrono::Utc::now().to_rfc3339()
    });

    // 保存到数据库
    match database
        .save_user_login_record(
            user.user_id.as_str(),
            user.user_name.as_deref(),
            user.certificate_type.as_str(),
            user.certificate_number.as_deref(),
            user.phone_number.as_deref(),
            user.email.as_deref(),
            user.organization_name.as_deref(),
            user.organization_code.as_deref(),
            login_type,
            user.login_time.as_str(),
            client_ip,
            user_agent,
            referer,
            cookie_info.as_str(),
            &login_record.to_string(),
        )
        .await
    {
        Ok(_) => {
            tracing::info!(
                " 用户登录记录保存成功: 用户={}, 类型={}, IP={}",
                user.user_id,
                login_type,
                client_ip
            );
            Ok(())
        }
        Err(e) => {
            tracing::error!(" 用户登录记录保存失败: 用户={}, 错误={}", user.user_id, e);
            Err(e.into())
        }
    }
}

async fn log_raw_preview_request(trace_id: &str, request_id: &str, payload: &[u8]) {
    let sanitized_id = sanitize_request_id(request_id);
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let logs_dir = ocr_conn::CURRENT_DIR
        .join("runtime")
        .join("logs")
        .join("requests");

    if let Err(err) = fs::create_dir_all(&logs_dir).await {
        tracing::warn!(
            trace_id = %trace_id,
            error = %err,
            "创建原始请求日志目录失败"
        );
        return;
    }

    let file_name = if sanitized_id.is_empty() {
        format!("unknown-{}-{}.json", trace_id, timestamp)
    } else {
        format!("{}-{}-{}.json", sanitized_id, trace_id, timestamp)
    };

    let path = logs_dir.join(file_name);
    match fs::write(&path, payload).await {
        Ok(_) => {
            tracing::info!(
                trace_id = %trace_id,
                request_id = %sanitized_id,
                path = %path.display(),
                "原始预审请求已记录"
            );
        }
        Err(err) => {
            tracing::warn!(
                trace_id = %trace_id,
                request_id = %sanitized_id,
                error = %err,
                "写入原始预审请求失败"
            );
        }
    }
}

fn sanitize_request_id(input: &str) -> String {
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
