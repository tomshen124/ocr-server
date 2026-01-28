//! 分布式链路追踪模块
//! 提供请求链路追踪、性能监控和问题排查能力

pub mod distributed_tracing;
pub mod metrics_collector;
pub mod middleware;
pub mod request_tracker;
pub mod span_context;

// 重新导出核心类型
pub use distributed_tracing::{DistributedTracer, TraceConfig, DISTRIBUTED_TRACER};
pub use metrics_collector::{MetricsCollector, RequestMetrics};
pub use middleware::{TracingLayer, TracingMiddleware};
pub use request_tracker::{RequestSpan, RequestTracker, TraceStatus};
pub use span_context::{SpanContext, SpanId, TraceId};

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

/// 全局追踪ID类型
pub type GlobalTraceId = String;

/// 请求追踪状态
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TracingStatus {
    /// 追踪开始
    Started,
    /// 进行中
    InProgress,
    /// 成功完成
    Completed,
    /// 发生错误
    Error,
    /// 超时
    Timeout,
}

/// 追踪事件类型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraceEventType {
    /// HTTP请求开始
    HttpRequestStart,
    /// HTTP请求结束
    HttpRequestEnd,
    /// 数据库操作开始
    DatabaseOpStart,
    /// 数据库操作结束
    DatabaseOpEnd,
    /// OCR处理开始
    OcrProcessStart,
    /// OCR处理结束
    OcrProcessEnd,
    /// 文件下载开始
    FileDownloadStart,
    /// 文件下载结束
    FileDownloadEnd,
    /// 存储操作开始
    StorageOpStart,
    /// 存储操作结束
    StorageOpEnd,
    /// 自定义事件
    Custom(String),
}

/// 追踪事件
#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
    /// 事件ID
    pub event_id: String,
    /// 追踪ID
    pub trace_id: GlobalTraceId,
    /// 父span ID
    pub parent_span_id: Option<String>,
    /// 当前span ID
    pub span_id: String,
    /// 事件类型
    pub event_type: TraceEventType,
    /// 事件名称
    pub event_name: String,
    /// 开始时间
    #[serde(skip)]
    pub start_time: Instant,
    /// 结束时间（可选）
    #[serde(skip)]
    pub end_time: Option<Instant>,
    /// 持续时间
    pub duration: Option<Duration>,
    /// 事件状态
    pub status: TracingStatus,
    /// 事件标签
    pub tags: std::collections::HashMap<String, String>,
    /// 事件日志
    pub logs: Vec<String>,
    /// 错误信息
    pub error: Option<String>,
}

impl TraceEvent {
    /// 创建新的追踪事件
    pub fn new(
        trace_id: GlobalTraceId,
        parent_span_id: Option<String>,
        event_type: TraceEventType,
        event_name: String,
    ) -> Self {
        Self {
            event_id: Uuid::new_v4().to_string(),
            trace_id,
            parent_span_id,
            span_id: Uuid::new_v4().to_string(),
            event_type,
            event_name,
            start_time: Instant::now(),
            end_time: None,
            duration: None,
            status: TracingStatus::Started,
            tags: std::collections::HashMap::new(),
            logs: Vec::new(),
            error: None,
        }
    }

    /// 完成事件
    pub fn finish(&mut self) {
        self.end_time = Some(Instant::now());
        self.duration = Some(self.start_time.elapsed());
        if matches!(
            self.status,
            TracingStatus::Started | TracingStatus::InProgress
        ) {
            self.status = TracingStatus::Completed;
        }
    }

    /// 添加标签
    pub fn add_tag(&mut self, key: String, value: String) {
        self.tags.insert(key, value);
    }

    /// 添加日志
    pub fn add_log(&mut self, message: String) {
        self.logs
            .push(format!("[{}] {}", chrono::Utc::now().to_rfc3339(), message));
    }

    /// 设置错误
    pub fn set_error(&mut self, error: String) {
        self.status = TracingStatus::Error;
        self.error = Some(error.clone());
        self.add_log(format!("Error: {}", error));
        self.finish();
    }
}

/// 追踪宏 - 简化追踪事件创建
#[macro_export]
macro_rules! trace_event {
    ($tracer:expr, $event_type:expr, $event_name:expr) => {
        $tracer.start_event($event_type, $event_name.to_string())
    };

    ($tracer:expr, $event_type:expr, $event_name:expr, $($key:expr => $value:expr),*) => {
        {
            let mut event = $tracer.start_event($event_type, $event_name.to_string());
            $(
                event.add_tag($key.to_string(), $value.to_string());
            )*
            event
        }
    };
}

/// 追踪span宏 - 自动管理span生命周期
#[macro_export]
macro_rules! traced_span {
    ($tracer:expr, $event_type:expr, $event_name:expr, $code:block) => {{
        let mut event = $tracer.start_event($event_type, $event_name.to_string());
        let result = $code;
        event.finish();
        $tracer.finish_event(event);
        result
    }};
}

/// 获取当前请求的追踪ID
pub fn current_trace_id() -> Option<GlobalTraceId> {
    // 从当前上下文中获取追踪ID
    // 这里可以使用tokio本地存储或其他上下文存储机制
    None // 临时实现
}

/// 生成新的追踪ID
pub fn generate_trace_id() -> GlobalTraceId {
    format!("trace_{}", Uuid::new_v4().to_string().replace('-', ""))
}

/// 生成新的span ID
pub fn generate_span_id() -> String {
    format!(
        "span_{}",
        &Uuid::new_v4().to_string().replace('-', "")[0..16]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_event_creation() {
        let trace_id = generate_trace_id();
        let mut event = TraceEvent::new(
            trace_id.clone(),
            None,
            TraceEventType::HttpRequestStart,
            "test_request".to_string(),
        );

        assert_eq!(event.trace_id, trace_id);
        assert_eq!(event.event_name, "test_request");
        assert!(matches!(event.status, TracingStatus::Started));

        event.add_tag("method".to_string(), "POST".to_string());
        event.add_log("Request started".to_string());
        event.finish();

        assert!(matches!(event.status, TracingStatus::Completed));
        assert!(event.duration.is_some());
        assert!(event.tags.contains_key("method"));
        assert!(!event.logs.is_empty());
    }

    #[test]
    fn test_trace_id_generation() {
        let id1 = generate_trace_id();
        let id2 = generate_trace_id();

        assert_ne!(id1, id2);
        assert!(id1.starts_with("trace_"));
        assert!(id2.starts_with("trace_"));
    }

    #[test]
    fn test_span_id_generation() {
        let id1 = generate_span_id();
        let id2 = generate_span_id();

        assert_ne!(id1, id2);
        assert!(id1.starts_with("span_"));
        assert!(id2.starts_with("span_"));
    }
}
