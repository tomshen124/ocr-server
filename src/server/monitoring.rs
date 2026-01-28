//! 监控服务管理模块
//! 负责启动和管理监控相关服务

use crate::util::config::Config;
use anyhow::Result;
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};

/// 监控服务管理器
pub struct MonitoringManager;

impl MonitoringManager {
    /// 启动监控服务
    pub async fn start_monitoring_services(config: &Config) -> Result<MonitoringServices> {
        info!("[stats] 启动监控服务...");
        let t0 = std::time::Instant::now();
        let mut services = MonitoringServices::new();

        #[cfg(feature = "monitoring")]
        {
            if config.monitoring.enabled {
                info!("[hourglass] 启动内置监控服务...");
                match timeout(Duration::from_secs(2), Self::start_built_in_monitoring()).await {
                    Ok(Ok(monitor_service)) => {
                        services.built_in_monitor = Some(monitor_service);
                        info!("[ok] 内置监控服务已启动 ({}ms)", t0.elapsed().as_millis());
                    }
                    Ok(Err(e)) => warn!(
                        "[warn] 内置监控服务启动失败: {} ({}ms)",
                        e,
                        t0.elapsed().as_millis()
                    ),
                    Err(_) => warn!("[warn] 内置监控服务启动超时2s，跳过"),
                }
            } else {
                info!("ℹ 内置监控功能已禁用");
            }
        }

        #[cfg(not(feature = "monitoring"))]
        {
            if config.monitoring.enabled {
                info!("[warn] 监控功能已配置但未编译，请使用 --features monitoring 启动");
            }
        }

        // 启动系统信息收集器（软超时）
        info!("[hourglass] 启动系统信息收集器...");
        match timeout(Duration::from_secs(2), Self::start_system_info_collector()).await {
            Ok(Ok(system_info_collector)) => {
                services.system_info_collector = Some(system_info_collector);
                info!("[ok] 系统信息收集器已启动 ({}ms)", t0.elapsed().as_millis());
            }
            Ok(Err(e)) => warn!("[warn] 启动系统信息收集器失败: {}", e),
            Err(_) => warn!("[warn] 启动系统信息收集器超时2s，跳过"),
        }

        // 启动性能监控器（软超时）
        info!("[hourglass] 启动性能监控器...");
        match timeout(Duration::from_secs(2), Self::start_performance_monitor()).await {
            Ok(Ok(performance_monitor)) => {
                services.performance_monitor = Some(performance_monitor);
                info!("[ok] 性能监控器已启动 ({}ms)", t0.elapsed().as_millis());
            }
            Ok(Err(e)) => warn!("[warn] 启动性能监控器失败: {}", e),
            Err(_) => warn!("[warn] 启动性能监控器超时2s，跳过"),
        }

        info!(
            "[ok] 监控服务启动完成，总耗时 {}ms",
            t0.elapsed().as_millis()
        );
        Ok(services)
    }

    /// 启动内置监控服务
    #[cfg(feature = "monitoring")]
    async fn start_built_in_monitoring() -> Result<BuiltInMonitor> {
        let monitor = std::sync::Arc::new(crate::monitor::MonitorService::new());
        monitor.start().await?;

        Ok(BuiltInMonitor {
            service: monitor,
            start_time: chrono::Utc::now(),
        })
    }

    /// 启动系统信息收集器
    async fn start_system_info_collector() -> Result<SystemInfoCollector> {
        info!("[monitor] 启动系统信息收集器...");

        Ok(SystemInfoCollector {
            start_time: chrono::Utc::now(),
            collection_interval: std::time::Duration::from_secs(30),
        })
    }

    /// 启动性能监控器
    async fn start_performance_monitor() -> Result<PerformanceMonitor> {
        info!("[fast] 启动性能监控器...");

        Ok(PerformanceMonitor {
            start_time: chrono::Utc::now(),
            metrics_history: Vec::new(),
        })
    }

    /// 停止监控服务
    pub async fn stop_monitoring_services(services: MonitoringServices) -> Result<()> {
        info!("[stop] 停止监控服务...");

        #[cfg(feature = "monitoring")]
        if let Some(monitor) = services.built_in_monitor {
            if let Err(e) = monitor.service.stop().await {
                tracing::warn!("停止内置监控服务失败: {}", e);
            } else {
                info!("[ok] 内置监控服务已停止");
            }
        }

        // 停止其他监控服务
        if services.system_info_collector.is_some() {
            info!("[ok] 系统信息收集器已停止");
        }

        if services.performance_monitor.is_some() {
            info!("[ok] 性能监控器已停止");
        }

        info!("[ok] 监控服务停止完成");
        Ok(())
    }

    /// 获取监控服务状态
    pub fn get_monitoring_status(services: &MonitoringServices) -> MonitoringStatus {
        MonitoringStatus {
            #[cfg(feature = "monitoring")]
            built_in_monitor_running: services.built_in_monitor.is_some(),
            #[cfg(not(feature = "monitoring"))]
            built_in_monitor_running: false,
            system_info_collector_running: services.system_info_collector.is_some(),
            performance_monitor_running: services.performance_monitor.is_some(),
            total_services: Self::count_running_services(services),
            uptime: Self::calculate_uptime(services),
        }
    }

    /// 统计运行中的服务数量
    fn count_running_services(services: &MonitoringServices) -> u32 {
        let mut count = 0;

        #[cfg(feature = "monitoring")]
        {
            if services.built_in_monitor.is_some() {
                count += 1;
            }
        }

        if services.system_info_collector.is_some() {
            count += 1;
        }
        if services.performance_monitor.is_some() {
            count += 1;
        }

        count
    }

    /// 计算运行时间
    fn calculate_uptime(services: &MonitoringServices) -> std::time::Duration {
        let now = chrono::Utc::now();

        // 取最早启动的服务时间作为整体启动时间
        let earliest_start = services
            .system_info_collector
            .as_ref()
            .map(|s| s.start_time)
            .unwrap_or(now);

        let duration = now.signed_duration_since(earliest_start);
        std::time::Duration::from_secs(duration.num_seconds().max(0) as u64)
    }
}

/// 监控服务集合
pub struct MonitoringServices {
    #[cfg(feature = "monitoring")]
    pub built_in_monitor: Option<BuiltInMonitor>,
    pub system_info_collector: Option<SystemInfoCollector>,
    pub performance_monitor: Option<PerformanceMonitor>,
}

impl MonitoringServices {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "monitoring")]
            built_in_monitor: None,
            system_info_collector: None,
            performance_monitor: None,
        }
    }
}

/// 内置监控服务
#[cfg(feature = "monitoring")]
pub struct BuiltInMonitor {
    pub service: std::sync::Arc<crate::monitor::MonitorService>,
    pub start_time: chrono::DateTime<chrono::Utc>,
}

/// 系统信息收集器
pub struct SystemInfoCollector {
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub collection_interval: std::time::Duration,
}

/// 性能监控器
pub struct PerformanceMonitor {
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub metrics_history: Vec<PerformanceMetric>,
}

/// 性能指标
#[derive(Debug, Clone)]
pub struct PerformanceMetric {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub cpu_usage: f64,
    pub memory_usage: f64,
    pub request_count: u64,
    pub response_time_avg: f64,
}

/// 监控服务状态
#[derive(Debug, Clone)]
pub struct MonitoringStatus {
    pub built_in_monitor_running: bool,
    pub system_info_collector_running: bool,
    pub performance_monitor_running: bool,
    pub total_services: u32,
    pub uptime: std::time::Duration,
}

impl Default for MonitoringServices {
    fn default() -> Self {
        Self::new()
    }
}
