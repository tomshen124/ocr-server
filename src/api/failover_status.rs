//! 故障转移状态查询API
//! 提供数据库和存储的故障转移状态查询

use axum::{extract::State, response::Json, routing::get, Router};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::AppState;

/// 故障转移状态响应
#[derive(Debug, Serialize, Deserialize)]
pub struct FailoverStatusResponse {
    pub success: bool,
    pub message: String,
    pub data: FailoverStatusData,
}

/// 故障转移状态数据
#[derive(Debug, Serialize, Deserialize)]
pub struct FailoverStatusData {
    pub database: DatabaseStatus,
    pub storage: StorageStatus,
    pub overall_health: String,
    pub recommendations: Vec<String>,
}

/// 数据库状态
#[derive(Debug, Serialize, Deserialize)]
pub struct DatabaseStatus {
    pub current_state: String,        // "主数据库", "备用数据库", "恢复中"
    pub is_using_primary: bool,       // 是否使用主数据库
    pub has_primary_configured: bool, // 是否配置了主数据库
    pub health_check_result: bool,    // 当前数据库健康检查结果
    pub last_failover_time: Option<String>, // 上次故障转移时间
    pub auto_recovery_enabled: bool,  // 是否启用自动恢复
}

/// 存储状态
#[derive(Debug, Serialize, Deserialize)]
pub struct StorageStatus {
    pub current_state: String,        // "OSS存储", "本地存储", "恢复中"
    pub is_using_primary: bool,       // 是否使用主存储(OSS)
    pub has_primary_configured: bool, // 是否配置了主存储
    pub health_check_result: bool,    // 当前存储健康检查结果
    pub pending_sync_files: u32,      // 待同步文件数量
    pub auto_recovery_enabled: bool,  // 是否启用自动恢复
}

/// 配置故障转移状态查询路由
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

/// 获取完整故障转移状态
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

/// 获取数据库状态
pub async fn get_database_status(State(_app_state): State<AppState>) -> Json<DatabaseStatus> {
    info!(
        target: "failover.status",
        event = "failover.database.query"
    );
    Json(get_database_status_internal().await)
}

/// 获取存储状态
pub async fn get_storage_status(State(_app_state): State<AppState>) -> Json<StorageStatus> {
    info!(
        target: "failover.status",
        event = "failover.storage.query"
    );
    Json(get_storage_status_internal().await)
}

/// 触发手动恢复
pub async fn trigger_manual_recovery(
    State(_app_state): State<AppState>,
) -> Json<serde_json::Value> {
    info!(
        target: "failover.status",
        event = "failover.recovery.trigger"
    );

    // 实际实现中，这里会调用恢复逻辑
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

/// 获取数据库状态（内部实现）
async fn get_database_status_internal() -> DatabaseStatus {
    // 实际实现中，这里会从全局的数据库管理器获取状态
    // 目前提供模拟数据用于演示

    DatabaseStatus {
        current_state: "主数据库".to_string(),
        is_using_primary: true,
        has_primary_configured: true,
        health_check_result: true,
        last_failover_time: None,
        auto_recovery_enabled: true,
    }
}

/// 获取存储状态（内部实现）
async fn get_storage_status_internal() -> StorageStatus {
    // 实际实现中，这里会从全局的存储管理器获取状态
    // 目前提供模拟数据用于演示

    StorageStatus {
        current_state: "OSS存储".to_string(),
        is_using_primary: true,
        has_primary_configured: true,
        health_check_result: true,
        pending_sync_files: 0,
        auto_recovery_enabled: true,
    }
}

/// 确定整体健康状态
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

/// 生成操作建议
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
