
use anyhow::Result;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures::stream::{self, StreamExt};
use regex::Regex;
use reqwest::Client;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tokio::task;
use tracing::{debug, error, info, warn};

use crate::util::logging::standards::events;

const INLINE_FETCH_CONCURRENCY: usize = 8;
const WKHTML_TIMEOUT: Duration = Duration::from_secs(60);

pub struct PdfGenerator;

impl PdfGenerator {
    pub async fn html_to_pdf(html_content: &str, output_path: &Path) -> Result<()> {
        debug!(
            target: "report.pdf",
            event = events::PIPELINE_STAGE,
            stage = "html_to_pdf",
            path = %output_path.display()
        );

        let inlined = Self::inline_images(html_content).await;

        let temp_html_path = output_path.with_extension("temp.html");
        tokio::fs::write(&temp_html_path, inlined.as_bytes())
            .await
            .map_err(|e| anyhow::anyhow!("无法写入临时HTML文件: {}", e))?;

        let result = Self::convert_with_wkhtmltopdf(&temp_html_path, output_path).await;

        if let Err(e) = tokio::fs::remove_file(&temp_html_path).await {
            warn!("清理临时文件失败: {} - {}", temp_html_path.display(), e);
        }

        result
    }

    pub async fn html_to_pdf_with_watermark(
        html_content: &str,
        output_path: &Path,
        watermark_text: &str,
    ) -> Result<()> {
        debug!(
            target: "report.pdf",
            event = events::PIPELINE_STAGE,
            stage = "html_to_pdf_with_watermark",
            path = %output_path.display()
        );

        let watermarked_html = Self::add_watermark_to_html(html_content, watermark_text);

        Self::html_to_pdf(&watermarked_html, output_path).await
    }

    async fn inline_images(html_content: &str) -> String {
        let re = Regex::new(r#"(?i)<img[^>]+src=["']([^"']+)["'][^>]*>"#).ok();
        if re.is_none() {
            return html_content.to_string();
        }
        let re = re.unwrap();
        let client = match Client::builder()
            .timeout(Duration::from_secs(6))
            .danger_accept_invalid_certs(true)
            .build()
        {
            Ok(c) => c,
            Err(_) => return html_content.to_string(),
        };

        let matches: Vec<(usize, usize, String)> = re
            .captures_iter(html_content)
            .filter_map(|caps| {
                let src_match = caps.get(1)?;
                Some((
                    src_match.start(),
                    src_match.end(),
                    src_match.as_str().to_string(),
                ))
            })
            .collect();

        if matches.is_empty() {
            return html_content.to_string();
        }

        let semaphore = Arc::new(tokio::sync::Semaphore::new(INLINE_FETCH_CONCURRENCY));
        let fetches =
            stream::iter(matches.clone().into_iter().enumerate()).map(|(idx, (_s, _e, url))| {
                let client = client.clone();
                let semaphore = semaphore.clone();
                async move {
                    let _permit = semaphore.acquire().await.ok();
                    let data_uri = Self::fetch_image_data_uri(&client, &url).await;
                    (idx, data_uri.unwrap_or(url))
                }
            });

        let mut replacements = vec![String::new(); matches.len()];
        let results: Vec<(usize, String)> = fetches
            .buffer_unordered(INLINE_FETCH_CONCURRENCY)
            .collect()
            .await;
        for (idx, uri) in results {
            replacements[idx] = uri;
        }

        let mut out = String::with_capacity(html_content.len());
        let mut last = 0usize;

        for (i, (start, end, _)) in matches.iter().enumerate() {
            out.push_str(&html_content[last..*start]);
            out.push_str(&replacements[i]);
            last = *end;
        }

        if last < html_content.len() {
            out.push_str(&html_content[last..]);
        }

        out
    }

    async fn fetch_image_data_uri(client: &Client, url: &str) -> Option<String> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return None;
        }

        let resp = client.get(url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }

        let mime = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .filter(|ct| ct.starts_with("image/"))
            .unwrap_or_else(|| "image/png".to_string());

        let bytes = resp.bytes().await.ok()?;
        if bytes.len() > 4 * 1024 * 1024 {
            return None;
        }

        let encoded = BASE64.encode(bytes);
        Some(format!("data:{};base64,{}", mime, encoded))
    }

    pub async fn batch_generate_pdfs(
        reports: Vec<(String, &Path)>,
    ) -> Result<Vec<Result<(), anyhow::Error>>> {
        debug!(
            target: "report.pdf",
            event = events::PIPELINE_STAGE,
            stage = "batch_pdf",
            count = reports.len()
        );

        let mut results = Vec::new();

        for (html_content, output_path) in reports {
            let result = Self::html_to_pdf(&html_content, output_path).await;
            results.push(result);
        }

        Ok(results)
    }

    async fn convert_with_wkhtmltopdf(html_path: &Path, output_path: &Path) -> Result<()> {
        let run_attempt = |attempt: usize| {
            let html = html_path.to_path_buf();
            let pdf = output_path.to_path_buf();
            async move {
                task::spawn_blocking(move || run_wkhtmltopdf_blocking(&html, &pdf, attempt))
                    .await
                    .map_err(|e| anyhow::anyhow!("wkhtmltopdf join error: {}", e))?
            }
        };

        if let Err(err) = run_attempt(1).await {
            warn!("wkhtmltopdf首次转换失败，准备重试: {}", err);
            run_attempt(2).await?;
        }

        debug!(
            target: "report.pdf",
            event = events::PIPELINE_COMPLETE,
            stage = "html_to_pdf",
            path = %output_path.display()
        );
        Ok(())
    }

    fn add_watermark_to_html(html_content: &str, watermark_text: &str) -> String {
        let watermark_style = format!(
            r#"
            <style>
            .watermark {{
                position: fixed;
                top: 50%;
                left: 50%;
                transform: translate(-50%, -50%) rotate(-45deg);
                font-size: 72px;
                color: rgba(200, 200, 200, 0.3);
                z-index: -1;
                user-select: none;
                pointer-events: none;
                font-weight: bold;
            }}
            </style>
            <div class="watermark">{}</div>
            "#,
            watermark_text
        );

        if let Some(body_pos) = html_content.find("<body") {
            if let Some(body_end) = html_content[body_pos..].find('>') {
                let insert_pos = body_pos + body_end + 1;
                let mut result = html_content.to_string();
                result.insert_str(insert_pos, &watermark_style);
                return result;
            }
        }

        format!("{}{}", html_content, watermark_style)
    }

    pub fn check_pdf_tools() -> Result<()> {
        let status = Command::new("wkhtmltopdf").arg("--version").status();

        match status {
            Ok(status) if status.success() => {
                info!("[ok] wkhtmltopdf工具可用");
                Ok(())
            }
            Ok(_) => {
                warn!("[warn] wkhtmltopdf工具执行失败");
                Err(anyhow::anyhow!("wkhtmltopdf工具不可用"))
            }
            Err(e) => {
                warn!("[warn] wkhtmltopdf工具未安装: {}", e);
                Err(anyhow::anyhow!("wkhtmltopdf工具未安装: {}", e))
            }
        }
    }

    pub async fn html_to_pdf_optimized(
        html_content: &str,
        output_path: &Path,
        compress: bool,
    ) -> Result<()> {
        info!("开始优化PDF转换: {}", output_path.display());

        let inlined = Self::inline_images(html_content).await;
        let temp_html_path = output_path.with_extension("temp.html");
        tokio::fs::write(&temp_html_path, inlined.as_bytes()).await?;

        let mut command = Command::new("wkhtmltopdf");
        command.args([
            "--page-size",
            "A4",
            "--margin-top",
            "15mm",
            "--margin-bottom",
            "15mm",
            "--margin-left",
            "10mm",
            "--margin-right",
            "10mm",
            "--encoding",
            "UTF-8",
            "--print-media-type",
            "--disable-smart-shrinking",
            "--enable-local-file-access",
            "--load-error-handling",
            "ignore",
            "--load-media-error-handling",
            "ignore",
        ]);

        if compress {
            command.args([
                "--lowquality",
                "--image-quality",
                "50",
                "--image-dpi",
                "150",
            ]);
        }

        command.arg(temp_html_path.to_str().unwrap_or_default());
        command.arg(output_path.to_str().unwrap_or_default());

        let output = command.output()?;

        let _ = tokio::fs::remove_file(&temp_html_path).await;

        if !output.status.success() {
            error!(
                "wkhtmltopdf优化转换失败，退出码: {:?}",
                output.status.code()
            );
            if !output.stderr.is_empty() {
                error!(
                    "wkhtmltopdf优化 stderr: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            if !output.stdout.is_empty() {
                info!(
                    "wkhtmltopdf优化 stdout: {}",
                    String::from_utf8_lossy(&output.stdout)
                );
            }
            return Err(anyhow::anyhow!("优化PDF转换失败"));
        }

        Ok(())
    }
}

fn run_wkhtmltopdf_blocking(html_path: &Path, output_path: &Path, attempt: usize) -> Result<()> {
    let mut command = Command::new("wkhtmltopdf");
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.args([
        "--page-size",
        "A4",
        "--margin-top",
        "20mm",
        "--margin-bottom",
        "20mm",
        "--margin-left",
        "15mm",
        "--margin-right",
        "15mm",
        "--encoding",
        "UTF-8",
        "--print-media-type",
        "--disable-smart-shrinking",
        "--zoom",
        "1.0",
        "--enable-local-file-access",
        "--load-error-handling",
        "ignore",
        "--load-media-error-handling",
        "ignore",
    ]);
    command.arg(html_path.to_str().unwrap_or_default());
    command.arg(output_path.to_str().unwrap_or_default());

    let start = Instant::now();
    let mut child = command
        .spawn()
        .map_err(|e| anyhow::anyhow!("执行wkhtmltopdf失败: {}", e))?;

    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| anyhow::anyhow!("等待wkhtmltopdf失败: {}", e))?
        {
            let output = child
                .wait_with_output()
                .map_err(|e| anyhow::anyhow!("获取wkhtmltopdf输出失败: {}", e))?;
            if !output.status.success() {
                error!(
                    "wkhtmltopdf转换失败，退出码: {:?}, attempt={}",
                    output.status.code(),
                    attempt
                );
                if !output.stderr.is_empty() {
                    error!(
                        "wkhtmltopdf stderr: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                if !output.stdout.is_empty() {
                    debug!(
                        target: "report.pdf",
                        event = events::PIPELINE_STAGE,
                        stage = "wkhtmltopdf_stdout",
                        message = %String::from_utf8_lossy(&output.stdout)
                    );
                }
                return Err(anyhow::anyhow!("PDF转换失败"));
            }
            return Ok(());
        }

        if start.elapsed() > WKHTML_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::anyhow!(
                "wkhtmltopdf 超时({:?})，已终止",
                WKHTML_TIMEOUT
            ));
        }

        std::thread::sleep(Duration::from_millis(200));
    }
}
