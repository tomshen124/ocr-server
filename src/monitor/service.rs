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

    pub async fn start(&self) -> anyhow::Result<()> {
        info!("启动OCR监控服务");

        self.spawn_system_monitor();

        self.spawn_health_monitor();

        self.spawn_memory_monitor();

        info!("[ok] OCR监控服务已在后台启动");
        Ok(())
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        info!("停止OCR监控服务");
        self.shutdown_signal
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }

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

                let metrics = Self::collect_system_metrics(&system).await;

                let latest_metrics = {
                    let mut history = metrics_history.lock();
                    history.push(metrics.clone());

                    if history.len() > 1440 {
                        history.remove(0);
                    }

                    metrics
                };

                Self::check_resource_alerts(&latest_metrics).await;
            }

            info!("系统监控线程已停止");
        });
    }

    fn spawn_health_monitor(&self) {
        let health_checker = self.health_checker.clone();
        let shutdown_signal = Arc::clone(&self.shutdown_signal);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300));

            loop {
                interval.tick().await;

                if shutdown_signal.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                match health_checker.check_ocr_service().await {
                    Ok(is_healthy) => {
                        if !is_healthy {
                            warn!("OCR服务健康检查失败");
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

    fn spawn_memory_monitor(&self) {
        let system = Arc::clone(&self.system);
        let shutdown_signal = Arc::clone(&self.shutdown_signal);

        tokio::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(300));

            loop {
                interval.tick().await;

                if shutdown_signal.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                if let Err(e) = Self::check_ocr_memory(&system).await {
                    error!("内存检查失败: {}", e);
                }
            }

            info!("内存监控线程已停止");
        });
    }

    async fn collect_system_metrics(system: &Arc<Mutex<System>>) -> SystemMetrics {
        let mut sys = system.lock();
        sys.refresh_all();

        let cpu_usage = sys.global_cpu_info().cpu_usage();
        let total_memory = sys.total_memory();
        let used_memory = sys.used_memory();
        let memory_usage = (used_memory as f32 / total_memory as f32) * 100.0;

        let disk_usage = 0.0;

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

    async fn check_ocr_memory(system: &Arc<Mutex<System>>) -> anyhow::Result<()> {
        let sys = system.lock();

        for (pid, process) in sys.processes() {
            if process.name().contains("ocr-server") {
                let memory_mb = process.memory() / 1024 / 1024;

                if memory_mb > 500 {
                    warn!("OCR服务内存使用过高: PID={}, 内存={}MB", pid, memory_mb);
                }

                break;
            }
        }

        Ok(())
    }

    pub async fn get_current_metrics(&self) -> SystemMetrics {
        Self::collect_system_metrics(&self.system).await
    }

    pub async fn get_metrics_history(&self) -> Vec<SystemMetrics> {
        self.metrics_history.lock().clone()
    }

    pub async fn get_ocr_status(&self) -> anyhow::Result<bool> {
        self.health_checker.check_ocr_service().await
    }
}
