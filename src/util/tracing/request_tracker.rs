//! 请求追踪器
//! 提供HTTP请求级别的链路追踪功能

use crate::util::tracing::distributed_tracing::{TraceChain, DISTRIBUTED_TRACER};
use crate::util::tracing::{
    generate_span_id, GlobalTraceId, TraceEvent, TraceEventType, TracingStatus,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// 请求追踪器
#[derive(Debug, Clone)]
pub struct RequestTracker {
    /// 追踪ID
    pub trace_id: GlobalTraceId,
    /// 请求ID
    pub request_id: String,
    /// 开始时间
    pub start_time: Instant,
    /// 请求元数据
    pub metadata: HashMap<String, String>,
    /// 当前活跃span
    pub current_span: Option<RequestSpan>,
}

/// 请求span
#[derive(Debug, Clone, Serialize)]
pub struct RequestSpan {
    /// span ID
    pub span_id: String,
    /// 父span ID
    pub parent_span_id: Option<String>,
    /// span名称
    pub name: String,
    /// span类型
    pub span_type: SpanType,
    /// 开始时间
    #[serde(skip)]
    pub start_time: Instant,
    /// 结束时间
    #[serde(skip)]
    pub end_time: Option<Instant>,
    /// 持续时间
    pub duration: Option<Duration>,
    /// span状态
    pub status: TraceStatus,
    /// span标签
    pub tags: HashMap<String, String>,
    /// span日志
    pub logs: Vec<SpanLog>,
    /// 错误信息
    pub error: Option<String>,
}

/// Span类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanType {
    /// HTTP请求
    HttpRequest,
    /// 数据库操作
    DatabaseOperation,
    /// OCR处理
    OcrProcessing,
    /// 文件操作
    FileOperation,
    /// 存储操作
    StorageOperation,
    /// 业务逻辑
    BusinessLogic,
    /// 外部API调用
    ExternalApiCall,
    /// 缓存操作
    CacheOperation,
    /// 自定义
    Custom(String),
}

/// 追踪状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TraceStatus {
    /// 进行中
    InProgress,
    /// 成功
    Success,
    /// 失败
    Failed,
    /// 超时
    Timeout,
    /// 取消
    Cancelled,
}

/// Span日志
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLog {
    /// 时间戳
    pub timestamp: SystemTime,
    /// 日志级别
    pub level: LogLevel,
    /// 日志消息
    pub message: String,
    /// 附加字段
    pub fields: HashMap<String, String>,
}

/// 日志级别
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl RequestTracker {
    /// 创建新的请求追踪器
    pub fn new(trace_id: Option<GlobalTraceId>) -> Self {
        let trace_id = match trace_id {
            Some(id) if !id.is_empty() => id,
            _ => DISTRIBUTED_TRACER.start_trace(None),
        };

        let request_id = format!(
            "req_{}",
            &Uuid::new_v4().to_string().replace('-', "")[0..12]
        );

        let mut tracker = Self {
            trace_id: trace_id.clone(),
            request_id: request_id.clone(),
            start_time: Instant::now(),
            metadata: HashMap::new(),
            current_span: None,
        };

        // 添加请求级别的元数据
        tracker.add_metadata("request_id".to_string(), request_id);
        tracker.add_metadata(
            "start_time".to_string(),
            SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .to_string(),
        );

        info!(
            "Created request tracker: {} for trace: {}",
            tracker.request_id, trace_id
        );
        tracker
    }

    /// 从HTTP请求创建追踪器
    pub fn from_http_request(
        method: &str,
        path: &str,
        trace_id: Option<GlobalTraceId>,
        user_id: Option<&str>,
    ) -> Self {
        let mut tracker = Self::new(trace_id);

        // 添加HTTP相关元数据
        tracker.add_metadata("http_method".to_string(), method.to_string());
        tracker.add_metadata("http_path".to_string(), path.to_string());
        tracker.add_metadata("request_type".to_string(), "http".to_string());

        if let Some(user_id) = user_id {
            tracker.add_metadata("user_id".to_string(), user_id.to_string());
        }

        // 开始HTTP请求span
        let span = tracker.start_span("http_request", SpanType::HttpRequest);
        span.add_tag("method".to_string(), method.to_string());
        span.add_tag("path".to_string(), path.to_string());

        tracker
    }

    /// 添加元数据
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// 获取元数据
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// 开始新的span
    pub fn start_span(&mut self, name: &str, span_type: SpanType) -> &mut RequestSpan {
        let parent_span_id = self.current_span.as_ref().map(|s| s.span_id.clone());

        let span = RequestSpan {
            span_id: generate_span_id(),
            parent_span_id,
            name: name.to_string(),
            span_type,
            start_time: Instant::now(),
            end_time: None,
            duration: None,
            status: TraceStatus::InProgress,
            tags: HashMap::new(),
            logs: Vec::new(),
            error: None,
        };

        debug!(
            "Started span: {} (type: {:?}) for request: {}",
            name, span.span_type, self.request_id
        );

        self.current_span = Some(span);
        self.current_span.as_mut().unwrap()
    }

    /// 完成当前span
    pub fn finish_current_span(&mut self) {
        if let Some(mut span) = self.current_span.take() {
            span.finish();
            self.send_span_to_tracer(span);
        }
    }

    /// 完成span并设置状态
    pub fn finish_span_with_status(&mut self, status: TraceStatus) {
        if let Some(span) = self.current_span.as_mut() {
            span.status = status;
        }
        self.finish_current_span();
    }

    /// 完成span并设置错误
    pub fn finish_span_with_error(&mut self, error: &str) {
        if let Some(span) = self.current_span.as_mut() {
            span.set_error(error.to_string());
        }
        self.finish_current_span();
    }

    /// 记录span事件
    pub fn record_event(&mut self, message: &str, level: LogLevel) {
        if let Some(span) = self.current_span.as_mut() {
            span.add_log(message.to_string(), level);
        }
    }

    /// 记录span事件（带字段）
    pub fn record_event_with_fields(
        &mut self,
        message: &str,
        level: LogLevel,
        fields: HashMap<String, String>,
    ) {
        if let Some(span) = self.current_span.as_mut() {
            span.add_log_with_fields(message.to_string(), level, fields);
        }
    }

    /// 添加span标签
    pub fn add_span_tag(&mut self, key: String, value: String) {
        if let Some(span) = self.current_span.as_mut() {
            span.add_tag(key, value);
        }
    }

    /// 完成整个请求追踪
    pub async fn finish(mut self) {
        let duration = self.start_time.elapsed();

        // 完成当前span（如果有的话）
        self.finish_current_span();

        // 创建请求完成事件
        if let Some(mut event) = DISTRIBUTED_TRACER.start_event(
            &self.trace_id,
            TraceEventType::HttpRequestEnd,
            "request_completed".to_string(),
        ) {
            event.add_tag("request_id".to_string(), self.request_id.clone());
            event.add_tag(
                "total_duration_ms".to_string(),
                duration.as_millis().to_string(),
            );

            // 添加元数据
            for (key, value) in &self.metadata {
                event.add_tag(key.clone(), value.clone());
            }

            DISTRIBUTED_TRACER.finish_event(event);
        }

        // 完成整个追踪
        DISTRIBUTED_TRACER.finish_trace(&self.trace_id).await;

        info!(
            "Finished request tracker: {} (duration: {:?})",
            self.request_id, duration
        );
    }

    /// 获取追踪信息摘要
    pub fn get_summary(&self) -> TracingSummary {
        TracingSummary {
            trace_id: self.trace_id.clone(),
            request_id: self.request_id.clone(),
            duration: self.start_time.elapsed(),
            current_span: self.current_span.as_ref().map(|s| s.name.clone()),
            metadata: self.metadata.clone(),
        }
    }

    /// 发送span到分布式追踪器
    fn send_span_to_tracer(&self, span: RequestSpan) {
        let event_type = match span.span_type {
            SpanType::HttpRequest => TraceEventType::HttpRequestEnd,
            SpanType::DatabaseOperation => TraceEventType::DatabaseOpEnd,
            SpanType::OcrProcessing => TraceEventType::OcrProcessEnd,
            SpanType::FileOperation => TraceEventType::FileDownloadEnd,
            SpanType::StorageOperation => TraceEventType::StorageOpEnd,
            _ => TraceEventType::Custom("span_completed".to_string()),
        };

        if let Some(mut event) =
            DISTRIBUTED_TRACER.start_event(&self.trace_id, event_type, span.name.clone())
        {
            // 添加span信息
            event.add_tag("span_id".to_string(), span.span_id.clone());
            event.add_tag("span_type".to_string(), format!("{:?}", span.span_type));

            if let Some(duration) = span.duration {
                event.add_tag("duration_ms".to_string(), duration.as_millis().to_string());
            }

            // 添加span标签
            for (key, value) in &span.tags {
                event.add_tag(key.clone(), value.clone());
            }

            // 设置状态和错误
            match span.status {
                TraceStatus::Success => event.status = TracingStatus::Completed,
                TraceStatus::Failed => {
                    event.status = TracingStatus::Error;
                    if let Some(error) = &span.error {
                        event.error = Some(error.clone());
                    }
                }
                TraceStatus::Timeout => event.status = TracingStatus::Timeout,
                _ => {}
            }

            DISTRIBUTED_TRACER.finish_event(event);
        }
    }
}

impl RequestSpan {
    /// 完成span
    pub fn finish(&mut self) {
        if self.end_time.is_none() {
            self.end_time = Some(Instant::now());
            self.duration = Some(self.start_time.elapsed());

            if self.status == TraceStatus::InProgress {
                self.status = TraceStatus::Success;
            }
        }
    }

    /// 添加标签
    pub fn add_tag(&mut self, key: String, value: String) {
        self.tags.insert(key, value);
    }

    /// 添加日志
    pub fn add_log(&mut self, message: String, level: LogLevel) {
        self.add_log_with_fields(message, level, HashMap::new());
    }

    /// 添加带字段的日志
    pub fn add_log_with_fields(
        &mut self,
        message: String,
        level: LogLevel,
        fields: HashMap<String, String>,
    ) {
        let log = SpanLog {
            timestamp: SystemTime::now(),
            level,
            message,
            fields,
        };

        self.logs.push(log);
    }

    /// 设置错误
    pub fn set_error(&mut self, error: String) {
        self.status = TraceStatus::Failed;
        self.error = Some(error.clone());
        self.add_log(format!("Error: {}", error), LogLevel::Error);
    }

    /// 获取持续时间（毫秒）
    pub fn duration_ms(&self) -> Option<u128> {
        self.duration.map(|d| d.as_millis())
    }
}

/// 追踪摘要
#[derive(Debug, Clone, Serialize)]
pub struct TracingSummary {
    /// 追踪ID
    pub trace_id: GlobalTraceId,
    /// 请求ID
    pub request_id: String,
    /// 持续时间
    pub duration: Duration,
    /// 当前span
    pub current_span: Option<String>,
    /// 元数据
    pub metadata: HashMap<String, String>,
}

/// 追踪宏 - 自动管理span生命周期
#[macro_export]
macro_rules! traced_operation {
    ($tracker:expr, $span_name:expr, $span_type:expr, $operation:block) => {{
        $tracker.start_span($span_name, $span_type);
        let result = $operation;
        match result {
            Ok(value) => {
                $tracker.finish_span_with_status(
                    crate::util::tracing::request_tracker::TraceStatus::Success,
                );
                Ok(value)
            }
            Err(error) => {
                $tracker.finish_span_with_error(&error.to_string());
                Err(error)
            }
        }
    }};
}

/// 异步追踪宏
#[macro_export]
macro_rules! traced_async_operation {
    ($tracker:expr, $span_name:expr, $span_type:expr, $operation:block) => {
        async {
            $tracker.start_span($span_name, $span_type);
            let result = $operation.await;
            match result {
                Ok(value) => {
                    $tracker.finish_span_with_status(
                        crate::util::tracing::request_tracker::TraceStatus::Success,
                    );
                    Ok(value)
                }
                Err(error) => {
                    $tracker.finish_span_with_error(&error.to_string());
                    Err(error)
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_request_tracker() {
        let mut tracker = RequestTracker::new(None);

        // 开始一个span
        tracker.start_span("test_operation", SpanType::BusinessLogic);
        tracker.add_span_tag("test_tag".to_string(), "test_value".to_string());
        tracker.record_event("Operation started", LogLevel::Info);

        // 模拟一些工作
        sleep(Duration::from_millis(10)).await;

        tracker.finish_current_span();

        // 完成追踪
        tracker.finish().await;
    }

    #[tokio::test]
    async fn test_http_request_tracker() {
        let mut tracker =
            RequestTracker::from_http_request("POST", "/api/preview", None, Some("test_user"));

        assert_eq!(
            tracker.get_metadata("http_method"),
            Some(&"POST".to_string())
        );
        assert_eq!(
            tracker.get_metadata("http_path"),
            Some(&"/api/preview".to_string())
        );
        assert_eq!(
            tracker.get_metadata("user_id"),
            Some(&"test_user".to_string())
        );

        tracker.finish().await;
    }

    #[test]
    fn test_request_span() {
        let mut span = RequestSpan {
            span_id: "test_span".to_string(),
            parent_span_id: None,
            name: "test".to_string(),
            span_type: SpanType::BusinessLogic,
            start_time: Instant::now(),
            end_time: None,
            duration: None,
            status: TraceStatus::InProgress,
            tags: HashMap::new(),
            logs: Vec::new(),
            error: None,
        };

        span.add_tag("test".to_string(), "value".to_string());
        span.add_log("Test log".to_string(), LogLevel::Info);
        span.finish();

        assert_eq!(span.status, TraceStatus::Success);
        assert!(span.duration.is_some());
        assert!(span.tags.contains_key("test"));
        assert_eq!(span.logs.len(), 1);
    }
}
