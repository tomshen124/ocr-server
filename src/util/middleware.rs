use crate::api::monitor_auth::MonitorAuthService;
use crate::model::SessionUser;
use crate::util::logging::standards::events;
use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use std::sync::Arc;
use std::time::Instant;
use tower_sessions::Session;
use uuid::Uuid;

use crate::{util::api_stats, AppState};
use chrono::Utc;
use url::form_urlencoded;

fn extract_client_ip(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
}

fn extract_user_agent(headers: &HeaderMap) -> &str {
    headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
}

fn parse_content_length(headers: &HeaderMap) -> usize {
    headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0)
}

fn is_quiet_path(path: &str) -> bool {
    const QUIET_EXACT: &[&str] = &[
        "/api/health",
        "/api/health/details",
        "/api/health/components",
        "/api/monitoring/status",
        "/api/logs/stats",
        "/api/logs/health",
        "/api/queue/status",
        "/api/permits/tracker",
        "/favicon.ico",
    ];

    const QUIET_PREFIX: &[&str] = &["/static", "/images", "/api/monitor/"];

    QUIET_EXACT.contains(&path) || QUIET_PREFIX.iter().any(|prefix| path.starts_with(prefix))
}

// 统一请求日志中间件
pub async fn request_logging_middleware(mut request: Request, next: Next) -> Response {
    let start_time = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let app_state = request
        .extensions()
        .get::<AppState>()
        .cloned()
        .map(|state| Arc::clone(&state.database));
    let headers = request.headers().clone();
    let user_agent = extract_user_agent(&headers);
    let client_ip = extract_client_ip(&headers);
    let request_size = parse_content_length(&headers);
    request.extensions_mut().insert(request_id.clone());

    let quiet_path = is_quiet_path(uri.path());
    if !quiet_path {
        tracing::debug!(
            target: "http.server",
            event = events::REQUEST_START,
            request_id = %request_id,
            method = %method,
            path = %uri.path(),
            query = uri.query().unwrap_or(""),
            user_agent = %user_agent,
            client_ip = client_ip.as_deref().unwrap_or("unknown")
        );
    }

    let response = next.run(request).await;
    let duration = start_time.elapsed();
    let status = response.status();
    let response_size = parse_content_length(response.headers());
    let query = uri.query().unwrap_or("");

    if status.is_server_error() {
        tracing::error!(
            target: "http.server",
            event = events::REQUEST_ERROR,
            request_id = %request_id,
            method = %method,
            path = %uri.path(),
            query = query,
            status = status.as_u16(),
            duration_ms = duration.as_millis() as u64,
            request_bytes = request_size,
            response_bytes = response_size,
            user_agent = %user_agent,
            client_ip = client_ip.as_deref().unwrap_or("unknown")
        );
    } else if status.is_client_error() {
        if quiet_path {
            tracing::debug!(
                target: "http.server",
                event = events::REQUEST_COMPLETE,
                level = "client_error",
                request_id = %request_id,
                method = %method,
                path = %uri.path(),
                query = query,
                status = status.as_u16(),
                duration_ms = duration.as_millis() as u64,
                request_bytes = request_size,
                response_bytes = response_size
            );
        } else {
            tracing::warn!(
                target: "http.server",
                event = events::REQUEST_COMPLETE,
                request_id = %request_id,
                method = %method,
                path = %uri.path(),
                query = query,
                status = status.as_u16(),
                duration_ms = duration.as_millis() as u64,
                request_bytes = request_size,
                response_bytes = response_size,
                user_agent = %user_agent,
                client_ip = client_ip.as_deref().unwrap_or("unknown")
            );
        }
    } else if quiet_path {
        tracing::debug!(
            target: "http.server",
            event = events::REQUEST_COMPLETE,
            request_id = %request_id,
            method = %method,
            path = %uri.path(),
            query = query,
            status = status.as_u16(),
            duration_ms = duration.as_millis() as u64,
            request_bytes = request_size,
            response_bytes = response_size
        );
    } else {
        tracing::info!(
            target: "http.server",
            event = events::REQUEST_COMPLETE,
            request_id = %request_id,
            method = %method,
            path = %uri.path(),
            query = query,
            status = status.as_u16(),
            duration_ms = duration.as_millis() as u64,
            request_bytes = request_size,
            response_bytes = response_size,
            user_agent = %user_agent,
            client_ip = client_ip.as_deref().unwrap_or("unknown")
        );
    }

    if !quiet_path && duration.as_millis() > 1_000 {
        tracing::warn!(
            target: "http.server",
            event = events::REQUEST_SLOW,
            request_id = %request_id,
            method = %method,
            path = %uri.path(),
            duration_ms = duration.as_millis() as u64,
            status = status.as_u16()
        );
    }

    crate::util::auth::simple_call_logging::record_api_call(
        method.as_str(),
        uri.path(),
        &headers,
        status.as_u16(),
        duration.as_millis() as u64,
        request_size,
        response_size,
    );

    if let Some(database) = app_state {
        let endpoint = uri.path().to_string();
        let method_string = method.to_string();
        let status_code = status.as_u16();
        let error_message = if status.is_server_error() {
            Some(format!("http_{}", status_code))
        } else {
            None
        };
        let request_bytes = request_size.min(u32::MAX as usize) as u32;
        let response_bytes = response_size.min(u32::MAX as usize) as u32;
        tokio::spawn(async move {
            let _ = api_stats::record_api_call(
                database,
                &endpoint,
                &method_string,
                None,
                None,
                status_code,
                start_time,
                error_message,
                request_bytes,
                response_bytes,
            )
            .await;
        });
    }

    response
}

// 认证中间件
pub async fn auth_required(
    State(app_state): State<AppState>,
    mut session: Session,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let uri = request.uri().clone();
    let request_id = Uuid::new_v4().to_string();

    let cookie_header = request
        .headers()
        .get("cookie")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let cookie_present = cookie_header.is_some();
    let cookie_len = cookie_header.as_ref().map(|v| v.len()).unwrap_or(0);

    let session_id_result = session.id();
    let session_present = session_id_result.is_some();

    tracing::debug!(
        target: "auth",
        event = events::AUTH_CHECK,
        request_id = %request_id,
        uri = %uri,
        cookie_present,
        cookie_len,
        session_present
    );

    // 检查会话中是否有用户信息
    let monitor_session_id = extract_monitor_session_id(&request);

    match session.get::<SessionUser>("session_user").await {
        Ok(Some(user)) => {
            tracing::info!(
                target: "auth",
                event = events::AUTH_SUCCESS,
                request_id = %request_id,
                uri = %uri,
                user_id = %user.user_id,
                username = %user.user_name.as_deref().unwrap_or("unknown")
            );

            // 将用户信息添加到请求扩展中，供后续处理使用
            request.extensions_mut().insert(user);
            Ok(next.run(request).await)
        }
        Ok(None) => {
            if let Some(monitor_session_id) = monitor_session_id {
                tracing::debug!(
                    target: "auth",
                    event = events::AUTH_CHECK,
                    request_id = %request_id,
                    monitor_session_id = %monitor_session_id,
                    "Attempting to verify monitor session"
                );

                match MonitorAuthService::new(app_state.database.clone())
                    .verify_session(&monitor_session_id)
                    .await
                {
                    Ok(Some(monitor_session)) => {
                        tracing::info!(
                            target: "auth",
                            event = events::AUTH_SUCCESS,
                            request_id = %request_id,
                            user_id = %monitor_session.user.id,
                            username = %monitor_session.user.username,
                            "Monitor session verified successfully"
                        );

                        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
                        let session_user = SessionUser {
                            user_id: monitor_session.user.id.clone(),
                            user_name: Some(monitor_session.user.username.clone()),
                            certificate_type: "monitor".to_string(),
                            certificate_number: None,
                            phone_number: None,
                            email: None,
                            organization_name: None,
                            organization_code: None,
                            login_time: now.clone(),
                            last_active: now,
                        };

                        if let Err(e) = session.insert("session_user", &session_user).await {
                            tracing::warn!(
                                target: "auth",
                                request_id = %request_id,
                                uri = %uri,
                                error = %e,
                                "无法写入会话"
                            );
                        }

                        request.extensions_mut().insert(session_user);
                        return Ok(next.run(request).await);
                    }
                    Ok(None) => {
                        tracing::warn!(
                            target: "auth",
                            event = events::AUTH_FAILURE,
                            request_id = %request_id,
                            uri = %uri,
                            monitor_session_id = %monitor_session_id,
                            reason = "invalid_monitor_session",
                            "Monitor session verification returned None"
                        );
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                    Err(e) => {
                        tracing::error!(
                            target: "auth",
                            event = events::AUTH_ERROR,
                            request_id = %request_id,
                            uri = %uri,
                            monitor_session_id = %monitor_session_id,
                            error = %e,
                            "监控会话验证失败"
                        );
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                }
            } else {
                tracing::debug!(
                    target: "auth",
                    request_id = %request_id,
                    "No monitor_session_id found in request"
                );
            }
            tracing::warn!(
                target: "auth",
                event = events::AUTH_FAILURE,
                request_id = %request_id,
                uri = %uri,
                reason = "missing_session_user",
                session_present,
                cookie_present
            );
            tracing::debug!(target: "auth", cookie_present, "session_user not found in session and no valid monitor token");
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(e) => {
            tracing::error!(
                target: "auth",
                event = events::AUTH_ERROR,
                request_id = %request_id,
                uri = %uri,
                error = %e
            );
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn extract_monitor_session_id(request: &Request) -> Option<String> {
    // 先从查询参数获取
    if let Some(query) = request.uri().query() {
        if let Some(value) = form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "monitor_session_id")
            .map(|(_, value)| value.into_owned())
        {
            if !value.is_empty() {
                return Some(value);
            }
        }
    }

    // 尝试从请求头获取
    request
        .headers()
        .get("x-monitor-session-id")
        .and_then(|value| value.to_str().ok())
        .map(|s| s.to_string())
}

// CORS中间件
pub async fn cors_middleware(request: Request, next: Next) -> Response {
    let response = next.run(request).await;

    // 这里可以添加CORS头部处理
    // 目前保持简单，返回原始响应
    response
}
