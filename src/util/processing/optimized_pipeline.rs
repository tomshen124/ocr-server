//!

use crate::util::logging::standards::events;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use anyhow::Result;
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex, RwLock,
};
use std::time::Instant;
use tokio::sync::{mpsc, oneshot, OwnedSemaphorePermit, Semaphore};
use tracing::{debug, error, info, warn};

pub struct OptimizedPdfOcrPipeline {
    pdf_semaphore: Arc<Semaphore>,
    convert_semaphore: Arc<Semaphore>,
    ocr_semaphore: Arc<Semaphore>,
    upload_semaphore: Arc<Semaphore>,

    ocr_limit: Arc<AtomicUsize>,
    convert_limit: Arc<AtomicUsize>,

    held_ocr_permits: Arc<Mutex<Vec<OwnedSemaphorePermit>>>,
    held_convert_permits: Arc<Mutex<Vec<OwnedSemaphorePermit>>>,

    memory_monitor: Arc<MemoryMonitor>,
    performance_tracker: Arc<PerformanceTracker>,

    config: RwLock<PipelineConfig>,
}

#[derive(Debug, Clone)]
pub struct PipelineConfig {
    pub batch_size: u32,
    pub max_pdf_size_mb: usize,
    pub max_pages: u32,

    pub memory_threshold: f64,
    pub gc_interval_secs: u64,
    pub enable_compression: bool,

    pub pdf_workers: usize,
    pub convert_workers: usize,
    pub ocr_workers: usize,
    pub upload_workers: usize,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            max_pdf_size_mb: 50,
            max_pages: 50,
            memory_threshold: 0.85,
            gc_interval_secs: 300,
            enable_compression: true,
            pdf_workers: 2,
            convert_workers: 4,
            ocr_workers: 2,
            upload_workers: 8,
        }
    }
}

pub struct MemoryMonitor {
    current_usage: std::sync::atomic::AtomicU64,
    peak_usage: std::sync::atomic::AtomicU64,
    gc_count: std::sync::atomic::AtomicU64,
}

impl MemoryMonitor {
    pub fn new() -> Self {
        Self {
            current_usage: std::sync::atomic::AtomicU64::new(0),
            peak_usage: std::sync::atomic::AtomicU64::new(0),
            gc_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub async fn check_memory_usage(&self) -> f64 {
        #[cfg(feature = "monitoring")]
        {
            // Use cached system info to avoid heavy re-initialization
            let memory = crate::util::system_info::get_memory_usage();
            let usage_rate = memory.usage_percent as f64 / 100.0;
            let used = memory.used_mb * 1024 * 1024; // Convert MB to bytes for consistency

            self.current_usage
                .store(used, std::sync::atomic::Ordering::Relaxed);

            // Update peak usage
            let current_peak = self.peak_usage.load(std::sync::atomic::Ordering::Relaxed);
            if used > current_peak {
                self.peak_usage
                    .store(used, std::sync::atomic::Ordering::Relaxed);
            }

            usage_rate
        }
        #[cfg(not(feature = "monitoring"))]
        {
            0.7
        }
    }

    pub async fn trigger_gc_if_needed(&self, threshold: f64) -> bool {
        let usage = self.check_memory_usage().await;
        if usage > threshold {
            debug!(
                target: "processing.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "memory_gc",
                memory_usage_pct = usage * 100.0
            );

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            self.gc_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

pub struct PerformanceTracker {
    total_pdfs: std::sync::atomic::AtomicU64,
    total_pages: std::sync::atomic::AtomicU64,
    total_ocr_time: std::sync::atomic::AtomicU64,
    avg_page_time: std::sync::atomic::AtomicU64,
}

impl PerformanceTracker {
    pub fn new() -> Self {
        Self {
            total_pdfs: std::sync::atomic::AtomicU64::new(0),
            total_pages: std::sync::atomic::AtomicU64::new(0),
            total_ocr_time: std::sync::atomic::AtomicU64::new(0),
            avg_page_time: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn record_processing(&self, pages: u32, elapsed_ms: u64) {
        self.total_pdfs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.total_pages
            .fetch_add(pages as u64, std::sync::atomic::Ordering::Relaxed);
        self.total_ocr_time
            .fetch_add(elapsed_ms, std::sync::atomic::Ordering::Relaxed);

        let total_pages = self.total_pages.load(std::sync::atomic::Ordering::Relaxed);
        let total_time = self
            .total_ocr_time
            .load(std::sync::atomic::Ordering::Relaxed);
        if total_pages > 0 {
            let avg = total_time / total_pages;
            self.avg_page_time
                .store(avg, std::sync::atomic::Ordering::Relaxed);
        }
    }

    pub fn generate_report(&self) -> serde_json::Value {
        let total_pdfs = self.total_pdfs.load(std::sync::atomic::Ordering::Relaxed);
        let total_pages = self.total_pages.load(std::sync::atomic::Ordering::Relaxed);
        let total_time = self
            .total_ocr_time
            .load(std::sync::atomic::Ordering::Relaxed);
        let avg_time = self
            .avg_page_time
            .load(std::sync::atomic::Ordering::Relaxed);

        json!({
            "total_pdfs_processed": total_pdfs,
            "total_pages_processed": total_pages,
            "total_processing_time_ms": total_time,
            "avg_time_per_page_ms": avg_time,
            "throughput_pages_per_minute": if total_time > 0 { (total_pages * 60000) / total_time } else { 0 },
            "avg_pdf_size_pages": if total_pdfs > 0 { total_pages / total_pdfs } else { 0 }
        })
    }
}

impl OptimizedPdfOcrPipeline {
    pub fn new(config: PipelineConfig) -> Self {
        Self {
            pdf_semaphore: Arc::new(Semaphore::new(config.pdf_workers)),
            convert_semaphore: Arc::new(Semaphore::new(config.convert_workers)),
            ocr_semaphore: Arc::new(Semaphore::new(config.ocr_workers)),
            upload_semaphore: Arc::new(Semaphore::new(config.upload_workers)),
            ocr_limit: Arc::new(AtomicUsize::new(config.ocr_workers)),
            convert_limit: Arc::new(AtomicUsize::new(config.convert_workers)),
            held_ocr_permits: Arc::new(Mutex::new(Vec::new())),
            held_convert_permits: Arc::new(Mutex::new(Vec::new())),
            memory_monitor: Arc::new(MemoryMonitor::new()),
            performance_tracker: Arc::new(PerformanceTracker::new()),
            config: RwLock::new(config),
        }
    }

    pub fn configure(&self, app_config: &crate::util::config::Config) {
        let mut config_guard = self.config.write().unwrap();

        if let Some(concurrency) = &app_config.concurrency {
            if let Some(ms) = concurrency.multi_stage.as_ref() {
                let ocr_limit = ms.ocr_processing_concurrency as usize;
                config_guard.ocr_workers = ocr_limit;
                self.ocr_limit.store(ocr_limit, Ordering::Relaxed);

                let convert_limit = ms.pdf_conversion_concurrency as usize;
                config_guard.convert_workers = convert_limit;
                self.convert_limit.store(convert_limit, Ordering::Relaxed);

                config_guard.pdf_workers = ms.download_concurrency as usize;
                config_guard.upload_workers = ms.storage_concurrency as usize;
            }
        }

        if let Some(adaptive) = &app_config.adaptive_concurrency {
            if adaptive.enabled {
                config_guard.memory_threshold = adaptive.memory_high_percent / 100.0;
            }
        }

        info!(
            "[pipeline] Pipeline configured with: OCR={}, Convert={}",
            config_guard.ocr_workers, config_guard.convert_workers
        );
    }

    pub async fn process_pdf_optimized(
        &self,
        pdf_path: PathBuf,
        request_id: String,
        material_code: String,
        storage: Option<Arc<dyn crate::storage::Storage>>,
    ) -> Result<Vec<String>> {
        let start_time = std::time::Instant::now();
        let cfg = self.config.read().unwrap().clone();

        let metadata = tokio::fs::metadata(&pdf_path).await?;
        let size_mb = metadata.len() / 1024 / 1024;

        info!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "optimized_start",
            material_code = %material_code,
            size_mb = size_mb,
            path = %pdf_path.display()
        );

        self.validate_pdf_input(&pdf_path).await?;

        let memory_usage = self.memory_monitor.check_memory_usage().await;
        if memory_usage > cfg.memory_threshold {
            self.memory_monitor
                .trigger_gc_if_needed(cfg.memory_threshold)
                .await;
        }

        self.adaptive_tuning().await;

        let batches = self
            .create_processing_batches(pdf_path.clone(), &material_code, cfg.batch_size)
            .await?;
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "optimized_batches",
            batch_total = batches.len()
        );

        let mut all_ocr_results = Vec::new();
        for (batch_index, batch) in batches.into_iter().enumerate() {
            debug!(
                target: "processing.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "optimized_batch_start",
                batch_index = batch_index + 1,
                range_count = batch.page_ranges.len()
            );

            let batch_results = self
                .process_batch_optimized(batch, request_id.clone(), storage.clone())
                .await?;

            all_ocr_results.extend(batch_results);

            if batch_index % 2 == 0 {
                self.memory_monitor
                    .trigger_gc_if_needed(cfg.memory_threshold)
                    .await;
                self.adaptive_tuning().await;
            }
        }

        let elapsed = start_time.elapsed();
        let pages_count = all_ocr_results.len() as u32;

        self.performance_tracker
            .record_processing(pages_count, elapsed.as_millis() as u64);

        let mut labels = HashMap::new();
        labels.insert("material".to_string(), material_code.clone());
        labels.insert("request".to_string(), request_id.clone());
        labels.insert("pages".to_string(), pages_count.to_string());
        METRICS_COLLECTOR.record_pipeline_stage(
            "optimized_pipeline",
            true,
            elapsed,
            Some(labels),
            None,
        );

        info!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "optimized_complete",
            pages = pages_count,
            duration_ms = elapsed.as_millis() as u64
        );

        Ok(all_ocr_results)
    }

    async fn validate_pdf_input(&self, pdf_path: &PathBuf) -> Result<()> {
        let cfg = self.config.read().unwrap().clone();
        let metadata = tokio::fs::metadata(pdf_path).await?;
        let size_mb = metadata.len() / 1024 / 1024;

        if size_mb > cfg.max_pdf_size_mb as u64 {
            return Err(anyhow::anyhow!(
                "PDF文件大小{}MB超过限制{}MB",
                size_mb,
                cfg.max_pdf_size_mb
            ));
        }

        use tokio::io::AsyncReadExt;
        let mut file = tokio::fs::File::open(pdf_path).await?;
        let mut buffer = [0u8; 4];
        let n = file.read(&mut buffer).await?;

        if n < 4 || &buffer != b"%PDF" {
            return Err(anyhow::anyhow!("不是有效的PDF文件"));
        }

        Ok(())
    }

    async fn create_processing_batches(
        &self,
        pdf_path: PathBuf,
        material_code: &str,
        batch_size: u32,
    ) -> Result<Vec<ProcessingBatch>> {
        let cfg = self.config.read().unwrap().clone();
        let total_pages = self.estimate_pdf_pages(&pdf_path).await?;

        if total_pages > cfg.max_pages {
            return Err(anyhow::anyhow!(
                "PDF页数{}超过限制{}",
                total_pages,
                cfg.max_pages
            ));
        }

        let mut batches = Vec::new();
        let mut current_page = 1;

        while current_page <= total_pages {
            let end_page = (current_page + batch_size - 1).min(total_pages);

            batches.push(ProcessingBatch {
                material_code: material_code.to_string(),
                page_ranges: vec![(current_page, end_page)],
                pdf_path: pdf_path.clone(),
                batch_id: batches.len(),
            });

            current_page = end_page + 1;
        }

        Ok(batches)
    }

    async fn estimate_pdf_pages(&self, pdf_path: &PathBuf) -> Result<u32> {
        let pdf = pdf2image::PDF::from_file(pdf_path)
            .map_err(|e| anyhow::anyhow!("PDF解析失败: {}", e))?;
        Ok(pdf.page_count())
    }

    async fn process_batch_optimized(
        &self,
        batch: ProcessingBatch,
        request_id: String,
        storage: Option<Arc<dyn crate::storage::Storage>>,
    ) -> Result<Vec<String>> {
        let batch_start = std::time::Instant::now();
        let _convert_permit = self.convert_semaphore.acquire().await?;

        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "batch_convert_start",
            batch_id = batch.batch_id,
            material_code = %batch.material_code
        );

        let batch_name = format!("{}_{}", batch.material_code, batch.batch_id);
        let image_paths = self
            .convert_pdf_batch_to_images(&batch, &batch_name)
            .await?;

        drop(_convert_permit);

        let mut ocr_tasks = Vec::new();
        for (index, image_path) in image_paths.iter().enumerate() {
            let image_path = image_path.clone();
            let request_id = request_id.clone();
            let storage = storage.clone();
            let semaphore = self.ocr_semaphore.clone();
            let upload_semaphore = self.upload_semaphore.clone();

            let task = tokio::spawn(async move {
                let _ocr_permit = semaphore.acquire().await?;

                let ocr_result = Self::process_single_image_ocr(&image_path).await?;

                if let Some(storage) = storage {
                    let _upload_permit = upload_semaphore.acquire().await?;
                    Self::upload_image_to_oss(&image_path, &request_id, index, storage).await?;
                }

                let _ = tokio::fs::remove_file(&image_path).await;

                Ok::<String, anyhow::Error>(ocr_result)
            });

            ocr_tasks.push(task);
        }

        let mut batch_results = Vec::new();
        for task in ocr_tasks {
            match task.await? {
                Ok(result) => batch_results.push(result),
                Err(e) => warn!("OCR任务失败: {}", e),
            }
        }

        let batch_elapsed = batch_start.elapsed();
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "batch_complete",
            batch_id = batch.batch_id,
            ocr_results = batch_results.len(),
            duration_ms = batch_elapsed.as_millis() as u64
        );

        let mut labels = HashMap::new();
        labels.insert("material".to_string(), batch.material_code.clone());
        labels.insert("batch_id".to_string(), batch.batch_id.to_string());
        METRICS_COLLECTOR.record_pipeline_stage(
            "optimized_batch",
            true,
            batch_elapsed,
            Some(labels),
            None,
        );

        Ok(batch_results)
    }

    async fn convert_pdf_batch_to_images(
        &self,
        batch: &ProcessingBatch,
        batch_name: &str,
    ) -> Result<Vec<PathBuf>> {
        use pdf2image::{Pages, RenderOptionsBuilder, DPI};

        let pdf = pdf2image::PDF::from_file(&batch.pdf_path)?;
        let cfg = self.config.read().unwrap().clone();

        let (start_page, end_page) = batch.page_ranges[0];
        let pages_range = Pages::Range(start_page..=end_page);

        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "pdf_page_convert",
            page_start = start_page,
            page_end = end_page,
            batch = %batch_name
        );

        let mut render_builder = RenderOptionsBuilder::default();
        if cfg.enable_compression {
            render_builder
                .resolution(DPI::Uniform(120))
                .greyscale(true)
                .pdftocairo(true);
        }
        let render_options = render_builder.build()?;

        let render_start = std::time::Instant::now();
        let mut labels = HashMap::new();
        labels.insert("batch".to_string(), batch_name.to_string());
        labels.insert("material".to_string(), batch.material_code.clone());

        let images = match pdf.render(pages_range, render_options) {
            Ok(imgs) => {
                METRICS_COLLECTOR.record_pipeline_stage(
                    "pdf_convert_batch",
                    true,
                    render_start.elapsed(),
                    Some(labels.clone()),
                    None,
                );
                imgs
            }
            Err(e) => {
                let duration = render_start.elapsed();
                METRICS_COLLECTOR.record_pipeline_stage(
                    "pdf_convert_batch",
                    false,
                    duration,
                    Some(labels.clone()),
                    Some(&e.to_string()),
                );
                return Err(anyhow::anyhow!("PDF渲染失败: {}", e));
            }
        };

        let temp_dir = std::env::temp_dir().join("ocr_batch");
        tokio::fs::create_dir_all(&temp_dir).await?;

        let mut image_paths = Vec::new();
        for (index, image) in images.into_iter().enumerate() {
            let image_path = temp_dir.join(format!("{}_{}.jpg", batch_name, index));

            let image_bytes = {
                let mut buf = Vec::new();
                image.write_to(
                    &mut std::io::Cursor::new(&mut buf),
                    image::ImageFormat::Jpeg,
                )?;
                buf
            };

            tokio::fs::write(&image_path, image_bytes).await?;
            image_paths.push(image_path);
        }

        let duration = render_start.elapsed();
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "image_batch_complete",
            batch = %batch_name,
            image_count = image_paths.len(),
            duration_ms = duration.as_millis() as u64
        );
        Ok(image_paths)
    }

    async fn process_single_image_ocr(image_path: &PathBuf) -> Result<String> {
        use ocr_conn::ocr::GLOBAL_POOL;

        let mut handle = GLOBAL_POOL
            .acquire()
            .await
            .map_err(|e| anyhow::anyhow!("获取OCR引擎失败: {}", e))?;

        let ocr_started = Instant::now();
        let ocr_result = handle.ocr_and_parse(image_path.clone().into());
        let duration = ocr_started.elapsed();
        METRICS_COLLECTOR.record_ocr_invocation(ocr_result.is_ok(), duration);
        let contents = ocr_result.map_err(|e| anyhow::anyhow!("OCR识别失败: {}", e))?;

        let text = contents
            .into_iter()
            .map(|content| content.text)
            .collect::<Vec<_>>()
            .join("\n");

        Ok(text)
    }

    async fn upload_image_to_oss(
        image_path: &PathBuf,
        request_id: &str,
        index: usize,
        storage: Arc<dyn crate::storage::Storage>,
    ) -> Result<()> {
        let image_content = tokio::fs::read(image_path).await?;

        let oss_key = format!(
            "materials/{}/images/{}_{}.jpg",
            request_id,
            image_path.file_stem().unwrap().to_str().unwrap(),
            index
        );

        storage.put(&oss_key, &image_content).await?;
        debug!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "image_upload_complete",
            oss_key = %oss_key
        );

        Ok(())
    }

    pub fn get_performance_report(&self) -> serde_json::Value {
        self.performance_tracker.generate_report()
    }

    pub async fn adaptive_tuning(&self) {
        let memory_usage = self.memory_monitor.check_memory_usage().await;

        let (max_ocr, max_convert, mem_high) = {
            let guard = self.config.read().unwrap();
            (
                guard.ocr_workers,
                guard.convert_workers,
                guard.memory_threshold,
            )
        };

        let mem_low = (mem_high - 0.25).max(0.4);

        let (target_ocr, target_convert) = if memory_usage > mem_high {
            ((max_ocr / 3).max(1), (max_convert / 3).max(1))
        } else if memory_usage < mem_low {
            (max_ocr, max_convert)
        } else {
            ((max_ocr * 2 / 3).max(1), (max_convert * 2 / 3).max(1))
        };

        self.adjust_semaphore(
            &self.ocr_semaphore,
            &self.held_ocr_permits,
            &self.ocr_limit,
            target_ocr,
            "OCR",
        )
        .await;

        self.adjust_semaphore(
            &self.convert_semaphore,
            &self.held_convert_permits,
            &self.convert_limit,
            target_convert,
            "Convert",
        )
        .await;
    }

    async fn adjust_semaphore(
        &self,
        semaphore: &Arc<Semaphore>,
        held_container: &Arc<Mutex<Vec<OwnedSemaphorePermit>>>,
        limit: &Arc<AtomicUsize>,
        target_capacity: usize,
        name: &str,
    ) {
        let current_limit = limit.load(Ordering::Relaxed);
        if target_capacity > current_limit {
            let delta = target_capacity - current_limit;
            semaphore.add_permits(delta);
            limit.store(target_capacity, Ordering::Relaxed);
        }

        let target = target_capacity.min(limit.load(Ordering::Relaxed));

        let mut held = held_container.lock().unwrap();
        let current_held = held.len();
        let current_capacity = limit.load(Ordering::Relaxed).saturating_sub(current_held);

        if target < current_capacity {
            let delta = current_capacity - target;
            debug!(
                target: "processing.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "adaptive_downscale",
                component = name,
                current = current_capacity,
                target = target,
                msg = "Reducing concurrency"
            );

            for _ in 0..delta {
                if let Ok(permit) = semaphore.clone().try_acquire_owned() {
                    held.push(permit);
                } else {
                    warn!("无法立即获取 {} 许可进行降级，将在下次尝试", name);
                    break;
                }
            }
        } else if target > current_capacity {
            let delta = target - current_capacity;
            debug!(
                target: "processing.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "adaptive_upscale",
                component = name,
                current = current_capacity,
                target = target,
                msg = "Increasing concurrency"
            );

            for _ in 0..delta {
                if held.pop().is_none() {
                    break;
                }
                // Permit is dropped here, returning it to the semaphore
            }
        }
    }
}

#[derive(Debug)]
struct ProcessingBatch {
    material_code: String,
    page_ranges: Vec<(u32, u32)>, // (start_page, end_page)
    pdf_path: PathBuf,
    batch_id: usize,
}

pub static OPTIMIZED_PIPELINE: std::sync::LazyLock<OptimizedPdfOcrPipeline> =
    std::sync::LazyLock::new(|| OptimizedPdfOcrPipeline::new(PipelineConfig::default()));
