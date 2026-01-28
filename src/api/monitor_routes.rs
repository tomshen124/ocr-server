//! 监控系统HTTP API端点
//! 提供登录、登出、会话验证等API

use crate::api::monitor_auth::{LoginRequest, LoginResponse, MonitorAuthService};
use crate::api::worker_proxy;
use crate::db::models::MonitorUser;
use crate::AppState;
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{delete, get, post, put},
    Router,
};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};

/// 会话验证查询参数
#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    #[serde(
        alias = "monitor_session_id",
        alias = "monitorSessionId",
        alias = "sessionId",
        alias = "session_id"
    )]
    session_id: String,
}

/// 预审修复查询参数
#[derive(Debug, Deserialize)]
pub struct PreviewRepairQuery {
    #[serde(
        alias = "monitor_session_id",
        alias = "monitorSessionId",
        alias = "sessionId",
        alias = "session_id"
    )]
    session_id: String,
    /// 允许修复的最大时间窗（小时），默认 168 小时（7 天）
    #[serde(default)]
    max_age_hours: Option<i64>,
}

/// 通用API响应
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            message: "操作成功".to_string(),
            data: Some(data),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: message.into(),
            data: None,
        }
    }
}

/// 创建用户请求
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: String,
    #[serde(default)]
    pub role: Option<String>,
}

/// 更新角色请求
#[derive(Debug, Deserialize)]
pub struct UpdateUserRoleRequest {
    pub role: String,
}

/// 重置密码请求
#[derive(Debug, Deserialize)]
pub struct ResetPasswordRequest {
    pub password: String,
}

/// 监控API路由
pub fn monitor_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/status", get(check_status))
        .route("/auth/sessions/active", get(active_sessions))
        .route("/auth/cleanup", post(cleanup_sessions))
        .route("/users", get(list_users).post(create_user))
        // 兼容旧前端：POST /users/{id} 直接视为禁用
        .route(
            "/users/{user_id}",
            delete(delete_user).post(deactivate_user_post),
        )
        .route("/users/{user_id}/deactivate", post(deactivate_user_post))
        .route("/users/{user_id}/role", put(update_user_role))
        .route(
            "/users/{user_id}/password/reset",
            post(reset_user_password_post),
        )
        .route("/users/{user_id}/password", put(reset_user_password))
        .route("/users/{user_id}/activate", post(activate_user))
        .route("/previews/{preview_id}/repair", post(repair_preview_links))
        // 全局限流控制 (仅 super_admin)
        .route("/system/throttle/status", get(throttle_status))
        .route("/system/throttle/enable", post(throttle_enable))
        .route("/system/throttle/disable", post(throttle_disable))
}

/// 用户登录
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());

    // 获取客户端IP
    let ip = get_client_ip(&headers).unwrap_or_else(|| "unknown".to_string());

    // 获取User-Agent
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    match auth_service
        .login(&req.username, &req.password, &ip, &user_agent)
        .await
    {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            tracing::error!("登录失败: {}", e);
            Ok(Json(LoginResponse {
                success: false,
                message: "登录失败，请稍后重试".to_string(),
                session: None,
            }))
        }
    }
}

/// 用户登出
pub async fn logout(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());

    match auth_service.logout(&query.session_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            tracing::error!("登出失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// 检查会话状态
pub async fn check_status(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<bool>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());

    match auth_service.verify_session(&query.session_id).await {
        Ok(Some(_session)) => Ok(Json(ApiResponse::success(true))),
        Ok(None) => Ok(Json(ApiResponse::success(false))),
        Err(e) => {
            tracing::error!("会话验证失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// 获取当前活跃会话数量
pub async fn active_sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<i64>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(
        &auth_service,
        &query.session_id,
        &["super_admin", "sys_admin"],
    )
    .await?;

    match auth_service.get_active_sessions_count().await {
        Ok(count) => Ok(Json(ApiResponse::success(count))),
        Err(e) => {
            tracing::error!("获取活跃会话数量失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// 清理过期会话 (管理员功能)
pub async fn cleanup_sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<u64>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());

    // 验证会话并检查超级管理员权限
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    // 执行清理
    match auth_service.cleanup_expired_sessions().await {
        Ok(count) => {
            tracing::info!("清理了 {} 个过期会话", count);
            Ok(Json(ApiResponse::success(count)))
        }
        Err(e) => {
            tracing::error!("清理会话失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

async fn require_role(
    auth_service: &MonitorAuthService,
    session_id: &str,
    allowed_roles: &[&str],
) -> Result<MonitorUser, StatusCode> {
    let normalize = |r: &str| -> String {
        let lower = r.trim().to_ascii_lowercase();
        if lower == "admin" {
            "super_admin".to_string()
        } else {
            lower
        }
    };

    match auth_service.verify_session(session_id).await {
        Ok(Some(session)) => {
            let user_role = normalize(&session.user.role);
            let allowed = allowed_roles
                .iter()
                .map(|r| normalize(r))
                .any(|r| r == user_role);
            if allowed {
                Ok(session.user)
            } else {
                Err(StatusCode::FORBIDDEN)
            }
        }
        Ok(None) => Err(StatusCode::UNAUTHORIZED),
        Err(e) => {
            tracing::error!("监控会话校验失败: {}", e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

/// 获取监控用户列表
pub async fn list_users(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<Vec<MonitorUser>>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(
        &auth_service,
        &query.session_id,
        &["super_admin", "sys_admin"],
    )
    .await?;

    match auth_service.list_users().await {
        Ok(users) => Ok(Json(ApiResponse::success(users))),
        Err(e) => {
            tracing::error!("获取监控用户列表失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 创建监控用户
pub async fn create_user(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<Json<ApiResponse<MonitorUser>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    let role = payload.role.as_deref().unwrap_or("ops_admin");

    match auth_service
        .create_user(&payload.username, &payload.password, role)
        .await
    {
        Ok(user) => Ok(Json(ApiResponse::success(user))),
        Err(e) => {
            tracing::error!("创建监控用户失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 更新监控用户角色
pub async fn update_user_role(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<SessionQuery>,
    Json(payload): Json<UpdateUserRoleRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    match auth_service.update_user_role(&user_id, &payload.role).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            tracing::error!("更新监控用户角色失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 重置监控用户密码
pub async fn reset_user_password(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<SessionQuery>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    match auth_service
        .reset_user_password(&user_id, &payload.password)
        .await
    {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            tracing::error!("重置监控用户密码失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 重置监控用户密码（POST 兼容）
pub async fn reset_user_password_post(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<SessionQuery>,
    Json(payload): Json<ResetPasswordRequest>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    match auth_service
        .reset_user_password(&user_id, &payload.password)
        .await
    {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            tracing::error!("重置监控用户密码失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 禁用监控用户
pub async fn delete_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    match auth_service.deactivate_user(&user_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            tracing::error!("禁用监控用户失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 禁用监控用户（POST 兼容）
pub async fn deactivate_user_post(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    match auth_service.deactivate_user(&user_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            tracing::error!("禁用监控用户失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 启用监控用户
pub async fn activate_user(
    State(state): State<AppState>,
    Path(user_id): Path<String>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    match auth_service.activate_user(&user_id).await {
        Ok(_) => Ok(Json(ApiResponse::success(()))),
        Err(e) => {
            tracing::error!("启用监控用户失败: {}", e);
            Ok(Json(ApiResponse::error(e.to_string())))
        }
    }
}

/// 修复预审记录的附件链接并回填稳定URL
pub async fn repair_preview_links(
    State(state): State<AppState>,
    Path(preview_id): Path<String>,
    Query(query): Query<PreviewRepairQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin"]).await?;

    let preview = match state.database.get_preview_record(&preview_id).await {
        Ok(Some(p)) => p,
        Ok(None) => return Ok(Json(ApiResponse::error("预审记录不存在"))),
        Err(e) => {
            tracing::error!("查询预审记录失败: {}", e);
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let max_age_hours = query.max_age_hours.unwrap_or(24 * 7);
    if max_age_hours > 0 {
        let cutoff = Utc::now() - ChronoDuration::hours(max_age_hours);
        if preview.created_at < cutoff {
            return Ok(Json(ApiResponse::success(serde_json::json!({
                "previewId": preview_id,
                "skipped": true,
                "reason": format!("创建时间早于 {} 小时，未自动修复", max_age_hours),
            }))));
        }
    }

    let evaluation_json = match preview.evaluation_result.as_ref() {
        Some(v) => v,
        None => {
            return Ok(Json(ApiResponse::error(
                "记录缺少 evaluation_result，无法修复",
            )))
        }
    };

    match worker_proxy::repair_preview_materials(
        &state.database,
        &state.storage,
        &preview_id,
        evaluation_json,
    )
    .await
    {
        Ok(result) => {
            let payload = serde_json::json!({
                "previewId": result.preview_id,
                "repaired": result.repaired,
                "attachmentsBefore": result.attachments_before,
                "attachmentsAfter": result.attachments_after,
                "persisted": result.persisted,
                "skipped": false
            });
            Ok(Json(ApiResponse::success(payload)))
        }
        Err(err) => {
            tracing::error!(preview_id = %preview_id, error = %err, "修复预审附件失败");
            Ok(Json(ApiResponse::error(format!("修复失败: {}", err))))
        }
    }
}

/// 从请求头中获取客户端IP
fn get_client_ip(headers: &HeaderMap) -> Option<String> {
    // 按优先级检查各种可能的IP头
    let ip_headers = [
        "x-forwarded-for",
        "x-real-ip",
        "x-client-ip",
        "cf-connecting-ip", // Cloudflare
        "true-client-ip",   // Akamai
    ];

    for header_name in &ip_headers {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(ip_str) = value.to_str() {
                // X-Forwarded-For 可能包含多个IP，取第一个
                let ip = ip_str.split(',').next().unwrap_or(ip_str).trim();
                if !ip.is_empty() && ip != "unknown" {
                    return Some(ip.to_string());
                }
            }
        }
    }

    None
}

// ============ 全局限流控制 API ============

/// 限流启用请求
#[derive(Debug, Deserialize)]
pub struct ThrottleEnableRequest {
    pub max_requests: u32,
    #[serde(default)]
    pub reason: Option<String>,
}

/// 获取限流状态
pub async fn throttle_status(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<crate::util::auth::global_throttle::ThrottleStatus>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    require_role(&auth_service, &query.session_id, &["super_admin", "admin"]).await?;

    let status = crate::util::auth::global_throttle::GlobalThrottleGuard::global().status();
    Ok(Json(ApiResponse::success(status)))
}

/// 启用限流
pub async fn throttle_enable(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
    Json(req): Json<ThrottleEnableRequest>,
) -> Result<Json<ApiResponse<crate::util::auth::global_throttle::ThrottleStatus>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    let session = require_role(&auth_service, &query.session_id, &["super_admin", "admin"]).await?;

    let operator = session.username;
    crate::util::auth::global_throttle::GlobalThrottleGuard::global().enable(
        req.max_requests,
        &operator,
        req.reason,
    );

    let status = crate::util::auth::global_throttle::GlobalThrottleGuard::global().status();
    Ok(Json(ApiResponse::success(status)))
}

/// 解除限流
pub async fn throttle_disable(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());
    let session = require_role(&auth_service, &query.session_id, &["super_admin", "admin"]).await?;

    let operator = session.username;
    let (blocked, processed, duration) =
        crate::util::auth::global_throttle::GlobalThrottleGuard::global().disable(&operator);

    Ok(Json(ApiResponse::success(serde_json::json!({
        "enabled": false,
        "total_blocked": blocked,
        "total_processed": processed,
        "duration_secs": duration
    }))))
}
