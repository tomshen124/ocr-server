//! 动态Worker状态查询API

use axum::{extract::State, Json};
use serde::Serialize;
use tracing::warn;

use crate::util::dynamic_worker::{
    get_dynamic_worker_manager, DynamicWorkerConfig, DynamicWorkerStatusSnapshot,
};
use crate::util::{IntoJson, WebResult};
use crate::AppState;

#[derive(Serialize)]
struct DynamicWorkerStatusResponse {
    enabled: bool,
    is_running: bool,
    queue_depth: u64,
    master_cpu_percent: f64,
    master_memory_percent: f64,
    master_memory_used_mb: u64,
    master_memory_total_mb: u64,
    uptime_seconds: Option<u64>,
    cooldown_remaining_seconds: Option<u64>,
    config: DynamicWorkerConfigInfo,
}

#[derive(Serialize)]
struct DynamicWorkerConfigInfo {
    enable_threshold: u64,
    disable_threshold: u64,
    check_interval_secs: u64,
    sustained_seconds: u64,
    max_concurrent_tasks: usize,
    cpu_threshold_percent: f64,
    memory_threshold_percent: f64,
    cooldown_seconds: u64,
}

impl From<DynamicWorkerConfig> for DynamicWorkerConfigInfo {
    fn from(value: DynamicWorkerConfig) -> Self {
        Self {
            enable_threshold: value.enable_threshold,
            disable_threshold: value.disable_threshold,
            check_interval_secs: value.check_interval_secs,
            sustained_seconds: value.sustained_seconds,
            max_concurrent_tasks: value.max_concurrent_tasks,
            cpu_threshold_percent: value.cpu_threshold_percent,
            memory_threshold_percent: value.memory_threshold_percent,
            cooldown_seconds: value.cooldown_seconds,
        }
    }
}

impl From<DynamicWorkerStatusSnapshot> for DynamicWorkerStatusResponse {
    fn from(snapshot: DynamicWorkerStatusSnapshot) -> Self {
        Self {
            enabled: snapshot.enabled,
            is_running: snapshot.is_running,
            queue_depth: snapshot.queue_depth,
            master_cpu_percent: snapshot.resource_stats.cpu_percent,
            master_memory_percent: snapshot.resource_stats.memory_percent,
            master_memory_used_mb: snapshot.resource_stats.memory_used_mb,
            master_memory_total_mb: snapshot.resource_stats.memory_total_mb,
            uptime_seconds: snapshot.uptime_seconds,
            cooldown_remaining_seconds: snapshot.cooldown_remaining_seconds,
            config: snapshot.config.into(),
        }
    }
}

pub async fn get_dynamic_worker_status(State(app_state): State<AppState>) -> Json<WebResult> {
    if let Some(manager) = get_dynamic_worker_manager() {
        match manager.current_status().await {
            Ok(snapshot) => WebResult::ok(DynamicWorkerStatusResponse::from(snapshot)).into_json(),
            Err(err) => {
                warn!("查询动态Worker状态失败: {:#}", err);
                WebResult::err_custom("动态Worker状态查询失败").into_json()
            }
        }
    } else {
        let config = app_state
            .config
            .dynamic_worker
            .clone()
            .unwrap_or_else(DynamicWorkerConfig::default);
        let response = DynamicWorkerStatusResponse {
            enabled: config.enabled,
            is_running: false,
            queue_depth: 0,
            master_cpu_percent: 0.0,
            master_memory_percent: 0.0,
            master_memory_used_mb: 0,
            master_memory_total_mb: 0,
            uptime_seconds: None,
            cooldown_remaining_seconds: None,
            config: config.into(),
        };
        WebResult::ok(response).into_json()
    }
}
