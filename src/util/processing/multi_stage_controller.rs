//! 多阶段并发控制器
//! 替代原有单一信号量机制，实现精细化资源管理

use once_cell::sync::Lazy;
use parking_lot::Mutex as ParkingMutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{LoadAvg, System};
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use crate::util::logging::standards::events;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;

/// 多阶段并发控制器
#[derive(Debug, Clone)]
pub struct MultiStageController {
    // 各阶段信号量
    pub download_semaphore: Arc<Semaphore>,
    pub pdf_convert_semaphore: Arc<Semaphore>,
    pub ocr_process_semaphore: Arc<Semaphore>,
    pub storage_semaphore: Arc<Semaphore>,

    // 配置参数
    config: MultiStageConfig,
    // 自适应节流（通过持有OwnedSemaphorePermit减少可用并发）
    pdf_throttle: Arc<ParkingMutex<Vec<OwnedSemaphorePermit>>>,
    ocr_throttle: Arc<ParkingMutex<Vec<OwnedSemaphorePermit>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiStageConfig {
    pub download_max_concurrent: usize,
    pub pdf_convert_max_concurrent: usize,
    pub pdf_convert_min_concurrent: usize,
    pub pdf_min_free_mem_mb: u32,
    pub pdf_max_load_one: f64,
    pub ocr_process_max_concurrent: usize,
    pub storage_max_concurrent: usize,
    pub resource_monitoring_enabled: bool,
}

impl Default for MultiStageConfig {
    fn default() -> Self {
        Self {
            download_max_concurrent: 12,   // 减少到12，避免网络IO过多
            pdf_convert_max_concurrent: 4, // PDF转换适度提升并发，保持内存安全
            pdf_convert_min_concurrent: 1, // 资源紧张时最少保留1
            pdf_min_free_mem_mb: 2048,     // 至少2GB空闲时才升档
            pdf_max_load_one: 1.5,         // load >1.5 时降档
            ocr_process_max_concurrent: 6, // 与全局OCR信号量保持一致
            storage_max_concurrent: 10,    // 减少存储并发
            resource_monitoring_enabled: true,
        }
    }
}

impl MultiStageController {
    fn record_adaptive_event(stage: &str, memory_usage_percent: f64) {
        let mut labels = HashMap::new();
        labels.insert("stage".to_string(), stage.to_string());
        labels.insert(
            "memory_usage_pct".to_string(),
            format!("{:.1}", memory_usage_percent),
        );
        METRICS_COLLECTOR.record_pipeline_stage(
            "concurrency_adjust",
            true,
            Duration::from_millis(0),
            Some(labels),
            None,
        );
        info!(
            target: "processing.adaptive",
            stage = %stage,
            memory_percent = format!("{:.1}", memory_usage_percent),
            "自适应并发调整"
        );
    }

    pub fn new(config: MultiStageConfig) -> Self {
        info!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "multi_stage_init",
            download_concurrency = config.download_max_concurrent,
            pdf_concurrency = config.pdf_convert_max_concurrent,
            ocr_concurrency = config.ocr_process_max_concurrent,
            storage_concurrency = config.storage_max_concurrent
        );

        Self {
            download_semaphore: Arc::new(Semaphore::new(config.download_max_concurrent)),
            pdf_convert_semaphore: Arc::new(Semaphore::new(config.pdf_convert_max_concurrent)),
            ocr_process_semaphore: Arc::new(Semaphore::new(config.ocr_process_max_concurrent)),
            storage_semaphore: Arc::new(Semaphore::new(config.storage_max_concurrent)),
            config,
            pdf_throttle: Arc::new(ParkingMutex::new(Vec::new())),
            ocr_throttle: Arc::new(ParkingMutex::new(Vec::new())),
        }
    }

    /// 获取下载阶段许可
    pub async fn acquire_download_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit, tokio::sync::AcquireError> {
        let permit = self.download_semaphore.acquire().await?;
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "download_permit_acquired",
            remaining_permits = self.download_semaphore.available_permits()
        );
        Ok(permit)
    }

    /// 尝试获取下载阶段许可 (非阻塞)
    pub fn try_acquire_download_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit, tokio::sync::TryAcquireError> {
        let permit = self.download_semaphore.try_acquire()?;
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "download_permit_try_acquired",
            remaining_permits = self.download_semaphore.available_permits()
        );
        Ok(permit)
    }

    /// 获取PDF转换阶段许可
    pub async fn acquire_pdf_convert_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit, tokio::sync::AcquireError> {
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "pdf_permit_wait"
        );
        // 动态调整PDF并发，基于当前系统负载/内存
        self.adjust_pdf_concurrency().await;
        let permit = self.pdf_convert_semaphore.acquire().await?;
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "pdf_permit_acquired",
            remaining_permits = self.pdf_convert_semaphore.available_permits()
        );
        Ok(permit)
    }

    /// 获取OCR处理阶段许可
    pub async fn acquire_ocr_process_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit, tokio::sync::AcquireError> {
        let permit = self.ocr_process_semaphore.acquire().await?;
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "ocr_permit_acquired",
            remaining_permits = self.ocr_process_semaphore.available_permits()
        );
        Ok(permit)
    }

    /// 加权获取OCR许可（占用多个并发槽位）
    pub async fn acquire_ocr_process_weighted(
        &self,
        units: usize,
    ) -> Result<WeightedOwnedPermit, tokio::sync::AcquireError> {
        let mut permits = Vec::with_capacity(units);
        for _ in 0..units {
            let p = self.ocr_process_semaphore.clone().acquire_owned().await?;
            permits.push(p);
        }
        Ok(WeightedOwnedPermit { permits })
    }

    /// 获取存储阶段许可
    pub async fn acquire_storage_permit(
        &self,
    ) -> Result<tokio::sync::SemaphorePermit, tokio::sync::AcquireError> {
        let permit = self.storage_semaphore.acquire().await?;
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "storage_permit_acquired",
            remaining_permits = self.storage_semaphore.available_permits()
        );
        Ok(permit)
    }

    /// 获取当前各阶段状态
    pub fn get_stage_status(&self) -> StageStatus {
        StageStatus {
            download_available: self.download_semaphore.available_permits(),
            download_total: self.config.download_max_concurrent,
            pdf_convert_available: self.pdf_convert_semaphore.available_permits(),
            pdf_convert_total: self.config.pdf_convert_max_concurrent,
            ocr_process_available: self.ocr_process_semaphore.available_permits(),
            ocr_process_total: self.config.ocr_process_max_concurrent,
            storage_available: self.storage_semaphore.available_permits(),
            storage_total: self.config.storage_max_concurrent,
        }
    }

    /// 获取系统总体负载情况
    pub fn get_system_load_info(&self) -> SystemLoadInfo {
        let stage_status = self.get_stage_status();

        let total_active_tasks = (stage_status.download_total - stage_status.download_available)
            + (stage_status.pdf_convert_total - stage_status.pdf_convert_available)
            + (stage_status.ocr_process_total - stage_status.ocr_process_available)
            + (stage_status.storage_total - stage_status.storage_available);

        let total_capacity = stage_status.download_total
            + stage_status.pdf_convert_total
            + stage_status.ocr_process_total
            + stage_status.storage_total;

        // 找出瓶颈阶段
        let bottleneck_stage = self.identify_bottleneck_stage(&stage_status);

        SystemLoadInfo {
            total_active_tasks,
            total_capacity,
            system_utilization_percent: (total_active_tasks as f64 / total_capacity as f64 * 100.0)
                .round(),
            bottleneck_stage,
            estimated_memory_usage_mb: self.estimate_current_memory_usage(&stage_status),
            can_accept_new_tasks: stage_status.pdf_convert_available > 0
                || stage_status.ocr_process_available > 0,
        }
    }

    fn identify_bottleneck_stage(&self, status: &StageStatus) -> String {
        let download_util = (status.download_total - status.download_available) as f64
            / status.download_total as f64;
        let pdf_util = (status.pdf_convert_total - status.pdf_convert_available) as f64
            / status.pdf_convert_total as f64;
        let ocr_util = (status.ocr_process_total - status.ocr_process_available) as f64
            / status.ocr_process_total as f64;
        let storage_util =
            (status.storage_total - status.storage_available) as f64 / status.storage_total as f64;

        if pdf_util >= download_util && pdf_util >= ocr_util && pdf_util >= storage_util {
            "pdf_convert".to_string()
        } else if ocr_util >= download_util && ocr_util >= storage_util {
            "ocr_process".to_string()
        } else if download_util >= storage_util {
            "download".to_string()
        } else {
            "storage".to_string()
        }
    }

    fn estimate_current_memory_usage(&self, status: &StageStatus) -> u32 {
        // 估算当前内存使用量 (MB)
        let download_usage = (status.download_total - status.download_available) as u32 * 100; // 100MB per download
        let pdf_usage = (status.pdf_convert_total - status.pdf_convert_available) as u32 * 4096; // 4GB per PDF convert
        let ocr_usage = (status.ocr_process_total - status.ocr_process_available) as u32 * 800; // 800MB per OCR
        let storage_usage = (status.storage_total - status.storage_available) as u32 * 50; // 50MB per storage

        download_usage + pdf_usage + ocr_usage + storage_usage
    }
}

/// 多个Owned许可的聚合，Drop时自动释放
pub struct WeightedOwnedPermit {
    permits: Vec<OwnedSemaphorePermit>,
}

impl Drop for WeightedOwnedPermit {
    fn drop(&mut self) {
        // OwnedSemaphorePermit 在drop时自动归还
        self.permits.clear();
    }
}

impl MultiStageController {
    /// 简易自适应：根据内存使用率调整OCR/PDF转换并发（通过节流持有Owned许可实现）
    pub async fn adaptive_tune_once(&self, memory_usage_percent: f64) {
        // 目标：>90% 内存时各减少1个并发；<60% 时各释放1个节流许可
        if memory_usage_percent > 90.0 {
            // OCR节流
            if self.ocr_process_semaphore.available_permits() > 0 {
                if let Ok(p) = self.ocr_process_semaphore.clone().acquire_owned().await {
                    self.ocr_throttle.lock().push(p);
                    warn!(
                        target: "processing.pipeline",
                        event = events::PIPELINE_STAGE,
                        stage = "adaptive_downscale_ocr",
                        memory_usage_pct = memory_usage_percent,
                        remaining_permits = self.ocr_process_semaphore.available_permits()
                    );
                    Self::record_adaptive_event("adaptive_downscale_ocr", memory_usage_percent);
                }
            }
            // PDF节流
            if self.pdf_convert_semaphore.available_permits() > 0 {
                if let Ok(p) = self.pdf_convert_semaphore.clone().acquire_owned().await {
                    self.pdf_throttle.lock().push(p);
                    warn!(
                        target: "processing.pipeline",
                        event = events::PIPELINE_STAGE,
                        stage = "adaptive_downscale_pdf",
                        memory_usage_pct = memory_usage_percent,
                        remaining_permits = self.pdf_convert_semaphore.available_permits()
                    );
                    Self::record_adaptive_event("adaptive_downscale_pdf", memory_usage_percent);
                }
            }
        } else if memory_usage_percent < 60.0 {
            // 释放一个OCR节流
            if let Some(_p) = self.ocr_throttle.lock().pop() {
                debug!(
                    target: "processing.pipeline",
                    event = events::PIPELINE_STAGE,
                    stage = "adaptive_release_ocr",
                    memory_usage_pct = memory_usage_percent,
                    remaining_permits = self.ocr_process_semaphore.available_permits()
                );
                // drop(_p) 自动释放
                Self::record_adaptive_event("adaptive_release_ocr", memory_usage_percent);
            }
            // 释放一个PDF节流
            if let Some(_p) = self.pdf_throttle.lock().pop() {
                debug!(
                    target: "processing.pipeline",
                    event = events::PIPELINE_STAGE,
                    stage = "adaptive_release_pdf",
                    memory_usage_pct = memory_usage_percent,
                    remaining_permits = self.pdf_convert_semaphore.available_permits()
                );
                Self::record_adaptive_event("adaptive_release_pdf", memory_usage_percent);
            }
        }
    }

    /// 基于实时资源的PDF并发调整：可用内存不足或load过高时降档，资源充足时升档
    pub async fn adjust_pdf_concurrency(&self) {
        if !self.config.resource_monitoring_enabled {
            return;
        }

        let mut sys = System::new_all();
        sys.refresh_memory();

        // sysinfo 0.30 返回字节，这里转换为 MB
        let free_mb = sys.available_memory() / 1024 / 1024;
        let LoadAvg { one, .. } = System::load_average();

        // 决定目标有效并发
        let target = if free_mb >= self.config.pdf_min_free_mem_mb as u64
            && one <= self.config.pdf_max_load_one
        {
            self.config.pdf_convert_max_concurrent
        } else {
            self.config
                .pdf_convert_min_concurrent
                .min(self.config.pdf_convert_max_concurrent)
        };

        let throttled = self.pdf_throttle.lock().len();
        let mut current_effective = self
            .config
            .pdf_convert_max_concurrent
            .saturating_sub(throttled);

        if target < current_effective {
            let need_throttle = current_effective - target;
            // 逐个占用许可以降低有效并发
            for _ in 0..need_throttle {
                match self.pdf_convert_semaphore.clone().acquire_owned().await {
                    Ok(p) => {
                        self.pdf_throttle.lock().push(p);
                    }
                    Err(_) => break,
                }
            }
            current_effective = target;
            debug!(
                target: "processing.adaptive",
                event = events::PIPELINE_STAGE,
                stage = "pdf_concurrency_downscale",
                free_mb,
                load_one = format!("{:.2}", one),
                target,
                effective = current_effective
            );
        } else if target > current_effective {
            let release = (target - current_effective).min(self.pdf_throttle.lock().len());
            let mut guard = self.pdf_throttle.lock();
            for _ in 0..release {
                guard.pop(); // drop释放许可
            }
            current_effective = current_effective + release;
            debug!(
                target: "processing.adaptive",
                event = events::PIPELINE_STAGE,
                stage = "pdf_concurrency_upscale",
                free_mb,
                load_one = format!("{:.2}", one),
                target,
                effective = current_effective
            );
        } else {
            // 保持现状
            debug!(
                target: "processing.adaptive",
                event = events::PIPELINE_STAGE,
                stage = "pdf_concurrency_hold",
                free_mb,
                load_one = format!("{:.2}", one),
                target,
                effective = current_effective
            );
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StageStatus {
    pub download_available: usize,
    pub download_total: usize,
    pub pdf_convert_available: usize,
    pub pdf_convert_total: usize,
    pub ocr_process_available: usize,
    pub ocr_process_total: usize,
    pub storage_available: usize,
    pub storage_total: usize,
}

#[derive(Debug, Serialize)]
pub struct SystemLoadInfo {
    pub total_active_tasks: usize,
    pub total_capacity: usize,
    pub system_utilization_percent: f64,
    pub bottleneck_stage: String,
    pub estimated_memory_usage_mb: u32,
    pub can_accept_new_tasks: bool,
}

// 全局多阶段控制器实例
pub static MULTI_STAGE_CONTROLLER: Lazy<MultiStageController> = Lazy::new(|| {
    // 从配置文件加载配置，如果失败则使用默认值
    let config = crate::CONFIG
        .concurrency
        .as_ref()
        .and_then(|c| c.multi_stage.as_ref())
        .map(|ms| MultiStageConfig {
            download_max_concurrent: ms.download_concurrency as usize,
            pdf_convert_max_concurrent: ms.pdf_conversion_concurrency as usize,
            pdf_convert_min_concurrent: ms.pdf_conversion_min_concurrency.max(1) as usize,
            pdf_min_free_mem_mb: ms.pdf_min_free_mem_mb,
            pdf_max_load_one: ms.pdf_max_load_one,
            ocr_process_max_concurrent: ms.ocr_processing_concurrency as usize,
            storage_max_concurrent: ms.storage_concurrency as usize,
            resource_monitoring_enabled: ms.resource_predictor.enabled,
        })
        .unwrap_or_else(|| {
            warn!("未找到多阶段配置，使用默认值");
            MultiStageConfig::default()
        });

    MultiStageController::new(config)
});

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_multi_stage_controller_basic() {
        let config = MultiStageConfig {
            download_max_concurrent: 2,
            pdf_convert_max_concurrent: 1,
            pdf_convert_min_concurrent: 1,
            pdf_min_free_mem_mb: 2048,
            pdf_max_load_one: 1.5,
            ocr_process_max_concurrent: 2,
            storage_max_concurrent: 3,
            resource_monitoring_enabled: true,
        };

        let controller = MultiStageController::new(config);

        // 测试许可获取
        let _download_permit = controller.acquire_download_permit().await.unwrap();
        let _pdf_permit = controller.acquire_pdf_convert_permit().await.unwrap();
        let _ocr_permit = controller.acquire_ocr_process_permit().await.unwrap();
        let _storage_permit = controller.acquire_storage_permit().await.unwrap();

        let status = controller.get_stage_status();
        assert_eq!(status.download_available, 1);
        assert_eq!(status.pdf_convert_available, 0);
        assert_eq!(status.ocr_process_available, 1);
        assert_eq!(status.storage_available, 2);
    }

    #[tokio::test]
    async fn test_system_load_info() {
        let controller = MultiStageController::new(MultiStageConfig::default());
        let load_info = controller.get_system_load_info();

        assert!(load_info.system_utilization_percent >= 0.0);
        assert!(load_info.system_utilization_percent <= 100.0);
        assert!(load_info.can_accept_new_tasks);
    }
}
