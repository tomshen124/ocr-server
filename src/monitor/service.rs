use crate::CONFIG;
use parking_lot::Mutex;
#[cfg(feature = "monitoring")]
use std::sync::Arc;
use std::time::Duration;
use sysinfo::System;
use tokio::time;
use tracing::{error, info, warn};

use super::health::HealthChecker;
use super::metrics::SystemMetrics;

/// OCR监控服务
/// 集成原有监控工具的核心功能
pub struct MonitorService {
    system: Arc<Mutex<System>>,
    health_checker: HealthChecker,
    metrics_history: Arc<Mutex<Vec<SystemMetrics>>>,
    shutdown_signal: Arc<std::sync::atomic::AtomicBool>,
}

impl MonitorService {
    pub fn new() -> Self {
        // Avoid heavy refresh during async startup; defer to background tasks.
        let system = System::new();

        Self {
            system: Arc::new(Mutex::new(system)),
            health_checker: HealthChecker::new(),
            metrics_history: Arc::new(Mutex::new(Vec::new())),
            shutdown_signal: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// 启动监控服务
    pub async fn start(&self) -> anyhow::Result<()> {
        info!("启动OCR监控服务");

        // 启动系统监控线程
        self.spawn_system_monitor();

        // 启动OCR服务健康检查线程
        self.spawn_health_monitor();

        // 启动内存监控线程
        self.spawn_memory_monitor();

        info!("[ok] OCR监控服务已在后台启动");
        Ok(())
    }

    /// 停止监控服务
    pub async fn stop(&self) -> anyhow::Result<()> {
        info!("停止OCR监控服务");
        self.shutdown_signal
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

    /// 启动系统监控线程
    fn spawn_system_monitor(&self) {
        let system = Arc::clone(&self.system);
        let metrics_history = Arc::clone(&self.metrics_history);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(60));

            loop {
                interval.tick().await;

                if shutdown_signal.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                // 收集系统指标
                let metrics = Self::collect_system_metrics(&system).await;

                // 保存到历史记录并获取最新数据用于告警检查
                let latest_metrics = {
                    let mut history = metrics_history.lock();
                    history.push(metrics.clone());

                    // 保持最近24小时的数据（1440个数据点）
                    if history.len() > 1440 {
                        history.remove(0);
                    }

                    metrics
                };

                // 检查资源使用率告警
                Self::check_resource_alerts(&latest_metrics).await;
            }

            info!("系统监控线程已停止");
        });
    }

    /// 启动健康检查线程
    fn spawn_health_monitor(&self) {
        let health_checker = self.health_checker.clone();
        let shutdown_signal = Arc::clone(&self.shutdown_signal);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300)); // 5分钟检查一次

            loop {
                interval.tick().await;

                if shutdown_signal.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                // 检查OCR服务健康状态
                match health_checker.check_ocr_service().await {
                    Ok(is_healthy) => {
                        if !is_healthy {
                            warn!("OCR服务健康检查失败");
                            // 这里可以添加自动重启逻辑
                        }
                    }
                    Err(e) => {
                        error!("健康检查出错: {}", e);
                    }
                }
            }

            info!("健康监控线程已停止");
        });
    }

    /// 启动内存监控线程
    fn spawn_memory_monitor(&self) {
        let system = Arc::clone(&self.system);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300)); // 5分钟检查一次

            loop {
                interval.tick().await;

                if shutdown_signal.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                // 检查OCR服务内存使用
                if let Err(e) = Self::check_ocr_memory(&system).await {
                    error!("内存检查失败: {}", e);
                }
            }

            info!("内存监控线程已停止");
        });
    }

    /// 收集系统指标
    async fn collect_system_metrics(system: &Arc<Mutex<System>>) -> SystemMetrics {
        let mut sys = system.lock();
        sys.refresh_all();

        let cpu_usage = sys.global_cpu_info().cpu_usage();
        let total_memory = sys.total_memory();
        let used_memory = sys.used_memory();
        let memory_usage = (used_memory as f32 / total_memory as f32) * 100.0;

        // 获取磁盘使用率（简化版本）
        let disk_usage = 0.0; // 这里可以添加具体的磁盘检查逻辑

        SystemMetrics {
            timestamp: chrono::Utc::now(),
            cpu_usage,
            memory_usage,
            disk_usage,
            total_memory,
            used_memory,
            process_count: sys.processes().len() as u32,
        }
    }

    /// 检查资源告警
    async fn check_resource_alerts(metrics: &SystemMetrics) {
        if metrics.cpu_usage > 90.0 {
            warn!("CPU使用率过高: {:.1}%", metrics.cpu_usage);
        }

        if metrics.memory_usage > 90.0 {
            warn!("内存使用率过高: {:.1}%", metrics.memory_usage);
        }

        if metrics.disk_usage > 90.0 {
            warn!("磁盘使用率过高: {:.1}%", metrics.disk_usage);
        }
    }

    /// 检查OCR服务内存使用
    async fn check_ocr_memory(system: &Arc<Mutex<System>>) -> anyhow::Result<()> {
        let sys = system.lock();

        // 查找OCR服务进程
        for (pid, process) in sys.processes() {
            if process.name().contains("ocr-server") {
                let memory_mb = process.memory() / 1024 / 1024;

                if memory_mb > 500 {
                    // 500MB阈值
                    warn!("OCR服务内存使用过高: PID={}, 内存={}MB", pid, memory_mb);
                    // 这里可以添加重启逻辑
                }

                break;
            }
        }

        Ok(())
    }

    /// 获取当前系统指标
    pub async fn get_current_metrics(&self) -> SystemMetrics {
        Self::collect_system_metrics(&self.system).await
    }

    /// 获取历史指标
    pub async fn get_metrics_history(&self) -> Vec<SystemMetrics> {
        self.metrics_history.lock().clone()
    }

    /// 获取OCR服务状态
    pub async fn get_ocr_status(&self) -> anyhow::Result<bool> {
        self.health_checker.check_ocr_service().await
    }
}
