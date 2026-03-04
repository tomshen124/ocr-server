
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracingConfig {
    pub enabled: bool,
    pub sampling_rate: f64,
    pub max_spans: usize,
    pub retention_seconds: u64,
    pub verbose_logging: bool,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sampling_rate: 1.0,
            max_spans: 10000,
            retention_seconds: 3600,
            verbose_logging: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceContext {
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub span_id: String,
    pub user_id: Option<String>,
    pub request_id: Option<String>,
}

impl TraceContext {
    pub fn new_root() -> Self {
        Self {
            trace_id: Uuid::new_v4().to_string(),
            parent_span_id: None,
            span_id: Uuid::new_v4().to_string(),
            user_id: None,
            request_id: None,
        }
    }

    pub fn create_child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            parent_span_id: Some(self.span_id.clone()),
            span_id: Uuid::new_v4().to_string(),
            user_id: self.user_id.clone(),
            request_id: self.request_id.clone(),
        }
    }

    pub fn with_user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn with_request_id(mut self, request_id: String) -> Self {
        self.request_id = Some(request_id);
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SpanStatus {
    InProgress,
    Success,
    Error,
    Timeout,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Span ID
    pub span_id: String,
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub operation_name: String,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub status: SpanStatus,
    pub tags: HashMap<String, String>,
    pub logs: Vec<SpanLog>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLog {
    pub timestamp: u64,
    pub level: String,
    pub message: String,
    pub fields: HashMap<String, String>,
}

impl Span {
    pub fn new(context: &TraceContext, operation_name: String) -> Self {
        Self {
            span_id: context.span_id.clone(),
            trace_id: context.trace_id.clone(),
            parent_span_id: context.parent_span_id.clone(),
            operation_name,
            start_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            end_time: None,
            status: SpanStatus::InProgress,
            tags: HashMap::new(),
            logs: Vec::new(),
            duration_ms: None,
        }
    }

    pub fn add_tag(&mut self, key: String, value: String) {
        self.tags.insert(key, value);
    }

    pub fn add_log(&mut self, level: String, message: String, fields: HashMap<String, String>) {
        self.logs.push(SpanLog {
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            level,
            message,
            fields,
        });
    }

    pub fn finish(&mut self, status: SpanStatus) {
        let end_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        
        self.end_time = Some(end_time);
        self.status = status;
        self.duration_ms = Some(end_time - self.start_time);
    }
}

#[derive(Debug)]
pub struct TracingManager {
    config: TracingConfig,
    spans: Arc<RwLock<HashMap<String, Span>>>,
    traces: Arc<RwLock<HashMap<String, Vec<String>>>>, // trace_id -> span_ids
}

impl TracingManager {
    pub fn new(config: TracingConfig) -> Self {
        Self {
            config,
            spans: Arc::new(RwLock::new(HashMap::new())),
            traces: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn should_sample(&self) -> bool {
        if !self.config.enabled {
            return false;
        }
        
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen::<f64>() < self.config.sampling_rate
    }

    pub async fn start_span(&self, context: &TraceContext, operation_name: String) -> Option<String> {
        if !self.should_sample() {
            return None;
        }

        let mut span = Span::new(context, operation_name);
        
        if let Some(user_id) = &context.user_id {
            span.add_tag("user.id".to_string(), user_id.clone());
        }
        if let Some(request_id) = &context.request_id {
            span.add_tag("request.id".to_string(), request_id.clone());
        }

        let span_id = span.span_id.clone();
        let trace_id = span.trace_id.clone();

        {
            let mut spans = self.spans.write().await;
            spans.insert(span_id.clone(), span);
        }

        {
            let mut traces = self.traces.write().await;
            traces.entry(trace_id.clone()).or_insert_with(Vec::new).push(span_id.clone());
        }

        if self.config.verbose_logging {
            info!(
                target: "tracing.span",
                event = "span.start",
                span_id = %span_id,
                trace_id = %trace_id
            );
        }

        Some(span_id)
    }

    pub async fn finish_span(&self, span_id: &str, status: SpanStatus) {
        let mut spans = self.spans.write().await;
        if let Some(span) = spans.get_mut(span_id) {
            span.finish(status);
            
            if self.config.verbose_logging {
                info!(
                    target: "tracing.span",
                    event = "span.finish",
                    span_id = %span_id,
                    duration_ms = span.duration_ms.unwrap_or(0)
                );
            }
        }
    }

    pub async fn add_span_tag(&self, span_id: &str, key: String, value: String) {
        let mut spans = self.spans.write().await;
        if let Some(span) = spans.get_mut(span_id) {
            span.add_tag(key, value);
        }
    }

    pub async fn add_span_log(&self, span_id: &str, level: String, message: String, fields: HashMap<String, String>) {
        let mut spans = self.spans.write().await;
        if let Some(span) = spans.get_mut(span_id) {
            span.add_log(level, message, fields);
        }
    }

    pub async fn get_trace(&self, trace_id: &str) -> Option<Vec<Span>> {
        let traces = self.traces.read().await;
        let spans_lock = self.spans.read().await;
        
        if let Some(span_ids) = traces.get(trace_id) {
            let mut spans = Vec::new();
            for span_id in span_ids {
                if let Some(span) = spans_lock.get(span_id) {
                    spans.push(span.clone());
                }
            }
            Some(spans)
        } else {
            None
        }
    }

    pub async fn get_active_traces(&self) -> Vec<String> {
        let traces = self.traces.read().await;
        let spans_lock = self.spans.read().await;
        
        traces.keys()
            .filter(|trace_id| {
                if let Some(span_ids) = traces.get(*trace_id) {
                    span_ids.iter().any(|span_id| {
                        if let Some(span) = spans_lock.get(span_id) {
                            matches!(span.status, SpanStatus::InProgress)
                        } else {
                            false
                        }
                    })
                } else {
                    false
                }
            })
            .cloned()
            .collect()
    }

    pub async fn cleanup_expired(&self) {
        let retention_ms = self.config.retention_seconds * 1000;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut spans_to_remove = Vec::new();
        let mut traces_to_remove = Vec::new();

        {
            let spans = self.spans.read().await;
            for (span_id, span) in spans.iter() {
                if let Some(end_time) = span.end_time {
                    if now - end_time > retention_ms {
                        spans_to_remove.push(span_id.clone());
                    }
                }
            }
        }

        {
            let mut spans = self.spans.write().await;
            for span_id in &spans_to_remove {
                spans.remove(span_id);
            }
        }

        {
            let mut traces = self.traces.write().await;
            let spans = self.spans.read().await;
            
            for (trace_id, span_ids) in traces.iter() {
                let has_active_spans = span_ids.iter().any(|span_id| spans.contains_key(span_id));
                if !has_active_spans {
                    traces_to_remove.push(trace_id.clone());
                }
            }
            
            for trace_id in traces_to_remove {
                traces.remove(&trace_id);
            }
        }

        if !spans_to_remove.is_empty() {
            info!(
                target: "tracing.span",
                event = "span.cleanup",
                removed = spans_to_remove.len()
            );
        }
    }

    pub async fn get_stats(&self) -> TracingStats {
        let spans = self.spans.read().await;
        let traces = self.traces.read().await;
        
        let total_spans = spans.len();
        let total_traces = traces.len();
        
        let mut active_spans = 0;
        let mut completed_spans = 0;
        let mut error_spans = 0;
        
        for span in spans.values() {
            match span.status {
                SpanStatus::InProgress => active_spans += 1,
                SpanStatus::Success => completed_spans += 1,
                SpanStatus::Error | SpanStatus::Timeout => error_spans += 1,
            }
        }

        TracingStats {
            total_spans,
            total_traces,
            active_spans,
            completed_spans,
            error_spans,
            config: self.config.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TracingStats {
    pub total_spans: usize,
    pub total_traces: usize,
    pub active_spans: usize,
    pub completed_spans: usize,
    pub error_spans: usize,
    pub config: TracingConfig,
}

static TRACING_MANAGER: std::sync::OnceLock<TracingManager> = std::sync::OnceLock::new();

pub fn init_tracing(config: TracingConfig) {
    let manager = TracingManager::new(config);
    if TRACING_MANAGER.set(manager).is_err() {
        warn!("Tracing manager already initialized");
    }
}

pub fn get_tracing_manager() -> Option<&'static TracingManager> {
    TRACING_MANAGER.get()
}

#[macro_export]
macro_rules! trace_span {
    ($context:expr, $operation:expr) => {
        if let Some(manager) = $crate::util::tracing::get_tracing_manager() {
            manager.start_span($context, $operation.to_string()).await
        } else {
            None
        }
    };
}

pub mod middleware {
    use super::*;
    use axum::{
        extract::Request,
        http::{HeaderMap, StatusCode},
        middleware::Next,
        response::Response,
    };

    pub async fn tracing_middleware(
        headers: HeaderMap,
        mut request: Request,
        next: Next,
    ) -> Result<Response, StatusCode> {
        let context = extract_or_create_context(&headers);
        
        request.extensions_mut().insert(context.clone());
        
        if let Some(manager) = get_tracing_manager() {
            let span_id = manager.start_span(&context, "http_request".to_string()).await;
            
            if let Some(span_id) = span_id {
                manager.add_span_tag(&span_id, "http.method".to_string(), request.method().to_string()).await;
                manager.add_span_tag(&span_id, "http.path".to_string(), request.uri().path().to_string()).await;
                
                let start_time = std::time::Instant::now();
                let response = next.run(request).await;
                let duration = start_time.elapsed();
                
                manager.add_span_tag(&span_id, "http.status_code".to_string(), response.status().as_u16().to_string()).await;
                manager.add_span_tag(&span_id, "duration_ms".to_string(), duration.as_millis().to_string()).await;
                
                let status = if response.status().is_success() {
                    SpanStatus::Success
                } else {
                    SpanStatus::Error
                };
                
                manager.finish_span(&span_id, status).await;
                
                Ok(response)
            } else {
                Ok(next.run(request).await)
            }
        } else {
            Ok(next.run(request).await)
        }
    }

    fn extract_or_create_context(headers: &HeaderMap) -> TraceContext {
        if let (Some(trace_id), Some(parent_span_id)) = (
            headers.get("X-Trace-Id").and_then(|v| v.to_str().ok()),
            headers.get("X-Span-Id").and_then(|v| v.to_str().ok()),
        ) {
            TraceContext {
                trace_id: trace_id.to_string(),
                parent_span_id: Some(parent_span_id.to_string()),
                span_id: Uuid::new_v4().to_string(),
                user_id: headers.get("X-User-Id").and_then(|v| v.to_str().ok().map(|s| s.to_string())),
                request_id: headers.get("X-Request-Id").and_then(|v| v.to_str().ok().map(|s| s.to_string())),
            }
        } else {
            TraceContext::new_root()
                .with_user_id(
                    headers.get("X-User-Id")
                        .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
                        .unwrap_or_else(|| "anonymous".to_string())
                )
                .with_request_id(
                    headers.get("X-Request-Id")
                        .and_then(|v| v.to_str().ok().map(|s| s.to_string()))
                        .unwrap_or_else(|| Uuid::new_v4().to_string())
                )
        }
    }
}
