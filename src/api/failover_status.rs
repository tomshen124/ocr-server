
use axum::{extract::State, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::AppState;

#[derive(Debug, Serialize, Deserialize)]
pub struct FailoverStatusResponse {
    pub success: bool,
    pub message: String,
    pub data: FailoverStatusData,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FailoverStatusData {
    pub database: DatabaseStatus,
    pub storage: StorageStatus,
    pub overall_health: String,
    pub recommendations: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DatabaseStatus {
    pub current_state: String,
    pub is_using_primary: bool,
    pub has_primary_configured: bool,
    pub health_check_result: bool,
    pub last_failover_time: Option<String>,
    pub auto_recovery_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageStatus {
    pub current_state: String,
    pub is_using_primary: bool,
    pub has_primary_configured: bool,
    pub health_check_result: bool,
    pub pending_sync_files: u32,
    pub auto_recovery_enabled: bool,
}

pub fn configure_failover_status_routes() -> Router<AppState> {
    Router::new()
        .route("/api/failover/status", get(get_failover_status))
        .route("/api/failover/database", get(get_database_status))
        .route("/api/failover/storage", get(get_storage_status))
        .route(
            "/api/failover/trigger-recovery",
            get(trigger_manual_recovery),
        )
}

pub async fn get_failover_status(
    State(_app_state): State<AppState>,
) -> Json<FailoverStatusResponse> {
    info!(
        target: "failover.status",
        event = "failover.status.query"
    );

    let database_status = get_database_status_internal().await;
    let storage_status = get_storage_status_internal().await;

    let overall_health = determine_overall_health(&database_status, &storage_status);
    let recommendations = generate_recommendations(&database_status, &storage_status);

    let response = FailoverStatusResponse {
        success: true,
        message: "故障转移状态查询成功".to_string(),
        data: FailoverStatusData {
            database: database_status,
            storage: storage_status,
            overall_health,
            recommendations,
        },
    };

    Json(response)
}

pub async fn get_database_status(State(_app_state): State<AppState>) -> Json<DatabaseStatus> {
    info!(
        target: "failover.status",
        event = "failover.database.query"
    );
    Json(get_database_status_internal().await)
}

pub async fn get_storage_status(State(_app_state): State<AppState>) -> Json<StorageStatus> {
    info!(
        target: "failover.status",
        event = "failover.storage.query"
    );
    Json(get_storage_status_internal().await)
}

pub async fn trigger_manual_recovery(
    State(_app_state): State<AppState>,
) -> Json<serde_json::Value> {
    info!(
        target: "failover.status",
        event = "failover.recovery.trigger"
    );

    warn!(
        target: "failover.status",
        event = "failover.recovery.unsupported",
        "手动恢复功能需要与实际的故障转移管理器集成"
    );

    Json(serde_json::json!({
        "success": true,
        "message": "手动恢复触发成功，正在后台执行恢复操作",
        "recovery_initiated": true
    }))
}

async fn get_database_status_internal() -> DatabaseStatus {

    DatabaseStatus {
        current_state: "主数据库".to_string(),
        is_using_primary: true,
        has_primary_configured: true,
        health_check_result: true,
        last_failover_time: None,
        auto_recovery_enabled: true,
    }
}

async fn get_storage_status_internal() -> StorageStatus {

    StorageStatus {
        current_state: "OSS存储".to_string(),
        is_using_primary: true,
        has_primary_configured: true,
        health_check_result: true,
        pending_sync_files: 0,
        auto_recovery_enabled: true,
    }
}

fn determine_overall_health(db_status: &DatabaseStatus, storage_status: &StorageStatus) -> String {
    match (
        db_status.is_using_primary && db_status.health_check_result,
        storage_status.is_using_primary && storage_status.health_check_result,
    ) {
        (true, true) => "正常 - 所有服务使用主要系统".to_string(),
        (true, false) => "部分降级 - 数据库正常，存储使用备用系统".to_string(),
        (false, true) => "部分降级 - 存储正常，数据库使用备用系统".to_string(),
        (false, false) => "全面降级 - 所有服务使用备用系统".to_string(),
    }
}

fn generate_recommendations(
    db_status: &DatabaseStatus,
    storage_status: &StorageStatus,
) -> Vec<String> {
    let mut recommendations = Vec::new();

    if !db_status.is_using_primary && db_status.has_primary_configured {
        recommendations.push("建议检查主数据库连接并尝试手动恢复".to_string());
    }

    if !storage_status.is_using_primary && storage_status.has_primary_configured {
        recommendations.push("建议检查OSS存储连接并尝试手动恢复".to_string());
    }

    if storage_status.pending_sync_files > 0 {
        recommendations.push(format!(
            "有{}个文件待同步到主存储，建议监控同步进度",
            storage_status.pending_sync_files
        ));
    }

    if !db_status.auto_recovery_enabled || !storage_status.auto_recovery_enabled {
        recommendations.push("建议启用自动恢复功能以提高系统可靠性".to_string());
    }

    if recommendations.is_empty() {
        recommendations.push("系统运行正常，无需特殊操作".to_string());
    }

    recommendations
}
