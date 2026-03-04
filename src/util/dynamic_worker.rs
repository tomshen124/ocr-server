//!
//!

use anyhow::{anyhow, Result};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Semaphore};
use tracing::{error, info, warn};

const SEMAPHORE_ACQUIRE_TIMEOUT_SECS: u64 = 600;

use crate::util::task_queue::{NatsTaskQueue, NatsTaskQueueConsumer, PreviewTaskHandler};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicWorkerConfig {
    pub enabled: bool,

    pub enable_threshold: u64,

    pub disable_threshold: u64,

    pub check_interval_secs: u64,

    pub sustained_seconds: u64,

    pub max_concurrent_tasks: usize,

    pub cpu_threshold_percent: f64,

    pub memory_threshold_percent: f64,

    pub cooldown_seconds: u64,
}

impl Default for DynamicWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            enable_threshold: 15,
            disable_threshold: 5,
            check_interval_secs: 10,
            sustained_seconds: 30,
            max_concurrent_tasks: 6,
            cpu_threshold_percent: 70.0,
            memory_threshold_percent: 70.0,
            cooldown_seconds: 60,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResourceStats {
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
}

struct QueueDepthHistory {
    records: VecDeque<(Instant, u64)>,
    max_records: usize,
}

impl QueueDepthHistory {
    fn new(max_records: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(max_records),
            max_records,
        }
    }

    fn push(&mut self, depth: u64) {
        self.records.push_back((Instant::now(), depth));
        while self.records.len() > self.max_records {
            self.records.pop_front();
        }
    }

    fn is_sustained_above(&self, threshold: u64, duration: Duration) -> bool {
        if self.records.is_empty() {
            return false;
        }

        let now = Instant::now();
        let cutoff = now - duration;

        let count = self
            .records
            .iter()
            .filter(|(ts, depth)| *ts >= cutoff && *depth >= threshold)
            .count();

        let total_in_window = self.records.iter().filter(|(ts, _)| *ts >= cutoff).count();

        total_in_window > 0 && count * 2 >= total_in_window
    }

    fn is_sustained_below(&self, threshold: u64, duration: Duration) -> bool {
        if self.records.is_empty() {
            return false;
        }

        let now = Instant::now();
        let cutoff = now - duration;

        let count = self
            .records
            .iter()
            .filter(|(ts, depth)| *ts >= cutoff && *depth < threshold)
            .count();

        let total_in_window = self.records.iter().filter(|(ts, _)| *ts >= cutoff).count();

        total_in_window > 0 && count * 2 >= total_in_window
    }
}

struct WorkerHandle {
    shutdown_tx: mpsc::Sender<()>,
    started_at: Instant,
}

struct SemaphoreBoundHandler {
    inner: Arc<dyn PreviewTaskHandler>,
    semaphore: Arc<Semaphore>,
}

impl SemaphoreBoundHandler {
    fn new(inner: Arc<dyn PreviewTaskHandler>, max_concurrent: usize) -> Self {
        let permits = max_concurrent.max(1);
        Self {
            inner,
            semaphore: Arc::new(Semaphore::new(permits)),
        }
    }
}

#[async_trait::async_trait]
impl PreviewTaskHandler for SemaphoreBoundHandler {
    async fn handle_preview_task(&self, task: crate::util::task_queue::PreviewTask) -> Result<()> {
        let acquire_future = self.semaphore.clone().acquire_owned();
        let permit = match tokio::time::timeout(
            Duration::from_secs(SEMAPHORE_ACQUIRE_TIMEOUT_SECS),
            acquire_future,
        )
        .await
        {
            Ok(result) => result.map_err(|_| anyhow!("动态Worker并发控制已失效"))?,
            Err(_) => {
                error!(
                    timeout_secs = SEMAPHORE_ACQUIRE_TIMEOUT_SECS,
                    "动态Worker获取并发许可超时"
                );
                return Err(anyhow!(
                    "动态Worker获取并发许可超过 {} 秒",
                    SEMAPHORE_ACQUIRE_TIMEOUT_SECS
                ));
            }
        };

        let result = self.inner.handle_preview_task(task).await;
        drop(permit);
        result
    }
}

pub struct DynamicWorkerManager {
    config: DynamicWorkerConfig,
    worker_handle: Arc<Mutex<Option<WorkerHandle>>>,
    queue_history: Arc<RwLock<QueueDepthHistory>>,
    last_stop_time: Arc<RwLock<Option<Instant>>>,
    handler: Arc<dyn PreviewTaskHandler>,
    consumer_factory: Arc<dyn Fn() -> Result<NatsTaskQueueConsumer> + Send + Sync>,
    queue: Arc<NatsTaskQueue>,
}

impl DynamicWorkerManager {
    pub fn new(
        config: DynamicWorkerConfig,
        queue: Arc<NatsTaskQueue>,
        handler: Arc<dyn PreviewTaskHandler>,
        consumer_factory: impl Fn() -> Result<NatsTaskQueueConsumer> + Send + Sync + 'static,
    ) -> Self {
        let interval = config.check_interval_secs.max(1);
        let sustained = config.sustained_seconds.max(interval);
        let history_size = ((sustained / interval) as usize).max(1) + 5;

        Self {
            config,
            worker_handle: Arc::new(Mutex::new(None)),
            queue_history: Arc::new(RwLock::new(QueueDepthHistory::new(history_size))),
            last_stop_time: Arc::new(RwLock::new(None)),
            handler,
            consumer_factory: Arc::new(consumer_factory),
            queue,
        }
    }

    pub async fn start_monitoring(self: Arc<Self>) {
        if !self.config.enabled {
            info!("[red] 动态Worker功能未启用");
            return;
        }

        info!("[green] 启动动态Worker监控");
        info!("  - 启用阈值: {} 个任务", self.config.enable_threshold);
        info!("  - 禁用阈值: {} 个任务", self.config.disable_threshold);
        info!("  - 检查间隔: {} 秒", self.config.check_interval_secs);
        info!(
            "  - Master最大并发: {} 个任务",
            self.config.max_concurrent_tasks
        );

        let mut interval =
            tokio::time::interval(Duration::from_secs(self.config.check_interval_secs));

        loop {
            interval.tick().await;

            if let Err(e) = self.check_and_adjust().await {
                error!("动态Worker检查失败: {:#}", e);
            }
        }
    }

    async fn check_and_adjust(&self) -> Result<()> {
        let queue_depth = self.get_queue_depth().await?;

        self.queue_history.write().push(queue_depth);

        let resource_stats = self.get_resource_stats()?;

        let is_running = self.worker_handle.lock().await.is_some();

        if is_running {
            if self.should_stop_worker(queue_depth, &resource_stats).await {
                info!(
                    "[stats] 队列深度={}, CPU={:.1}%, MEM={:.1}% - 停止Master参与OCR",
                    queue_depth, resource_stats.cpu_percent, resource_stats.memory_percent
                );
                self.stop_worker().await?;
            }
        } else {
            if self.should_start_worker(queue_depth, &resource_stats).await {
                info!(
                    "[launch] 队列深度={}, CPU={:.1}%, MEM={:.1}% - 启动Master参与OCR",
                    queue_depth, resource_stats.cpu_percent, resource_stats.memory_percent
                );
                self.start_worker().await?;
            }
        }

        Ok(())
    }

    async fn should_start_worker(&self, queue_depth: u64, stats: &ResourceStats) -> bool {
        let queue_pressure = self.queue_history.read().is_sustained_above(
            self.config.enable_threshold,
            Duration::from_secs(self.config.sustained_seconds),
        );

        if !queue_pressure {
            return false;
        }

        let cpu_ok = stats.cpu_percent < self.config.cpu_threshold_percent;
        let memory_ok = stats.memory_percent < self.config.memory_threshold_percent;

        if !cpu_ok {
            warn!(
                "[warn] Master CPU使用率过高 ({:.1}%)，不启动Worker",
                stats.cpu_percent
            );
            return false;
        }

        if !memory_ok {
            warn!(
                "[warn] Master内存使用率过高 ({:.1}%)，不启动Worker",
                stats.memory_percent
            );
            return false;
        }

        if let Some(last_stop) = *self.last_stop_time.read() {
            let elapsed = Instant::now().duration_since(last_stop).as_secs();
            if elapsed < self.config.cooldown_seconds {
                warn!(
                    "[hourglass] Worker冷却中 (剩余{}秒)",
                    self.config.cooldown_seconds - elapsed
                );
                return false;
            }
        }

        true
    }

    async fn should_stop_worker(&self, queue_depth: u64, stats: &ResourceStats) -> bool {
        let queue_normal = self.queue_history.read().is_sustained_below(
            self.config.disable_threshold,
            Duration::from_secs(self.config.sustained_seconds),
        );

        let cpu_high = stats.cpu_percent > self.config.cpu_threshold_percent;
        let memory_high = stats.memory_percent > self.config.memory_threshold_percent;

        if queue_normal {
            info!("[ok] 队列恢复正常 (深度={})", queue_depth);
            return true;
        }

        if cpu_high {
            warn!(
                "[warn] Master CPU过高 ({:.1}%)，停止Worker",
                stats.cpu_percent
            );
            return true;
        }

        if memory_high {
            warn!(
                "[warn] Master内存过高 ({:.1}%)，停止Worker",
                stats.memory_percent
            );
            return true;
        }

        false
    }

    async fn start_worker(&self) -> Result<()> {
        let mut guard = self.worker_handle.lock().await;

        if guard.is_some() {
            return Ok(());
        }

        let consumer = (self.consumer_factory)().map_err(|e| anyhow!("创建Consumer失败: {}", e))?;

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        let handler: Arc<dyn PreviewTaskHandler> = Arc::new(SemaphoreBoundHandler::new(
            Arc::clone(&self.handler),
            self.config.max_concurrent_tasks,
        ));
        tokio::spawn(async move {
            tokio::select! {
                result = consumer.run(handler) => {
                    if let Err(e) = result {
                        error!("动态Worker运行失败: {:#}", e);
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("[stop] 收到关闭信号，停止动态Worker");
                }
            }
        });

        *guard = Some(WorkerHandle {
            shutdown_tx,
            started_at: Instant::now(),
        });

        info!("[ok] 动态Worker已启动");
        Ok(())
    }

    async fn stop_worker(&self) -> Result<()> {
        let mut guard = self.worker_handle.lock().await;

        if let Some(handle) = guard.take() {
            let running_duration = Instant::now().duration_since(handle.started_at);
            info!(
                "[pause] 停止动态Worker (运行时长: {}秒)",
                running_duration.as_secs()
            );

            let _ = handle.shutdown_tx.send(()).await;

            *self.last_stop_time.write() = Some(Instant::now());
        }

        Ok(())
    }

    async fn get_queue_depth(&self) -> Result<u64> {
        self.queue.get_queue_depth().await
    }

    fn get_resource_stats(&self) -> Result<ResourceStats> {
        #[cfg(feature = "monitoring")]
        {
            use sysinfo::System;

            let mut sys = System::new_all();
            sys.refresh_all();

            let cpu_percent = sys.global_cpu_info().cpu_usage() as f64;
            let memory_used = sys.used_memory();
            let memory_total = sys.total_memory();
            let memory_percent = (memory_used as f64 / memory_total as f64) * 100.0;

            Ok(ResourceStats {
                cpu_percent,
                memory_percent,
                memory_used_mb: memory_used / 1024 / 1024,
                memory_total_mb: memory_total / 1024 / 1024,
            })
        }

        #[cfg(not(feature = "monitoring"))]
        {
            Ok(ResourceStats {
                cpu_percent: 30.0,
                memory_percent: 40.0,
                memory_used_mb: 4096,
                memory_total_mb: 65536,
            })
        }
    }

    fn cooldown_remaining_seconds(&self) -> Option<u64> {
        let last_stop = *self.last_stop_time.read();
        last_stop.and_then(|ts| {
            let elapsed = Instant::now().saturating_duration_since(ts).as_secs();
            if elapsed < self.config.cooldown_seconds {
                Some(self.config.cooldown_seconds - elapsed)
            } else {
                None
            }
        })
    }

    pub async fn current_status(&self) -> Result<DynamicWorkerStatusSnapshot> {
        let queue_depth = self.get_queue_depth().await?;
        let resource_stats = self.get_resource_stats()?;

        let (is_running, uptime_seconds) = {
            let guard = self.worker_handle.lock().await;
            if let Some(handle) = guard.as_ref() {
                (true, Some(handle.started_at.elapsed().as_secs()))
            } else {
                (false, None)
            }
        };

        Ok(DynamicWorkerStatusSnapshot {
            enabled: self.config.enabled,
            is_running,
            queue_depth,
            resource_stats,
            uptime_seconds,
            cooldown_remaining_seconds: self.cooldown_remaining_seconds(),
            config: self.config.clone(),
        })
    }
}

pub static DYNAMIC_WORKER_MANAGER: Lazy<RwLock<Option<Arc<DynamicWorkerManager>>>> =
    Lazy::new(|| RwLock::new(None));

pub fn init_dynamic_worker_manager(manager: Arc<DynamicWorkerManager>) {
    *DYNAMIC_WORKER_MANAGER.write() = Some(manager);
}

pub fn get_dynamic_worker_manager() -> Option<Arc<DynamicWorkerManager>> {
    DYNAMIC_WORKER_MANAGER.read().clone()
}

#[derive(Debug, Clone)]
pub struct DynamicWorkerStatusSnapshot {
    pub enabled: bool,
    pub is_running: bool,
    pub queue_depth: u64,
    pub resource_stats: ResourceStats,
    pub uptime_seconds: Option<u64>,
    pub cooldown_remaining_seconds: Option<u64>,
    pub config: DynamicWorkerConfig,
}
