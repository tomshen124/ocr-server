//! HTTP链路追踪中间件
//! 自动为HTTP请求创建追踪上下文

use crate::util::tracing::request_tracker::{LogLevel, RequestTracker, SpanType, TraceStatus};
use crate::util::tracing::{generate_trace_id, GlobalTraceId};
use axum::{
    extract::{Request, State},
    http::{HeaderMap, HeaderName, HeaderValue},
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

/// 追踪上下文键
pub const TRACE_ID_HEADER: &str = "x-trace-id";
pub const SPAN_ID_HEADER: &str = "x-span-id";
pub const PARENT_SPAN_ID_HEADER: &str = "x-parent-span-id";
pub const REQUEST_ID_HEADER: &str = "x-request-id";

/// 追踪中间件层
#[derive(Clone)]
pub struct TracingMiddleware {
    /// 中间件配置
    config: TracingConfig,
}

/// 追踪配置
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// 是否启用追踪
    pub enabled: bool,
    /// 是否在响应中包含追踪头
    pub include_response_headers: bool,
    /// 慢请求阈值（毫秒）
    pub slow_request_threshold: u64,
    /// 是否记录请求体（仅用于调试）
    pub log_request_body: bool,
    /// 最大请求体记录大小
    pub max_body_log_size: usize,
    /// 排除的路径模式（不进行追踪）
    pub excluded_paths: Vec<String>,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            include_response_headers: true,
            slow_request_threshold: 1000, // 1秒
            log_request_body: false,      // 生产环境关闭
            max_body_log_size: 1024,      // 1KB
            excluded_paths: vec![
                "/health".to_string(),
                "/api/health".to_string(),
                "/static".to_string(),
                "/favicon.ico".to_string(),
            ],
        }
    }
}

impl TracingMiddleware {
    /// 创建新的追踪中间件
    pub fn new(config: TracingConfig) -> Self {
        Self { config }
    }

    /// 创建默认中间件
    pub fn default() -> Self {
        Self::new(TracingConfig::default())
    }

    /// 检查路径是否应该被排除
    fn should_exclude_path(&self, path: &str) -> bool {
        for excluded in &self.config.excluded_paths {
            if path.starts_with(excluded) {
                return true;
            }
        }
        false
    }
}

/// 追踪层 - Axum中间件层
#[derive(Clone)]
pub struct TracingLayer {
    middleware: TracingMiddleware,
}

impl TracingLayer {
    /// 创建新的追踪层
    pub fn new(config: TracingConfig) -> Self {
        Self {
            middleware: TracingMiddleware::new(config),
        }
    }

    /// 创建默认追踪层
    pub fn default() -> Self {
        Self::new(TracingConfig::default())
    }
}

/// HTTP请求追踪中间件函数
pub async fn tracing_middleware(headers: HeaderMap, request: Request, next: Next) -> Response {
    let config = TracingConfig::default(); // 在实际使用中应该从状态中获取

    // 检查是否启用追踪
    if !config.enabled {
        return next.run(request).await;
    }

    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or("").to_string();

    // 检查是否应该排除此路径
    if config
        .excluded_paths
        .iter()
        .any(|excluded| path.starts_with(excluded))
    {
        return next.run(request).await;
    }

    let start_time = Instant::now();

    // 提取或生成追踪ID
    let trace_id = extract_or_generate_trace_id(&headers);
    let user_id = extract_user_id(&headers);

    // 创建请求追踪器
    let mut tracker = RequestTracker::from_http_request(
        &method,
        &path,
        Some(trace_id.clone()),
        user_id.as_deref(),
    );

    // 添加请求元数据
    if !query.is_empty() {
        tracker.add_metadata("query_string".to_string(), query);
    }

    // 添加HTTP头信息
    add_http_headers_to_tracker(&mut tracker, &headers);

    // 记录请求开始
    tracker.record_event(
        &format!("HTTP request started: {} {}", method, path),
        LogLevel::Info,
    );

    debug!(
        "Starting HTTP request: {} {} (trace: {})",
        method, path, trace_id
    );

    // 执行请求处理
    let response = next.run(request).await;

    // 计算请求处理时间
    let duration = start_time.elapsed();
    let status_code = response.status().as_u16();

    // 记录请求完成
    tracker.add_span_tag("status_code".to_string(), status_code.to_string());
    tracker.add_span_tag(
        "response_time_ms".to_string(),
        duration.as_millis().to_string(),
    );

    let success = status_code < 400;
    let event_message = format!(
        "HTTP request completed: {} {} - {} ({}ms)",
        method,
        path,
        status_code,
        duration.as_millis()
    );

    if success {
        tracker.record_event(&event_message, LogLevel::Info);
        tracker.finish_span_with_status(TraceStatus::Success);
    } else {
        tracker.record_event(&event_message, LogLevel::Error);
        tracker.finish_span_with_status(TraceStatus::Failed);
    }

    // 检查慢请求
    if duration.as_millis() > config.slow_request_threshold as u128 {
        warn!(
            "Slow request detected: {} {} took {}ms (trace: {})",
            method,
            path,
            duration.as_millis(),
            trace_id
        );
    }

    info!(
        "HTTP request: {} {} -> {} ({}ms, trace: {})",
        method,
        path,
        status_code,
        duration.as_millis(),
        trace_id
    );

    // 在移动前克隆需要的值
    let request_id_for_headers = tracker.request_id.clone();

    // 完成追踪（异步）
    tokio::spawn(async move {
        tracker.finish().await;
    });

    // 添加追踪头到响应
    let mut response = response;
    if config.include_response_headers {
        let headers = response.headers_mut();

        if let Ok(trace_id_value) = HeaderValue::from_str(&trace_id) {
            headers.insert(HeaderName::from_static(TRACE_ID_HEADER), trace_id_value);
        }

        if let Ok(request_id) = HeaderValue::from_str(&request_id_for_headers) {
            headers.insert(HeaderName::from_static(REQUEST_ID_HEADER), request_id);
        }
    }

    response
}

/// 提取或生成追踪ID
fn extract_or_generate_trace_id(headers: &HeaderMap) -> GlobalTraceId {
    // 尝试从请求头中提取追踪ID
    if let Some(trace_id) = headers.get(TRACE_ID_HEADER) {
        if let Ok(trace_id_str) = trace_id.to_str() {
            if !trace_id_str.is_empty() {
                return trace_id_str.to_string();
            }
        }
    }

    // 尝试从其他常见的追踪头中提取
    let common_trace_headers = [
        "x-trace-id",
        "x-request-id",
        "x-correlation-id",
        "traceparent",
        "uber-trace-id",
    ];

    for header_name in &common_trace_headers {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(value_str) = value.to_str() {
                if !value_str.is_empty() {
                    return value_str.to_string();
                }
            }
        }
    }

    // 生成新的追踪ID
    generate_trace_id()
}

/// 提取用户ID
fn extract_user_id(headers: &HeaderMap) -> Option<String> {
    // 尝试从Authorization头中提取用户信息
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            // 这里可以根据实际的认证方案来解析用户ID
            // 比如从JWT token中解析
            return Some(format!("user_from_auth_{}", auth_str.len()));
        }
    }

    // 尝试从自定义用户头中提取
    if let Some(user_header) = headers.get("x-user-id") {
        if let Ok(user_str) = user_header.to_str() {
            return Some(user_str.to_string());
        }
    }

    None
}

/// 添加HTTP头信息到追踪器
fn add_http_headers_to_tracker(tracker: &mut RequestTracker, headers: &HeaderMap) {
    // 记录重要的HTTP头
    let important_headers = [
        "user-agent",
        "content-type",
        "content-length",
        "accept",
        "accept-encoding",
        "x-forwarded-for",
        "x-real-ip",
        "referer",
    ];

    for header_name in &important_headers {
        if let Some(value) = headers.get(*header_name) {
            if let Ok(value_str) = value.to_str() {
                tracker.add_metadata(
                    format!("http_{}", header_name.replace('-', "_")),
                    value_str.to_string(),
                );
            }
        }
    }
}

/// 请求上下文
#[derive(Debug, Clone)]
pub struct RequestContext {
    /// 追踪ID
    pub trace_id: GlobalTraceId,
    /// 请求ID
    pub request_id: String,
    /// 用户ID
    pub user_id: Option<String>,
    /// 开始时间
    pub start_time: Instant,
    /// 元数据
    pub metadata: HashMap<String, String>,
}

impl RequestContext {
    /// 创建新的请求上下文
    pub fn new(trace_id: GlobalTraceId, request_id: String, user_id: Option<String>) -> Self {
        Self {
            trace_id,
            request_id,
            user_id,
            start_time: Instant::now(),
            metadata: HashMap::new(),
        }
    }

    /// 获取请求持续时间
    pub fn duration(&self) -> std::time::Duration {
        self.start_time.elapsed()
    }

    /// 添加元数据
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }
}

/// 从当前请求上下文中提取追踪信息的宏
#[macro_export]
macro_rules! current_trace_id {
    () => {
        // 在实际实现中，这里应该从当前请求上下文中获取追踪ID
        // 可以使用tokio的任务本地存储或其他上下文传递机制
        None::<String>
    };
}

/// 创建子span的宏
#[macro_export]
macro_rules! with_span {
    ($tracker:expr, $span_name:expr, $span_type:expr, $block:block) => {{
        $tracker.start_span($span_name, $span_type);
        let result = $block;
        $tracker.finish_current_span();
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Method, Uri};

    #[test]
    fn test_extract_trace_id() {
        let mut headers = HeaderMap::new();
        headers.insert("x-trace-id", "test-trace-123".parse().unwrap());

        let trace_id = extract_or_generate_trace_id(&headers);
        assert_eq!(trace_id, "test-trace-123");
    }

    #[test]
    fn test_generate_trace_id() {
        let headers = HeaderMap::new();
        let trace_id = extract_or_generate_trace_id(&headers);

        assert!(!trace_id.is_empty());
        assert!(trace_id.starts_with("trace_"));
    }

    #[test]
    fn test_should_exclude_path() {
        let config = TracingConfig::default();
        let middleware = TracingMiddleware::new(config);

        assert!(middleware.should_exclude_path("/health"));
        assert!(middleware.should_exclude_path("/api/health"));
        assert!(middleware.should_exclude_path("/static/css/main.css"));
        assert!(!middleware.should_exclude_path("/api/preview"));
    }

    #[test]
    fn test_request_context() {
        let mut context = RequestContext::new(
            "test-trace".to_string(),
            "test-request".to_string(),
            Some("test-user".to_string()),
        );

        context.add_metadata("test".to_string(), "value".to_string());

        assert_eq!(context.trace_id, "test-trace");
        assert_eq!(context.request_id, "test-request");
        assert_eq!(context.user_id, Some("test-user".to_string()));
        assert!(context.metadata.contains_key("test"));
    }
}
