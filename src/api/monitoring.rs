//! 监控和统计模块
//! 处理系统状态监控、预审统计、队列状态等功能

use crate::api::worker_proxy;
use crate::build_info;
use crate::db::{
    PreviewFilter, PreviewRecord, PreviewRequestFilter, PreviewRequestRecord, PreviewStatus,
};
use crate::model::{
    ComponentStatus, ComponentsHealth, DetailedHealthStatus, ErrorInfo, HealthStatus, QueueStatus,
};
use crate::util::logging::runtime::ATTACHMENT_LOGGING_RUNTIME;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use crate::util::{system_info, task_queue::NatsTaskQueue};
use crate::{AppState, CONFIG};
use axum::extract::{Path, Query, State};
use axum::response::IntoResponse;
use axum::Json;
use chrono::{Datelike, Utc};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

/// 基本健康检查
pub async fn basic_health_check() -> impl IntoResponse {
    let status = HealthStatus {
        status: "healthy".to_string(),
        version: build_info::summary(),
        uptime: system_info::get_uptime_seconds(),
        timestamp: Utc::now().to_rfc3339(),
    };

    let mut resp = Json(status).into_response();
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    resp
}

/// 获取系统监控状态
pub async fn get_system_status() -> impl IntoResponse {
    let system_metrics = serde_json::json!({
        "cpu_usage": system_info::get_cpu_usage(),
        "memory_usage": system_info::get_memory_usage(),
        "disk_usage": system_info::get_disk_usage(),
        "ocr_status": "正常",
        "timestamp": Utc::now().to_rfc3339(),
        "uptime": system_info::get_uptime_seconds(),
        "version": build_info::summary()
    });

    Json(system_metrics)
}

/// 详细健康检查
pub async fn detailed_health_check() -> impl IntoResponse {
    // 使用 tokio::time::timeout 添加超时
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        collect_detailed_health_info(),
    )
    .await
    {
        Ok(status) => Json(status),
        Err(_) => {
            let error_status = DetailedHealthStatus {
                status: "error".to_string(),
                version: build_info::summary(),
                uptime: system_info::get_uptime_seconds(),
                timestamp: Utc::now().to_rfc3339(),
                memory: system_info::get_memory_usage(),
                cpu: system_info::get_cpu_usage(),
                disk: system_info::get_disk_usage(),
                queue: QueueStatus {
                    pending: 0,
                    processing: 0,
                    completed_last_hour: 0,
                    failed_last_hour: 0,
                },
                last_error: Some(ErrorInfo {
                    timestamp: Utc::now().to_rfc3339(),
                    message: "Health check timed out".to_string(),
                }),
            };
            Json(error_status)
        }
    }
}

/// 收集详细健康信息
async fn collect_detailed_health_info() -> DetailedHealthStatus {
    let memory = system_info::get_memory_usage();
    let cpu = system_info::get_cpu_usage();
    let disk = system_info::get_disk_usage();
    let queue = system_info::get_queue_status().await;

    // 确定服务状态
    let status = if cpu.usage_percent > 90.0 || memory.usage_percent > 90.0 {
        "degraded"
    } else {
        "healthy"
    };

    DetailedHealthStatus {
        status: status.to_string(),
        version: build_info::summary(),
        uptime: system_info::get_uptime_seconds(),
        timestamp: Utc::now().to_rfc3339(),
        memory,
        cpu,
        disk,
        queue,
        last_error: None,
    }
}

/// 组件健康检查
pub async fn components_health_check() -> impl IntoResponse {
    let db_connection = system_info::check_database_connection().await;

    let components = vec![
        ComponentStatus {
            name: "database".to_string(),
            status: if db_connection {
                "healthy"
            } else {
                "unhealthy"
            }
            .to_string(),
            details: Some(
                if db_connection {
                    "Connection successful"
                } else {
                    "Connection failed"
                }
                .to_string(),
            ),
            response_time_ms: None,
        },
        ComponentStatus {
            name: "file_system".to_string(),
            status: "healthy".to_string(),
            details: Some("Read/Write operations normal".to_string()),
            response_time_ms: None,
        },
        // 可以添加更多组件状态
    ];

    Json(ComponentsHealth { components })
}

/// 获取日志统计信息
pub async fn get_log_stats() -> impl IntoResponse {
    use crate::util::log::get_log_stats;

    let log_dir = std::path::Path::new(&CONFIG.logging.file.directory);
    match get_log_stats(log_dir) {
        Ok(stats) => Json(serde_json::json!({
            "success": true,
            "errorCode": 200,
            "errorMsg": "",
            "data": stats
        })),
        Err(e) => {
            tracing::error!("获取日志统计信息失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("获取日志统计信息失败: {}", e),
                "data": null
            }))
        }
    }
}

/// 手动清理日志
pub async fn cleanup_logs() -> impl IntoResponse {
    use crate::util::log::cleanup_old_logs;

    let retention_days = CONFIG.logging.file.retention_days.unwrap_or(7);
    let log_dir = std::path::Path::new(&CONFIG.logging.file.directory);

    match cleanup_old_logs(log_dir, retention_days) {
        Ok(_) => {
            tracing::info!("手动清理日志完成");
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "message": "日志清理完成",
                    "retention_days": retention_days
                }
            }))
        }
        Err(e) => {
            tracing::error!("手动清理日志失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("日志清理失败: {}", e),
                "data": null
            }))
        }
    }
}

/// 检查日志系统健康状态
pub async fn check_log_health() -> impl IntoResponse {
    use crate::util::log::check_log_health;

    let log_dir = std::path::Path::new(&CONFIG.logging.file.directory);
    match check_log_health(log_dir) {
        Ok(health) => Json(serde_json::json!({
            "success": true,
            "errorCode": 200,
            "errorMsg": "",
            "data": health
        })),
        Err(e) => {
            tracing::error!("检查日志健康状态失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("检查日志健康状态失败: {}", e),
                "data": null
            }))
        }
    }
}

/// 获取预审统计数据 - 简化版本
pub async fn get_preview_stats(
    State(app_state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    tracing::info!("获取预审统计数据");

    // 解析查询参数
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(100); // 默认返回最近100条记录

    let offset = params
        .get("offset")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    // 构建查询过滤条件
    let filter = PreviewFilter {
        user_id: None,  // 不过滤用户，显示所有记录
        status: None,   // 不过滤状态
        theme_id: None, // 不过滤主题
        third_party_request_id: None,
        start_date: None, // 不过滤开始时间
        end_date: None,   // 不过滤结束时间
        limit: Some(limit),
        offset: Some(offset),
    };

    // 从数据库获取预审记录
    match app_state.database.list_preview_records(&filter).await {
        Ok(records) => {
            // 构建简化的统计数据：只包含ID、事项名称、时间
            let stats_data: Vec<serde_json::Value> = records
                .iter()
                .map(|record| {
                    // 尝试从评估结果中提取事项名称
                    let matter_name = if let Some(eval_result) = &record.evaluation_result {
                        if let Ok(eval_data) =
                            serde_json::from_str::<serde_json::Value>(eval_result)
                        {
                            eval_data
                                .get("matter_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or(&record.file_name)
                                .to_string()
                        } else {
                            record.file_name.clone()
                        }
                    } else {
                        // 如果没有评估结果，使用文件名作为事项名称
                        record.file_name.clone()
                    };

                    serde_json::json!({
                        "id": record.id,
                        "matter_name": matter_name,
                        "created_at": record.created_at.to_rfc3339(),
                        "status": record.status.to_string(),
                        "user_id": record.user_id
                    })
                })
                .collect();

            tracing::info!(
                target: "monitoring.preview",
                event = "monitoring.preview_stats.success",
                record_count = stats_data.len()
            );

            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "records": stats_data,
                    "total": stats_data.len(),
                    "limit": limit,
                    "offset": offset
                }
            }))
        }
        Err(e) => {
            tracing::error!(
                target: "monitoring.preview",
                event = "monitoring.preview_stats.error",
                error = %e
            );
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取预审统计数据失败",
                "data": null
            }))
        }
    }
}

/// 获取预审统计数据
pub async fn get_preview_statistics(State(app_state): State<AppState>) -> impl IntoResponse {
    tracing::info!(
        target: "monitoring.preview",
        event = "monitoring.preview_stats.fetch"
    );

    // 获取各状态的统计数据
    let statistics = match calculate_preview_statistics(&app_state.database).await {
        Ok(stats) => stats,
        Err(e) => {
            tracing::error!("获取预审统计失败: {}", e);
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取统计数据失败",
                "data": null
            }))
            .into_response();
        }
    };

    let mut resp = Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "refreshed_at": chrono::Utc::now().to_rfc3339(),
        "data": statistics
    }))
    .into_response();
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    resp
}

/// 获取预审记录列表
pub async fn get_preview_records_list(
    State(app_state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    // 解析查询参数
    let page = params
        .get("page")
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(1);
    let size = params
        .get("size")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(20)
        .min(100); // 限制最大页面大小

    tracing::info!(
        target: "monitoring.preview",
        event = "monitoring.preview_records.fetch",
        page,
        size,
        has_filters = !params.is_empty()
    );

    // 构建过滤条件
    let mut filter = PreviewFilter {
        user_id: None,
        status: None,
        theme_id: None,
        third_party_request_id: None,
        start_date: None,
        end_date: None,
        limit: None,
        offset: None,
    };

    // 状态过滤
    if let Some(status_str) = params.get("status") {
        if !status_str.is_empty() {
            filter.status = match status_str.as_str() {
                "pending" => Some(PreviewStatus::Pending),
                "queued" => Some(PreviewStatus::Queued),
                "processing" => Some(PreviewStatus::Processing),
                "completed" => Some(PreviewStatus::Completed),
                "failed" => Some(PreviewStatus::Failed),
                _ => None,
            };
        }
    }

    // 日期过滤
    if let Some(date_from) = params.get("date_from") {
        if !date_from.is_empty() {
            if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_from, "%Y-%m-%d") {
                filter.start_date = Some(dt.and_hms_opt(0, 0, 0).unwrap().and_utc());
            }
        }
    }

    if let Some(date_to) = params.get("date_to") {
        if !date_to.is_empty() {
            if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_to, "%Y-%m-%d") {
                filter.end_date = Some(dt.and_hms_opt(23, 59, 59).unwrap().and_utc());
            }
        }
    }

    // 设置分页参数
    filter.limit = Some(size);
    filter.offset = Some((page - 1) * size);

    // 首先获取总数（不带分页的查询）
    let total_filter = PreviewFilter {
        user_id: filter.user_id.clone(),
        status: filter.status.clone(),
        theme_id: filter.theme_id.clone(),
        third_party_request_id: filter.third_party_request_id.clone(),
        start_date: filter.start_date,
        end_date: filter.end_date,
        limit: None,
        offset: None,
    };

    // 查询数据
    match app_state.database.list_preview_records(&filter).await {
        Ok(records) => {
            let mut request_cache: HashMap<String, PreviewRequestRecord> = HashMap::new();

            for record in &records {
                if let Some(tp_id) = record.third_party_request_id.as_ref() {
                    if !request_cache.contains_key(tp_id) {
                        if let Ok(Some(request)) = app_state
                            .database
                            .find_preview_request_by_third_party(tp_id)
                            .await
                        {
                            request_cache.insert(tp_id.clone(), request);
                        }
                    }
                }
            }

            // 获取总数用于分页计算
            let total = match app_state.database.list_preview_records(&total_filter).await {
                Ok(all_records) => all_records.len() as u32,
                Err(_) => records.len() as u32, // 降级处理
            };

            let total_pages = (total + size - 1) / size;

            // 增强记录信息
            let enhanced_records: Vec<_> = records
                .into_iter()
                .map(|record| {
                    let third_party_id = record.third_party_request_id.clone();
                    let mut value = enhance_preview_record(record);

                    if let Some(tp_id) = third_party_id {
                        if let Some(request) = request_cache.get(&tp_id) {
                            if !request.matter_name.is_empty() {
                                value["matter_name"] =
                                    serde_json::Value::String(request.matter_name.clone());
                            }
                            if !request.matter_id.is_empty() {
                                value["matter_id"] =
                                    serde_json::Value::String(request.matter_id.clone());
                            }
                            value["preview_request_id"] =
                                serde_json::Value::String(request.id.clone());
                            value["channel"] = serde_json::Value::String(request.channel.clone());
                            value["sequence_no"] =
                                serde_json::Value::String(request.sequence_no.clone());
                            if let Some(tp) = request.third_party_request_id.as_ref() {
                                value["third_party_request_id"] =
                                    serde_json::Value::String(tp.clone());
                            }
                        }
                    }

                    value
                })
                .collect();

            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "records": enhanced_records,
                    "pagination": {
                        "current_page": page,
                        "page_size": size,
                        "total_records": total,
                        "total_pages": total_pages
                    }
                }
            }))
        }
        Err(e) => {
            tracing::error!("查询预审记录失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "查询记录失败",
                "data": null
            }))
        }
    }
}

/// 获取预审请求列表（聚合视图）
pub async fn get_preview_requests_list(
    State(app_state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let page = params
        .get("page")
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(1);
    let size = params
        .get("size")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(20)
        .min(100);

    tracing::info!(
        target: "monitoring.preview",
        event = "monitoring.preview_requests.fetch",
        page,
        size,
        has_filters = !params.is_empty()
    );

    let mut filter = PreviewRequestFilter::default();

    if let Some(status_str) = params.get("status").filter(|s| !s.is_empty()) {
        filter.latest_status = match status_str.as_str().to_lowercase().as_str() {
            "pending" => Some(PreviewStatus::Pending),
            "queued" => Some(PreviewStatus::Queued),
            "processing" => Some(PreviewStatus::Processing),
            "completed" => Some(PreviewStatus::Completed),
            "failed" => Some(PreviewStatus::Failed),
            _ => None,
        };
    }

    if let Some(user_id) = params.get("user_id").filter(|s| !s.is_empty()) {
        filter.user_id = Some(user_id.clone());
    }

    if let Some(channel) = params.get("channel").filter(|s| !s.is_empty()) {
        filter.channel = Some(channel.clone());
    }

    if let Some(matter_id) = params.get("matter_id").filter(|s| !s.is_empty()) {
        filter.matter_id = Some(matter_id.clone());
    }

    if let Some(sequence_no) = params.get("sequence_no").filter(|s| !s.is_empty()) {
        filter.sequence_no = Some(sequence_no.clone());
    }

    if let Some(tp_id) = params
        .get("third_party_request_id")
        .filter(|s| !s.is_empty())
    {
        filter.third_party_request_id = Some(tp_id.clone());
    }

    if let Some(search) = params.get("search").filter(|s| !s.is_empty()) {
        filter.search = Some(search.clone());
    }

    if let Some(date_from) = params.get("date_from").filter(|s| !s.is_empty()) {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_from, "%Y-%m-%d") {
            filter.created_from = Some(dt.and_hms_opt(0, 0, 0).unwrap().and_utc());
        }
    }

    if let Some(date_to) = params.get("date_to").filter(|s| !s.is_empty()) {
        if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_to, "%Y-%m-%d") {
            filter.created_to = Some(dt.and_hms_opt(23, 59, 59).unwrap().and_utc());
        }
    }

    if filter.created_from.is_none() && filter.created_to.is_none() {
        let now = Utc::now();
        let weekday = now.date_naive().weekday().num_days_from_monday() as i64;
        let week_start = now
            .date_naive()
            .checked_sub_signed(chrono::Duration::days(weekday))
            .unwrap_or(now.date_naive());
        filter.created_from = Some(week_start.and_hms_opt(0, 0, 0).unwrap().and_utc());
        filter.created_to = Some(now.date_naive().and_hms_opt(23, 59, 59).unwrap().and_utc());
    }

    let mut page_filter = filter.clone();
    page_filter.limit = Some(size);
    page_filter.offset = Some((page - 1) * size);

    let paged_records = match app_state.database.list_preview_requests(&page_filter).await {
        Ok(list) => list,
        Err(err) => {
            tracing::error!("查询预审请求列表失败: {}", err);
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "查询预审请求失败",
                "data": null
            }));
        }
    };

    let mut total_filter = filter;
    total_filter.limit = None;
    total_filter.offset = None;

    let total = match app_state
        .database
        .list_preview_requests(&total_filter)
        .await
    {
        Ok(records) => records.len() as u32,
        Err(err) => {
            tracing::warn!("统计预审请求总数失败: {}", err);
            paged_records.len() as u32 + ((page - 1) * size)
        }
    };

    let total_pages = if total == 0 {
        0
    } else {
        (total + size - 1) / size
    };

    let formatted_records: Vec<_> = paged_records
        .into_iter()
        .map(format_preview_request_summary)
        .collect();

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "records": formatted_records,
            "pagination": {
                "current_page": page,
                "page_size": size,
                "total_records": total,
                "total_pages": total_pages
            }
        }
    }))
}

/// 获取单个预审请求详情（含历史尝试）
pub async fn get_preview_request_detail(
    State(app_state): State<AppState>,
    Path(request_id): Path<String>,
) -> impl IntoResponse {
    let database = &app_state.database;

    let request = match database.get_preview_request(&request_id).await {
        Ok(Some(record)) => record,
        Ok(None) => {
            tracing::warn!(request_id = %request_id, "未找到预审请求记录");
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 404,
                "errorMsg": "请求不存在",
                "data": null
            }));
        }
        Err(err) => {
            tracing::error!(request_id = %request_id, error = %err, "查询预审请求失败");
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "查询请求失败",
                "data": null
            }));
        }
    };

    let attempts = match request.third_party_request_id.as_deref() {
        Some(third_party_id) if !third_party_id.is_empty() => {
            let mut filter = PreviewFilter::default();
            filter.third_party_request_id = Some(third_party_id.to_string());
            match database.list_preview_records(&filter).await {
                Ok(records) => records,
                Err(err) => {
                    tracing::warn!(
                        request_id = %request_id,
                        error = %err,
                        "查询预审尝试列表失败"
                    );
                    Vec::new()
                }
            }
        }
        _ => match request.latest_preview_id.as_deref() {
            Some(preview_id) => match database.get_preview_record(preview_id).await {
                Ok(Some(record)) => vec![record],
                Ok(None) => Vec::new(),
                Err(err) => {
                    tracing::warn!(
                        request_id = %request_id,
                        preview_id = %preview_id,
                        error = %err,
                        "查询最新预审记录失败"
                    );
                    Vec::new()
                }
            },
            None => Vec::new(),
        },
    };

    let mut attempts = attempts;
    attempts.sort_by_key(|record| record.created_at);

    let attempt_values: Vec<_> = attempts
        .into_iter()
        .enumerate()
        .map(|(idx, record)| {
            let mut value = enhance_preview_record(record);
            value["attempt_no"] = serde_json::json!(idx + 1);
            value
        })
        .collect();

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "request": format_preview_request_record(request),
            "attempts": attempt_values
        }
    }))
}

/// 脱敏用户信息，移除敏感字段
fn redact_user_info(value: serde_json::Value) -> Option<serde_json::Value> {
    let mut obj = value.as_object()?.clone();
    for key in [
        "certificateNumber",
        "certificate_number",
        "id_card",
        "idNumber",
    ] {
        if let Some(val) = obj.get_mut(key) {
            if let Some(s) = val.as_str() {
                *val = serde_json::Value::String(mask_str(s, 3, 2));
            }
        }
    }
    for key in ["phone", "mobile", "phone_number"] {
        if let Some(val) = obj.get_mut(key) {
            if let Some(s) = val.as_str() {
                *val = serde_json::Value::String(mask_str(s, 3, 2));
            }
        }
    }
    if let Some(val) = obj.get_mut("name") {
        if let Some(s) = val.as_str() {
            *val = serde_json::Value::String(mask_str(s, 1, 0));
        }
    }
    if let Some(exts) = obj.get_mut("extInfos").and_then(|v| v.as_object_mut()) {
        for key in ["aliuserid", "headpicture", "alipayId"] {
            exts.remove(key);
        }
    }
    Some(serde_json::Value::Object(obj))
}

fn mask_str(input: &str, prefix: usize, suffix: usize) -> String {
    if input.is_empty() {
        return "".to_string();
    }
    let len = input.chars().count();
    if len <= prefix + suffix {
        return "*".repeat(len);
    }
    let mut res = String::new();
    for (idx, ch) in input.chars().enumerate() {
        if idx < prefix || idx >= len.saturating_sub(suffix) {
            res.push(ch);
        } else {
            res.push('*');
        }
    }
    res
}

/// 获取最近失败的预审记录
pub async fn get_recent_failed_previews(
    State(app_state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v.clamp(1, 100))
        .unwrap_or(20);
    let hours = params
        .get("hours")
        .and_then(|v| v.parse::<i64>().ok())
        .map(|v| v.max(1))
        .unwrap_or(24);

    let mut filter = PreviewFilter::default();
    filter.status = Some(PreviewStatus::Failed);
    filter.limit = Some(limit);
    filter.start_date = Some(Utc::now() - chrono::Duration::hours(hours));

    match app_state.database.list_preview_records(&filter).await {
        Ok(records) => {
            let payload: Vec<_> = records.into_iter().map(enhance_preview_record).collect();
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "records": payload
                }
            }))
        }
        Err(err) => {
            tracing::error!(error = %err, "获取最近失败的预审记录失败");
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取失败任务列表失败",
                "data": null
            }))
        }
    }
}

/// 计算预审统计数据
pub async fn calculate_preview_statistics(
    database: &Arc<dyn crate::db::Database>,
) -> anyhow::Result<serde_json::Value> {
    let counts = database.get_preview_status_counts().await?;
    let total = counts.total as usize;
    let completed = counts.completed as usize;
    let processing = counts.processing as usize;
    let failed = counts.failed as usize;
    let pending = counts.pending as usize;
    let queued = counts.queued as usize;

    Ok(serde_json::json!({
        "total": total,
        "completed": completed,
        "processing": processing,
        "failed": failed,
        "pending": pending,
        "queued": queued,
        "success_rate": if total > 0 { (completed as f64 / total as f64 * 100.0).round() } else { 0.0 }
    }))
}

/// 增强预审记录信息（从evaluation_result中提取matter_name和matter_id）
pub fn enhance_preview_record(record: PreviewRecord) -> serde_json::Value {
    let user_info = record
        .user_info_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());

    let mut result = serde_json::json!({
        "id": record.id,
        "user_id": record.user_id,
        "user_info": user_info,
        "file_name": record.file_name,
        "third_party_request_id": record.third_party_request_id,
        "status": format!("{:?}", record.status).to_lowercase(),
        "created_at": record.created_at.to_rfc3339(),
        "updated_at": record.updated_at.to_rfc3339(),
        "preview_url": record.preview_url,
        "preview_download_url": record.preview_download_url,
        "queued_at": record.queued_at.map(|dt| dt.to_rfc3339()),
        "processing_started_at": record
            .processing_started_at
            .map(|dt| dt.to_rfc3339()),
        "retry_count": record.retry_count,
        "last_worker_id": record.last_worker_id,
        "last_attempt_id": record.last_attempt_id,
        "failure_reason": record.failure_reason,
        "last_error_code": record.last_error_code,
        "matter_name": None::<String>,
        "matter_id": None::<String>
    });

    // 尝试从evaluation_result中提取matter信息
    if let Some(eval_result) = &record.evaluation_result {
        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
            if let Some(matter_name) = eval_data.get("matter_name").and_then(|v| v.as_str()) {
                result["matter_name"] = serde_json::Value::String(matter_name.to_string());
            }
            if let Some(matter_id) = eval_data.get("matter_id").and_then(|v| v.as_str()) {
                result["matter_id"] = serde_json::Value::String(matter_id.to_string());
            }
        }
    }

    result
}

fn format_preview_request_record(record: PreviewRequestRecord) -> serde_json::Value {
    let PreviewRequestRecord {
        id,
        third_party_request_id,
        user_id,
        user_info_json,
        matter_id,
        matter_type,
        matter_name,
        channel,
        sequence_no,
        agent_info_json,
        subject_info_json,
        form_data_json,
        scene_data_json,
        material_data_json,
        latest_preview_id,
        latest_status,
        created_at,
        updated_at,
    } = record;

    let agent_info = agent_info_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(redact_user_info);
    let subject_info = subject_info_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(redact_user_info);
    let user_info = user_info_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(redact_user_info);
    let form_data = form_data_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());
    let scene_data = scene_data_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());
    let material_data = material_data_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok());

    serde_json::json!({
        "request_id": id,
        "third_party_request_id": third_party_request_id,
        "user_id": user_id,
        "user_info": user_info,
        "matter_id": matter_id,
        "matter_type": matter_type,
        "matter_name": matter_name,
        "channel": channel,
        "sequence_no": sequence_no,
        "latest_preview_id": latest_preview_id,
        "latest_status": latest_status.map(|s| s.as_str().to_string()),
        "created_at": created_at.to_rfc3339(),
        "updated_at": updated_at.to_rfc3339(),
        "agent_info": agent_info,
        "subject_info": subject_info,
        "form_data": form_data,
        "scene_data": scene_data,
        "material_data": material_data,
    })
}

fn format_preview_request_summary(record: PreviewRequestRecord) -> serde_json::Value {
    let mut user_name: Option<String> = None;
    let mut certificate_number: Option<String> = None;
    let mut phone_number: Option<String> = None;

    if let Some(user_info_json) = record.user_info_json.as_deref() {
        if let Some(user_info) = serde_json::from_str::<serde_json::Value>(user_info_json)
            .ok()
            .and_then(redact_user_info)
        {
            if let Some(name) = user_info.get("user_name").and_then(|v| v.as_str()) {
                user_name = Some(name.to_string());
            }
            if let Some(cert) = user_info.get("certificate_number").and_then(|v| v.as_str()) {
                certificate_number = Some(cert.to_string());
            }
            if let Some(phone) = user_info.get("phone_number").and_then(|v| v.as_str()) {
                phone_number = Some(phone.to_string());
            }
        }
    }

    serde_json::json!({
        "request_id": record.id,
        "preview_id": record.latest_preview_id, // 预审ID
        "third_party_request_id": record.third_party_request_id,
        "matter_name": record.matter_name,
        "matter_id": record.matter_id,
        "matter_type": record.matter_type,
        "channel": record.channel,
        "sequence_no": record.sequence_no,
        "user_id": record.user_id,
        "user_name": user_name,
        "user_certificate_number": certificate_number,
        "user_phone_number": phone_number,
        "user_info": None::<serde_json::Value>, // 摘要中不返回完整用户信息
        "latest_preview_id": record.latest_preview_id,
        "latest_status": record.latest_status.map(|s| s.as_str().to_string()),
        "created_at": record.created_at.to_rfc3339(),
        "updated_at": record.updated_at.to_rfc3339(),
    })
}

/// 获取信号量追踪状态
pub async fn get_permit_tracker_status() -> impl IntoResponse {
    let active_permits = crate::util::permit_tracker::get_active_permits().await;
    let leaked_permits = crate::util::permit_tracker::check_leaked_permits(5).await; // 5分钟阈值

    let (status_code, recommendations) = if !leaked_permits.is_empty() {
        (
            "leak_detected",
            vec![
                format!("发现 {} 个可能的信号量泄漏", leaked_permits.len()),
                "建议检查日志并考虑重启服务".to_string(),
            ],
        )
    } else if active_permits.is_empty() {
        ("idle", vec!["系统空闲，无活跃任务".to_string()])
    } else {
        (
            "normal",
            vec![format!("系统正常，{} 个任务处理中", active_permits.len())],
        )
    };

    let mut metric_labels = HashMap::new();
    metric_labels.insert("status".to_string(), status_code.to_string());
    metric_labels.insert(
        "active_permits".to_string(),
        active_permits.len().to_string(),
    );
    metric_labels.insert(
        "leaked_permits".to_string(),
        leaked_permits.len().to_string(),
    );
    METRICS_COLLECTOR.record_pipeline_stage(
        "permit_tracker",
        leaked_permits.is_empty(),
        Duration::from_millis(0),
        Some(metric_labels),
        None,
    );

    let status = serde_json::json!({
        "success": true,
        "data": {
            "summary": {
                "total_active": active_permits.len(),
                "suspected_leaks": leaked_permits.len(),
                "max_concurrent": crate::CONFIG.concurrency
                    .as_ref()
                    .map(|c| c.ocr_processing.max_concurrent_tasks)
                    .unwrap_or(6),
                "available_permits": crate::OCR_SEMAPHORE.available_permits(),
                "status": status_code,
            },
            "active_permits": active_permits,
            "leaked_permits": leaked_permits,
            "recommendations": recommendations
        }
    });

    Json(status)
}

/// 获取系统队列状态 - 并发控制监控
/// 提供OCR处理队列的实时状态信息
pub async fn get_queue_status(State(app_state): State<AppState>) -> impl IntoResponse {
    tracing::info!(
        target: "monitoring.queue",
        event = "monitoring.queue.fetch"
    );

    // 获取当前信号量状态
    let available_permits = crate::OCR_SEMAPHORE.available_permits();
    // 从配置获取最大并发数，与实际信号量保持一致
    let max_concurrent = crate::CONFIG
        .concurrency
        .as_ref()
        .map(|c| c.ocr_processing.max_concurrent_tasks as usize)
        .unwrap_or(6); // 默认6个，与main.rs保持一致
    let processing_tasks = max_concurrent.saturating_sub(available_permits);

    // 计算系统负载百分比
    let system_load_percent = if max_concurrent > 0 {
        (processing_tasks as f64 / max_concurrent as f64 * 100.0).round()
    } else {
        0.0
    };

    let queue_status = if system_load_percent >= 95.0 {
        "exhausted"
    } else if system_load_percent >= 80.0 {
        "busy"
    } else if processing_tasks == 0 {
        "idle"
    } else {
        "normal"
    };
    let recommended_action = match queue_status {
        "exhausted" => "建议等待当前任务完成后再提交",
        "busy" => "建议稍后提交任务",
        "idle" => "系统运行正常",
        _ => "系统运行正常",
    };

    let mut metric_labels = HashMap::new();
    metric_labels.insert("status".to_string(), queue_status.to_string());
    metric_labels.insert("processing_tasks".to_string(), processing_tasks.to_string());
    metric_labels.insert("max_concurrent".to_string(), max_concurrent.to_string());
    METRICS_COLLECTOR.record_pipeline_stage(
        "queue_status",
        queue_status != "exhausted",
        Duration::from_millis(0),
        Some(metric_labels),
        None,
    );

    let mut data = Map::new();
    // 入口并发（提交/下载） + OCR池并发
    let (submit_max, submit_available) = {
        let cfg_max = app_state
            .config
            .concurrency
            .as_ref()
            .map(|c| c.queue_monitoring.max_queue_length.max(1) as usize)
            .unwrap_or(50);
        let available = app_state.submission_semaphore.available_permits();
        (cfg_max, available)
    };
    let (download_max, download_available) = {
        let cfg_max = app_state
            .config
            .concurrency
            .as_ref()
            .map(|c| c.queue_monitoring.max_queue_length.max(1) as usize)
            .unwrap_or(50);
        let available = app_state.download_semaphore.available_permits();
        (cfg_max, available)
    };
    let ocr_used = max_concurrent.saturating_sub(available_permits);
    data.insert(
        "semaphores".to_string(),
        json!({
            "submission": {
                "max": submit_max,
                "used": submit_max.saturating_sub(submit_available),
                "available": submit_available,
                "usage_percent": if submit_max > 0 { ((submit_max.saturating_sub(submit_available)) as f64 / submit_max as f64 * 100.0).round() } else { 0.0 }
            },
            "download": {
                "max": download_max,
                "used": download_max.saturating_sub(download_available),
                "available": download_available,
                "usage_percent": if download_max > 0 { ((download_max.saturating_sub(download_available)) as f64 / download_max as f64 * 100.0).round() } else { 0.0 }
            },
            "ocr_pool": {
                "max": max_concurrent,
                "used": ocr_used,
                "available": available_permits,
                "usage_percent": system_load_percent
            }
        }),
    );

    data.insert(
        "queue".to_string(),
        json!({
            "max_concurrent_tasks": max_concurrent,
            "available_slots": available_permits,
            "processing_tasks": processing_tasks,
            "system_load_percent": system_load_percent,
            "status": queue_status
        }),
    );
    data.insert(
        "system_info".to_string(),
        json!({
            "cpu_cores": num_cpus::get(),  // 动态 CPU 核心数
            "memory_gb": (crate::util::system_info::get_memory_usage().total_mb as f64 / 1024.0).round(),  // 物理内存
            "concurrency_strategy": "信号量控制"
        }),
    );
    data.insert(
        "performance".to_string(),
        json!({
            "current_capacity": format!("{}/{} 处理槽位", processing_tasks, max_concurrent),
            "status": match queue_status {
                "idle" | "normal" => "正常",
                "busy" => "繁忙",
                "exhausted" => "过载",
                _ => "正常"
            },
            "recommended_action": recommended_action
        }),
    );

    // 如配置启用 NATS，追加 JetStream 队列指标
    if let Some(nats_config) = app_state.config.task_queue.nats.as_ref() {
        let mut jetstream_detail = Map::new();
        jetstream_detail.insert("stream".to_string(), json!(nats_config.stream));
        jetstream_detail.insert("subject".to_string(), json!(nats_config.subject));
        jetstream_detail.insert("retention_policy".to_string(), json!("work_queue"));
        jetstream_detail.insert(
            "max_messages".to_string(),
            json!(nats_config.max_messages.unwrap_or(-1)),
        );
        jetstream_detail.insert(
            "max_bytes".to_string(),
            json!(nats_config.max_bytes.unwrap_or(-1)),
        );
        jetstream_detail.insert(
            "max_age_seconds".to_string(),
            match nats_config.max_age_seconds {
                Some(secs) => json!(secs),
                None => Value::Null,
            },
        );
        jetstream_detail.insert("backlog_messages".to_string(), Value::Null);
        jetstream_detail.insert("backlog_bytes".to_string(), Value::Null);
        jetstream_detail.insert("connection_reconnecting".to_string(), Value::Null);
        jetstream_detail.insert("connection_healthy".to_string(), Value::Null);

        if let Some(nats_queue) = app_state
            .task_queue
            .as_any()
            .downcast_ref::<NatsTaskQueue>()
        {
            match nats_queue.stream_metrics().await {
                Ok(metrics) => {
                    jetstream_detail.insert(
                        "backlog_messages".to_string(),
                        json!(metrics.backlog_messages),
                    );
                    jetstream_detail
                        .insert("backlog_bytes".to_string(), json!(metrics.backlog_bytes));
                    jetstream_detail
                        .insert("max_messages".to_string(), json!(metrics.max_messages));
                    jetstream_detail.insert("max_bytes".to_string(), json!(metrics.max_bytes));
                    jetstream_detail.insert(
                        "max_age_seconds".to_string(),
                        match metrics.max_age_seconds {
                            Some(secs) => json!(secs),
                            None => Value::Null,
                        },
                    );
                }
                Err(err) => {
                    tracing::warn!("获取 JetStream 流信息失败: {:#}", err);
                }
            }

            jetstream_detail.insert(
                "connection_reconnecting".to_string(),
                json!(nats_queue.is_reconnecting()),
            );
            jetstream_detail.insert(
                "connection_healthy".to_string(),
                json!(nats_queue.is_healthy()),
            );
        }

        data.insert("jetstream".to_string(), Value::Object(jetstream_detail));
    }

    // Worker 心跳信息
    let heartbeat_snapshot = worker_proxy::collect_worker_heartbeat_snapshot().await;
    let active_worker_ids: HashSet<String> = heartbeat_snapshot
        .iter()
        .map(|snapshot| snapshot.worker_id.clone())
        .collect();
    let mut worker_statuses: Vec<Value> = heartbeat_snapshot
        .into_iter()
        .map(|snapshot| {
            json!({
                "worker_id": snapshot.worker_id,
                "last_seen": snapshot.last_seen,
                "seconds_since": snapshot.seconds_since,
                "interval_secs": snapshot.interval_secs,
                "queue_depth": snapshot.queue_depth,
                "running_tasks": snapshot.running_tasks,
                "metrics": snapshot.metrics,
                "timed_out": snapshot.timed_out,
                "status": if snapshot.timed_out { "timeout" } else { "ok" },
            })
        })
        .collect();

    for worker_cfg in app_state
        .config
        .worker_proxy
        .workers
        .iter()
        .filter(|w| w.enabled)
    {
        if !active_worker_ids.contains(&worker_cfg.worker_id) {
            worker_statuses.push(json!({
                "worker_id": worker_cfg.worker_id,
                "status": "missing",
            }));
        }
    }

    data.insert(
        "worker_heartbeats".to_string(),
        Value::Array(worker_statuses),
    );

    // OCR 流水线阶段指标
    let pipeline_metrics = METRICS_COLLECTOR.get_pipeline_metrics();
    let mut pipeline_entries: Vec<(String, _)> = pipeline_metrics.into_iter().collect();
    pipeline_entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut pipeline_values: Vec<Value> = Vec::new();
    for (stage, stats) in pipeline_entries {
        let total_count = stats.success_count + stats.failure_count;
        let avg_duration = if total_count > 0 {
            stats.total_duration_ms / total_count as f64
        } else {
            0.0
        };
        pipeline_values.push(json!({
            "stage": stage,
            "success": stats.success_count,
            "failure": stats.failure_count,
            "avg_duration_ms": avg_duration,
            "max_duration_ms": stats.max_duration_ms,
            "last_success_ts": stats.last_success_ts,
            "last_failure_ts": stats.last_failure_ts,
            "last_error": stats.last_error,
        }));
    }
    data.insert(
        "pipeline_metrics".to_string(),
        Value::Array(pipeline_values),
    );

    let queue_status = json!({
        "success": true,
        "refreshed_at": chrono::Utc::now().to_rfc3339(),
        "data": serde_json::Value::Object(data),
    });

    tracing::info!(
        "队列状态: 处理中任务={}, 可用槽位={}, 负载={}%",
        processing_tasks,
        available_permits,
        system_load_percent
    );

    let mut resp = Json(queue_status).into_response();
    resp.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    resp
}

#[derive(Debug, Deserialize)]
pub struct AttachmentLoggingUpdateRequest {
    pub enabled: Option<bool>,
    pub sampling_rate: Option<u32>,
    pub slow_threshold_ms: Option<u64>,
}

/// 动态调整附件日志采样/阈值
pub async fn update_attachment_logging_settings(
    Json(payload): Json<AttachmentLoggingUpdateRequest>,
) -> impl IntoResponse {
    let runtime = &*ATTACHMENT_LOGGING_RUNTIME;
    let current = runtime.snapshot();

    let new_enabled = payload.enabled.unwrap_or(current.enabled);
    let new_sampling = payload.sampling_rate.unwrap_or(current.sampling_rate);
    let new_slow = payload
        .slow_threshold_ms
        .unwrap_or(current.slow_threshold_ms);

    runtime.update(new_enabled, new_sampling, new_slow);

    tracing::info!(
        target: "monitoring.preview",
        event = "monitoring.attachment_logging.update",
        enabled = new_enabled,
        sampling_rate = new_sampling,
        slow_threshold_ms = new_slow
    );

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "enabled": new_enabled,
            "sampling_rate": new_sampling,
            "slow_threshold_ms": new_slow
        }
    }))
}
