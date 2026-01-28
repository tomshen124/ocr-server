use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::LazyLock;

use crate::util::config::types::AttachmentLoggingConfig;

pub static ATTACHMENT_LOGGING_RUNTIME: LazyLock<AttachmentLoggingRuntime> = LazyLock::new(|| {
    AttachmentLoggingRuntime::from_config(&crate::CONFIG.logging.attachment_logging)
});

#[derive(Debug)]
pub struct AttachmentLoggingRuntime {
    enabled: AtomicBool,
    sampling_rate: AtomicU32,
    slow_threshold_ms: AtomicU64,
}

#[derive(Debug, Clone, Copy)]
pub struct AttachmentLoggingSnapshot {
    pub enabled: bool,
    pub sampling_rate: u32,
    pub slow_threshold_ms: u64,
}

impl AttachmentLoggingRuntime {
    fn from_config(cfg: &AttachmentLoggingConfig) -> Self {
        Self {
            enabled: AtomicBool::new(cfg.enabled),
            sampling_rate: AtomicU32::new(cfg.sampling_rate.max(1)),
            slow_threshold_ms: AtomicU64::new(cfg.slow_threshold_ms.max(1)),
        }
    }

    pub fn snapshot(&self) -> AttachmentLoggingSnapshot {
        AttachmentLoggingSnapshot {
            enabled: self.enabled.load(Ordering::Relaxed),
            sampling_rate: self.sampling_rate.load(Ordering::Relaxed).max(1),
            slow_threshold_ms: self.slow_threshold_ms.load(Ordering::Relaxed).max(1),
        }
    }

    pub fn update(&self, enabled: bool, sampling_rate: u32, slow_threshold_ms: u64) {
        self.enabled.store(enabled, Ordering::Relaxed);
        self.sampling_rate
            .store(sampling_rate.max(1), Ordering::Relaxed);
        self.slow_threshold_ms
            .store(slow_threshold_ms.max(1), Ordering::Relaxed);
    }
}
