
use crate::util::tracing::{
    generate_span_id, generate_trace_id, GlobalTraceId, TraceEvent, TraceEventType, TracingStatus,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

pub static DISTRIBUTED_TRACER: Lazy<Arc<DistributedTracer>> =
    Lazy::new(|| Arc::new(DistributedTracer::new(TraceConfig::default())));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    pub enabled: bool,
    pub sample_rate: f64,
    pub max_trace_retention: u64,
    pub max_in_memory_traces: usize,
    pub performance_monitoring: bool,
    pub error_tracking: bool,
    pub slow_request_threshold: u64,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_rate: 1.0,
            max_trace_retention: 3600,
            max_in_memory_traces: 10000,
            performance_monitoring: true,
            error_tracking: true,
            slow_request_threshold: 5000,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceChain {
    pub trace_id: GlobalTraceId,
    pub start_time: SystemTime,
    pub end_time: Option<SystemTime>,
    pub total_duration: Option<Duration>,
    pub events: Vec<TraceEvent>,
    pub root_span_id: String,
    pub status: TracingStatus,
    pub metadata: HashMap<String, String>,
    pub error_count: usize,
    pub warning_count: usize,
}

impl TraceChain {
    pub fn new(trace_id: GlobalTraceId) -> Self {
        Self {
            trace_id,
            start_time: SystemTime::now(),
            end_time: None,
            total_duration: None,
            events: Vec::new(),
            root_span_id: generate_span_id(),
            status: TracingStatus::Started,
            metadata: HashMap::new(),
            error_count: 0,
            warning_count: 0,
        }
    }

    pub fn add_event(&mut self, event: TraceEvent) {
        if matches!(event.status, TracingStatus::Error) {
            self.error_count += 1;
            self.status = TracingStatus::Error;
        }

        self.events.push(event);
    }

    pub fn finish(&mut self) {
        self.end_time = Some(SystemTime::now());
        if let Ok(duration) = self.end_time.unwrap().duration_since(self.start_time) {
            self.total_duration = Some(duration);
        }

        if matches!(
            self.status,
            TracingStatus::Started | TracingStatus::InProgress
        ) {
            self.status = TracingStatus::Completed;
        }
    }

    pub fn is_slow_request(&self, threshold_ms: u64) -> bool {
        self.total_duration
            .map(|d| d.as_millis() > threshold_ms as u128)
            .unwrap_or(false)
    }

    pub fn get_event_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();

        for event in &self.events {
            let event_type = format!("{:?}", event.event_type);
            *stats.entry(event_type).or_insert(0) += 1;
        }

        stats
    }
}

pub struct DistributedTracer {
    config: Arc<TraceConfig>,
    active_traces: Arc<RwLock<HashMap<GlobalTraceId, TraceChain>>>,
    completed_traces: Arc<Mutex<HashMap<GlobalTraceId, TraceChain>>>,
    performance_stats: Arc<RwLock<PerformanceStats>>,
}

#[derive(Debug, Clone)]
pub struct PerformanceStats {
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub avg_response_time: f64,
    pub max_response_time: u64,
    pub min_response_time: u64,
    pub slow_requests: usize,
    pub last_updated: SystemTime,
}

impl Default for PerformanceStats {
    fn default() -> Self {
        Self {
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            avg_response_time: 0.0,
            max_response_time: 0,
            min_response_time: 0,
            slow_requests: 0,
            last_updated: SystemTime::now(),
        }
    }
}

impl DistributedTracer {
    pub fn new(config: TraceConfig) -> Self {
        Self {
            config: Arc::new(config),
            active_traces: Arc::new(RwLock::new(HashMap::new())),
            completed_traces: Arc::new(Mutex::new(HashMap::new())),
            performance_stats: Arc::new(RwLock::new(PerformanceStats::default())),
        }
    }

    pub fn start_trace(&self, trace_id: Option<GlobalTraceId>) -> GlobalTraceId {
        if !self.config.enabled {
            return String::new();
        }

        if !self.should_sample() {
            return String::new();
        }

        let trace_id = trace_id.unwrap_or_else(generate_trace_id);
        let trace_chain = TraceChain::new(trace_id.clone());

        if let Ok(mut traces) = self.active_traces.write() {
            traces.insert(trace_id.clone(), trace_chain);
            debug!("Started new trace: {}", trace_id);
        }

        trace_id
    }

    pub fn start_event(
        &self,
        trace_id: &str,
        event_type: TraceEventType,
        event_name: String,
    ) -> Option<TraceEvent> {
        if !self.config.enabled || trace_id.is_empty() {
            return None;
        }

        let parent_span_id = self.get_current_span_id(trace_id);
        let mut event =
            TraceEvent::new(trace_id.to_string(), parent_span_id, event_type, event_name);

        event.add_tag("tracer".to_string(), "distributed_tracer".to_string());
        event.add_tag(
            "timestamp".to_string(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .to_string(),
        );

        debug!(
            "Started event: {} for trace: {}",
            event.event_name, trace_id
        );
        Some(event)
    }

    pub fn finish_event(&self, mut event: TraceEvent) {
        if !self.config.enabled {
            return;
        }

        event.finish();

        if let Some(duration) = event.duration {
            if duration.as_millis() > self.config.slow_request_threshold as u128 {
                warn!(
                    "Slow operation detected: {} took {}ms",
                    event.event_name,
                    duration.as_millis()
                );
            }
        }

        if let Ok(mut traces) = self.active_traces.write() {
            if let Some(trace_chain) = traces.get_mut(&event.trace_id) {
                trace_chain.add_event(event);
            }
        }
    }

    pub async fn finish_trace(&self, trace_id: &str) {
        if !self.config.enabled || trace_id.is_empty() {
            return;
        }

        let trace_chain = {
            let mut traces = match self.active_traces.write() {
                Ok(traces) => traces,
                Err(_) => return,
            };

            match traces.remove(trace_id) {
                Some(mut chain) => {
                    chain.finish();
                    chain
                }
                None => return,
            }
        };

        self.update_performance_stats(&trace_chain);

        let mut completed = self.completed_traces.lock().await;
        completed.insert(trace_id.to_string(), trace_chain.clone());

        self.cleanup_old_traces(&mut completed).await;

        info!(
            "Finished trace: {} (duration: {:?})",
            trace_id, trace_chain.total_duration
        );
    }

    pub async fn get_trace(&self, trace_id: &str) -> Option<TraceChain> {
        if let Ok(traces) = self.active_traces.read() {
            if let Some(trace) = traces.get(trace_id) {
                return Some(trace.clone());
            }
        }

        let completed = self.completed_traces.lock().await;
        completed.get(trace_id).cloned()
    }

    pub fn get_active_traces(&self) -> Vec<TraceChain> {
        if let Ok(traces) = self.active_traces.read() {
            traces.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    pub fn get_performance_stats(&self) -> PerformanceStats {
        if let Ok(stats) = self.performance_stats.read() {
            stats.clone()
        } else {
            PerformanceStats::default()
        }
    }

    pub async fn search_traces(&self, query: &TraceSearchQuery) -> Vec<TraceChain> {
        let mut results = Vec::new();
        let completed = self.completed_traces.lock().await;

        for trace in completed.values() {
            if self.matches_query(trace, query) {
                results.push(trace.clone());
            }
        }

        results.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        if results.len() > query.limit.unwrap_or(100) {
            results.truncate(query.limit.unwrap_or(100));
        }

        results
    }

    fn should_sample(&self) -> bool {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen::<f64>() < self.config.sample_rate
    }

    fn get_current_span_id(&self, trace_id: &str) -> Option<String> {
        if let Ok(traces) = self.active_traces.read() {
            traces
                .get(trace_id)
                .and_then(|chain| chain.events.last())
                .map(|event| event.span_id.clone())
        } else {
            None
        }
    }

    fn update_performance_stats(&self, trace_chain: &TraceChain) {
        if let Ok(mut stats) = self.performance_stats.write() {
            stats.total_requests += 1;

            match trace_chain.status {
                TracingStatus::Completed => stats.successful_requests += 1,
                TracingStatus::Error => stats.failed_requests += 1,
                _ => {}
            }

            if let Some(duration) = trace_chain.total_duration {
                let duration_ms = duration.as_millis() as u64;

                let total_time = stats.avg_response_time * (stats.total_requests - 1) as f64;
                stats.avg_response_time =
                    (total_time + duration_ms as f64) / stats.total_requests as f64;

                if stats.max_response_time == 0 || duration_ms > stats.max_response_time {
                    stats.max_response_time = duration_ms;
                }
                if stats.min_response_time == 0 || duration_ms < stats.min_response_time {
                    stats.min_response_time = duration_ms;
                }

                if trace_chain.is_slow_request(self.config.slow_request_threshold) {
                    stats.slow_requests += 1;
                }
            }

            stats.last_updated = SystemTime::now();
        }
    }

    async fn cleanup_old_traces(&self, completed: &mut HashMap<GlobalTraceId, TraceChain>) {
        let retention_duration = Duration::from_secs(self.config.max_trace_retention);
        let now = SystemTime::now();

        completed.retain(|_, trace| {
            trace
                .end_time
                .map(|end| now.duration_since(end).unwrap_or(Duration::ZERO) < retention_duration)
                .unwrap_or(true)
        });

        if completed.len() > self.config.max_in_memory_traces {
            let mut traces: Vec<_> = completed.drain().collect();
            traces.sort_by(|a, b| b.1.start_time.cmp(&a.1.start_time));
            traces.truncate(self.config.max_in_memory_traces);

            for (id, trace) in traces {
                completed.insert(id, trace);
            }
        }
    }

    fn matches_query(&self, trace: &TraceChain, query: &TraceSearchQuery) -> bool {
        if let Some(start) = query.start_time {
            if trace.start_time < start {
                return false;
            }
        }
        if let Some(end) = query.end_time {
            if trace.start_time > end {
                return false;
            }
        }

        if let Some(status) = &query.status {
            if &trace.status != status {
                return false;
            }
        }

        if let Some(min_duration) = query.min_duration {
            if trace.total_duration.unwrap_or(Duration::ZERO) < min_duration {
                return false;
            }
        }

        if query.has_errors && trace.error_count == 0 {
            return false;
        }

        true
    }
}

#[derive(Debug, Default)]
pub struct TraceSearchQuery {
    pub start_time: Option<SystemTime>,
    pub end_time: Option<SystemTime>,
    pub status: Option<TracingStatus>,
    pub min_duration: Option<Duration>,
    pub has_errors: bool,
    pub limit: Option<usize>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_distributed_tracer() {
        let tracer = DistributedTracer::new(TraceConfig::default());

        let trace_id = tracer.start_trace(None);
        assert!(!trace_id.is_empty());

        let mut event = tracer
            .start_event(
                &trace_id,
                TraceEventType::HttpRequestStart,
                "test_request".to_string(),
            )
            .unwrap();

        sleep(Duration::from_millis(10)).await;

        event.add_tag("method".to_string(), "POST".to_string());
        tracer.finish_event(event);

        tracer.finish_trace(&trace_id).await;

        let trace = tracer.get_trace(&trace_id).await.unwrap();
        assert_eq!(trace.events.len(), 1);
        assert!(matches!(trace.status, TracingStatus::Completed));
    }

    #[test]
    fn test_performance_stats() {
        let tracer = DistributedTracer::new(TraceConfig::default());
        let mut trace_chain = TraceChain::new("test_trace".to_string());
        trace_chain.total_duration = Some(Duration::from_millis(1000));
        trace_chain.status = TracingStatus::Completed;

        tracer.update_performance_stats(&trace_chain);

        let stats = tracer.get_performance_stats();
        assert_eq!(stats.total_requests, 1);
        assert_eq!(stats.successful_requests, 1);
        assert_eq!(stats.avg_response_time, 1000.0);
    }
}
