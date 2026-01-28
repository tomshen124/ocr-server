#[cfg(feature = "monitoring")]
use axum::{extract::State, response::Json, routing::get, Router};
use serde_json::json;
use std::sync::Arc;

use super::health::HealthCheckResult;
use super::metrics::{MonitoringStats, SystemMetrics};
use super::service::MonitorService;

/// 监控API路由
pub fn monitoring_routes() -> Router<Arc<MonitorService>> {
    Router::new()
        .route("/api/monitor-service/status", get(get_monitoring_status))
        .route("/api/monitor-service/metrics", get(get_current_metrics))
        .route("/api/monitor-service/history", get(get_metrics_history))
        .route("/api/monitor-service/health", get(get_health_status))
        .route("/api/monitor-service/ocr-status", get(get_ocr_status))
        .route("/api/monitor-service/system-info", get(get_system_info))
}

/// 获取监控状态概览
async fn get_monitoring_status(
    State(monitor): State<Arc<MonitorService>>,
) -> Json<serde_json::Value> {
    let current_metrics = monitor.get_current_metrics().await;
    let ocr_healthy = monitor.get_ocr_status().await.unwrap_or(false);

    Json(json!({
        "success": true,
        "data": {
            "timestamp": chrono::Utc::now(),
            "system": {
                "cpu_usage": current_metrics.cpu_usage,
                "memory_usage": current_metrics.memory_usage,
                "disk_usage": current_metrics.disk_usage,
                "process_count": current_metrics.process_count
            },
            "ocr_service": {
                "healthy": ocr_healthy,
                "status": if ocr_healthy { "running" } else { "stopped" }
            },
            "monitoring": {
                "enabled": true,
                "uptime": crate::util::system_info::get_uptime_seconds()
            }
        }
    }))
}

/// 获取当前系统指标
async fn get_current_metrics(
    State(monitor): State<Arc<MonitorService>>,
) -> Json<serde_json::Value> {
    let metrics = monitor.get_current_metrics().await;

    Json(json!({
        "success": true,
        "data": metrics
    }))
}

/// 获取历史指标数据
async fn get_metrics_history(
    State(monitor): State<Arc<MonitorService>>,
) -> Json<serde_json::Value> {
    let history = monitor.get_metrics_history().await;

    // 只返回最近24小时的数据
    let recent_history: Vec<&SystemMetrics> = history
        .iter()
        .rev()
        .take(1440) // 24小时 * 60分钟
        .collect();

    Json(json!({
        "success": true,
        "data": {
            "metrics": recent_history,
            "count": recent_history.len(),
            "period": "24h"
        }
    }))
}

/// 获取健康状态
async fn get_health_status(State(monitor): State<Arc<MonitorService>>) -> Json<serde_json::Value> {
    let mut result = HealthCheckResult::new();

    // 检查OCR服务状态
    match monitor.get_ocr_status().await {
        Ok(healthy) => {
            result.overall_healthy = healthy;
            result.api_responsive = healthy;
        }
        Err(e) => {
            result.error_message = Some(e.to_string());
        }
    }

    Json(json!({
        "success": true,
        "data": result
    }))
}

/// 获取OCR服务状态
async fn get_ocr_status(State(monitor): State<Arc<MonitorService>>) -> Json<serde_json::Value> {
    let health_checker = super::health::HealthChecker::new();

    let mut status = json!({
        "running": false,
        "port_listening": false,
        "api_responsive": false,
        "response_time_ms": null,
        "process_info": null,
        "last_check": chrono::Utc::now()
    });

    // 检查端口监听
    if let Ok(port_ok) = health_checker.check_port_listening().await {
        status["port_listening"] = json!(port_ok);
    }

    // 检查API响应
    if let Ok(api_ok) = health_checker.check_api_health().await {
        status["api_responsive"] = json!(api_ok);
    }

    // 测量响应时间
    if let Ok(response_time) = health_checker.measure_api_response_time().await {
        status["response_time_ms"] = json!(response_time);
    }

    // 获取进程信息
    if let Ok(Some(process_info)) = health_checker.get_process_info().await {
        status["process_info"] = json!(process_info);
        status["running"] = json!(true);
    }

    Json(json!({
        "success": true,
        "data": status
    }))
}

/// 获取系统信息
async fn get_system_info() -> Json<serde_json::Value> {
    use sysinfo::System;

    let mut system = System::new_all();
    system.refresh_all();

    let load = System::load_average();
    let system_info = json!({
        "hostname": System::host_name().unwrap_or_else(|| "unknown".to_string()),
        "os": format!("{} {}", System::name().unwrap_or_else(|| "unknown".to_string()),
                     System::os_version().unwrap_or_else(|| "unknown".to_string())),
        "kernel": System::kernel_version().unwrap_or_else(|| "unknown".to_string()),
        "cpu_count": system.cpus().len(),
        "total_memory_gb": system.total_memory() / 1024 / 1024 / 1024,
        "uptime": System::uptime(),
        "boot_time": System::boot_time(),
        "load_average": {
            "one": load.one,
            "five": load.five,
            "fifteen": load.fifteen
        }
    });

    Json(json!({
        "success": true,
        "data": system_info
    }))
}

/// 监控仪表盘数据
pub async fn get_dashboard_data(
    State(monitor): State<Arc<MonitorService>>,
) -> Json<serde_json::Value> {
    let current_metrics = monitor.get_current_metrics().await;
    let history = monitor.get_metrics_history().await;
    let ocr_healthy = monitor.get_ocr_status().await.unwrap_or(false);

    // 计算趋势数据
    let cpu_trend: Vec<f32> = history.iter().rev().take(60).map(|m| m.cpu_usage).collect();
    let memory_trend: Vec<f32> = history
        .iter()
        .rev()
        .take(60)
        .map(|m| m.memory_usage)
        .collect();

    Json(json!({
        "success": true,
        "data": {
            "current": {
                "cpu": current_metrics.cpu_usage,
                "memory": current_metrics.memory_usage,
                "disk": current_metrics.disk_usage,
                "processes": current_metrics.process_count
            },
            "trends": {
                "cpu": cpu_trend,
                "memory": memory_trend,
                "labels": (0..cpu_trend.len()).map(|i| format!("{}m", i)).collect::<Vec<_>>()
            },
            "services": {
                "ocr": {
                    "status": if ocr_healthy { "healthy" } else { "unhealthy" },
                    "uptime": crate::util::system_info::get_uptime_seconds()
                }
            },
            "alerts": [],
            "last_update": chrono::Utc::now()
        }
    }))
}
