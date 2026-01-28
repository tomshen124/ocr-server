//! 分布式追踪核心实现
//! 负责管理请求链路追踪、性能监控和问题排查

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

/// 全局分布式追踪器实例
pub static DISTRIBUTED_TRACER: Lazy<Arc<DistributedTracer>> =
    Lazy::new(|| Arc::new(DistributedTracer::new(TraceConfig::default())));

/// 追踪配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceConfig {
    /// 是否启用追踪
    pub enabled: bool,
    /// 追踪采样率 (0.0-1.0)
    pub sample_rate: f64,
    /// 最大追踪保留时间（秒）
    pub max_trace_retention: u64,
    /// 最大内存中保留的追踪数量
    pub max_in_memory_traces: usize,
    /// 是否启用性能监控
    pub performance_monitoring: bool,
    /// 是否启用错误追踪
    pub error_tracking: bool,
    /// 慢请求阈值（毫秒）
    pub slow_request_threshold: u64,
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_rate: 1.0,          // 生产环境建议0.1
            max_trace_retention: 3600, // 1小时
            max_in_memory_traces: 10000,
            performance_monitoring: true,
            error_tracking: true,
            slow_request_threshold: 5000, // 5秒
        }
    }
}

/// 追踪链路
#[derive(Debug, Clone, Serialize)]
pub struct TraceChain {
    /// 追踪ID
    pub trace_id: GlobalTraceId,
    /// 开始时间
    pub start_time: SystemTime,
    /// 结束时间
    pub end_time: Option<SystemTime>,
    /// 总持续时间
    pub total_duration: Option<Duration>,
    /// 所有事件
    pub events: Vec<TraceEvent>,
    /// 根span ID
    pub root_span_id: String,
    /// 追踪状态
    pub status: TracingStatus,
    /// 请求元数据
    pub metadata: HashMap<String, String>,
    /// 错误计数
    pub error_count: usize,
    /// 警告计数
    pub warning_count: usize,
}

impl TraceChain {
    /// 创建新的追踪链路
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

    /// 添加事件
    pub fn add_event(&mut self, event: TraceEvent) {
        if matches!(event.status, TracingStatus::Error) {
            self.error_count += 1;
            self.status = TracingStatus::Error;
        }

        self.events.push(event);
    }

    /// 完成追踪
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

    /// 是否为慢请求
    pub fn is_slow_request(&self, threshold_ms: u64) -> bool {
        self.total_duration
            .map(|d| d.as_millis() > threshold_ms as u128)
            .unwrap_or(false)
    }

    /// 获取事件统计
    pub fn get_event_stats(&self) -> HashMap<String, usize> {
        let mut stats = HashMap::new();

        for event in &self.events {
            let event_type = format!("{:?}", event.event_type);
            *stats.entry(event_type).or_insert(0) += 1;
        }

        stats
    }
}

/// 分布式追踪器
pub struct DistributedTracer {
    /// 配置
    config: Arc<TraceConfig>,
    /// 活跃的追踪链路
    active_traces: Arc<RwLock<HashMap<GlobalTraceId, TraceChain>>>,
    /// 已完成的追踪链路（用于查询）
    completed_traces: Arc<Mutex<HashMap<GlobalTraceId, TraceChain>>>,
    /// 性能统计
    performance_stats: Arc<RwLock<PerformanceStats>>,
}

/// 性能统计
#[derive(Debug, Clone)]
pub struct PerformanceStats {
    /// 总请求数
    pub total_requests: usize,
    /// 成功请求数
    pub successful_requests: usize,
    /// 失败请求数
    pub failed_requests: usize,
    /// 平均响应时间（毫秒）
    pub avg_response_time: f64,
    /// 最大响应时间（毫秒）
    pub max_response_time: u64,
    /// 最小响应时间（毫秒）
    pub min_response_time: u64,
    /// 慢请求数
    pub slow_requests: usize,
    /// 最后更新时间
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
    /// 创建新的分布式追踪器
    pub fn new(config: TraceConfig) -> Self {
        Self {
            config: Arc::new(config),
            active_traces: Arc::new(RwLock::new(HashMap::new())),
            completed_traces: Arc::new(Mutex::new(HashMap::new())),
            performance_stats: Arc::new(RwLock::new(PerformanceStats::default())),
        }
    }

    /// 开始新的追踪
    pub fn start_trace(&self, trace_id: Option<GlobalTraceId>) -> GlobalTraceId {
        if !self.config.enabled {
            return String::new();
        }

        // 采样检查
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

    /// 开始新的事件
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

        // 添加默认标签
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

    /// 完成事件
    pub fn finish_event(&self, mut event: TraceEvent) {
        if !self.config.enabled {
            return;
        }

        event.finish();

        // 记录性能数据
        if let Some(duration) = event.duration {
            if duration.as_millis() > self.config.slow_request_threshold as u128 {
                warn!(
                    "Slow operation detected: {} took {}ms",
                    event.event_name,
                    duration.as_millis()
                );
            }
        }

        // 添加到追踪链路
        if let Ok(mut traces) = self.active_traces.write() {
            if let Some(trace_chain) = traces.get_mut(&event.trace_id) {
                trace_chain.add_event(event);
            }
        }
    }

    /// 完成追踪
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

        // 更新性能统计
        self.update_performance_stats(&trace_chain);

        // 移动到已完成追踪
        let mut completed = self.completed_traces.lock().await;
        completed.insert(trace_id.to_string(), trace_chain.clone());

        // 清理过期的追踪
        self.cleanup_old_traces(&mut completed).await;

        info!(
            "Finished trace: {} (duration: {:?})",
            trace_id, trace_chain.total_duration
        );
    }

    /// 获取追踪信息
    pub async fn get_trace(&self, trace_id: &str) -> Option<TraceChain> {
        // 先检查活跃追踪
        if let Ok(traces) = self.active_traces.read() {
            if let Some(trace) = traces.get(trace_id) {
                return Some(trace.clone());
            }
        }

        // 再检查已完成追踪
        let completed = self.completed_traces.lock().await;
        completed.get(trace_id).cloned()
    }

    /// 获取所有活跃追踪
    pub fn get_active_traces(&self) -> Vec<TraceChain> {
        if let Ok(traces) = self.active_traces.read() {
            traces.values().cloned().collect()
        } else {
            Vec::new()
        }
    }

    /// 获取性能统计
    pub fn get_performance_stats(&self) -> PerformanceStats {
        if let Ok(stats) = self.performance_stats.read() {
            stats.clone()
        } else {
            PerformanceStats::default()
        }
    }

    /// 搜索追踪
    pub async fn search_traces(&self, query: &TraceSearchQuery) -> Vec<TraceChain> {
        let mut results = Vec::new();
        let completed = self.completed_traces.lock().await;

        for trace in completed.values() {
            if self.matches_query(trace, query) {
                results.push(trace.clone());
            }
        }

        // 按时间倒序排序
        results.sort_by(|a, b| b.start_time.cmp(&a.start_time));

        // 限制结果数量
        if results.len() > query.limit.unwrap_or(100) {
            results.truncate(query.limit.unwrap_or(100));
        }

        results
    }

    /// 检查是否应该采样
    fn should_sample(&self) -> bool {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        rng.gen::<f64>() < self.config.sample_rate
    }

    /// 获取当前span ID
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

    /// 更新性能统计
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

                // 更新平均响应时间
                let total_time = stats.avg_response_time * (stats.total_requests - 1) as f64;
                stats.avg_response_time =
                    (total_time + duration_ms as f64) / stats.total_requests as f64;

                // 更新最大最小响应时间
                if stats.max_response_time == 0 || duration_ms > stats.max_response_time {
                    stats.max_response_time = duration_ms;
                }
                if stats.min_response_time == 0 || duration_ms < stats.min_response_time {
                    stats.min_response_time = duration_ms;
                }

                // 检查慢请求
                if trace_chain.is_slow_request(self.config.slow_request_threshold) {
                    stats.slow_requests += 1;
                }
            }

            stats.last_updated = SystemTime::now();
        }
    }

    /// 清理过期追踪
    async fn cleanup_old_traces(&self, completed: &mut HashMap<GlobalTraceId, TraceChain>) {
        let retention_duration = Duration::from_secs(self.config.max_trace_retention);
        let now = SystemTime::now();

        // 按时间清理
        completed.retain(|_, trace| {
            trace
                .end_time
                .map(|end| now.duration_since(end).unwrap_or(Duration::ZERO) < retention_duration)
                .unwrap_or(true)
        });

        // 按数量清理
        if completed.len() > self.config.max_in_memory_traces {
            let mut traces: Vec<_> = completed.drain().collect();
            traces.sort_by(|a, b| b.1.start_time.cmp(&a.1.start_time));
            traces.truncate(self.config.max_in_memory_traces);

            for (id, trace) in traces {
                completed.insert(id, trace);
            }
        }
    }

    /// 检查追踪是否匹配查询条件
    fn matches_query(&self, trace: &TraceChain, query: &TraceSearchQuery) -> bool {
        // 时间范围检查
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

        // 状态检查
        if let Some(status) = &query.status {
            if &trace.status != status {
                return false;
            }
        }

        // 持续时间检查
        if let Some(min_duration) = query.min_duration {
            if trace.total_duration.unwrap_or(Duration::ZERO) < min_duration {
                return false;
            }
        }

        // 错误检查
        if query.has_errors && trace.error_count == 0 {
            return false;
        }

        true
    }
}

/// 追踪搜索查询
#[derive(Debug, Default)]
pub struct TraceSearchQuery {
    /// 开始时间
    pub start_time: Option<SystemTime>,
    /// 结束时间
    pub end_time: Option<SystemTime>,
    /// 状态过滤
    pub status: Option<TracingStatus>,
    /// 最小持续时间
    pub min_duration: Option<Duration>,
    /// 只包含有错误的追踪
    pub has_errors: bool,
    /// 结果限制
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
