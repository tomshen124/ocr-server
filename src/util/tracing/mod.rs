
pub mod distributed_tracing;
pub mod metrics_collector;
pub mod middleware;
pub mod request_tracker;
pub mod span_context;

pub use distributed_tracing::{DistributedTracer, TraceConfig, DISTRIBUTED_TRACER};
pub use metrics_collector::{MetricsCollector, RequestMetrics};
pub use middleware::{TracingLayer, TracingMiddleware};
pub use request_tracker::{RequestSpan, RequestTracker, TraceStatus};
pub use span_context::{SpanContext, SpanId, TraceId};

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub type GlobalTraceId = String;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum TracingStatus {
    Started,
    InProgress,
    Completed,
    Error,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraceEventType {
    HttpRequestStart,
    HttpRequestEnd,
    DatabaseOpStart,
    DatabaseOpEnd,
    OcrProcessStart,
    OcrProcessEnd,
    FileDownloadStart,
    FileDownloadEnd,
    StorageOpStart,
    StorageOpEnd,
    Custom(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
    pub event_id: String,
    pub trace_id: GlobalTraceId,
    pub parent_span_id: Option<String>,
    pub span_id: String,
    pub event_type: TraceEventType,
    pub event_name: String,
    #[serde(skip)]
    pub start_time: Instant,
    #[serde(skip)]
    pub end_time: Option<Instant>,
    pub duration: Option<Duration>,
    pub status: TracingStatus,
    pub tags: std::collections::HashMap<String, String>,
    pub logs: Vec<String>,
    pub error: Option<String>,
}

impl TraceEvent {
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

    pub fn add_tag(&mut self, key: String, value: String) {
        self.tags.insert(key, value);
    }

    pub fn add_log(&mut self, message: String) {
        self.logs
            .push(format!("[{}] {}", chrono::Utc::now().to_rfc3339(), message));
    }

    pub fn set_error(&mut self, error: String) {
        self.status = TracingStatus::Error;
        self.error = Some(error.clone());
        self.add_log(format!("Error: {}", error));
        self.finish();
    }
}

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

pub fn current_trace_id() -> Option<GlobalTraceId> {
    None
}

pub fn generate_trace_id() -> GlobalTraceId {
    format!("trace_{}", Uuid::new_v4().to_string().replace('-', ""))
}

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
