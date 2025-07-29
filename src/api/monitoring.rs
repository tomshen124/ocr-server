//! 监控和统计模块
//! 处理系统状态监控、预审统计、队列状态等功能

use crate::{CONFIG, AppState};
use crate::model::{ComponentStatus, ComponentsHealth, DetailedHealthStatus, HealthStatus, QueueStatus, ErrorInfo};
use crate::util::system_info;
use crate::db::{PreviewFilter, PreviewStatus, PreviewRecord};
use chrono::Utc;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::{Json};
use std::sync::Arc;
use std::collections::HashMap;

/// 基本健康检查
pub async fn basic_health_check() -> impl IntoResponse {
    let status = HealthStatus {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime: system_info::get_uptime_seconds(),
        timestamp: Utc::now().to_rfc3339(),
    };

    Json(status)
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
        "version": env!("CARGO_PKG_VERSION")
    });

    Json(system_metrics)
}

/// 详细健康检查
pub async fn detailed_health_check() -> impl IntoResponse {
    // 使用 tokio::time::timeout 添加超时
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        collect_detailed_health_info()
    ).await {
        Ok(status) => Json(status),
        Err(_) => {
            let error_status = DetailedHealthStatus {
                status: "error".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
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
        version: env!("CARGO_PKG_VERSION").to_string(),
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
            status: if db_connection { "healthy" } else { "unhealthy" }.to_string(),
            details: Some(if db_connection { "Connection successful" } else { "Connection failed" }.to_string()),
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
        Ok(stats) => {
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": stats
            }))
        }
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
        Ok(health) => {
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": health
            }))
        }
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
    Query(params): Query<HashMap<String, String>>
) -> impl IntoResponse {
    tracing::info!("获取预审统计数据");
    
    // 解析查询参数
    let limit = params.get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(100); // 默认返回最近100条记录
    
    let offset = params.get("offset")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    
    // 构建查询过滤条件
    let filter = PreviewFilter {
        user_id: None, // 不过滤用户，显示所有记录
        status: None,  // 不过滤状态
        theme_id: None, // 不过滤主题
        start_date: None, // 不过滤开始时间
        end_date: None,   // 不过滤结束时间
        limit: Some(limit),
        offset: Some(offset),
    };
    
    // 从数据库获取预审记录
    match app_state.database.list_preview_records(&filter).await {
        Ok(records) => {
            // 构建简化的统计数据：只包含ID、事项名称、时间
            let stats_data: Vec<serde_json::Value> = records.iter().map(|record| {
                // 尝试从评估结果中提取事项名称
                let matter_name = if let Some(eval_result) = &record.evaluation_result {
                    if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                        eval_data.get("matter_name")
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
            }).collect();
            
            tracing::info!("✅ 成功获取 {} 条预审统计记录", stats_data.len());
            
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
            tracing::error!("❌ 获取预审统计数据失败: {}", e);
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
pub async fn get_preview_statistics(
    State(app_state): State<AppState>
) -> impl IntoResponse {
    tracing::info!("=== 获取预审统计数据 ===");
    
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
            }));
        }
    };
    
    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": statistics
    }))
}

/// 获取预审记录列表
pub async fn get_preview_records_list(
    State(app_state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>
) -> impl IntoResponse {
    tracing::info!("=== 获取预审记录列表 ===");
    tracing::info!("查询参数: {:?}", params);
    
    // 解析查询参数
    let page = params.get("page")
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(1);
    let size = params.get("size")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(20)
        .min(100); // 限制最大页面大小
    
    // 构建过滤条件
    let mut filter = PreviewFilter {
        user_id: None,
        status: None,
        theme_id: None,
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
        start_date: filter.start_date,
        end_date: filter.end_date,
        limit: None,
        offset: None,
    };
    
    // 查询数据
    match app_state.database.list_preview_records(&filter).await {
        Ok(records) => {
            // 获取总数用于分页计算
            let total = match app_state.database.list_preview_records(&total_filter).await {
                Ok(all_records) => all_records.len() as u32,
                Err(_) => records.len() as u32, // 降级处理
            };
            
            let total_pages = (total + size - 1) / size;
            
            // 增强记录信息
            let enhanced_records: Vec<_> = records
                .into_iter()
                .map(|record| enhance_preview_record(record))
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

/// 计算预审统计数据
pub async fn calculate_preview_statistics(database: &Arc<dyn crate::db::Database>) -> anyhow::Result<serde_json::Value> {
    // 查询所有记录
    let all_records = database.list_preview_records(&PreviewFilter {
        user_id: None,
        status: None,
        theme_id: None,
        start_date: None,
        end_date: None,
        limit: None,
        offset: None,
    }).await?;
    
    let total = all_records.len();
    let completed = all_records.iter().filter(|r| r.status == PreviewStatus::Completed).count();
    let processing = all_records.iter().filter(|r| r.status == PreviewStatus::Processing).count();
    let failed = all_records.iter().filter(|r| r.status == PreviewStatus::Failed).count();
    let pending = all_records.iter().filter(|r| r.status == PreviewStatus::Pending).count();
    
    Ok(serde_json::json!({
        "total": total,
        "completed": completed,
        "processing": processing,
        "failed": failed,
        "pending": pending,
        "success_rate": if total > 0 { (completed as f64 / total as f64 * 100.0).round() } else { 0.0 }
    }))
}

/// 增强预审记录信息（从evaluation_result中提取matter_name和matter_id）
pub fn enhance_preview_record(record: PreviewRecord) -> serde_json::Value {
    let mut result = serde_json::json!({
        "id": record.id,
        "user_id": record.user_id,
        "file_name": record.file_name,
        "third_party_request_id": record.third_party_request_id,
        "status": format!("{:?}", record.status).to_lowercase(),
        "created_at": record.created_at.to_rfc3339(),
        "updated_at": record.updated_at.to_rfc3339(),
        "preview_url": record.preview_url,
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

/// 获取系统队列状态 - 并发控制监控
/// 提供OCR处理队列的实时状态信息
pub async fn get_queue_status() -> impl IntoResponse {
    tracing::info!("=== 获取系统队列状态 ===");
    
    // 获取当前信号量状态
    let available_permits = crate::OCR_SEMAPHORE.available_permits();
    let max_concurrent = 12; // 与main.rs中的设置保持一致
    let processing_tasks = max_concurrent - available_permits;
    
    // 计算系统负载百分比
    let system_load_percent = if max_concurrent > 0 {
        (processing_tasks as f64 / max_concurrent as f64 * 100.0).round()
    } else {
        0.0
    };
    
    let queue_status = serde_json::json!({
        "success": true,
        "data": {
            "queue": {
                "max_concurrent_tasks": max_concurrent,
                "available_slots": available_permits,
                "processing_tasks": processing_tasks,
                "system_load_percent": system_load_percent
            },
            "system_info": {
                "cpu_cores": 32,
                "memory_gb": 64,
                "optimized_for": "32核64G服务器",
                "concurrency_strategy": "信号量控制"
            },
            "performance": {
                "current_capacity": format!("{}/{} 处理槽位", processing_tasks, max_concurrent),
                "status": if system_load_percent < 70.0 { "正常" } 
                         else if system_load_percent < 90.0 { "繁忙" } 
                         else { "过载" },
                "recommended_action": if system_load_percent < 70.0 { "系统运行正常" }
                                    else if system_load_percent < 90.0 { "建议稍后提交任务" }
                                    else { "建议等待当前任务完成后再提交" }
            }
        }
    });
    
    tracing::info!("队列状态: 处理中任务={}, 可用槽位={}, 负载={}%", 
                  processing_tasks, available_permits, system_load_percent);
    
    Json(queue_status)
}