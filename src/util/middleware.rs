use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use tower_sessions::Session;
use uuid::Uuid;
use std::time::Instant;
use tracing::{info, warn, debug};

// 请求日志中间件
pub async fn log_request(request: Request, next: Next) -> Response {
    let start_time = Instant::now();
    let request_id = Uuid::new_v4().to_string();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();
    
    // 从header中获取信息
    let user_agent = request
        .headers()
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    
    let content_length = request
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    // 记录请求开始
    info!(
        "请求开始 request_id={} method={} uri={} version={:?} client_ip=unknown content_length={} user_agent={}",
        request_id, method, uri, version, content_length, user_agent
    );

    let response = next.run(request).await;
    let duration = start_time.elapsed();
    let status = response.status();
    
    // 记录请求结果
    if status.is_success() {
        info!(
            "请求成功 request_id={} method={} uri={} status={} duration_ms={}",
            request_id, method, uri, status, duration.as_millis()
        );
    } else if status.is_client_error() {
        warn!(
            "客户端错误 request_id={} method={} uri={} status={} duration_ms={} client_ip=unknown",
            request_id, method, uri, status, duration.as_millis()
        );
    } else if status.is_server_error() {
        warn!(
            "服务器错误 request_id={} method={} uri={} status={} duration_ms={} client_ip=unknown",
            request_id, method, uri, status, duration.as_millis()
        );
    }

    // 记录业务指标
    debug!(
        "业务指标记录 method=\"{}\" path=\"{}\" status={} duration_ms={} timestamp={}",
        method, uri, status.as_u16(), duration.as_millis(),
        chrono::Utc::now().to_rfc3339()
    );

    // 根据不同的API路径记录专门的指标
    let path = uri.path();
    if path.starts_with("/api/auth/") {
        info!(
            "认证请求指标 metric_type=\"auth_request\" method=\"{}\" path=\"{}\" status={} duration_ms={}",
            method, path, status.as_u16(), duration.as_millis()
        );
    } else if path == "/api/preview" {
        info!(
            "预览生成请求指标 metric_type=\"preview_request\" method=\"{}\" status={} duration_ms={}",
            method, status.as_u16(), duration.as_millis()
        );
    }

    response
}

// 认证中间件
pub async fn auth_required(
    session: Session,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let uri = request.uri().clone();
    let request_id = Uuid::new_v4().to_string();

    info!("=== 认证检查开始 === request_id={} uri={}", request_id, uri);

    // 检查会话中是否有用户信息
    match session.get::<crate::model::SessionUser>("session_user").await {
        Ok(Some(user)) => {
            info!("认证成功 request_id={} user_id={} uri={}", request_id, user.user_id, uri);

            // 将用户信息添加到请求扩展中，供后续处理使用
            let mut request = request;
            request.extensions_mut().insert(user);

            let response = next.run(request).await;
            Ok(response)
        }
        Ok(None) => {
            warn!("认证失败：用户未登录 request_id={} uri={}", request_id, uri);
            Err(StatusCode::UNAUTHORIZED)
        }
        Err(e) => {
            warn!("认证失败：会话错误 request_id={} uri={} error={}", request_id, uri, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

// CORS中间件
pub async fn cors_middleware(request: Request, next: Next) -> Response {
    let response = next.run(request).await;
    
    // 这里可以添加CORS头部处理
    // 目前保持简单，返回原始响应
    response
}

pub async fn logging_middleware(request: Request, next: Next) -> Result<Response, StatusCode> {
    let method = request.method().clone();
    let uri = request.uri().clone();

    let response = next.run(request).await;

    tracing::info!("{} {} - {}", method, uri, response.status());

    Ok(response)
}

