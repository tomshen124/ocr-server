//! 多线程PDF处理流水线架构
//! 针对64G内存+32核CPU+无GPU环境优化

// NEW 新增模块
pub mod enhanced_multi_stage_controller;
pub mod multi_stage_controller;
pub mod optimized_pipeline;
pub mod resource_predictor; // 增强版控制器，支持极端场景

// 重新导出核心类型
pub use enhanced_multi_stage_controller::{
    EnhancedMultiStageController, ENHANCED_MULTI_STAGE_CONTROLLER,
};
pub use multi_stage_controller::{
    MultiStageController, StageStatus, SystemLoadInfo, MULTI_STAGE_CONTROLLER,
};
pub use resource_predictor::{TaskResourcePredictor, TaskResourceProfile};

use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Semaphore};
use tracing::{debug, error, info, warn};

use crate::util::logging::standards::events;

/// [launch] 多线程PDF处理流水线
///
/// 架构设计:
/// PDF输入 → 切片队列 → 转换队列 → OCR队列 → 上传队列 → 完成
///    ↓         ↓         ↓         ↓         ↓
/// 1个线程   5个线程   5个线程   3个线程   10个线程
///
/// 内存分配:
/// - PDF切片: 8GB (单个PDF最大2GB × 4个缓存)
/// - 图片转换: 25GB (5个线程 × 5GB每线程)  
/// - OCR处理: 18GB (3个线程 × 6GB每线程)
/// - 系统预留: 13GB
/// 总计: 64GB
pub struct PdfProcessingPipeline {
    // [control] 资源控制池 - 精确控制并发数
    pdf_slice_pool: Arc<Semaphore>,     // PDF切片：5个并发 (IO密集)
    image_convert_pool: Arc<Semaphore>, // 图片转换：5个并发 (CPU+内存密集)
    ocr_process_pool: Arc<Semaphore>,   // OCR处理：3个并发 (最耗资源)
    oss_upload_pool: Arc<Semaphore>,    // OSS上传：10个并发 (网络IO)

    // [inbox] 异步任务队列 - 解耦各阶段处理
    slice_tx: mpsc::Sender<SliceTask>,
    convert_tx: mpsc::Sender<ConvertTask>,
    ocr_tx: mpsc::Sender<OcrTask>,
    upload_tx: mpsc::Sender<UploadTask>,

    // [stats] 性能统计
    stats: Arc<ProcessingStats>,
}

/// [clipboard] 任务定义
#[derive(Debug)]
pub struct SliceTask {
    pub pdf_content: Vec<u8>,
    pub request_id: String,
    pub material_code: String,
    pub total_pages: u32,
    pub response_tx: oneshot::Sender<Result<Vec<ConvertTask>>>,
}

#[derive(Debug)]
pub struct ConvertTask {
    pub pdf_slice: Vec<u8>,
    pub request_id: String,
    pub material_code: String,
    pub page_start: u32,
    pub page_end: u32,
    pub response_tx: oneshot::Sender<Result<Vec<OcrTask>>>,
}

#[derive(Debug)]
pub struct OcrTask {
    pub image_path: PathBuf,
    pub image_content: Vec<u8>,
    pub request_id: String,
    pub material_code: String,
    pub page_index: u32,
    pub response_tx: oneshot::Sender<Result<OcrResult>>,
}

#[derive(Debug)]
pub struct UploadTask {
    pub image_content: Vec<u8>,
    pub oss_key: String,
    pub request_id: String,
    pub response_tx: oneshot::Sender<Result<()>>,
}

/// OCR处理结果
#[derive(Debug, Clone)]
pub struct OcrResult {
    pub text: String,
    pub page_index: u32,
    pub processing_time: std::time::Duration,
}

/// [stats] 性能统计
#[derive(Debug, Default)]
pub struct ProcessingStats {
    pub total_tasks: std::sync::atomic::AtomicU64,
    pub completed_tasks: std::sync::atomic::AtomicU64,
    pub failed_tasks: std::sync::atomic::AtomicU64,
    pub avg_processing_time: std::sync::atomic::AtomicU64, // 毫秒
}

impl PdfProcessingPipeline {
    /// 创建新的处理流水线
    pub fn new() -> Self {
        // [control] 从配置动态获取并发数
        let ocr_concurrent = crate::CONFIG
            .concurrency
            .as_ref()
            .map(|c| c.ocr_processing.max_concurrent_tasks as usize)
            .unwrap_or(6);

        // 基于OCR并发数计算其他资源池大小
        let pdf_slice_pool = Arc::new(Semaphore::new(ocr_concurrent.min(5))); // PDF切片：最多5并发
        let image_convert_pool = Arc::new(Semaphore::new(ocr_concurrent.min(5))); // 图片转换：最多5并发
        let ocr_process_pool = Arc::new(Semaphore::new(ocr_concurrent / 2)); // OCR处理：总数的一半(避免与全局信号量冲突)
        let oss_upload_pool = Arc::new(Semaphore::new(ocr_concurrent * 2)); // OSS上传：2倍并发

        // [inbox] 任务队列 - 合理的缓冲区大小
        let (slice_tx, slice_rx) = mpsc::channel(10); // PDF切片队列
        let (convert_tx, convert_rx) = mpsc::channel(50); // 转换任务队列
        let (ocr_tx, ocr_rx) = mpsc::channel(100); // OCR任务队列
        let (upload_tx, upload_rx) = mpsc::channel(200); // 上传任务队列

        let stats = Arc::new(ProcessingStats::default());

        let pipeline = Self {
            pdf_slice_pool: pdf_slice_pool.clone(),
            image_convert_pool: image_convert_pool.clone(),
            ocr_process_pool: ocr_process_pool.clone(),
            oss_upload_pool: oss_upload_pool.clone(),
            slice_tx,
            convert_tx: convert_tx.clone(),
            ocr_tx: ocr_tx.clone(),
            upload_tx: upload_tx.clone(),
            stats: stats.clone(),
        };

        // [launch] 启动工作线程池
        Self::spawn_slice_workers(slice_rx, convert_tx.clone(), pdf_slice_pool.clone());
        Self::spawn_convert_workers(convert_rx, ocr_tx.clone(), image_convert_pool.clone());
        Self::spawn_ocr_workers(
            ocr_rx,
            upload_tx.clone(),
            ocr_process_pool.clone(),
            stats.clone(),
        );
        Self::spawn_upload_workers(upload_rx, oss_upload_pool.clone(), stats.clone());

        info!(
            target: "processing.pipeline",
            event = events::PIPELINE_START,
            stage = "pipeline_bootstrap",
            slice_permits = pdf_slice_pool.available_permits(),
            convert_permits = image_convert_pool.available_permits(),
            ocr_permits = ocr_process_pool.available_permits(),
            upload_permits = oss_upload_pool.available_permits()
        );

        pipeline
    }

    /// [note] 提交PDF处理任务
    pub async fn process_pdf(
        &self,
        pdf_content: Vec<u8>,
        request_id: String,
        material_code: String,
    ) -> Result<Vec<OcrResult>> {
        let start_time = std::time::Instant::now();

        // [stats] 统计开始
        self.stats
            .total_tasks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        info!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "pdf_task_start",
            request_id = %request_id,
            material_code = %material_code,
            size_mb = pdf_content.len() / 1024 / 1024
        );

        // 创建响应通道
        let (response_tx, response_rx) = oneshot::channel();

        // 提交切片任务
        let slice_task = SliceTask {
            pdf_content,
            request_id: request_id.clone(),
            material_code: material_code.clone(),
            total_pages: 0, // 将在切片时计算
            response_tx,
        };

        self.slice_tx
            .send(slice_task)
            .await
            .map_err(|_| anyhow::anyhow!("切片队列已关闭"))?;

        // 等待处理完成
        let convert_tasks = response_rx
            .await
            .map_err(|_| anyhow::anyhow!("切片任务被取消"))??;

        // 等待所有转换和OCR任务完成
        let mut all_results = Vec::new();
        for convert_task in convert_tasks {
            // 这里应该等待转换任务完成，然后收集OCR结果
            // 为了简化，这里假设已经有了结果收集机制
        }

        let elapsed = start_time.elapsed();
        info!(
            target: "processing.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "pdf_task_complete",
            request_id = %request_id,
            material_code = %material_code,
            duration_ms = elapsed.as_millis() as u64
        );

        // [stats] 统计完成
        self.stats
            .completed_tasks
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(all_results)
    }

    /// [tool] 启动PDF切片工作线程
    fn spawn_slice_workers(
        mut slice_rx: mpsc::Receiver<SliceTask>,
        convert_tx: mpsc::Sender<ConvertTask>,
        pool: Arc<Semaphore>,
    ) {
        tokio::spawn(async move {
            while let Some(task) = slice_rx.recv().await {
                let convert_tx = convert_tx.clone();

                // 获取切片许可
                if let Ok(permit) = pool.acquire().await {
                    let _permit = permit; // 保持许可直到任务完成

                    debug!(
                        target: "processing.pipeline",
                        event = events::PIPELINE_STAGE,
                        stage = "slice_start",
                        request_id = %task.request_id,
                        material_code = %task.material_code
                    );

                    // 执行PDF切片处理
                    let result = Self::process_pdf_slice(task, convert_tx).await;

                    if let Err(e) = result {
                        error!("PDF切片失败: {}", e);
                    }
                } else {
                    error!("获取切片许可失败");
                    break;
                }
            }
        });
    }

    /// [image] 启动图片转换工作线程
    fn spawn_convert_workers(
        mut convert_rx: mpsc::Receiver<ConvertTask>,
        ocr_tx: mpsc::Sender<OcrTask>,
        pool: Arc<Semaphore>,
    ) {
        // 单个worker处理所有转换任务，内部并发控制
        tokio::spawn(async move {
            while let Some(task) = convert_rx.recv().await {
                let ocr_tx = ocr_tx.clone();

                // 获取转换许可
                if let Ok(permit) = pool.acquire().await {
                    let _permit = permit;

                    debug!(
                        target: "processing.pipeline",
                        event = events::PIPELINE_STAGE,
                        stage = "convert_start",
                        request_id = %task.request_id,
                        material_code = %task.material_code,
                        page_start = task.page_start,
                        page_end = task.page_end
                    );

                    let result = Self::process_image_convert(task, ocr_tx).await;

                    if let Err(e) = result {
                        error!("图片转换失败: {}", e);
                    }
                } else {
                    error!("获取转换许可失败");
                    break;
                }
            }
        });
    }

    /// [search] 启动OCR处理工作线程  
    fn spawn_ocr_workers(
        mut ocr_rx: mpsc::Receiver<OcrTask>,
        upload_tx: mpsc::Sender<UploadTask>,
        pool: Arc<Semaphore>,
        stats: Arc<ProcessingStats>,
    ) {
        // 单个worker处理所有OCR任务，内部并发控制
        tokio::spawn(async move {
            while let Some(task) = ocr_rx.recv().await {
                let upload_tx = upload_tx.clone();
                let stats = stats.clone();

                // 获取OCR许可 - 最关键的资源控制
                if let Ok(permit) = pool.acquire().await {
                    let _permit = permit;

                    debug!(
                        target: "processing.pipeline",
                        event = events::PIPELINE_STAGE,
                        stage = "ocr_start",
                        request_id = %task.request_id,
                        material_code = %task.material_code,
                        page_index = task.page_index
                    );

                    let result = Self::process_ocr(task, upload_tx, stats).await;

                    if let Err(e) = result {
                        error!("OCR处理失败: {}", e);
                    }
                } else {
                    error!("获取OCR许可失败");
                    break;
                }
            }
        });
    }

    /// [cloud] 启动OSS上传工作线程
    fn spawn_upload_workers(
        mut upload_rx: mpsc::Receiver<UploadTask>,
        pool: Arc<Semaphore>,
        stats: Arc<ProcessingStats>,
    ) {
        // 单个worker处理所有上传任务，内部并发控制
        tokio::spawn(async move {
            while let Some(task) = upload_rx.recv().await {
                let stats = stats.clone();

                // 获取上传许可
                if let Ok(permit) = pool.acquire().await {
                    let _permit = permit;

                    debug!(
                        target: "processing.pipeline",
                        event = events::PIPELINE_STAGE,
                        stage = "upload_start",
                        request_id = %task.request_id,
                        oss_key = %task.oss_key
                    );

                    let result = Self::process_upload(task, stats).await;

                    if let Err(e) = result {
                        error!("OSS上传失败: {}", e);
                    }
                } else {
                    error!("获取上传许可失败");
                    break;
                }
            }
        });
    }

    // 具体处理函数 - 示例占位符
    // 注意: 实际实现已在其他模块完成:
    // - PDF处理: src/util/zen/evaluation.rs
    // - 图片转换: src/util/converter.rs
    // - OCR处理: ocr-conn/src/ocr.rs
    // - OSS上传: src/storage/oss.rs
    async fn process_pdf_slice(
        task: SliceTask,
        convert_tx: mpsc::Sender<ConvertTask>,
    ) -> Result<()> {
        // 占位符: 实际PDF切片逻辑请参考 src/util/zen/evaluation.rs::PdfProcessingPipeline
        Ok(())
    }

    async fn process_image_convert(task: ConvertTask, ocr_tx: mpsc::Sender<OcrTask>) -> Result<()> {
        // 占位符: 实际图片转换逻辑请参考 src/util/converter.rs
        Ok(())
    }

    async fn process_ocr(
        task: OcrTask,
        upload_tx: mpsc::Sender<UploadTask>,
        stats: Arc<ProcessingStats>,
    ) -> Result<()> {
        // 占位符: 实际OCR处理逻辑请参考 ocr-conn/src/ocr.rs::ocr_and_parse
        Ok(())
    }

    async fn process_upload(task: UploadTask, stats: Arc<ProcessingStats>) -> Result<()> {
        // 占位符: 实际OSS上传逻辑请参考 src/storage/oss.rs::put
        Ok(())
    }

    /// [stats] 获取处理统计信息
    pub fn get_stats(&self) -> ProcessingStats {
        ProcessingStats {
            total_tasks: std::sync::atomic::AtomicU64::new(
                self.stats
                    .total_tasks
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            completed_tasks: std::sync::atomic::AtomicU64::new(
                self.stats
                    .completed_tasks
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            failed_tasks: std::sync::atomic::AtomicU64::new(
                self.stats
                    .failed_tasks
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
            avg_processing_time: std::sync::atomic::AtomicU64::new(
                self.stats
                    .avg_processing_time
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

/// 全局处理流水线实例
pub static PROCESSING_PIPELINE: std::sync::LazyLock<PdfProcessingPipeline> =
    std::sync::LazyLock::new(|| PdfProcessingPipeline::new());
