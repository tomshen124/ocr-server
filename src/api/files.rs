//! 文件管理模块
//! 处理文件上传、下载、预审结果展示等功能

use crate::db::traits::{MaterialFileFilter, MaterialFileRecord, PreviewRecord, PreviewStatus};
use crate::model::evaluation::PreviewEvaluationResult;
use crate::model::preview::PreviewBody;
use crate::model::Goto;
use crate::util::config::types::is_internal_host;
use crate::util::config::Config;
use crate::util::report::{pdf::PdfGenerator, PreviewReportGenerator};
use crate::util::{IntoJson, ServerError};
use crate::AppState;
use axum::body::Body;
use axum::extract::{Multipart, Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use mime_guess::MimeGuess;
use ocr_conn::CURRENT_DIR;
use serde_json;
use std::collections::HashMap;
use std::path::PathBuf;
use tokio::fs;
use tracing::{info, warn};
use url::Url;
use rand::{distributions::Alphanumeric, rngs::OsRng, Rng};

/// 文件上传接口
pub async fn upload(multipart: Multipart) -> impl IntoResponse {
    let result = crate::model::ocr::upload(multipart).await;
    result.into_json()
}

/// 文件下载接口
pub async fn download(Query(goto): Query<Goto>) -> impl IntoResponse {
    let result = PreviewBody::download(goto).await;
    result.map_err(|err| ServerError::Custom(err.to_string()))
}

/// 第三方系统回调处理 (POST方式，用于预审完成通知)
pub async fn third_party_callback(
    headers: axum::http::HeaderMap,
    Json(callback_data): Json<serde_json::Value>,
) -> impl IntoResponse {
    tracing::info!("=== 第三方系统回调接收 ===");
    tracing::info!(
        "接收时间: {}",
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    // 记录请求头信息
    tracing::info!("请求头信息:");
    for (name, value) in headers.iter() {
        let header_name = name.as_str();
        let header_value = value.to_str().unwrap_or("无法解析");

        // 记录关键头部信息
        if header_name.to_lowercase().contains("content")
            || header_name.to_lowercase().contains("user-agent")
            || header_name.to_lowercase().contains("authorization")
            || header_name.to_lowercase().contains("x-")
        {
            // 敏感信息脱敏
            let safe_value = if header_name.to_lowercase().contains("auth")
                || header_name.to_lowercase().contains("token")
            {
                format!(
                    "{}***{}",
                    &header_value[..2.min(header_value.len())],
                    &header_value[header_value.len().saturating_sub(2)..]
                )
            } else {
                header_value.to_string()
            };
            tracing::info!("  {}: {}", header_name, safe_value);
        }
    }

    // 记录回调数据
    tracing::info!("回调数据结构:");
    tracing::info!(
        "{}",
        serde_json::to_string_pretty(&callback_data).unwrap_or_default()
    );

    // 分析回调数据字段
    tracing::info!("数据字段分析:");
    for (key, value) in callback_data.as_object().unwrap_or(&serde_json::Map::new()) {
        match value {
            serde_json::Value::String(s) => tracing::info!("  {} (字符串): {}", key, s),
            serde_json::Value::Number(n) => tracing::info!("  {} (数字): {}", key, n),
            serde_json::Value::Bool(b) => tracing::info!("  {} (布尔): {}", key, b),
            serde_json::Value::Array(arr) => {
                tracing::info!("  {} (数组): {} 个元素", key, arr.len())
            }
            serde_json::Value::Object(obj) => {
                tracing::info!("  {} (对象): {} 个字段", key, obj.len())
            }
            serde_json::Value::Null => tracing::info!("  {} (空值)", key),
        }
    }

    // 处理预审相关信息
    if let Some(preview_id) = callback_data.get("previewId").and_then(|v| v.as_str()) {
        tracing::info!("[ok] 第三方系统预审完成通知: {}", preview_id);

        if let Some(status) = callback_data.get("status").and_then(|v| v.as_str()) {
            tracing::info!("预审状态: {}", status);
        }

        if let Some(third_party_id) = callback_data
            .get("thirdPartyRequestId")
            .and_then(|v| v.as_str())
        {
            tracing::info!("第三方请求ID: {}", third_party_id);
        }

        // 检查是否有材料URL信息
        if let Some(materials) = callback_data.get("materials").and_then(|v| v.as_array()) {
            tracing::info!("材料信息: {} 个材料", materials.len());
            for (i, material) in materials.iter().enumerate() {
                if let Some(url) = material.get("url").and_then(|v| v.as_str()) {
                    tracing::info!("  材料{}: {}", i + 1, url);

                    // 分析URL特征
                    if let Ok(parsed_url) = url::Url::parse(url) {
                        tracing::info!("    域名: {}", parsed_url.host_str().unwrap_or("未知"));
                        if let Some(query) = parsed_url.query() {
                            tracing::info!("    查询参数: {}", query);
                        }
                    }
                }
            }
        }
    }

    tracing::info!("=== 第三方系统回调处理完成 ===");

    // 返回成功响应（模拟第三方系统接收成功）
    Json(serde_json::json!({
        "success": true,
        "message": "回调接收成功",
        "timestamp": Utc::now().to_rfc3339()
    }))
}

/// 获取预审结果详情（用于政务风格展示页面）
pub async fn get_preview_result(
    Path(preview_id): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("获取预审结果详情: {}", preview_id);

    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            // 解析评估结果（如果存在）
            let evaluation_json = preview
                .evaluation_result
                .as_ref()
                .and_then(|eval_result| serde_json::from_str::<serde_json::Value>(eval_result).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            let mut evaluation_struct = preview
                .evaluation_result
                .as_deref()
                .and_then(|raw| serde_json::from_str::<PreviewEvaluationResult>(raw).ok());

            if let Some(eval) = evaluation_struct.as_mut() {
                crate::api::utils::sanitize_evaluation_result(eval);
            }

            // 如果状态已完成但 evaluation 为空，增加告警日志，提示前端等待
            if preview.status == PreviewStatus::Completed && preview.evaluation_result.is_none() {
                warn!(
                    preview_id = %preview_id,
                    "预审已完成但 evaluation_result 缺失，可能尚未落库"
                );
            }

            // 构建政务风格的预审结果数据
            let result_data = serde_json::json!({
                "preview_id": preview_id,
                "applicant": evaluation_json
                    .get("applicant")
                    .and_then(|v| v.as_str())
                    .unwrap_or("申请人"),
                "applicant_name": evaluation_json
                    .get("applicant")
                    .and_then(|v| v.as_str())
                    .unwrap_or("申请人"),
                "matter_name": evaluation_json
                    .get("matter_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&preview.file_name),
                "theme_name": evaluation_json
                    .get("matter_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&preview.file_name),
                "status": preview.status,
                "created_at": preview.created_at,

                // [image] 增强材料数据，包含图片信息
                "materials": build_enhanced_materials(evaluation_struct.as_ref(), &preview, &preview_id),

                // [doc] 文档信息 - 新增OCR图片支持
                "documents": build_document_list(evaluation_struct.as_ref(), &preview, &preview_id),

                // [stats] 基本统计信息
                "statistics": {
                    "total_materials": evaluation_struct.as_ref()
                        .map(|eval| eval.material_results.len())
                        .unwrap_or_else(|| evaluation_json.get("materials").and_then(|v| v.as_array()).map(|arr| arr.len()).unwrap_or(1)),
                    "total_pages": get_total_pages(&preview),
                    "has_ocr_images": evaluation_struct
                        .as_ref()
                        .map(|eval| eval.material_results.iter().any(|material| material.attachments.iter().any(|a| a.preview_url.as_ref().map(|url| !url.is_empty()).unwrap_or(false))))
                        .unwrap_or_else(|| check_ocr_images_exist(&preview))
                }
            });

            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": result_data
            }))
        }
        Ok(None) => Json(serde_json::json!({
            "success": false,
            "errorCode": 404,
            "errorMsg": "预审记录不存在",
            "data": null
        })),
        Err(e) => {
            tracing::error!("获取预审结果失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取预审结果失败",
                "data": null
            }))
        }
    }
}

fn resolve_preview_file(preview_id: &str, extension: &str) -> Option<PathBuf> {
    let file_name = format!("{}.{}", preview_id, extension);
    let candidate_dirs = [
        CURRENT_DIR.join("preview"),
        CURRENT_DIR
            .join("runtime")
            .join("fallback")
            .join("storage")
            .join("previews"),
        CURRENT_DIR.join("storage").join("previews"),
    ];

    for dir in candidate_dirs {
        let candidate = dir.join(&file_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn render_report_html(preview: &PreviewRecord) -> Option<String> {
    match preview.evaluation_result.as_deref() {
        Some(json) => match serde_json::from_str::<PreviewEvaluationResult>(json) {
            Ok(result) => Some(PreviewReportGenerator::generate_html(&result)),
            Err(err) => {
                tracing::error!(
                    preview_id = %preview.id,
                    error = %err,
                    "解析 evaluation_result JSON 失败"
                );
                None
            }
        },
        None => None,
    }
}

/// Renders the report HTML with attachment URL enrichment (async version)
/// This fetches OSS public URLs for historical records that may have stale URLs
async fn render_report_html_enriched(
    preview: &PreviewRecord,
    database: &std::sync::Arc<dyn crate::db::Database>,
    storage: &std::sync::Arc<dyn crate::storage::Storage>,
) -> Option<String> {
    match preview.evaluation_result.as_deref() {
        Some(json) => match serde_json::from_str::<PreviewEvaluationResult>(json) {
            Ok(mut result) => {
                // Enrich attachment URLs with OSS public URLs
                if let Err(e) = super::worker_proxy::enrich_preview_attachments(
                    database,
                    storage,
                    &preview.id,
                    &mut result,
                )
                .await
                {
                    tracing::warn!(
                        preview_id = %preview.id,
                        error = %e,
                        "Failed to enrich attachment URLs, continuing with original URLs"
                    );
                }
                Some(PreviewReportGenerator::generate_html(&result))
            }
            Err(err) => {
                tracing::error!(
                    preview_id = %preview.id,
                    error = %err,
                    "解析 evaluation_result JSON 失败"
                );
                None
            }
        },
        None => None,
    }
}

fn render_error_report(preview_id: &str, reason: &str) -> String {
    PreviewReportGenerator::generate_error_html(preview_id, reason)
}

const DEFAULT_REPORT_UNAVAILABLE_MESSAGE: &str = "报告数据暂不可用，请稍后重试或联系运维人员。";

fn build_missing_evaluation_message(preview: &PreviewRecord) -> String {
    match preview.status {
        PreviewStatus::Pending | PreviewStatus::Queued | PreviewStatus::Processing => {
            "报告生成中，OCR流程尚未完成，请稍后刷新页面。".to_string()
        }
        PreviewStatus::Failed => {
            if let Some(reason) = preview
                .failure_reason
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                format!("预审任务失败：{}。请联系运维人员处理。", reason)
            } else if let Some(context) = preview
                .failure_context
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                format!("预审任务失败，详细原因：{}。请联系运维人员处理。", context)
            } else {
                "预审任务失败，未生成报告，请联系运维人员处理。".to_string()
            }
        }
        PreviewStatus::Completed => {
            "预审任务已标记为完成，但系统未收到评估结果。请稍后重试，如问题持续请联系运维人员。"
                .to_string()
        }
    }
}

async fn generate_pdf_on_demand(preview_id: &str, html: &str, state: &AppState) -> Option<Vec<u8>> {
    let temp_dir = std::path::PathBuf::from(&state.config.master.temp_pdf_dir);
    if let Err(err) = fs::create_dir_all(&temp_dir).await {
        tracing::warn!(
            preview_id = %preview_id,
            error = %err,
            "创建临时PDF目录失败，跳过按需生成PDF"
        );
        return None;
    }

    let temp_pdf = temp_dir.join(format!("{}_on_demand.pdf", preview_id));
    match PdfGenerator::html_to_pdf(html, &temp_pdf).await {
        Ok(_) => match fs::read(&temp_pdf).await {
            Ok(bytes) => {
                let preview_dir = CURRENT_DIR.join("preview");
                if let Err(err) = fs::create_dir_all(&preview_dir).await {
                    tracing::warn!(
                        preview_id = %preview_id,
                        error = %err,
                        "创建预览目录失败，跳过落盘缓存"
                    );
                } else {
                    let dest = preview_dir.join(format!("{}.pdf", preview_id));
                    if let Err(err) = fs::write(&dest, &bytes).await {
                        tracing::warn!(
                            preview_id = %preview_id,
                            error = %err,
                            "写入本地PDF缓存失败"
                        );
                    }
                }

                let _ = fs::remove_file(&temp_pdf).await;
                Some(bytes)
            }
            Err(err) => {
                tracing::warn!(
                    preview_id = %preview_id,
                    error = %err,
                    "读取按需生成的PDF失败"
                );
                let _ = fs::remove_file(&temp_pdf).await;
                None
            }
        },
        Err(err) => {
            tracing::warn!(
                preview_id = %preview_id,
                error = %err,
                "按需生成PDF失败"
            );
            let _ = fs::remove_file(&temp_pdf).await;
            None
        }
    }
}

fn build_attachment_header(preview_id: &str, extension: &str) -> String {
    let utf8_filename = format!("预审报告_{}.{}", preview_id, extension);
    let ascii_fallback = format!("{}_report.{}", preview_id, extension);
    format!(
        "attachment; filename=\"{}\"; filename*=UTF-8''{}",
        ascii_fallback,
        urlencoding::encode(&utf8_filename)
    )
}

fn build_pdf_download_response(preview_id: &str, bytes: Vec<u8>) -> Response {
    let disposition = build_attachment_header(preview_id, "pdf");
    Response::builder()
        .header(header::CONTENT_TYPE, "application/pdf")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from(bytes))
        .unwrap_or_else(|e| {
            tracing::error!("构建PDF下载响应失败: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("下载报告失败"))
                .unwrap_or_else(|_| Response::new(Body::from("下载报告失败")))
        })
}

fn build_html_download_response(preview_id: &str, html_content: String) -> Response {
    let disposition = build_attachment_header(preview_id, "html");
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CONTENT_DISPOSITION, disposition)
        .body(Body::from(html_content))
        .unwrap_or_else(|e| {
            tracing::error!("构建HTML下载响应失败: {}", e);
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::from("下载报告失败"))
                .unwrap_or_else(|_| Response::new(Body::from("下载报告失败")))
        })
}

fn canonicalize_external_url(
    url: &str,
    monitor_session_id: Option<&str>,
    config: &Config,
) -> Option<String> {
    if url.trim().is_empty() {
        return None;
    }

    let base_url = resolve_external_base_url(config);

    let mut parsed = match Url::parse(url) {
        Ok(mut parsed) => {
            if let Some(host) = parsed.host_str() {
                if is_internal_host(host) {
                    if let Some(base) = base_url.as_ref() {
                        let _ = parsed.set_scheme(base.scheme());
                        if let Some(base_host) = base.host_str() {
                            let _ = parsed.set_host(Some(base_host));
                        }
                        let _ = parsed.set_port(base.port());

                        if parsed.host_str().map(is_internal_host).unwrap_or(true) {
                            tracing::warn!(
                                original = %url,
                                "规范化下载链接失败：基准URL仍指向内网地址"
                            );
                            return None;
                        }
                    } else {
                        tracing::warn!(
                            original = %url,
                            "规范化下载链接失败：缺少可对外访问的基准URL"
                        );
                        return None;
                    }
                }
            }
            parsed
        }
        Err(_) => {
            if url.starts_with('/') {
                if let Some(base) = base_url {
                    match base.join(url) {
                        Ok(mut joined) => {
                            if let Some(session) = monitor_session_id {
                                append_monitor_session(&mut joined, session);
                            }
                            return Some(joined.into_string());
                        }
                        Err(err) => {
                            tracing::warn!(
                                original = %url,
                                error = %err,
                                "规范化相对路径下载链接失败"
                            );
                            return None;
                        }
                    }
                } else {
                    tracing::warn!(
                        original = %url,
                        "规范化相对路径下载链接失败：缺少可对外访问的基准URL"
                    );
                }
            }
            return None;
        }
    };

    if let Some(session) = monitor_session_id {
        append_monitor_session(&mut parsed, session);
    }

    Some(parsed.into_string())
}

fn is_self_preview_download_url(url: &str, preview_id: &str) -> bool {
    if let Ok(parsed) = Url::parse(url) {
        let path = parsed.path();
        return path.ends_with(&format!("/api/preview/download/{}", preview_id));
    }

    // 相对路径的简单判断
    url.contains("/api/preview/download/") && url.contains(preview_id)
}

fn append_monitor_session(parsed: &mut Url, session: &str) {
    let has_param = parsed
        .query_pairs()
        .any(|(key, _)| key == "monitor_session_id");
    if !has_param {
        parsed
            .query_pairs_mut()
            .append_pair("monitor_session_id", session);
    }
}

fn resolve_external_base_url(config: &Config) -> Option<Url> {
    if let Some(public) = config
        .public_base_url
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        return parse_external_url("public_base_url", public.trim_end_matches('/'));
    }

    parse_external_url("server.base_url", &config.base_url())
}

fn parse_external_url(source: &str, value: &str) -> Option<Url> {
    match Url::parse(value) {
        Ok(url) => {
            if let Some(host) = url.host_str() {
                if is_internal_host(host) {
                    tracing::warn!(
                        source = %source,
                        host = %host,
                        "基准URL指向内网地址，无法用于对外访问"
                    );
                    None
                } else {
                    Some(url)
                }
            } else {
                tracing::warn!(
                    source = %source,
                    "基准URL缺少主机名，无法用于对外访问: {}",
                    value
                );
                None
            }
        }
        Err(err) => {
            tracing::warn!(
                source = %source,
                error = %err,
                "解析基准URL失败: {}",
                value
            );
            None
        }
    }
}

/// 下载预审报告
pub async fn download_preview_report(
    Path(preview_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let format = params.get("format").unwrap_or(&"pdf".to_string()).clone();
    let monitor_session_id = params.get("monitor_session_id").map(String::as_str);

    tracing::info!(
        preview_id = %preview_id,
        format = %format,
        has_monitor_session = monitor_session_id.is_some(),
        "📥 [下载请求] 开始处理预审报告下载"
    );

    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            let redirect_to = |url: &str| {
                Response::builder()
                    .status(StatusCode::FOUND)
                    .header(header::LOCATION, url)
                    .body(Body::empty())
                    .map(IntoResponse::into_response)
            };

            // 准备回退HTML内容 (使用 async 版本以解析 OSS URL)
            let report_html =
                render_report_html_enriched(&preview, &state.database, &state.storage).await;
            let fallback_reason = if report_html.is_some() {
                None
            } else {
                let reason = build_missing_evaluation_message(&preview);
                tracing::warn!(
                    preview_id = %preview_id,
                    status = %preview.status.as_str(),
                    reason = %reason,
                    "⚠️  [回退准备] evaluation_result 不可用，准备提示页面"
                );
                Some(reason)
            };
            let fallback = || {
                let reason = fallback_reason
                    .as_deref()
                    .unwrap_or(DEFAULT_REPORT_UNAVAILABLE_MESSAGE);
                render_error_report(&preview_id, reason)
            };

            match format.as_str() {
                "pdf" => {
                    // 🎯 第一步：优先检查本地PDF文件
                    tracing::debug!(
                        preview_id = %preview_id,
                        "🔍 [本地查找] 检查本地PDF文件: preview/{}.pdf",
                        preview_id
                    );

                    if let Some(pdf_path) = resolve_preview_file(&preview_id, "pdf") {
                        tracing::info!(
                            preview_id = %preview_id,
                            path = %pdf_path.display(),
                            "✅ [本地命中] 找到本地PDF文件，开始读取"
                        );

                        match tokio::fs::read(&pdf_path).await {
                            Ok(bytes) => {
                                tracing::info!(
                                    preview_id = %preview_id,
                                    path = %pdf_path.display(),
                                    size_bytes = bytes.len(),
                                    "📄 [本地返回] 成功读取本地PDF文件，直接返回"
                                );
                                return build_pdf_download_response(&preview_id, bytes);
                            }
                            Err(e) => {
                                tracing::error!(
                                    preview_id = %preview_id,
                                    path = %pdf_path.display(),
                                    error = %e,
                                    "❌ [读取失败] 本地PDF文件读取失败，回退为HTML"
                                );
                                let html_content =
                                    report_html.clone().unwrap_or_else(|| fallback());
                                return build_html_download_response(&preview_id, html_content);
                            }
                        }
                    }

                    // 🔄 第二步：尝试按需生成PDF（基于 evaluation_result 渲染的HTML）
                    tracing::warn!(
                        preview_id = %preview_id,
                        "⚠️  [本地缺失] 本地PDF文件不存在，尝试按需生成"
                    );

                    if let Some(html) = report_html.clone() {
                        if let Some(bytes) =
                            generate_pdf_on_demand(&preview_id, &html, &state).await
                        {
                            tracing::info!(
                                preview_id = %preview_id,
                                "✅ [按需生成] PDF已生成并返回"
                            );
                            return build_pdf_download_response(&preview_id, bytes);
                        }
                    }

                    // 🔄 第三步：本地文件不存在，尝试外部链接
                    tracing::warn!(
                        preview_id = %preview_id,
                        "⚠️  [按需生成失败] 检查外部下载地址"
                    );

                    if let Some(download_url) = preview.preview_download_url.as_deref() {
                        tracing::debug!(
                            preview_id = %preview_id,
                            original_url = %download_url,
                            "🔗 [外链检查] 找到外部下载URL，尝试规范化"
                        );

                        match canonicalize_external_url(
                            download_url,
                            monitor_session_id,
                            &state.config,
                        ) {
                            Some(normalized)
                                if normalized.starts_with("http://")
                                    || normalized.starts_with("https://") =>
                            {
                                if is_self_preview_download_url(&normalized, &preview_id) {
                                    tracing::warn!(
                                        preview_id = %preview_id,
                                        url = %normalized,
                                        "⚠️  [外链重定向跳过] 目标指向自身download接口，避免302循环"
                                    );
                                } else {
                                    tracing::info!(
                                        preview_id = %preview_id,
                                        original_url = %download_url,
                                        redirect_to = %normalized,
                                        "🔀 [外链重定向] 本地文件缺失，302重定向到外部PDF链接"
                                    );
                                    if let Ok(resp) = redirect_to(&normalized) {
                                        return resp;
                                    }
                                    tracing::error!(
                                        preview_id = %preview_id,
                                        target = %normalized,
                                        "❌ [重定向失败] 构建302响应失败，回退为HTML"
                                    );
                                }
                            }
                            Some(other) => {
                                tracing::warn!(
                                    preview_id = %preview_id,
                                    url = %other,
                                    "⚠️  [协议不支持] 外部PDF链接协议不受支持，回退为HTML"
                                );
                            }
                            None => {
                                tracing::warn!(
                                    preview_id = %preview_id,
                                    original_url = %download_url,
                                    "⚠️  [规范化失败] 外部PDF链接规范化失败，回退为HTML"
                                );
                            }
                        }
                    } else {
                        tracing::warn!(
                            preview_id = %preview_id,
                            "⚠️  [配置缺失] 预审记录未配置外部PDF下载地址"
                        );
                    }

                    // 🧭 第三步：尝试使用已生成的本地HTML作为兜底
                    if let Some(html_path) = resolve_preview_file(&preview_id, "html") {
                        tracing::info!(
                            preview_id = %preview_id,
                            path = %html_path.display(),
                            "🔄 [HTML兜底] 本地PDF缺失，使用已生成的HTML文件"
                        );
                        match tokio::fs::read_to_string(&html_path).await {
                            Ok(content) => {
                                return build_html_download_response(&preview_id, content);
                            }
                            Err(err) => {
                                tracing::warn!(
                                    preview_id = %preview_id,
                                    path = %html_path.display(),
                                    error = %err,
                                    "读取本地HTML兜底失败，继续使用生成内容"
                                );
                            }
                        }
                    }

                    // 🛡️ 第三步：最终回退为生成的HTML
                    tracing::info!(
                        preview_id = %preview_id,
                        "🔄 [最终回退] 本地文件和外部链接都不可用，返回生成的HTML提示页面"
                    );
                    let html_content = report_html.clone().unwrap_or_else(|| fallback());
                    build_html_download_response(&preview_id, html_content)
                }
                "html" => {
                    // 🎯 第一步：优先检查本地HTML文件
                    tracing::debug!(
                        preview_id = %preview_id,
                        "🔍 [本地查找] 检查本地HTML文件: preview/{}.html",
                        preview_id
                    );

                    if let Some(html_path) = resolve_preview_file(&preview_id, "html") {
                        tracing::info!(
                            preview_id = %preview_id,
                            path = %html_path.display(),
                            "✅ [本地命中] 找到本地HTML文件，开始读取"
                        );

                        match tokio::fs::read_to_string(&html_path).await {
                            Ok(content) => {
                                tracing::info!(
                                    preview_id = %preview_id,
                                    path = %html_path.display(),
                                    size_chars = content.len(),
                                    "📄 [本地返回] 成功读取本地HTML文件，直接返回"
                                );
                                return build_html_download_response(&preview_id, content);
                            }
                            Err(e) => {
                                tracing::error!(
                                    preview_id = %preview_id,
                                    path = %html_path.display(),
                                    error = %e,
                                    "❌ [读取失败] 本地HTML文件读取失败，尝试外部链接"
                                );

                                // HTML读取失败，尝试外部链接
                                if let Some(normalized) = canonicalize_external_url(
                                    &preview.preview_url,
                                    monitor_session_id,
                                    &state.config,
                                ) {
                                    if normalized.starts_with("http://")
                                        || normalized.starts_with("https://")
                                    {
                                        tracing::info!(
                                            preview_id = %preview_id,
                                            redirect_to = %normalized,
                                            "🔀 [外链重定向] 本地HTML读取失败，302重定向到外部预览链接"
                                        );
                                        if let Ok(resp) = redirect_to(&normalized) {
                                            return resp;
                                        }
                                    }
                                }

                                tracing::info!(
                                    preview_id = %preview_id,
                                    "🔄 [最终回退] 外部链接也不可用，返回生成的HTML内容"
                                );
                                let html_content =
                                    report_html.clone().unwrap_or_else(|| fallback());
                                return build_html_download_response(&preview_id, html_content);
                            }
                        }
                    }

                    // 🔄 第二步：本地文件不存在，尝试外部链接
                    tracing::warn!(
                        preview_id = %preview_id,
                        "⚠️  [本地缺失] 本地HTML文件不存在，检查外部预览地址"
                    );

                    if let Some(normalized) = canonicalize_external_url(
                        &preview.preview_url,
                        monitor_session_id,
                        &state.config,
                    ) {
                        if normalized.starts_with("http://") || normalized.starts_with("https://") {
                            if is_self_preview_download_url(&normalized, &preview_id) {
                                tracing::warn!(
                                    preview_id = %preview_id,
                                    url = %normalized,
                                    "⚠️  [外链重定向跳过] 目标指向自身download接口，避免302循环"
                                );
                            } else {
                                tracing::info!(
                                    preview_id = %preview_id,
                                    redirect_to = %normalized,
                                    "🔀 [外链重定向] 本地文件缺失，302重定向到外部HTML预览链接"
                                );
                                if let Ok(resp) = redirect_to(&normalized) {
                                    return resp;
                                }
                                tracing::error!(
                                    preview_id = %preview_id,
                                    target = %normalized,
                                    "❌ [重定向失败] 构建302响应失败，回退为生成内容"
                                );
                            }
                        }
                    } else {
                        tracing::warn!(
                            preview_id = %preview_id,
                            "⚠️  [配置缺失] 预审记录缺少可用的外部HTML预览地址"
                        );
                    }

                    // 🛡️ 第三步：最终回退为生成的HTML
                    tracing::info!(
                        preview_id = %preview_id,
                        "🔄 [最终回退] 本地文件和外部链接都不可用，返回生成的HTML内容"
                    );
                    let html_content = report_html.clone().unwrap_or_else(|| fallback());
                    build_html_download_response(&preview_id, html_content)
                }
                _ => {
                    tracing::error!(
                        preview_id = %preview_id,
                        format = %format,
                        "❌ [格式错误] 不支持的下载格式"
                    );
                    (StatusCode::BAD_REQUEST, "不支持的格式").into_response()
                }
            }
        }
        Ok(None) => {
            tracing::error!(
                preview_id = %preview_id,
                "❌ [记录不存在] 数据库中找不到该预审记录"
            );
            (StatusCode::NOT_FOUND, "预审记录不存在").into_response()
        }
        Err(e) => {
            tracing::error!(
                preview_id = %preview_id,
                error = %e,
                "❌ [数据库错误] 获取预审记录失败"
            );
            (StatusCode::INTERNAL_SERVER_ERROR, "获取预审记录失败").into_response()
        }
    }
}

/// 获取OCR处理后的图片
pub async fn get_ocr_image(
    Path((pdf_name, page_index)): Path<(String, usize)>,
) -> impl IntoResponse {
    tracing::info!("获取OCR图片: {} 页码: {}", pdf_name, page_index);

    let image_path = CURRENT_DIR
        .join("images")
        .join(format!("{}_{}.jpg", pdf_name, page_index));

    tracing::debug!("OCR图片路径: {:?}", image_path);

    match fs::read(&image_path).await {
        Ok(image_data) => {
            tracing::info!("[ok] OCR图片读取成功: {} bytes", image_data.len());
            let response = Response::builder()
                .header("Content-Type", "image/jpeg")
                .header("Cache-Control", "public, max-age=3600")
                .header("Access-Control-Allow-Origin", "*")
                .body(Body::from(image_data));

            match response {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::error!("构建响应失败: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
        Err(e) => {
            tracing::warn!("[fail] OCR图片不存在: {:?} - {}", image_path, e);

            // 返回默认的文档占位图
            let default_image = include_bytes!("../../static/images/智能预审_审核依据材料1.3.png");
            let response = Response::builder()
                .header("Content-Type", "image/png")
                .header("Cache-Control", "public, max-age=3600")
                .body(Body::from(&default_image[..]));

            match response {
                Ok(resp) => resp,
                Err(e) => {
                    tracing::error!("构建默认图片响应失败: {}", e);
                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                }
            }
        }
    }
}

/// 获取预审文档的缩略图
pub async fn get_preview_thumbnail(
    Path((preview_id, page_index)): Path<(String, usize)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("获取预审缩略图: {} 页码: {}", preview_id, page_index);

    // 先尝试从数据库获取预审记录，获取原始文件名
    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            // 使用文件名生成图片路径
            let file_name_base = preview
                .file_name
                .split('.')
                .next()
                .unwrap_or(&preview.file_name);
            get_ocr_image(Path((file_name_base.to_string(), page_index))).await
        }
        _ => {
            // 如果无法获取记录，尝试直接使用preview_id
            get_ocr_image(Path((preview_id, page_index))).await
        }
    }
}

/// 获取材料预览图片（智能匹配OCR结果）
pub async fn get_material_preview(
    Path((preview_id, material_name)): Path<(String, String)>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    tracing::info!("获取材料预览图片: {} 材料: {}", preview_id, material_name);

    // 优先从材料文件表中查找已落库的附件
    if let Ok(records) = state
        .database
        .list_material_files(&MaterialFileFilter {
            preview_id: Some(preview_id.clone()),
            material_code: None,
        })
        .await
    {
        if let Some(record) = find_material_record(&records, &material_name) {
            if let Some(resp) = serve_material_from_storage(&state, record).await {
                return resp;
            }
        }
    }

    // 兼容旧逻辑：回退到 OCR 生成的第一页缩略图
    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            let file_name_base = preview
                .file_name
                .split('.')
                .next()
                .unwrap_or(&preview.file_name);

            let image_path = CURRENT_DIR
                .join("images")
                .join(format!("{}_0.jpg", file_name_base));

            match fs::read(&image_path).await {
                Ok(image_data) => match Response::builder()
                    .header("Content-Type", "image/jpeg")
                    .header("Cache-Control", "public, max-age=3600")
                    .body(Body::from(image_data))
                {
                    Ok(response) => response,
                    Err(e) => {
                        tracing::error!("构建图片响应失败: {}", e);
                        StatusCode::INTERNAL_SERVER_ERROR.into_response()
                    }
                },
                Err(_) => {
                    let material_status = "pending";
                    let icon_path =
                        crate::api::utils::get_material_image_path(&material_name, material_status);
                    let static_path = format!("./static{}", icon_path);
                    match fs::read(&static_path).await {
                        Ok(icon_data) => {
                            match Response::builder()
                                .header("Content-Type", "image/png")
                                .header("Cache-Control", "public, max-age=3600")
                                .body(Body::from(icon_data))
                            {
                                Ok(response) => response,
                                Err(e) => {
                                    tracing::error!("构建图标响应失败: {}", e);
                                    StatusCode::INTERNAL_SERVER_ERROR.into_response()
                                }
                            }
                        }
                        Err(_) => (StatusCode::NOT_FOUND, "图片不存在").into_response(),
                    }
                }
            }
        }
        _ => (StatusCode::NOT_FOUND, "预审记录不存在").into_response(),
    }
}

fn find_material_record<'a>(
    records: &'a [MaterialFileRecord],
    material_name: &str,
) -> Option<&'a MaterialFileRecord> {
    let lower = material_name.to_ascii_lowercase();
    records.iter().find(|rec| {
        rec.attachment_name
            .as_ref()
            .map(|n| n.to_ascii_lowercase() == lower)
            .unwrap_or(false)
            || rec
                .source_url
                .as_deref()
                .map(|u| u.contains(material_name))
                .unwrap_or(false)
            || rec.stored_original_key.contains(material_name)
    })
}

async fn serve_material_from_storage(
    state: &AppState,
    record: &MaterialFileRecord,
) -> Option<Response> {
    let key = record
        .stored_processed_keys
        .as_deref()
        .and_then(|keys| {
            keys.split(&[',', ';'][..])
                .map(|k| k.trim())
                .find(|k| !k.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| record.stored_original_key.clone());

    let storage_key = key.trim_start_matches('/').to_string();

    match state.storage.get(&storage_key).await {
        Ok(Some(bytes)) => {
            let content_type = record
                .mime_type
                .clone()
                .or_else(|| {
                    MimeGuess::from_path(&storage_key)
                        .first()
                        .map(|m| m.to_string())
                })
                .unwrap_or_else(|| "application/octet-stream".to_string());

            match Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::CACHE_CONTROL, "public, max-age=600")
                .body(Body::from(bytes))
            {
                Ok(resp) => Some(resp),
                Err(e) => {
                    warn!("构建材料预览响应失败: {}", e);
                    None
                }
            }
        }
        Ok(None) => {
            warn!(
                "材料预览文件不存在: preview={} key={}",
                record.preview_id, storage_key
            );
            None
        }
        Err(e) => {
            warn!(
                "读取材料预览文件失败: preview={} key={} err={}",
                record.preview_id, storage_key, e
            );
            None
        }
    }
}

/// 代理存储文件，防止直接暴露 OSS/本地存储路径
pub async fn proxy_storage_file(
    Path(encoded_key): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let decoded = match urlencoding::decode(&encoded_key) {
        Ok(path) => path.to_string(),
        Err(_) => return (StatusCode::BAD_REQUEST, "无效的存储路径").into_response(),
    };
    let storage_key = decoded.trim_start_matches('/');

    let bytes = match state.storage.get(storage_key).await {
        Ok(Some(data)) => data,
        Ok(None) => return (StatusCode::NOT_FOUND, "文件不存在").into_response(),
        Err(err) => {
            tracing::error!(
                storage_key = %storage_key,
                error = %err,
                "读取存储文件失败"
            );
            return (StatusCode::INTERNAL_SERVER_ERROR, "读取存储文件失败").into_response();
        }
    };

    let content_type = state
        .storage
        .get_metadata(storage_key)
        .await
        .ok()
        .and_then(|meta| meta.content_type)
        .or_else(|| {
            MimeGuess::from_path(storage_key)
                .first()
                .map(|m| m.to_string())
        })
        .unwrap_or_else(|| "application/octet-stream".to_string());

    match Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "private, max-age=60")
        .body(Body::from(bytes))
    {
        Ok(resp) => resp,
        Err(err) => {
            tracing::error!(error = %err, "构建存储代理响应失败");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// 构建增强的材料数据，包含OCR图片信息
fn build_enhanced_materials(
    evaluation: Option<&PreviewEvaluationResult>,
    preview: &crate::db::PreviewRecord,
    preview_id: &str,
) -> Vec<serde_json::Value> {
    if let Some(evaluation) = evaluation {
        return evaluation
            .material_results
            .iter()
            .enumerate()
            .map(|(index, material)| {
                let status = material_status_from_code(material.rule_evaluation.status_code);
                let status_icon =
                    crate::api::utils::get_material_image_path(&material.material_name, status);

                let attachment_url = material
                    .attachments
                    .iter()
                    .filter_map(|att| att.preview_url.as_deref().filter(|u| !u.is_empty()))
                    .next();

                let fallback_base = preview
                    .file_name
                    .split('.')
                    .next()
                    .unwrap_or(&preview.file_name);
                let default_preview = format!("/api/files/ocr-image/{}/{}", fallback_base, index);

                let preview_url = attachment_url.unwrap_or(&default_preview);

                serde_json::json!({
                    "id": index as u64 + 1,
                    "name": material.material_name,
                    "status": status,
                    "pages": material.attachments.len().max(1),
                    "count": material.attachments.len().max(1),
                    "image": {
                        "status_icon": status_icon,
                        "ocr_image": preview_url,
                        "preview_url": preview_url,
                        "has_ocr_image": true
                    },
                    "review_points": material.rule_evaluation.suggestions.clone(),
                    "review_notes": material.display_detail,
                    "expanded": false,
                    "items": build_material_items_from_result(material)
                })
            })
            .collect();
    }

    // 如果没有材料数据，创建默认材料项
    let file_name_base = preview
        .file_name
        .split('.')
        .next()
        .unwrap_or(&preview.file_name);
    let status_icon = crate::api::utils::get_material_image_path(&preview.file_name, "pending");
    let ocr_image_url = format!("/api/files/ocr-image/{}/0", file_name_base);
    let material_preview_url = format!(
        "/api/files/material-preview/{}/{}",
        preview_id, preview.file_name
    );

    vec![serde_json::json!({
        "id": 1,
        "name": preview.file_name,
        "status": "pending",
        "pages": 1,
        "count": 1,
        "image": {
            "status_icon": status_icon,
            "ocr_image": ocr_image_url,
            "preview_url": material_preview_url,
            "has_ocr_image": check_ocr_image_exists(&preview.file_name, 0)
        },
        "review_points": [],
        "review_notes": null,
        "expanded": false,
        "items": []
    })]
}

/// 构建材料子项列表
fn build_material_items_from_result(
    material: &crate::model::evaluation::MaterialEvaluationResult,
) -> Vec<serde_json::Value> {
    if let Some(detail) = material.display_detail.as_ref() {
        return vec![serde_json::json!({
            "id": 1,
            "name": material.material_name,
            "status": material_status_from_code(material.rule_evaluation.status_code),
            "hasDocument": true,
            "checkPoint": detail
        })];
    }
    Vec::new()
}

/// 构建文档列表，包含OCR图片信息
fn build_document_list(
    evaluation: Option<&PreviewEvaluationResult>,
    preview: &crate::db::PreviewRecord,
    preview_id: &str,
) -> Vec<serde_json::Value> {
    if let Some(evaluation) = evaluation {
        let mut documents = Vec::new();
        for (index, material) in evaluation.material_results.iter().enumerate() {
            for (att_index, attachment) in material.attachments.iter().enumerate() {
                if let Some(url) = attachment.preview_url.as_ref() {
                    documents.push(serde_json::json!({
                        "id": format!("doc_{}_{}_{}", preview_id, index, att_index),
                        "name": attachment.file_name,
                        "type": "image",
                        "url": url,
                        "thumbnail": url,
                        "page_number": att_index + 1
                    }));
                }
            }
        }
        if !documents.is_empty() {
            return documents;
        }
    }

    let file_name_base = preview
        .file_name
        .split('.')
        .next()
        .unwrap_or(&preview.file_name);
    let mut documents = Vec::new();

    // 检查OCR图片数量
    let total_pages = get_total_pages(preview);

    for page_index in 0..total_pages {
        let image_path = CURRENT_DIR
            .join("images")
            .join(format!("{}_{}.jpg", file_name_base, page_index));

        if image_path.exists() {
            documents.push(serde_json::json!({
                "id": format!("doc_{}_{}", preview_id, page_index),
                "name": format!("第{}页", page_index + 1),
                "type": "image",
                "url": format!("/api/files/ocr-image/{}/{}", file_name_base, page_index),
                "thumbnail": format!("/api/files/preview-thumbnail/{}/{}", preview_id, page_index),
                "page_number": page_index + 1
            }));
        }
    }

    // 如果没有OCR图片，提供默认文档
    if documents.is_empty() {
        documents.push(serde_json::json!({
            "id": format!("doc_{}_default", preview_id),
            "name": "预审文档",
            "type": "pdf",
            "url": format!("/api/download?goto=storage/previews/{}.pdf", preview_id),
            "thumbnail": "/static/images/document-placeholder.png",
            "page_number": 1
        }));
    }

    documents
}

/// 检查OCR图片是否存在
fn check_ocr_image_exists(file_name: &str, page_index: usize) -> bool {
    let file_name_base = file_name.split('.').next().unwrap_or(file_name);
    let image_path = CURRENT_DIR
        .join("images")
        .join(format!("{}_{}.jpg", file_name_base, page_index));
    image_path.exists()
}

/// 检查是否有OCR图片
fn check_ocr_images_exist(preview: &crate::db::PreviewRecord) -> bool {
    check_ocr_image_exists(&preview.file_name, 0)
}

/// 获取总页数
fn get_total_pages(preview: &crate::db::PreviewRecord) -> usize {
    let file_name_base = preview
        .file_name
        .split('.')
        .next()
        .unwrap_or(&preview.file_name);
    let images_dir = CURRENT_DIR.join("images");

    if !images_dir.exists() {
        return 1;
    }

    // 查找匹配的图片文件
    let mut page_count = 0;
    for index in 0..50 {
        // 假设最多50页
        let image_path = images_dir.join(format!("{}_{}.jpg", file_name_base, index));
        if image_path.exists() {
            page_count = index + 1;
        } else {
            break;
        }
    }

    std::cmp::max(page_count, 1) // 至少1页
}

fn material_status_from_code(code: u64) -> &'static str {
    match code {
        200 => "passed",
        201..=399 => "warning",
        _ => "error",
    }
}

/// 生成外部分享一次性URL（需要监控后台登录）
pub async fn create_preview_share_url(
    Path(preview_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let format = params
        .get("format")
        .map(|s| s.as_str())
        .unwrap_or("pdf");

    if format != "pdf" && format != "html" {
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 400,
            "errorMsg": "不支持的格式",
            "data": null
        }));
    }

    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(_)) => {}
        Ok(None) => {
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 404,
                "errorMsg": "预审记录不存在",
                "data": null
            }));
        }
        Err(err) => {
            warn!(preview_id = %preview_id, error = %err, "查询预审记录失败");
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "查询预审记录失败",
                "data": null
            }));
        }
    }

    let ttl_secs: i64 = 3600;
    let mut last_error: Option<String> = None;

    for _ in 0..5 {
        let token: String = OsRng
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        match state
            .database
            .create_preview_share_token(&preview_id, &token, format, ttl_secs)
            .await
        {
            Ok(()) => {
                let base = state.config.base_url();
                let share_url = format!(
                    "{}/api/share/{}",
                    base.trim_end_matches('/'),
                    token
                );
                return Json(serde_json::json!({
                    "success": true,
                    "errorCode": 200,
                    "errorMsg": "",
                    "data": {
                        "previewId": preview_id,
                        "token": token,
                        "format": format,
                        "shareUrl": share_url,
                        "expiresIn": ttl_secs
                    }
                }));
            }
            Err(err) => {
                last_error = Some(err.to_string());
            }
        }
    }

    warn!(
        preview_id = %preview_id,
        error = %last_error.as_deref().unwrap_or_default(),
        "生成分享token失败"
    );

    Json(serde_json::json!({
        "success": false,
        "errorCode": 500,
        "errorMsg": "生成分享链接失败",
        "data": null
    }))
}

/// 消费外部分享一次性URL（免登录，打开后立即失效）
pub async fn download_shared_preview_report(
    Path(token): Path<String>,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let record = match state.database.consume_preview_share_token(&token).await {
        Ok(rec) => rec,
        Err(err) => {
            warn!(token = %token, error = %err, "消费分享token失败");
            return Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(Body::from("分享链接处理失败"))
                .unwrap_or_else(|_| Response::new(Body::from("分享链接处理失败")));
        }
    };

    let Some(record) = record else {
        return Response::builder()
            .status(StatusCode::GONE)
            .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Body::from("分享链接已失效"))
            .unwrap_or_else(|_| Response::new(Body::from("分享链接已失效")));
    };

    let mut params = HashMap::new();
    params.insert("format".to_string(), record.format.clone());

    download_preview_report(
        Path(record.preview_id.clone()),
        Query(params),
        State(state),
    )
    .await
    .into_response()
}
