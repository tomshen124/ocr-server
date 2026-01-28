use crate::api::preview::{parse_flexible_json_to_preview_body, persist_preview_request_record};
use crate::db::Database;
use crate::model::preview::PreviewBody;
use crate::util::material_cache;
use crate::util::task_queue::{PreviewTask, TaskQueue};
use crate::util::zen::downloader::download_file_content;
use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use image::{GenericImageView, ImageFormat};
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tokio::time::timeout;

#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};

const DOWNLOAD_TIMEOUT_SECS: u64 = 20;
const DEDUP_TTL_SECS: u64 = 1800; // 30 minutes

pub struct MaterialDownloaderService {
    database: Arc<dyn Database>,
    task_queue: Arc<dyn TaskQueue>,
    batch_size: usize,
    max_concurrency: usize,
    idle_backoff: Duration,
    max_backoff: Duration,
    max_attempts: u32,
}

impl MaterialDownloaderService {
    pub fn new(database: Arc<dyn Database>, task_queue: Arc<dyn TaskQueue>) -> Self {
        let cfg = &crate::CONFIG
            .master
            .background_processing
            .material_downloader;

        Self {
            database,
            task_queue,
            batch_size: cfg.batch_size.max(1) as usize,
            max_concurrency: cfg.max_concurrency.max(1) as usize,
            idle_backoff: Duration::from_millis(cfg.idle_backoff_ms.max(10)),
            max_backoff: Duration::from_millis(cfg.max_backoff_ms.max(cfg.idle_backoff_ms)),
            max_attempts: cfg.max_attempts.max(1),
        }
    }

    pub async fn run(&self) {
        tracing::info!("MaterialDownloaderService started");
        let mut backoff = self.idle_backoff;
        loop {
            tokio::select! {
                _ = wait_for_shutdown_signal() => {
                    tracing::info!("MaterialDownloaderService received shutdown signal, exiting");
                    break;
                }
                res = self.process_batch() => {
                    match res {
                        Ok(0) => {
                            sleep(backoff).await;
                            backoff = (backoff.saturating_mul(2)).min(self.max_backoff);
                        }
                        Ok(_) => {
                            backoff = self.idle_backoff;
                        }
                        Err(e) => {
                            tracing::error!("Error processing material download batch: {}", e);
                            sleep(backoff).await;
                            backoff = (backoff.saturating_mul(2)).min(self.max_backoff);
                        }
                    }
                }
            }
        }
        tracing::info!("MaterialDownloaderService stopped");
    }

    async fn process_batch(&self) -> Result<usize> {
        let tasks = self
            .database
            .fetch_pending_material_downloads(self.batch_size as u32)
            .await?;
        if tasks.is_empty() {
            return Ok(0);
        }

        let db = self.database.clone();
        let queue = self.task_queue.clone();
        let concurrency = self.max_concurrency;

        stream::iter(tasks.into_iter())
            .map(|task| {
                let db = db.clone();
                let queue = queue.clone();
                let max_attempts = self.max_attempts;
                async move {
                    let task_id = task.id.clone();
                    let preview_id = task.preview_id.clone();

                    if task.attempts >= max_attempts as i32 {
                        let reason =
                            format!("Max attempts reached ({}), not retrying", task.attempts);
                        tracing::warn!(preview_id = %preview_id, %reason);
                        let _ = db
                            .update_material_download_status(&task_id, "failed", Some(&reason))
                            .await;
                        return;
                    }

                    match process_single_task_inner(&db, &queue, &task, max_attempts).await {
                        Ok(_) => {
                            let _ = db
                                .update_material_download_status(&task_id, "completed", None)
                                .await;
                            tracing::info!(
                                preview_id = %preview_id,
                                "Material download task completed"
                            );
                        }
                        Err(e) => {
                            let next_attempt = task.attempts + 1;
                            tracing::error!(
                                preview_id = %preview_id,
                                attempts = next_attempt,
                                max_attempts,
                                "Material download failed: {}",
                                e
                            );
                            let _ = db
                                .update_material_download_status(
                                    &task_id,
                                    "failed",
                                    Some(&e.to_string()),
                                )
                                .await;
                        }
                    }
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        Ok(self.batch_size)
    }
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = signal(SignalKind::terminate()).ok();
        let mut sighup = signal(SignalKind::hangup()).ok();

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = async {
                if let Some(ref mut s) = sigterm {
                    let _ = s.recv().await;
                }
            } => {},
            _ = async {
                if let Some(ref mut s) = sighup {
                    let _ = s.recv().await;
                }
            } => {},
        }
        return;
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn process_single_task_inner(
    database: &Arc<dyn Database>,
    task_queue: &Arc<dyn TaskQueue>,
    task: &crate::db::traits::MaterialDownloadQueueRecord,
    max_attempts: u32,
) -> Result<()> {
    // Parse payload
    let mut preview_body: PreviewBody = serde_json::from_str(&task.payload)
        .or_else(|_| parse_flexible_json_to_preview_body(task.payload.as_bytes()))
        .context("Failed to parse preview body")?;

    let preview_id = task.preview_id.clone();

    // third_party_request_id 从原始请求中取
    let third_party_request_id = preview_body.preview.request_id.clone();

    let mut failed: Vec<String> = Vec::new();

    for material in preview_body.preview.material_data.iter_mut() {
        for (attachment_index, attachment) in material.attachment_list.iter_mut().enumerate() {
            let url = &attachment.attach_url;
            if url.starts_with(material_cache::WORKER_CACHE_SCHEME) {
                continue; // 已经处理过
            }

            tracing::info!(preview_id = %preview_id, url = %url, "Downloading attachment");

            let bytes = match download_with_retries(database, url, max_attempts).await {
                Ok(bytes) => bytes,
                Err(e) => {
                    failed.push(format!("{}: {}", url, e));
                    continue;
                }
            };

            let (normalized_bytes, normalized_name, mime, dimensions) =
                match normalize_attachment(&material.code, attachment, url, &bytes).await {
                    Ok(tuple) => tuple,
                    Err(e) => {
                        tracing::warn!(
                            preview_id = %preview_id,
                            material_code = %material.code,
                            attachment_index,
                            url = %url,
                            error = %e,
                            "附件类型不受支持或解码失败，标记为数据错误"
                        );
                        failed.push(format!("{}: {}", url, e));
                        continue;
                    }
                };

            let token = material_cache::store_material(
                &preview_id,
                &material.code,
                &normalized_name,
                &normalized_bytes,
                mime.clone(),
            )
            .await?;

            tracing::info!(
                preview_id = %preview_id,
                material_code = %material.code,
                attachment_index,
                bytes = normalized_bytes.len(),
                mime = mime.as_deref().unwrap_or("unknown"),
                width = dimensions.map(|d| d.0),
                height = dimensions.map(|d| d.1),
                token = %token.token,
                "附件已标准化并缓存"
            );

            attachment.attach_name = normalized_name;
            attachment.attach_url =
                format!("{}{}", material_cache::WORKER_CACHE_SCHEME, token.token);
        }
    }

    if !failed.is_empty() {
        // Persist partial progress so下一次重试可以复用缓存
        let payload = serde_json::to_string(&preview_body)
            .context("serialize preview_body after partial downloads")?;
        let _ = database
            .update_material_download_payload(&task.id, &payload)
            .await;

        let msg = format!("attachments failed: {}", failed.join("; "));
        return Err(anyhow::anyhow!(msg));
    }

    persist_preview_request_record(
        database,
        &preview_body,
        &preview_id,
        Some(&third_party_request_id),
        None,
    )
    .await?;

    let task = PreviewTask {
        preview_body,
        preview_id: preview_id.clone(),
        third_party_request_id: third_party_request_id.clone(),
    };

    task_queue.enqueue(task).await?;

    Ok(())
}

async fn normalize_attachment(
    material_code: &str,
    attachment: &crate::model::preview::Attachment,
    url: &str,
    bytes: &[u8],
) -> Result<(Vec<u8>, String, Option<String>, Option<(u32, u32)>)> {
    let name = attachment.attach_name.clone();
    let file_lower = name.to_ascii_lowercase();

    // DOCX -> PDF
    if file_lower.ends_with(".docx") {
        match crate::util::converter::docx_to_pdf_bytes(bytes.to_vec()).await {
            Ok(pdf_bytes) => {
                return Ok((
                    pdf_bytes,
                    ensure_pdf_extension(&name, "pdf"),
                    Some("application/pdf".to_string()),
                    None,
                ));
            }
            Err(e) => {
                return Err(anyhow::anyhow!(
                    "[DATA_ERR:CONVERT_FAIL] docx 转 pdf 失败: {} (url={}, material={})",
                    e,
                    url,
                    material_code
                ));
            }
        }
    }

    // PDF 直接通过
    if file_lower.ends_with(".pdf") || bytes.starts_with(b"%PDF") {
        return Ok((
            bytes.to_vec(),
            ensure_pdf_extension(&name, "pdf"),
            Some("application/pdf".to_string()),
            None,
        ));
    }

    // 尝试图片解码并转 PNG
    match image::load_from_memory(bytes) {
        Ok(img) => {
            let (w, h) = img.dimensions();
            let mut buf = Vec::new();
            img.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)
                .map_err(|e| {
                    anyhow::anyhow!(
                        "[DATA_ERR:CONVERT_FAIL] 图片重编码失败: {} (url={}, material={})",
                        e,
                        url,
                        material_code
                    )
                })?;
            Ok((
                buf,
                ensure_image_extension(&name, "png"),
                Some("image/png".to_string()),
                Some((w, h)),
            ))
        }
        Err(e) => Err(anyhow::anyhow!(
            "[DATA_ERR:UNSUPPORTED_MEDIA] 附件无法解码为图片或PDF: {} (url={}, material={})",
            e,
            url,
            material_code
        )),
    }
}

fn ensure_pdf_extension(filename: &str, ext: &str) -> String {
    let sanitized_ext = ext.trim_start_matches('.');
    let mut path = std::path::Path::new(filename).to_path_buf();
    path.set_extension(sanitized_ext);
    let candidate = path.to_string_lossy().into_owned();
    if candidate.trim().is_empty() {
        format!("{}.{}", filename, sanitized_ext)
    } else {
        candidate
    }
}

fn ensure_image_extension(filename: &str, ext: &str) -> String {
    let sanitized_ext = ext.trim_start_matches('.');
    let mut path = std::path::Path::new(filename).to_path_buf();
    path.set_extension(sanitized_ext);
    let candidate = path.to_string_lossy().into_owned();
    if candidate.trim().is_empty() {
        format!("{}.{}", filename, sanitized_ext)
    } else {
        candidate
    }
}

async fn download_with_retries(
    database: &Arc<dyn Database>,
    url: &str,
    max_attempts: u32,
) -> Result<Vec<u8>> {
    // 先查持久化去重缓存
    if let Ok(Some(entry)) = database.get_download_cache_token(url).await {
        if let Ok(bytes) = material_cache::read_material(&entry.token).await {
            return Ok(bytes);
        }
    }

    let mut attempt = 0;
    let mut delay = Duration::from_millis(200);

    loop {
        attempt += 1;
        match timeout(
            Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            download_file_content(url),
        )
        .await
        {
            Ok(Ok(bytes)) => {
                // 下载成功，写入去重缓存
                let _ = database
                    .upsert_download_cache_token(
                        url,
                        &material_cache::store_material("dedup", "dedup", url, &bytes, None)
                            .await?
                            .token,
                        DEDUP_TTL_SECS as i64,
                    )
                    .await;
                return Ok(bytes);
            }
            Ok(Err(e)) => {
                if attempt >= max_attempts {
                    return Err(e).context("download failed after retries");
                }
                tracing::warn!(
                    url = %url,
                    attempt,
                    max_attempts,
                    "Download attempt failed, retrying: {}",
                    e
                );
                sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(3));
            }
            Err(_) => {
                if attempt >= max_attempts {
                    return Err(anyhow::anyhow!(
                        "download timeout after {}s",
                        DOWNLOAD_TIMEOUT_SECS
                    ));
                }
                tracing::warn!(
                    url = %url,
                    attempt,
                    max_attempts,
                    "Download attempt timed out ({}s), retrying",
                    DOWNLOAD_TIMEOUT_SECS
                );
                sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(3));
            }
        }
    }
}
