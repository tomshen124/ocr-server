use anyhow::{anyhow, Context, Result};
use once_cell::sync::Lazy;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use tempfile::TempDir;
use tokio::sync::Semaphore;
use tokio::task;

use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;

const LIBREOFFICE_MAX_CONCURRENT: usize = 1;
static LIBREOFFICE_SEMAPHORE: Lazy<Semaphore> =
    Lazy::new(|| Semaphore::new(LIBREOFFICE_MAX_CONCURRENT));

/// Convert a DOCX document into PDF bytes using LibreOffice/soffice.
pub async fn docx_to_pdf_bytes(docx: Vec<u8>) -> Result<Vec<u8>> {
    let _permit = LIBREOFFICE_SEMAPHORE
        .acquire()
        .await
        .map_err(|e| anyhow!("获取LibreOffice并发许可失败: {}", e))?;

    let start = Instant::now();
    match task::spawn_blocking(move || convert_docx_blocking(docx)).await {
        Ok(Ok(bytes)) => {
            METRICS_COLLECTOR.record_pipeline_stage(
                "docx_convert",
                true,
                start.elapsed(),
                None,
                None,
            );
            Ok(bytes)
        }
        Ok(Err(err)) => {
            let err_msg = err.to_string();
            METRICS_COLLECTOR.record_pipeline_stage(
                "docx_convert",
                false,
                start.elapsed(),
                None,
                Some(&err_msg),
            );
            Err(err)
        }
        Err(join_err) => {
            let err_msg = join_err.to_string();
            METRICS_COLLECTOR.record_pipeline_stage(
                "docx_convert",
                false,
                start.elapsed(),
                None,
                Some(&err_msg),
            );
            Err(anyhow!(join_err))
        }
    }
}

fn convert_docx_blocking(docx: Vec<u8>) -> Result<Vec<u8>> {
    let temp_dir = tempfile::tempdir().context("创建临时目录失败")?;
    let input_path = temp_dir.path().join("input.docx");
    fs::write(&input_path, &docx).context("写入临时DOCX文件失败")?;

    run_libreoffice_convert(&temp_dir, &input_path)?;
    let pdf_bytes = read_converted_pdf(&temp_dir)?;
    Ok(pdf_bytes)
}

fn run_libreoffice_convert(temp_dir: &TempDir, input_path: &Path) -> Result<()> {
    let outdir = temp_dir.path();
    let status = Command::new("libreoffice")
        .args([
            "--headless",
            "--nologo",
            "--nolockcheck",
            "--invisible",
            "--convert-to",
            "pdf:writer_pdf_Export",
            "--outdir",
            outdir
                .to_str()
                .ok_or_else(|| anyhow!("无效的临时目录路径"))?,
        ])
        .arg(input_path)
        .status();

    let status = match status {
        Ok(status) if status.success() => return Ok(()),
        Ok(_) | Err(_) => Command::new("soffice")
            .args([
                "--headless",
                "--nologo",
                "--nolockcheck",
                "--invisible",
                "--convert-to",
                "pdf:writer_pdf_Export",
                "--outdir",
                outdir
                    .to_str()
                    .ok_or_else(|| anyhow!("无效的临时目录路径"))?,
            ])
            .arg(input_path)
            .status(),
    };

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(anyhow!("LibreOffice 转换失败，退出码: {:?}", status.code())),
        Err(e) => Err(anyhow!("调用 LibreOffice/soffice 失败: {}", e)),
    }
}

fn read_converted_pdf(temp_dir: &TempDir) -> Result<Vec<u8>> {
    // LibreOffice 会在输出目录放置与输入同名的 PDF
    let mut pdf_path = temp_dir.path().join("input.pdf");
    if !pdf_path.exists() {
        // 兼容不同版本可能输出大写或其它命名
        let mut candidate: Option<PathBuf> = None;
        for entry in fs::read_dir(temp_dir.path())? {
            let path = entry?.path();
            if path
                .extension()
                .map(|ext| ext.eq_ignore_ascii_case("pdf"))
                .unwrap_or(false)
            {
                candidate = Some(path);
                break;
            }
        }
        pdf_path = candidate.ok_or_else(|| anyhow!("未找到转换后的PDF文件"))?;
    }

    fs::read(&pdf_path).context("读取转换后的PDF文件失败")
}
