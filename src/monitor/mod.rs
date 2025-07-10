// OCR监控模块
// 集成独立开发的OCR监控工具功能

#[cfg(feature = "monitoring")]
pub mod service;

#[cfg(feature = "monitoring")]
pub mod metrics;

#[cfg(feature = "monitoring")]
pub mod health;

#[cfg(feature = "monitoring")]
pub mod api;

#[cfg(feature = "monitoring")]
pub mod config;

#[cfg(feature = "monitoring")]
pub use service::MonitorService;

#[cfg(feature = "monitoring")]
pub use metrics::SystemMetrics;

#[cfg(feature = "monitoring")]
pub use health::HealthChecker;

#[cfg(feature = "monitoring")]
pub use api::monitoring_routes;

// 当监控功能未启用时的空实现
#[cfg(not(feature = "monitoring"))]
pub struct MonitorService;

#[cfg(not(feature = "monitoring"))]
impl MonitorService {
    pub fn new() -> Self {
        Self
    }

    pub async fn start(&self) -> anyhow::Result<()> {
        tracing::info!("监控功能未启用");
        Ok(())
    }

    pub async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(not(feature = "monitoring"))]
pub fn monitoring_routes() -> axum::Router<()> {
    use axum::Router;
    Router::new()
}
