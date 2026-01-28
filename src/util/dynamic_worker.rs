//! 动态Worker管理器 - 根据队列压力自动调整Master参与度
//!
//! 设计思路：
//! - 正常情况下Master只做控制平面工作
//! - 队列积压严重时Master自动参与OCR处理
//! - 队列恢复正常后Master自动退出OCR处理
//!
//! 核心机制：
//! 1. 周期性监控队列深度
//! 2. 监控Master资源使用情况
//! 3. 根据阈值和资源情况动态启停本地Worker
//! 4. 防抖机制避免频繁开关

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

/// 动态Worker配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicWorkerConfig {
    /// 是否启用动态Worker功能
    pub enabled: bool,

    /// 触发阈值：队列深度超过此值启动Master处理（建议：10-20）
    pub enable_threshold: u64,

    /// 禁用阈值：队列深度低于此值停止Master处理（建议：3-5）
    pub disable_threshold: u64,

    /// 检查间隔（秒）
    pub check_interval_secs: u64,

    /// 持续时间（秒）- 防抖，避免频繁开关
    pub sustained_seconds: u64,

    /// Master最大并发OCR任务数（建议：6-8，留30%资源给控制平面）
    pub max_concurrent_tasks: usize,

    /// Master CPU使用率阈值（%），超过此值不启动Worker
    pub cpu_threshold_percent: f64,

    /// Master内存使用率阈值（%），超过此值不启动Worker
    pub memory_threshold_percent: f64,

    /// 冷却时间（秒）- 停止Worker后多久才能再次启动
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

/// 资源统计信息
#[derive(Debug, Clone)]
pub struct ResourceStats {
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub memory_used_mb: u64,
    pub memory_total_mb: u64,
}

/// 队列深度历史记录（用于防抖）
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

    /// 检查队列深度是否持续超过阈值
    fn is_sustained_above(&self, threshold: u64, duration: Duration) -> bool {
        if self.records.is_empty() {
            return false;
        }

        let now = Instant::now();
        let cutoff = now - duration;

        // 统计在时间窗口内超过阈值的记录数
        let count = self
            .records
            .iter()
            .filter(|(ts, depth)| *ts >= cutoff && *depth >= threshold)
            .count();

        // 如果至少有一半的记录超过阈值，认为是持续状态
        let total_in_window = self.records.iter().filter(|(ts, _)| *ts >= cutoff).count();

        total_in_window > 0 && count * 2 >= total_in_window
    }

    /// 检查队列深度是否持续低于阈值
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

/// Worker句柄
struct WorkerHandle {
    shutdown_tx: mpsc::Sender<()>,
    started_at: Instant,
}

/// 限流后的任务处理器，将内部处理约束在给定并发内
struct SemaphoreBoundHandler {
    inner: Arc<dyn PreviewTaskHandler>,
    semaphore: Arc<Semaphore>,
}

impl SemaphoreBoundHandler {
    fn new(inner: Arc<dyn PreviewTaskHandler>, max_concurrent: usize) -> Self {
        // 至少保留1个许可，避免配置错误导致死锁
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

/// 动态Worker管理器
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

    /// 启动动态Worker监控
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

    /// 检查并调整Worker状态
    async fn check_and_adjust(&self) -> Result<()> {
        // 1. 获取队列深度
        let queue_depth = self.get_queue_depth().await?;

        // 2. 记录历史
        self.queue_history.write().push(queue_depth);

        // 3. 获取Master资源使用情况
        let resource_stats = self.get_resource_stats()?;

        // 4. 获取当前Worker状态
        let is_running = self.worker_handle.lock().await.is_some();

        // 5. 决策
        if is_running {
            // Worker正在运行，检查是否应该停止
            if self.should_stop_worker(queue_depth, &resource_stats).await {
                info!(
                    "[stats] 队列深度={}, CPU={:.1}%, MEM={:.1}% - 停止Master参与OCR",
                    queue_depth, resource_stats.cpu_percent, resource_stats.memory_percent
                );
                self.stop_worker().await?;
            }
        } else {
            // Worker未运行，检查是否应该启动
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

    /// 判断是否应该启动Worker
    async fn should_start_worker(&self, queue_depth: u64, stats: &ResourceStats) -> bool {
        // 条件1：队列积压严重
        let queue_pressure = self.queue_history.read().is_sustained_above(
            self.config.enable_threshold,
            Duration::from_secs(self.config.sustained_seconds),
        );

        if !queue_pressure {
            return false;
        }

        // 条件2：Master资源充足
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

        // 条件3：冷却时间已过
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

    /// 判断是否应该停止Worker
    async fn should_stop_worker(&self, queue_depth: u64, stats: &ResourceStats) -> bool {
        // 条件1：队列恢复正常
        let queue_normal = self.queue_history.read().is_sustained_below(
            self.config.disable_threshold,
            Duration::from_secs(self.config.sustained_seconds),
        );

        // 条件2：Master资源紧张
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

    /// 启动本地Worker
    async fn start_worker(&self) -> Result<()> {
        let mut guard = self.worker_handle.lock().await;

        if guard.is_some() {
            return Ok(()); // 已经在运行
        }

        // 创建Consumer
        let consumer = (self.consumer_factory)().map_err(|e| anyhow!("创建Consumer失败: {}", e))?;

        // 创建关闭通道
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);

        // 启动Consumer
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

    /// 停止本地Worker
    async fn stop_worker(&self) -> Result<()> {
        let mut guard = self.worker_handle.lock().await;

        if let Some(handle) = guard.take() {
            let running_duration = Instant::now().duration_since(handle.started_at);
            info!(
                "[pause] 停止动态Worker (运行时长: {}秒)",
                running_duration.as_secs()
            );

            // 发送关闭信号
            let _ = handle.shutdown_tx.send(()).await;

            // 记录停止时间
            *self.last_stop_time.write() = Some(Instant::now());
        }

        Ok(())
    }

    /// 获取队列深度
    async fn get_queue_depth(&self) -> Result<u64> {
        self.queue.get_queue_depth().await
    }

    /// 获取Master资源使用情况
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
            // 如果没有启用monitoring特性，返回保守值
            Ok(ResourceStats {
                cpu_percent: 30.0,
                memory_percent: 40.0,
                memory_used_mb: 4096,
                memory_total_mb: 65536,
            })
        }
    }

    /// 冷却时间剩余秒数
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

    /// 查询当前状态快照，供监控API使用
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

/// 全局动态Worker管理器
pub static DYNAMIC_WORKER_MANAGER: Lazy<RwLock<Option<Arc<DynamicWorkerManager>>>> =
    Lazy::new(|| RwLock::new(None));

/// 初始化动态Worker管理器
pub fn init_dynamic_worker_manager(manager: Arc<DynamicWorkerManager>) {
    *DYNAMIC_WORKER_MANAGER.write() = Some(manager);
}

/// 获取动态Worker管理器
pub fn get_dynamic_worker_manager() -> Option<Arc<DynamicWorkerManager>> {
    DYNAMIC_WORKER_MANAGER.read().clone()
}

/// 状态快照，用于外部观测
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
