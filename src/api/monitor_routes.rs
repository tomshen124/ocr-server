//! 监控系统HTTP API端点
//! 提供登录、登出、会话验证等API

use crate::api::monitor_auth::{LoginRequest, LoginResponse, MonitorAuthService};
use crate::AppState;
use axum::{
    extract::{State, Query},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};

/// 会话验证查询参数
#[derive(Debug, Deserialize)]
pub struct SessionQuery {
    session_id: String,
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

/// 监控API路由
pub fn monitor_routes() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/status", get(check_status))
        .route("/auth/cleanup", post(cleanup_sessions))
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

    match auth_service.login(&req.username, &req.password, &ip, &user_agent).await {
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

/// 清理过期会话 (管理员功能)
pub async fn cleanup_sessions(
    State(state): State<AppState>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<ApiResponse<u64>>, StatusCode> {
    let auth_service = MonitorAuthService::new(state.database.clone());

    // 验证会话并检查管理员权限
    match auth_service.verify_session(&query.session_id).await {
        Ok(Some(session)) => {
            if session.user.role != "admin" {
                return Err(StatusCode::FORBIDDEN);
            }
        }
        Ok(None) => return Err(StatusCode::UNAUTHORIZED),
        Err(_) => return Err(StatusCode::INTERNAL_SERVER_ERROR),
    }

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