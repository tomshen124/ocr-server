//! 资源监控API
//! 提供系统资源监控、多阶段处理状态和链路追踪数据的API接口

use crate::api::worker_proxy;
use crate::util::processing::multi_stage_controller::MULTI_STAGE_CONTROLLER;
use crate::util::service_watchdog::{self, WatchdogStateSnapshot};
use crate::util::tracing::distributed_tracing::DISTRIBUTED_TRACER;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use crate::util::{system_info, IntoJson, WebResult};
use crate::AppState;
use axum::{
    extract::{Query, State},
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

/// 资源监控状态响应
#[derive(Debug, Serialize)]
pub struct ResourceMonitoringResponse {
    /// 多阶段处理器状态
    pub multi_stage_status: MultiStageStatus,
    /// 系统资源状态
    pub system_resources: SystemResourceStatus,
    /// 链路追踪状态
    pub tracing_status: TracingStatus,
    /// 性能指标摘要
    pub performance_metrics: PerformanceMetricsSummary,
    /// 健康状态评分 (0-100)
    pub health_score: u8,
    /// 生成时间戳
    pub timestamp: u64,
    /// OCR引擎池状态
    pub ocr_pool: Option<OcrPoolStats>,
    /// Worker 节点状态
    pub worker_heartbeats: Vec<WorkerStatusSummary>,
    /// Worker 概要统计
    pub worker_summary: WorkerClusterSummary,
}

/// 多阶段处理器状态
#[derive(Debug, Serialize)]
pub struct MultiStageStatus {
    /// 各阶段当前并发数
    pub stage_concurrency: HashMap<String, StageConcurrencyInfo>,
    /// 各阶段队列长度
    pub stage_queues: HashMap<String, u32>,
    /// 各阶段处理统计
    pub stage_statistics: HashMap<String, StageStatistics>,
    /// 资源预测器状态
    pub resource_predictor_status: ResourcePredictorStatus,
}

/// 阶段并发信息
#[derive(Debug, Serialize)]
pub struct StageConcurrencyInfo {
    /// 当前活跃任务数
    pub active_tasks: u32,
    /// 最大并发数
    pub max_concurrency: u32,
    /// 使用率百分比
    pub utilization_percent: f64,
    /// 是否过载
    pub is_overloaded: bool,
}

/// 阶段统计信息
#[derive(Debug, Serialize)]
pub struct StageStatistics {
    /// 总处理任务数
    pub total_processed: u64,
    /// 成功任务数
    pub successful_tasks: u64,
    /// 失败任务数
    pub failed_tasks: u64,
    /// 平均处理时间（毫秒）
    pub avg_processing_time_ms: f64,
    /// 最大处理时间（毫秒）
    pub max_processing_time_ms: u64,
    /// 最小处理时间（毫秒）
    pub min_processing_time_ms: u64,
    /// 成功率
    pub success_rate: f64,
}

/// 资源预测器状态
#[derive(Debug, Serialize)]
pub struct ResourcePredictorStatus {
    /// 预测器是否启用
    pub enabled: bool,
    /// 总预测次数
    pub total_predictions: u64,
    /// 高风险预测次数
    pub high_risk_predictions: u64,
    /// 临界风险预测次数
    pub critical_risk_predictions: u64,
    /// 预测准确度评估
    pub prediction_accuracy: Option<f64>,
}

/// 系统资源状态
#[derive(Debug, Serialize)]
pub struct SystemResourceStatus {
    /// CPU使用率（百分比）
    pub cpu_usage_percent: f64,
    /// 内存使用率（百分比）
    pub memory_usage_percent: f64,
    /// 磁盘使用率（百分比）
    pub disk_usage_percent: f64,
    /// 可用内存（MB）
    pub available_memory_mb: u64,
    /// 系统负载
    pub system_load: SystemLoad,
    /// 网络状态
    pub network_status: NetworkStatus,
    /// 看门狗快照
    pub watchdog_states: Vec<WatchdogStateSnapshot>,
}

/// 系统负载信息
#[derive(Debug, Serialize)]
pub struct SystemLoad {
    /// 1分钟平均负载
    pub load_1min: f64,
    /// 5分钟平均负载
    pub load_5min: f64,
    /// 15分钟平均负载
    pub load_15min: f64,
}

/// 网络状态
#[derive(Debug, Serialize)]
pub struct NetworkStatus {
    /// 网络流入流量（字节/秒）
    pub bytes_in_per_sec: u64,
    /// 网络流出流量（字节/秒）
    pub bytes_out_per_sec: u64,
    /// 活跃连接数
    pub active_connections: u32,
}

/// 链路追踪状态
#[derive(Debug, Serialize)]
pub struct TracingStatus {
    /// 是否启用追踪
    pub enabled: bool,
    /// 当前活跃追踪数量
    pub active_traces: u32,
    /// 已完成追踪数量
    pub completed_traces: u32,
    /// 总span数量
    pub total_spans: u64,
    /// 平均追踪持续时间（毫秒）
    pub avg_trace_duration_ms: f64,
    /// 采样率
    pub sampling_rate: f64,
    /// 追踪数据大小（字节）
    pub trace_data_size_bytes: u64,
}

/// 性能指标摘要
#[derive(Debug, Serialize)]
pub struct PerformanceMetricsSummary {
    /// 请求指标
    pub request_metrics: RequestMetricsSummary,
    /// OCR处理指标
    pub ocr_metrics: OcrMetricsSummary,
    /// 业务指标
    pub business_metrics: BusinessMetricsSummary,
}

/// OCR引擎池状态
#[derive(Debug, Serialize)]
pub struct OcrPoolStats {
    pub capacity: usize,
    pub available: usize,
    pub in_use: usize,
    pub total_started: u64,
    pub total_restarted: u64,
    pub total_failures: u64,
    pub consecutive_failures: u32,
    pub circuit_open: bool,
    pub circuit_open_until_epoch: Option<u64>,
}

/// 请求指标摘要
#[derive(Debug, Serialize)]
pub struct RequestMetricsSummary {
    /// 总请求数
    pub total_requests: u64,
    /// 成功率
    pub success_rate: f64,
    /// 平均响应时间（毫秒）
    pub avg_response_time_ms: f64,
    /// P95响应时间（毫秒）
    pub p95_response_time_ms: f64,
    /// P99响应时间（毫秒）
    pub p99_response_time_ms: f64,
    /// 吞吐量（请求/秒）
    pub throughput_per_sec: f64,
}

/// OCR处理指标摘要
#[derive(Debug, Serialize)]
pub struct OcrMetricsSummary {
    /// OCR任务总数
    pub total_tasks: u64,
    /// OCR成功率
    pub success_rate: f64,
    /// 平均处理时间（毫秒）
    pub avg_processing_time_ms: f64,
    /// 处理的总页数
    pub total_pages_processed: u64,
    /// 识别的总字符数
    pub total_characters_recognized: u64,
    /// 平均识别置信度
    pub avg_confidence_score: f64,
}

/// 业务指标摘要
#[derive(Debug, Serialize)]
pub struct BusinessMetricsSummary {
    /// 预审请求总数
    pub total_preview_requests: u64,
    /// 预审成功数
    pub preview_successes: u64,
    /// 预审失败数
    pub preview_failures: u64,
    /// 规则执行总数
    pub total_rule_executions: u64,
    /// 规则匹配数
    pub rule_matches: u64,
}

/// 查询参数
#[derive(Debug, Deserialize)]
pub struct MonitoringQuery {
    /// 是否包含详细信息
    #[serde(default)]
    pub detailed: bool,
    /// 时间范围（秒）
    #[serde(default = "default_time_range")]
    pub time_range: u64,
}

fn default_time_range() -> u64 {
    3600 // 1小时
}

/// 创建资源监控路由
pub fn create_resource_monitoring_routes() -> Router<AppState> {
    Router::new()
        .route("/api/resources/status", get(get_monitoring_status))
        .route("/api/resources/multi-stage", get(get_multi_stage_status))
        .route("/api/resources/tracing", get(get_tracing_status))
        .route("/api/resources/performance", get(get_performance_metrics))
        .route("/api/resources/health", get(get_health_check))
}

/// 获取监控状态总览
pub async fn get_monitoring_status(
    State(app_state): State<AppState>,
    Query(params): Query<MonitoringQuery>,
) -> Json<WebResult> {
    info!("获取资源监控状态，详细模式: {}", params.detailed);

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // 收集各模块状态
    let multi_stage_status = collect_multi_stage_status().await;
    let system_resources = collect_system_resource_status();
    let tracing_status = collect_tracing_status().await;
    let performance_metrics = collect_performance_metrics();
    let ocr_pool = collect_ocr_pool_stats();

    // 计算健康评分
    let health_score = calculate_health_score(
        &multi_stage_status,
        &system_resources,
        &tracing_status,
        &performance_metrics,
    );

    let (worker_heartbeats, worker_summary) = collect_worker_statuses(&app_state).await;

    let response = ResourceMonitoringResponse {
        multi_stage_status,
        system_resources,
        tracing_status,
        performance_metrics,
        health_score,
        timestamp,
        ocr_pool,
        worker_heartbeats,
        worker_summary,
    };

    WebResult::ok(response).into_json()
}

/// 获取多阶段处理器状态
pub async fn get_multi_stage_status() -> Json<WebResult> {
    info!("获取多阶段处理器状态");

    let status = collect_multi_stage_status().await;
    WebResult::ok(status).into_json()
}

/// 获取链路追踪状态
pub async fn get_tracing_status() -> Json<WebResult> {
    info!("获取链路追踪状态");

    let status = collect_tracing_status().await;
    WebResult::ok(status).into_json()
}

/// 获取性能指标
pub async fn get_performance_metrics() -> Json<WebResult> {
    info!("获取性能指标");

    let metrics = collect_performance_metrics();
    WebResult::ok(metrics).into_json()
}

/// 健康检查
pub async fn get_health_check() -> Json<WebResult> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();

    let health_data = HashMap::from([("status", "healthy"), ("timestamp", timestamp.as_str())]);

    WebResult::ok(health_data).into_json()
}

/// 收集多阶段处理器状态
async fn collect_multi_stage_status() -> MultiStageStatus {
    let status = MULTI_STAGE_CONTROLLER.get_stage_status();

    let mut stage_concurrency = HashMap::new();
    let mut stage_queues = HashMap::new();
    let mut stage_statistics = HashMap::new();

    // 收集各阶段信息
    stage_concurrency.insert(
        "download".to_string(),
        StageConcurrencyInfo {
            active_tasks: (status.download_total - status.download_available) as u32,
            max_concurrency: status.download_total as u32,
            utilization_percent: ((status.download_total - status.download_available) as f64
                / status.download_total as f64)
                * 100.0,
            is_overloaded: status.download_available == 0,
        },
    );

    stage_concurrency.insert(
        "pdf_convert".to_string(),
        StageConcurrencyInfo {
            active_tasks: (status.pdf_convert_total - status.pdf_convert_available) as u32,
            max_concurrency: status.pdf_convert_total as u32,
            utilization_percent: ((status.pdf_convert_total - status.pdf_convert_available) as f64
                / status.pdf_convert_total as f64)
                * 100.0,
            is_overloaded: status.pdf_convert_available == 0,
        },
    );

    stage_concurrency.insert(
        "ocr_process".to_string(),
        StageConcurrencyInfo {
            active_tasks: (status.ocr_process_total - status.ocr_process_available) as u32,
            max_concurrency: status.ocr_process_total as u32,
            utilization_percent: ((status.ocr_process_total - status.ocr_process_available) as f64
                / status.ocr_process_total as f64)
                * 100.0,
            is_overloaded: status.ocr_process_available == 0,
        },
    );

    stage_concurrency.insert(
        "storage".to_string(),
        StageConcurrencyInfo {
            active_tasks: (status.storage_total - status.storage_available) as u32,
            max_concurrency: status.storage_total as u32,
            utilization_percent: ((status.storage_total - status.storage_available) as f64
                / status.storage_total as f64)
                * 100.0,
            is_overloaded: status.storage_available == 0,
        },
    );

    // 队列信息（暂时设为0，因为当前实现没有队列）
    stage_queues.insert("download".to_string(), 0);
    stage_queues.insert("pdf_convert".to_string(), 0);
    stage_queues.insert("ocr_process".to_string(), 0);
    stage_queues.insert("storage".to_string(), 0);

    // 统计数据（模拟）
    stage_statistics.insert(
        "download".to_string(),
        StageStatistics {
            total_processed: 100, // 示例数据
            successful_tasks: 95,
            failed_tasks: 5,
            avg_processing_time_ms: 1500.0,
            max_processing_time_ms: 5000,
            min_processing_time_ms: 200,
            success_rate: 0.95,
        },
    );

    stage_statistics.insert(
        "pdf_convert".to_string(),
        StageStatistics {
            total_processed: 80,
            successful_tasks: 75,
            failed_tasks: 5,
            avg_processing_time_ms: 3500.0,
            max_processing_time_ms: 8000,
            min_processing_time_ms: 1000,
            success_rate: 0.94,
        },
    );

    stage_statistics.insert(
        "ocr_process".to_string(),
        StageStatistics {
            total_processed: 120,
            successful_tasks: 118,
            failed_tasks: 2,
            avg_processing_time_ms: 2200.0,
            max_processing_time_ms: 6000,
            min_processing_time_ms: 500,
            success_rate: 0.98,
        },
    );

    stage_statistics.insert(
        "storage".to_string(),
        StageStatistics {
            total_processed: 150,
            successful_tasks: 149,
            failed_tasks: 1,
            avg_processing_time_ms: 800.0,
            max_processing_time_ms: 2000,
            min_processing_time_ms: 100,
            success_rate: 0.99,
        },
    );

    MultiStageStatus {
        stage_concurrency,
        stage_queues,
        stage_statistics,
        resource_predictor_status: ResourcePredictorStatus {
            enabled: true,
            total_predictions: 250,
            high_risk_predictions: 15,
            critical_risk_predictions: 3,
            prediction_accuracy: Some(0.87),
        },
    }
}

fn collect_system_resource_status() -> SystemResourceStatus {
    let cpu = system_info::get_cpu_usage();
    let memory = system_info::get_memory_usage();
    let disk = system_info::get_disk_usage();
    let load = system_info::get_load_average();
    let network = system_info::get_network_usage();
    let watchdog_states = service_watchdog::list_watchdog_states();

    SystemResourceStatus {
        cpu_usage_percent: cpu.usage_percent as f64,
        memory_usage_percent: memory.usage_percent as f64,
        disk_usage_percent: disk.usage_percent as f64,
        available_memory_mb: memory.total_mb.saturating_sub(memory.used_mb),
        system_load: SystemLoad {
            load_1min: load.one,
            load_5min: load.five,
            load_15min: load.fifteen,
        },
        network_status: NetworkStatus {
            bytes_in_per_sec: network.bytes_in_per_sec,
            bytes_out_per_sec: network.bytes_out_per_sec,
            active_connections: network.active_interfaces,
        },
        watchdog_states,
    }
}

/// 收集链路追踪状态
async fn collect_tracing_status() -> TracingStatus {
    let active_traces = DISTRIBUTED_TRACER.get_active_traces();
    let performance_stats = DISTRIBUTED_TRACER.get_performance_stats();

    TracingStatus {
        enabled: true,
        active_traces: active_traces.len() as u32,
        completed_traces: performance_stats.total_requests as u32,
        total_spans: (active_traces.len() * 5) as u64, // 估计每个trace平均5个span
        avg_trace_duration_ms: performance_stats.avg_response_time,
        sampling_rate: 1.0,
        trace_data_size_bytes: (active_traces.len() * 1024) as u64, // 估计每个trace 1KB
    }
}

/// 收集性能指标
fn collect_performance_metrics() -> PerformanceMetricsSummary {
    let request_metrics = METRICS_COLLECTOR.get_request_metrics();
    let ocr_metrics = METRICS_COLLECTOR.get_ocr_metrics();
    let business_metrics = METRICS_COLLECTOR.get_business_metrics();

    PerformanceMetricsSummary {
        request_metrics: RequestMetricsSummary {
            total_requests: request_metrics.request_count as u64,
            success_rate: if request_metrics.request_count > 0 {
                request_metrics.success_count as f64 / request_metrics.request_count as f64
            } else {
                1.0
            },
            avg_response_time_ms: request_metrics.avg_response_time,
            p95_response_time_ms: request_metrics.p95_response_time,
            p99_response_time_ms: request_metrics.p99_response_time,
            throughput_per_sec: request_metrics.throughput,
        },
        ocr_metrics: OcrMetricsSummary {
            total_tasks: ocr_metrics.ocr_count,
            success_rate: if ocr_metrics.ocr_count > 0 {
                ocr_metrics.ocr_success_count as f64 / ocr_metrics.ocr_count as f64
            } else {
                0.0
            },
            avg_processing_time_ms: ocr_metrics.avg_processing_time,
            total_pages_processed: ocr_metrics.pages_processed,
            total_characters_recognized: ocr_metrics.characters_recognized,
            avg_confidence_score: ocr_metrics.avg_confidence,
        },
        business_metrics: BusinessMetricsSummary {
            total_preview_requests: business_metrics.preview_requests,
            preview_successes: business_metrics.preview_success,
            preview_failures: business_metrics.preview_failures,
            total_rule_executions: business_metrics.rule_executions,
            rule_matches: business_metrics.rule_matches,
        },
    }
}

fn collect_ocr_pool_stats() -> Option<OcrPoolStats> {
    let stats = ocr_conn::ocr::ocr_pool_stats();
    Some(OcrPoolStats {
        capacity: stats.capacity,
        available: stats.available,
        in_use: stats.in_use,
        total_started: stats.total_started,
        total_restarted: stats.total_restarted,
        total_failures: stats.total_failures,
        consecutive_failures: stats.consecutive_failures,
        circuit_open: stats.circuit_open,
        circuit_open_until_epoch: stats.circuit_open_until_epoch,
    })
}

/// 计算系统健康评分
fn calculate_health_score(
    multi_stage: &MultiStageStatus,
    system_resources: &SystemResourceStatus,
    tracing: &TracingStatus,
    performance: &PerformanceMetricsSummary,
) -> u8 {
    let mut score = 100u8;

    // 系统资源评分（40%权重）
    if system_resources.cpu_usage_percent > 90.0 {
        score = score.saturating_sub(20);
    } else if system_resources.cpu_usage_percent > 80.0 {
        score = score.saturating_sub(10);
    } else if system_resources.cpu_usage_percent > 70.0 {
        score = score.saturating_sub(5);
    }

    if system_resources.memory_usage_percent > 95.0 {
        score = score.saturating_sub(25);
    } else if system_resources.memory_usage_percent > 85.0 {
        score = score.saturating_sub(15);
    } else if system_resources.memory_usage_percent > 75.0 {
        score = score.saturating_sub(8);
    }

    // 多阶段处理器健康度（30%权重）
    let overloaded_stages = multi_stage
        .stage_concurrency
        .values()
        .filter(|info| info.is_overloaded)
        .count();

    if overloaded_stages >= 3 {
        score = score.saturating_sub(20);
    } else if overloaded_stages >= 2 {
        score = score.saturating_sub(10);
    } else if overloaded_stages >= 1 {
        score = score.saturating_sub(5);
    }

    // 请求成功率评分（20%权重）
    let success_rate = performance.request_metrics.success_rate;
    if success_rate < 0.9 {
        score = score.saturating_sub(15);
    } else if success_rate < 0.95 {
        score = score.saturating_sub(8);
    } else if success_rate < 0.98 {
        score = score.saturating_sub(3);
    }

    // 响应时间评分（10%权重）
    if performance.request_metrics.avg_response_time_ms > 10000.0 {
        score = score.saturating_sub(10);
    } else if performance.request_metrics.avg_response_time_ms > 5000.0 {
        score = score.saturating_sub(5);
    } else if performance.request_metrics.avg_response_time_ms > 2000.0 {
        score = score.saturating_sub(2);
    }

    score
}

#[derive(Debug, Serialize)]
pub struct WorkerStatusSummary {
    pub worker_id: String,
    pub status: String,
    pub last_seen: Option<String>,
    pub seconds_since: Option<i64>,
    pub interval_secs: Option<u64>,
    pub queue_depth: Option<u64>,
    pub running_tasks: Vec<String>,
    pub metrics: Option<worker_proxy::WorkerHeartbeatMetrics>,
}

#[derive(Debug, Serialize, Default)]
pub struct WorkerClusterSummary {
    pub total: usize,
    pub ok: usize,
    pub timeout: usize,
    pub missing: usize,
}

async fn collect_worker_statuses(
    app_state: &AppState,
) -> (Vec<WorkerStatusSummary>, WorkerClusterSummary) {
    let snapshots = worker_proxy::collect_worker_heartbeat_snapshot().await;
    let mut statuses: Vec<WorkerStatusSummary> = snapshots
        .into_iter()
        .map(|snapshot| WorkerStatusSummary {
            worker_id: snapshot.worker_id.clone(),
            status: if snapshot.timed_out {
                "timeout".to_string()
            } else {
                "ok".to_string()
            },
            last_seen: Some(snapshot.last_seen.to_rfc3339()),
            seconds_since: Some(snapshot.seconds_since),
            interval_secs: Some(snapshot.interval_secs),
            queue_depth: snapshot.queue_depth,
            running_tasks: snapshot.running_tasks,
            metrics: snapshot.metrics,
        })
        .collect();

    let mut existing_ids: HashSet<String> = statuses
        .iter()
        .map(|status| status.worker_id.clone())
        .collect();

    for worker in app_state
        .config
        .worker_proxy
        .workers
        .iter()
        .filter(|w| w.enabled)
    {
        if existing_ids.contains(&worker.worker_id) {
            continue;
        }
        existing_ids.insert(worker.worker_id.clone());
        statuses.push(WorkerStatusSummary {
            worker_id: worker.worker_id.clone(),
            status: "missing".to_string(),
            last_seen: None,
            seconds_since: None,
            interval_secs: None,
            queue_depth: None,
            running_tasks: Vec::new(),
            metrics: None,
        });
    }

    let mut summary = WorkerClusterSummary::default();
    for status in &statuses {
        summary.total += 1;
        match status.status.as_str() {
            "ok" => summary.ok += 1,
            "timeout" => summary.timeout += 1,
            "missing" => summary.missing += 1,
            _ => {}
        }
    }

    (statuses, summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_score_calculation() {
        let multi_stage = MultiStageStatus {
            stage_concurrency: HashMap::from([(
                "download".to_string(),
                StageConcurrencyInfo {
                    active_tasks: 5,
                    max_concurrency: 20,
                    utilization_percent: 25.0,
                    is_overloaded: false,
                },
            )]),
            stage_queues: HashMap::new(),
            stage_statistics: HashMap::new(),
            resource_predictor_status: ResourcePredictorStatus {
                enabled: true,
                total_predictions: 100,
                high_risk_predictions: 5,
                critical_risk_predictions: 1,
                prediction_accuracy: Some(0.9),
            },
        };

        let system_resources = SystemResourceStatus {
            cpu_usage_percent: 45.0,
            memory_usage_percent: 60.0,
            disk_usage_percent: 30.0,
            available_memory_mb: 16384,
            system_load: SystemLoad {
                load_1min: 1.0,
                load_5min: 1.0,
                load_15min: 1.0,
            },
            network_status: NetworkStatus {
                bytes_in_per_sec: 1000000,
                bytes_out_per_sec: 1000000,
                active_connections: 10,
            },
        };

        let tracing = TracingStatus {
            enabled: true,
            active_traces: 5,
            completed_traces: 100,
            total_spans: 500,
            avg_trace_duration_ms: 2000.0,
            sampling_rate: 1.0,
            trace_data_size_bytes: 5120,
        };

        let performance = PerformanceMetricsSummary {
            request_metrics: RequestMetricsSummary {
                total_requests: 1000,
                success_rate: 0.98,
                avg_response_time_ms: 1500.0,
                p95_response_time_ms: 3000.0,
                p99_response_time_ms: 5000.0,
                throughput_per_sec: 10.0,
            },
            ocr_metrics: OcrMetricsSummary {
                total_tasks: 50,
                success_rate: 0.96,
                avg_processing_time_ms: 5000.0,
                total_pages_processed: 150,
                total_characters_recognized: 50000,
                avg_confidence_score: 0.92,
            },
            business_metrics: BusinessMetricsSummary {
                total_preview_requests: 100,
                preview_successes: 95,
                preview_failures: 5,
                total_rule_executions: 200,
                rule_matches: 150,
            },
        };

        let score = calculate_health_score(&multi_stage, &system_resources, &tracing, &performance);
        assert!(score > 90); // 健康系统应该有高分
    }

    #[tokio::test]
    async fn test_monitoring_status_collection() {
        let status = collect_multi_stage_status().await;
        assert!(!status.stage_concurrency.is_empty());

        let system_status = collect_system_resource_status();
        assert!(system_status.cpu_usage_percent >= 0.0);
        assert!(system_status.memory_usage_percent >= 0.0);
    }
}
