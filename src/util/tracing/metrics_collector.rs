
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

pub static METRICS_COLLECTOR: Lazy<Arc<MetricsCollector>> =
    Lazy::new(|| Arc::new(MetricsCollector::new(MetricsConfig::default())));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    pub enabled: bool,
    pub aggregation_interval: u64,
    pub retention_period: u64,
    pub max_metrics_in_memory: usize,
    pub enable_detailed_metrics: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            aggregation_interval: 60,
            retention_period: 3600,
            max_metrics_in_memory: 10000,
            enable_detailed_metrics: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
    Summary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricValue {
    Integer(i64),
    Float(f64),
    Histogram {
        buckets: Vec<HistogramBucket>,
        sum: f64,
        count: u64,
    },
    Summary {
        sum: f64,
        count: u64,
        quantiles: Vec<Quantile>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramBucket {
    pub upper_bound: f64,
    pub cumulative_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quantile {
    pub quantile: f64,
    pub value: f64,
}

pub type MetricLabels = HashMap<String, String>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub name: String,
    pub metric_type: MetricType,
    pub value: MetricValue,
    pub labels: MetricLabels,
    pub timestamp: u64,
    pub help: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMetrics {
    pub request_count: u64,
    pub success_count: u64,
    pub error_count: u64,
    pub total_response_time: f64,
    pub avg_response_time: f64,
    pub min_response_time: f64,
    pub max_response_time: f64,
    pub p95_response_time: f64,
    pub p99_response_time: f64,
    pub throughput: f64,
    pub error_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrMetrics {
    pub ocr_count: u64,
    pub ocr_success_count: u64,
    pub ocr_error_count: u64,
    pub total_processing_time: f64,
    pub avg_processing_time: f64,
    pub pages_processed: u64,
    pub characters_recognized: u64,
    pub avg_confidence: f64,
    pub total_file_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub disk_usage: f64,
    pub network_in: u64,
    pub network_out: u64,
    pub active_connections: u64,
    pub thread_count: u64,
    pub fd_usage: u64,
    pub nats_connection_failures: u64,
    pub nats_connection_timeouts: u64,
    pub worker_heartbeat_failures: u64,
    pub worker_heartbeat_timeouts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessMetrics {
    pub user_count: u64,
    pub active_users: u64,
    pub preview_requests: u64,
    pub preview_success: u64,
    pub preview_failures: u64,
    pub preview_download_failures: u64,
    pub preview_ocr_timeouts: u64,
    pub preview_persistence_failures: u64,
    pub file_uploads: u64,
    pub storage_usage: u64,
    pub rule_executions: u64,
    pub rule_matches: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineStageStats {
    pub success_count: u64,
    pub failure_count: u64,
    pub total_duration_ms: f64,
    pub max_duration_ms: f64,
    pub last_success_ts: Option<u64>,
    pub last_failure_ts: Option<u64>,
    pub last_error: Option<String>,
}

impl Default for PipelineStageStats {
    fn default() -> Self {
        Self {
            success_count: 0,
            failure_count: 0,
            total_duration_ms: 0.0,
            max_duration_ms: 0.0,
            last_success_ts: None,
            last_failure_ts: None,
            last_error: None,
        }
    }
}

pub struct MetricsCollector {
    config: Arc<MetricsConfig>,
    samples: Arc<RwLock<Vec<MetricSample>>>,
    aggregated_metrics: Arc<Mutex<HashMap<String, MetricSample>>>,
    request_metrics: Arc<RwLock<RequestMetrics>>,
    ocr_metrics: Arc<RwLock<OcrMetrics>>,
    system_metrics: Arc<RwLock<SystemMetrics>>,
    business_metrics: Arc<RwLock<BusinessMetrics>>,
    pipeline_metrics: Arc<RwLock<HashMap<String, PipelineStageStats>>>,
    response_time_samples: Arc<RwLock<Vec<f64>>>,
}

impl MetricsCollector {
    pub fn new(config: MetricsConfig) -> Self {
        Self {
            config: Arc::new(config),
            samples: Arc::new(RwLock::new(Vec::new())),
            aggregated_metrics: Arc::new(Mutex::new(HashMap::new())),
            request_metrics: Arc::new(RwLock::new(RequestMetrics::default())),
            ocr_metrics: Arc::new(RwLock::new(OcrMetrics::default())),
            system_metrics: Arc::new(RwLock::new(SystemMetrics::default())),
            business_metrics: Arc::new(RwLock::new(BusinessMetrics::default())),
            pipeline_metrics: Arc::new(RwLock::new(HashMap::new())),
            response_time_samples: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub fn record(&self, sample: MetricSample) {
        if !self.config.enabled {
            return;
        }

        if let Ok(mut samples) = self.samples.write() {
            samples.push(sample);

            if samples.len() > self.config.max_metrics_in_memory {
                let excess = samples.len() - self.config.max_metrics_in_memory;
                samples.drain(0..excess);
            }
        }
    }

    pub fn record_counter(&self, name: &str, value: i64, labels: MetricLabels) {
        let sample = MetricSample {
            name: name.to_string(),
            metric_type: MetricType::Counter,
            value: MetricValue::Integer(value),
            labels,
            timestamp: current_timestamp(),
            help: None,
        };
        self.record(sample);
    }

    pub fn record_gauge(&self, name: &str, value: f64, labels: MetricLabels) {
        let sample = MetricSample {
            name: name.to_string(),
            metric_type: MetricType::Gauge,
            value: MetricValue::Float(value),
            labels,
            timestamp: current_timestamp(),
            help: None,
        };
        self.record(sample);
    }

    pub fn record_histogram(&self, name: &str, value: f64, labels: MetricLabels) {
        let buckets = vec![
            10.0, 25.0, 50.0, 100.0, 250.0, 500.0, 1000.0, 2500.0, 5000.0, 10000.0,
        ];

        let histogram_buckets: Vec<HistogramBucket> = buckets
            .iter()
            .map(|&upper_bound| HistogramBucket {
                upper_bound,
                cumulative_count: if value <= upper_bound { 1 } else { 0 },
            })
            .collect();

        let sample = MetricSample {
            name: name.to_string(),
            metric_type: MetricType::Histogram,
            value: MetricValue::Histogram {
                buckets: histogram_buckets,
                sum: value,
                count: 1,
            },
            labels,
            timestamp: current_timestamp(),
            help: None,
        };
        self.record(sample);
    }

    pub fn record_http_request(
        &self,
        method: &str,
        path: &str,
        status_code: u16,
        duration: Duration,
    ) {
        let duration_ms = duration.as_millis() as f64;

        if let Ok(mut metrics) = self.request_metrics.write() {
            metrics.request_count += 1;

            if status_code < 400 {
                metrics.success_count += 1;
            } else {
                metrics.error_count += 1;
            }

            metrics.total_response_time += duration_ms;
            metrics.avg_response_time = metrics.total_response_time / metrics.request_count as f64;

            if metrics.min_response_time == 0.0 || duration_ms < metrics.min_response_time {
                metrics.min_response_time = duration_ms;
            }
            if duration_ms > metrics.max_response_time {
                metrics.max_response_time = duration_ms;
            }

            metrics.error_rate = metrics.error_count as f64 / metrics.request_count as f64;
        }

        if let Ok(mut samples) = self.response_time_samples.write() {
            samples.push(duration_ms);

            if samples.len() > 1000 {
                samples.drain(0..500);
            }
        }

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert("method".to_string(), method.to_string());
            labels.insert("path".to_string(), path.to_string());
            labels.insert("status_code".to_string(), status_code.to_string());

            self.record_counter("http_requests_total", 1, labels.clone());
            self.record_histogram("http_request_duration_ms", duration_ms, labels);
        }
    }

    pub fn record_ocr_processing(
        &self,
        success: bool,
        duration: Duration,
        pages: u32,
        characters: u64,
        confidence: f64,
        file_size: u64,
    ) {
        if let Ok(mut metrics) = self.ocr_metrics.write() {
            metrics.ocr_count += 1;

            if success {
                metrics.ocr_success_count += 1;
            } else {
                metrics.ocr_error_count += 1;
            }

            let duration_ms = duration.as_millis() as f64;
            metrics.total_processing_time += duration_ms;
            metrics.avg_processing_time = metrics.total_processing_time / metrics.ocr_count as f64;
            metrics.pages_processed += pages as u64;
            metrics.characters_recognized += characters;

            if confidence > 0.0 {
                let total_confidence =
                    metrics.avg_confidence * (metrics.ocr_count - 1) as f64 + confidence;
                metrics.avg_confidence = total_confidence / metrics.ocr_count as f64;
            }

            metrics.total_file_size += file_size;
        }

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert("success".to_string(), success.to_string());

            self.record_counter("ocr_processing_total", 1, labels.clone());
            self.record_histogram(
                "ocr_processing_duration_ms",
                duration.as_millis() as f64,
                labels.clone(),
            );
            self.record_gauge("ocr_confidence", confidence, labels);
        }
    }

    pub fn record_ocr_invocation(&self, success: bool, duration: Duration) {
        if let Ok(mut metrics) = self.ocr_metrics.write() {
            metrics.ocr_count += 1;
            if success {
                metrics.ocr_success_count += 1;
            } else {
                metrics.ocr_error_count += 1;
            }
            let duration_ms = duration.as_secs_f64() * 1000.0;
            metrics.total_processing_time += duration_ms;
            metrics.avg_processing_time = metrics.total_processing_time / metrics.ocr_count as f64;
        }

        let mut labels = HashMap::new();
        labels.insert(
            "status".to_string(),
            if success {
                "success".to_string()
            } else {
                "failure".to_string()
            },
        );

        self.record_counter("ocr_calls_total", 1, labels.clone());
        if !success {
            self.record_counter("ocr_failures_total", 1, labels.clone());
        }
        self.record_histogram("ocr_duration_seconds", duration.as_secs_f64(), labels);
    }

    pub fn record_preview_request(&self, status_code: u16, reason: &str, duration: Duration) {
        self.record_http_request("POST", "/api/preview", status_code, duration);
        self.record_business_metric("preview_requests", 1);

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert("endpoint".to_string(), "/api/preview".to_string());
            labels.insert("status".to_string(), status_code.to_string());
            labels.insert("reason".to_string(), reason.to_string());

            self.record_counter("preview_requests_total", 1, labels.clone());
            self.record_histogram(
                "preview_request_duration_ms",
                duration.as_secs_f64() * 1000.0,
                labels,
            );
        }
    }

    pub fn record_preview_download(&self, success: bool, duration: Duration, source: &str) {
        if !success {
            self.record_business_metric("preview_download_failures", 1);
        }

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert("source".to_string(), source.to_string());
            labels.insert(
                "status".to_string(),
                if success { "success" } else { "failure" }.to_string(),
            );

            self.record_histogram(
                "preview_download_duration_ms",
                duration.as_secs_f64() * 1000.0,
                labels.clone(),
            );

            if !success {
                self.record_counter("preview_download_failures_total", 1, labels);
            }
        }
    }

    pub fn record_preview_ocr_timeout(&self, material: &str) {
        self.record_business_metric("preview_ocr_timeouts", 1);

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert("material".to_string(), material.to_string());
            self.record_counter("preview_ocr_timeouts_total", 1, labels);
        }
    }

    pub fn record_preview_persistence_failure(&self, stage: &str) {
        self.record_business_metric("preview_persistence_failures", 1);

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert("stage".to_string(), stage.to_string());
            self.record_counter("preview_persistence_failures_total", 1, labels);
        }
    }

    pub fn record_preview_rule_execution(&self, duration: Duration, success: bool) {
        self.record_business_metric("rule_executions", 1);

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert(
                "status".to_string(),
                if success { "success" } else { "failure" }.to_string(),
            );
            self.record_histogram(
                "preview_rule_execution_duration_ms",
                duration.as_secs_f64() * 1000.0,
                labels,
            );
        }
    }

    pub fn record_preview_job(&self, success: bool, duration: Duration) {
        if success {
            self.record_business_metric("preview_success", 1);
        } else {
            self.record_business_metric("preview_failures", 1);
        }

        if self.config.enable_detailed_metrics {
            let mut labels = HashMap::new();
            labels.insert(
                "status".to_string(),
                if success { "success" } else { "failure" }.to_string(),
            );
            self.record_histogram(
                "preview_job_duration_ms",
                duration.as_secs_f64() * 1000.0,
                labels,
            );
        }
    }

    pub fn record_queue_depth(&self, queue: &str, depth: u64) {
        if !self.config.enable_detailed_metrics {
            return;
        }

        let mut labels = HashMap::new();
        labels.insert("queue".to_string(), queue.to_string());
        self.record_gauge("queue_depth", depth as f64, labels);
    }

    pub fn record_queue_enqueue(&self, queue: &str, depth: Option<u64>) {
        if !self.config.enable_detailed_metrics {
            return;
        }

        let mut labels = HashMap::new();
        labels.insert("queue".to_string(), queue.to_string());
        self.record_counter("queue_enqueue_total", 1, labels.clone());

        if let Some(depth) = depth {
            self.record_queue_depth(queue, depth);
        }
    }

    pub fn record_queue_dequeue(&self, queue: &str, success: bool, depth: Option<u64>) {
        if !self.config.enable_detailed_metrics {
            return;
        }

        let mut labels = HashMap::new();
        labels.insert("queue".to_string(), queue.to_string());
        labels.insert(
            "status".to_string(),
            if success { "success" } else { "failure" }.to_string(),
        );
        self.record_counter("queue_processed_total", 1, labels);

        if let Some(depth) = depth {
            self.record_queue_depth(queue, depth);
        }
    }

    pub fn record_queue_retry(&self, queue: &str) {
        if !self.config.enable_detailed_metrics {
            return;
        }

        let mut labels = HashMap::new();
        labels.insert("queue".to_string(), queue.to_string());
        self.record_counter("queue_retry_total", 1, labels);
    }

    pub fn record_worker_inflight(&self, worker: &str, inflight: u64) {
        if !self.config.enable_detailed_metrics {
            return;
        }

        let mut labels = HashMap::new();
        labels.insert("worker".to_string(), worker.to_string());
        self.record_gauge("worker_inflight_tasks", inflight as f64, labels);
    }

    pub fn record_pipeline_stage(
        &self,
        stage: &str,
        success: bool,
        duration: Duration,
        extra_labels: Option<MetricLabels>,
        error_reason: Option<&str>,
    ) {
        let duration_ms = duration.as_secs_f64() * 1000.0;
        let now = current_timestamp();

        if let Ok(mut stages) = self.pipeline_metrics.write() {
            let stats = stages
                .entry(stage.to_string())
                .or_insert_with(PipelineStageStats::default);
            stats.total_duration_ms += duration_ms;
            if duration_ms > stats.max_duration_ms {
                stats.max_duration_ms = duration_ms;
            }
            if success {
                stats.success_count += 1;
                stats.last_success_ts = Some(now);
            } else {
                stats.failure_count += 1;
                stats.last_failure_ts = Some(now);
                if let Some(reason) = error_reason {
                    stats.last_error = Some(reason.to_string());
                }
            }
        }

        if self.config.enable_detailed_metrics {
            let mut labels = extra_labels.unwrap_or_default();
            labels.insert("stage".to_string(), stage.to_string());
            labels.insert(
                "status".to_string(),
                if success {
                    "success".to_string()
                } else {
                    "failure".to_string()
                },
            );

            self.record_counter("ocr_pipeline_stage_total", 1, labels.clone());
            self.record_histogram(
                "ocr_pipeline_stage_duration_ms",
                duration_ms,
                labels.clone(),
            );

            if let Some(reason) = error_reason {
                let mut error_labels = labels;
                error_labels.insert("reason".to_string(), reason.to_string());
                self.record_counter("ocr_pipeline_stage_errors_total", 1, error_labels);
            }
        }
    }

    pub fn get_pipeline_metrics(&self) -> HashMap<String, PipelineStageStats> {
        self.pipeline_metrics
            .read()
            .map(|map| map.clone())
            .unwrap_or_default()
    }

    pub fn record_worker_heartbeat_success(&self, worker: &str, duration: Duration) {
        let mut labels = HashMap::new();
        labels.insert("worker".to_string(), worker.to_string());
        self.record_pipeline_stage("heartbeat", true, duration, Some(labels), None);
    }

    pub fn record_worker_heartbeat_failure(&self, worker: &str, reason: &str, duration: Duration) {
        if let Ok(mut metrics) = self.system_metrics.write() {
            metrics.worker_heartbeat_failures += 1;
        }

        let mut labels = HashMap::new();
        labels.insert("worker".to_string(), worker.to_string());
        self.record_pipeline_stage("heartbeat", false, duration, Some(labels), Some(reason));
    }

    pub fn record_worker_heartbeat_timeout(&self, worker: &str) {
        if let Ok(mut metrics) = self.system_metrics.write() {
            metrics.worker_heartbeat_timeouts += 1;
            metrics.worker_heartbeat_failures += 1;
        }

        let mut labels = HashMap::new();
        labels.insert("worker".to_string(), worker.to_string());
        self.record_pipeline_stage(
            "heartbeat",
            false,
            Duration::from_millis(0),
            Some(labels),
            Some("timeout"),
        );
    }

    pub fn record_nats_connection_failure(&self) {
        if let Ok(mut metrics) = self.system_metrics.write() {
            metrics.nats_connection_failures += 1;
        }

        if self.config.enable_detailed_metrics {
            self.record_counter("nats_connection_failures_total", 1, HashMap::new());
        }
    }

    pub fn record_nats_connection_timeout(&self) {
        if let Ok(mut metrics) = self.system_metrics.write() {
            metrics.nats_connection_timeouts += 1;
        }

        if self.config.enable_detailed_metrics {
            self.record_counter("nats_connection_timeouts_total", 1, HashMap::new());
        }
    }

    pub fn record_system_metrics(&self, cpu: f64, memory: f64, disk: f64) {
        if let Ok(mut metrics) = self.system_metrics.write() {
            metrics.cpu_usage = cpu;
            metrics.memory_usage = memory;
            metrics.disk_usage = disk;
        }

        if self.config.enable_detailed_metrics {
            self.record_gauge("system_cpu_usage_percent", cpu, HashMap::new());
            self.record_gauge("system_memory_usage_percent", memory, HashMap::new());
            self.record_gauge("system_disk_usage_percent", disk, HashMap::new());
        }
    }

    pub fn record_business_metric(&self, metric_name: &str, value: u64) {
        if let Ok(mut metrics) = self.business_metrics.write() {
            match metric_name {
                "preview_requests" => metrics.preview_requests += value,
                "preview_success" => metrics.preview_success += value,
                "preview_failures" => metrics.preview_failures += value,
                "preview_download_failures" => metrics.preview_download_failures += value,
                "preview_ocr_timeouts" => metrics.preview_ocr_timeouts += value,
                "preview_persistence_failures" => metrics.preview_persistence_failures += value,
                "file_uploads" => metrics.file_uploads += value,
                "rule_executions" => metrics.rule_executions += value,
                "rule_matches" => metrics.rule_matches += value,
                _ => {}
            }
        }

        if self.config.enable_detailed_metrics {
            self.record_counter(metric_name, value as i64, HashMap::new());
        }
    }

    pub fn get_request_metrics(&self) -> RequestMetrics {
        let mut metrics = if let Ok(metrics) = self.request_metrics.read() {
            metrics.clone()
        } else {
            RequestMetrics::default()
        };

        if let Ok(samples) = self.response_time_samples.read() {
            if !samples.is_empty() {
                let mut sorted_samples = samples.clone();
                sorted_samples.sort_by(|a, b| a.partial_cmp(b).unwrap());

                let len = sorted_samples.len();
                if len > 0 {
                    metrics.p95_response_time = percentile(&sorted_samples, 0.95);
                    metrics.p99_response_time = percentile(&sorted_samples, 0.99);
                }
            }
        }

        metrics
    }

    pub fn get_ocr_metrics(&self) -> OcrMetrics {
        if let Ok(metrics) = self.ocr_metrics.read() {
            metrics.clone()
        } else {
            OcrMetrics::default()
        }
    }

    pub fn get_system_metrics(&self) -> SystemMetrics {
        if let Ok(metrics) = self.system_metrics.read() {
            metrics.clone()
        } else {
            SystemMetrics::default()
        }
    }

    pub fn get_business_metrics(&self) -> BusinessMetrics {
        if let Ok(metrics) = self.business_metrics.read() {
            metrics.clone()
        } else {
            BusinessMetrics::default()
        }
    }

    pub fn get_all_samples(&self) -> Vec<MetricSample> {
        if let Ok(samples) = self.samples.read() {
            samples.clone()
        } else {
            Vec::new()
        }
    }

    pub fn reset(&self) {
        if let Ok(mut samples) = self.samples.write() {
            samples.clear();
        }

        *self.request_metrics.write().unwrap() = RequestMetrics::default();
        *self.ocr_metrics.write().unwrap() = OcrMetrics::default();
        *self.system_metrics.write().unwrap() = SystemMetrics::default();
        *self.business_metrics.write().unwrap() = BusinessMetrics::default();

        if let Ok(mut samples) = self.response_time_samples.write() {
            samples.clear();
        }
    }

    pub async fn cleanup_expired_metrics(&self) {
        let retention_period = self.config.retention_period;
        let cutoff_timestamp = current_timestamp() - retention_period * 1000;

        if let Ok(mut samples) = self.samples.write() {
            samples.retain(|sample| sample.timestamp > cutoff_timestamp);
        }
    }
}

fn percentile(sorted_values: &[f64], p: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }

    if p <= 0.0 {
        return sorted_values[0];
    }

    if p >= 1.0 {
        return *sorted_values.last().unwrap();
    }

    let len = sorted_values.len();
    let rank = (p * len as f64).ceil() as usize;
    let index = rank.max(1).min(len) - 1;
    sorted_values[index]
}

fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

impl Default for RequestMetrics {
    fn default() -> Self {
        Self {
            request_count: 0,
            success_count: 0,
            error_count: 0,
            total_response_time: 0.0,
            avg_response_time: 0.0,
            min_response_time: 0.0,
            max_response_time: 0.0,
            p95_response_time: 0.0,
            p99_response_time: 0.0,
            throughput: 0.0,
            error_rate: 0.0,
        }
    }
}

impl Default for OcrMetrics {
    fn default() -> Self {
        Self {
            ocr_count: 0,
            ocr_success_count: 0,
            ocr_error_count: 0,
            total_processing_time: 0.0,
            avg_processing_time: 0.0,
            pages_processed: 0,
            characters_recognized: 0,
            avg_confidence: 0.0,
            total_file_size: 0,
        }
    }
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self {
            cpu_usage: 0.0,
            memory_usage: 0.0,
            disk_usage: 0.0,
            network_in: 0,
            network_out: 0,
            active_connections: 0,
            thread_count: 0,
            fd_usage: 0,
            nats_connection_failures: 0,
            nats_connection_timeouts: 0,
            worker_heartbeat_failures: 0,
            worker_heartbeat_timeouts: 0,
        }
    }
}

impl Default for BusinessMetrics {
    fn default() -> Self {
        Self {
            user_count: 0,
            active_users: 0,
            preview_requests: 0,
            preview_success: 0,
            preview_failures: 0,
            preview_download_failures: 0,
            preview_ocr_timeouts: 0,
            preview_persistence_failures: 0,
            file_uploads: 0,
            storage_usage: 0,
            rule_executions: 0,
            rule_matches: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_metrics_collector() {
        let collector = MetricsCollector::new(MetricsConfig::default());

        collector.record_http_request("GET", "/api/test", 200, Duration::from_millis(150));
        collector.record_http_request("POST", "/api/test", 500, Duration::from_millis(300));

        let metrics = collector.get_request_metrics();
        assert_eq!(metrics.request_count, 2);
        assert_eq!(metrics.success_count, 1);
        assert_eq!(metrics.error_count, 1);
        assert_eq!(metrics.error_rate, 0.5);
    }

    #[test]
    fn test_ocr_metrics() {
        let collector = MetricsCollector::new(MetricsConfig::default());

        collector.record_ocr_processing(true, Duration::from_secs(2), 3, 1000, 0.95, 1024000);

        let metrics = collector.get_ocr_metrics();
        assert_eq!(metrics.ocr_count, 1);
        assert_eq!(metrics.ocr_success_count, 1);
        assert_eq!(metrics.pages_processed, 3);
        assert_eq!(metrics.characters_recognized, 1000);
        assert_eq!(metrics.avg_confidence, 0.95);
    }

    #[test]
    fn test_percentile_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];

        assert_eq!(percentile(&values, 0.5), 5.0);
        assert_eq!(percentile(&values, 0.9), 9.0);
        assert_eq!(percentile(&values, 0.95), 10.0);
    }

    #[test]
    fn test_metric_sample_creation() {
        let mut labels = HashMap::new();
        labels.insert("method".to_string(), "GET".to_string());

        let sample = MetricSample {
            name: "test_counter".to_string(),
            metric_type: MetricType::Counter,
            value: MetricValue::Integer(42),
            labels,
            timestamp: current_timestamp(),
            help: Some("Test counter".to_string()),
        };

        assert_eq!(sample.name, "test_counter");
        assert!(matches!(sample.metric_type, MetricType::Counter));
        assert!(matches!(sample.value, MetricValue::Integer(42)));
    }
}
