use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use crate::util::WebResult;
use axum::extract::Multipart;
use ocr_conn::ocr::{OcrEngineOptions, GLOBAL_POOL};
use std::path::PathBuf;
use std::time::Instant;
use tracing::warn;

fn estimate_pdf_pages_quick(data: &[u8]) -> Option<usize> {
    if data.len() < 8 {
        return None;
    }
    let s = if data.len() > 4 * 1024 * 1024 {
        // 最多扫描前4MB
        &data[..4 * 1024 * 1024]
    } else {
        data
    };
    let hay = std::str::from_utf8(s).ok()?;
    Some(hay.matches("/Type /Page").count())
}

pub async fn upload(mut multipart: Multipart) -> anyhow::Result<WebResult> {
    let mut data = vec![];
    // 构建OCR引擎启动选项（支持配置覆盖）
    let engine_opts = if let Some(cfg) = &crate::CONFIG.ocr_engine {
        let work_dir = cfg.work_dir.as_ref().map(|s| std::path::PathBuf::from(s));
        let binary = cfg.binary.as_ref().map(|s| std::path::PathBuf::from(s));
        let lib_path = cfg.lib_path.as_ref().map(|s| std::path::PathBuf::from(s));
        OcrEngineOptions {
            work_dir,
            binary,
            lib_path,
            timeout_secs: cfg.timeout_secs,
        }
    } else {
        OcrEngineOptions::default()
    };
    // 配置全局池参数并获取一个引擎句柄
    GLOBAL_POOL.set_options_if_empty(engine_opts);
    while let Some(field) = multipart.next_field().await? {
        let file = PathBuf::from(field.file_name().unwrap_or_default());
        let bytes = field.bytes().await?;
        if file
            .extension()
            .is_some_and(|ext| ext.to_string_lossy().eq("pdf"))
        {
            let limits = &crate::CONFIG.download_limits;
            // 大小与页数超限直接拒绝
            let size_ok = (bytes.len() as u64) <= limits.max_pdf_mb * 1024 * 1024;
            let pages_ok = match estimate_pdf_pages_quick(&bytes) {
                Some(p) => (p as u32) <= limits.pdf_max_pages,
                None => true, // 无法估算页数则放行，由渲染层再判断
            };
            if !size_ok || !pages_ok {
                let msg = format!(
                    "文件超限: 大小<= {}MB 且页数<= {}",
                    limits.max_pdf_mb, limits.pdf_max_pages
                );
                warn!("PDF超限已拒绝处理: {}", msg);
                return Ok(WebResult::err_with_code(422, msg));
            }
            let lim = &crate::CONFIG.download_limits;
            let Ok(image_paths) = ocr_conn::pdf_render_jpg_range(
                file.to_str().unwrap_or_default(),
                &bytes,
                1,
                lim.pdf_max_pages,
                lim.max_pdf_mb as usize,
                lim.pdf_render_dpi,
                Some(lim.pdf_jpeg_quality),
            ) else {
                continue;
            };
            // 逐页识别（每次短借用池中的引擎，避免长时间占用）
            for image in image_paths {
                let mut handle = match GLOBAL_POOL.acquire().await {
                    Ok(h) => h,
                    Err(_) => {
                        continue;
                    }
                };
                let ocr_started = Instant::now();
                let contents_result = handle.ocr_and_parse(image.into());
                let duration = ocr_started.elapsed();
                METRICS_COLLECTOR.record_ocr_invocation(contents_result.is_ok(), duration);
                let Ok(contents) = contents_result else {
                    continue;
                };
                data.extend(contents.into_iter().map(|content| content.text));
            }
        } else {
            let mut handle = match GLOBAL_POOL.acquire().await {
                Ok(h) => h,
                Err(_) => {
                    return Ok(WebResult::ok(Vec::<String>::new()));
                }
            };
            let ocr_started = Instant::now();
            let contents_result = handle.ocr_and_parse(bytes.to_vec().into());
            let duration = ocr_started.elapsed();
            METRICS_COLLECTOR.record_ocr_invocation(contents_result.is_ok(), duration);
            let Ok(contents) = contents_result else {
                continue;
            };
            data.extend(contents.into_iter().map(|content| content.text));
        }
    }
    Ok(WebResult::ok(data))
}
