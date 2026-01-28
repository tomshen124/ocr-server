use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::util::config::types::AdaptiveConcurrencyConfig;
use crate::util::system_info;
use crate::AppState;
use crate::OCR_SEMAPHORE;

struct AdaptiveResource {
    name: &'static str,
    semaphore: Arc<Semaphore>,
    total: usize,
    min: usize,
    held: Vec<OwnedSemaphorePermit>,
    current_target: usize,
}

impl AdaptiveResource {
    fn new(
        name: &'static str,
        semaphore: Arc<Semaphore>,
        total: usize,
        min: usize,
    ) -> Option<Self> {
        if total == 0 {
            warn!(resource = name, "自适应限流初始化失败：总并发为0");
            return None;
        }
        let clamped_min = min.min(total);
        Some(Self {
            name,
            semaphore,
            total,
            min: clamped_min.max(1),
            held: Vec::new(),
            current_target: total,
        })
    }

    fn apply_target(&mut self, target: usize) {
        let clamped = target.clamp(self.min, self.total);
        if clamped == self.current_target {
            return;
        }
        let desired_held = self.total.saturating_sub(clamped);
        let mut changed = false;
        while self.held.len() < desired_held {
            match self.semaphore.clone().try_acquire_owned() {
                Ok(permit) => {
                    self.held.push(permit);
                    changed = true;
                }
                Err(_) => break,
            }
        }
        while self.held.len() > desired_held {
            self.held.pop();
            changed = true;
        }
        if changed {
            info!(
                resource = self.name,
                total = self.total,
                target = clamped,
                held = self.held.len(),
                "自适应限流更新"
            );
        }
        self.current_target = clamped;
    }

    fn total(&self) -> usize {
        self.total
    }
}

struct LoadSnapshot {
    cpu: f64,
    memory: f64,
    load_one: f64,
}

pub fn spawn_for_master(app_state: &AppState) {
    let cfg = app_state.config.master.adaptive_limits.clone();
    if !cfg.enabled {
        return;
    }

    let download_total = app_state.download_semaphore.available_permits();
    let submission_total = app_state.submission_semaphore.available_permits();
    let ocr_total = crate::OCR_SEMAPHORE.available_permits();

    let mut resources: Vec<AdaptiveResource> = vec![];
    if let Some(res) = AdaptiveResource::new(
        "download",
        Arc::clone(&app_state.download_semaphore),
        download_total,
        cfg.min_download_permits,
    ) {
        resources.push(res);
    }
    if let Some(res) = AdaptiveResource::new(
        "submission",
        Arc::clone(&app_state.submission_semaphore),
        submission_total,
        cfg.min_submission_permits,
    ) {
        resources.push(res);
    }
    if let Some(res) = AdaptiveResource::new(
        "ocr",
        crate::OCR_SEMAPHORE.clone(),
        ocr_total,
        cfg.min_ocr_permits,
    ) {
        resources.push(res);
    }

    if resources.is_empty() {
        warn!("没有可自适应的资源，跳过限流管理");
        return;
    }

    let interval_secs = cfg.check_interval_secs.max(3);
    info!(interval_secs, "启动自适应并发调节任务");
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;
            let snapshot = collect_load_snapshot();
            let factor = compute_factor(&snapshot, &cfg);
            for resource in resources.iter_mut() {
                let target = ((resource.total() as f64) * factor).ceil() as usize;
                resource.apply_target(target);
            }
            debug!(
                cpu = snapshot.cpu,
                memory = snapshot.memory,
                load_one = snapshot.load_one,
                factor,
                "自适应并发检查完成"
            );
        }
    });
}

pub fn spawn_for_worker(config: &crate::util::config::Config) {
    let cfg = config.master.adaptive_limits.clone();
    if !cfg.enabled {
        return;
    }
    let ocr_total = OCR_SEMAPHORE.available_permits();
    let mut resources =
        match AdaptiveResource::new("ocr", OCR_SEMAPHORE.clone(), ocr_total, cfg.min_ocr_permits) {
            Some(res) => vec![res],
            None => return,
        };

    let interval_secs = cfg.check_interval_secs.max(3);
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(interval_secs));
        loop {
            ticker.tick().await;
            let snapshot = collect_load_snapshot();
            let factor = compute_factor(&snapshot, &cfg);
            if let Some(resource) = resources.get_mut(0) {
                let target = ((resource.total() as f64) * factor).ceil() as usize;
                resource.apply_target(target);
            }
        }
    });
}

fn collect_load_snapshot() -> LoadSnapshot {
    let cpu = system_info::get_cpu_usage().usage_percent as f64;
    let memory = system_info::get_memory_usage().usage_percent as f64;
    let load = system_info::get_load_average().one;
    LoadSnapshot {
        cpu,
        memory,
        load_one: load,
    }
}

fn compute_factor(snapshot: &LoadSnapshot, cfg: &AdaptiveConcurrencyConfig) -> f64 {
    let mut factor: f64 = 1.0;
    factor = factor.min(scale_for_value(snapshot.cpu, cfg.cpu_high_percent, 5.0));
    factor = factor.min(scale_for_value(
        snapshot.memory,
        cfg.memory_high_percent,
        5.0,
    ));
    factor = factor.min(scale_for_value(
        snapshot.load_one,
        cfg.load_high_threshold,
        1.0,
    ));
    factor.clamp(0.2, 1.0)
}

fn scale_for_value(value: f64, threshold: f64, tolerance: f64) -> f64 {
    if value >= threshold {
        0.5
    } else if value >= threshold - tolerance {
        0.8
    } else {
        1.0
    }
}
