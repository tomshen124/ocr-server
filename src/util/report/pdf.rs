//! PDF报告生成模块
//! 负责将HTML内容转换为PDF格式

use anyhow::Result;
use std::path::Path;
use std::process::Command;
use tracing::{info, warn, error};

/// PDF生成器
pub struct PdfGenerator;

impl PdfGenerator {
    /// 将HTML转换为PDF
    pub async fn html_to_pdf(html_content: &str, output_path: &Path) -> Result<()> {
        info!("开始PDF转换: {}", output_path.display());
        
        // 先保存HTML到临时文件
        let temp_html_path = output_path.with_extension("temp.html");
        tokio::fs::write(&temp_html_path, html_content).await
            .map_err(|e| anyhow::anyhow!("无法写入临时HTML文件: {}", e))?;

        // 使用wkhtmltopdf转换
        let result = Self::convert_with_wkhtmltopdf(&temp_html_path, output_path).await;

        // 清理临时文件
        if let Err(e) = tokio::fs::remove_file(&temp_html_path).await {
            warn!("清理临时文件失败: {} - {}", temp_html_path.display(), e);
        }

        result
    }

    /// 生成带水印的PDF
    pub async fn html_to_pdf_with_watermark(
        html_content: &str, 
        output_path: &Path,
        watermark_text: &str
    ) -> Result<()> {
        info!("开始生成带水印的PDF: {}", output_path.display());
        
        // 在HTML内容中添加水印样式
        let watermarked_html = Self::add_watermark_to_html(html_content, watermark_text);
        
        Self::html_to_pdf(&watermarked_html, output_path).await
    }

    /// 批量生成PDF报告
    pub async fn batch_generate_pdfs(
        reports: Vec<(String, &Path)>
    ) -> Result<Vec<Result<(), anyhow::Error>>> {
        info!("开始批量生成PDF，共{}个文件", reports.len());
        
        let mut results = Vec::new();
        
        for (html_content, output_path) in reports {
            let result = Self::html_to_pdf(&html_content, output_path).await;
            results.push(result);
        }
        
        Ok(results)
    }

    /// 使用wkhtmltopdf转换HTML到PDF
    async fn convert_with_wkhtmltopdf(
        html_path: &Path, 
        output_path: &Path
    ) -> Result<()> {
        let status = Command::new("wkhtmltopdf")
            .args([
                "--page-size", "A4",
                "--margin-top", "20mm",
                "--margin-bottom", "20mm",
                "--margin-left", "15mm",
                "--margin-right", "15mm",
                "--encoding", "UTF-8",
                "--print-media-type",
                "--disable-smart-shrinking",
                "--zoom", "1.0",
                html_path.to_str().unwrap_or_default(),
                output_path.to_str().unwrap_or_default(),
            ])
            .status()
            .map_err(|e| anyhow::anyhow!("执行wkhtmltopdf失败: {}", e))?;

        if !status.success() {
            error!("wkhtmltopdf转换失败，退出码: {:?}", status.code());
            return Err(anyhow::anyhow!("PDF转换失败"));
        }

        info!("PDF转换成功: {}", output_path.display());
        Ok(())
    }

    /// 在HTML中添加水印
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

        // 在HTML body标签后插入水印
        if let Some(body_pos) = html_content.find("<body") {
            if let Some(body_end) = html_content[body_pos..].find('>') {
                let insert_pos = body_pos + body_end + 1;
                let mut result = html_content.to_string();
                result.insert_str(insert_pos, &watermark_style);
                return result;
            }
        }

        // 如果找不到body标签，直接追加到末尾
        format!("{}{}", html_content, watermark_style)
    }

    /// 检查PDF转换工具是否可用
    pub fn check_pdf_tools() -> Result<()> {
        // 检查wkhtmltopdf是否安装
        let status = Command::new("wkhtmltopdf")
            .arg("--version")
            .status();

        match status {
            Ok(status) if status.success() => {
                info!("✅ wkhtmltopdf工具可用");
                Ok(())
            }
            Ok(_) => {
                warn!("⚠️ wkhtmltopdf工具执行失败");
                Err(anyhow::anyhow!("wkhtmltopdf工具不可用"))
            }
            Err(e) => {
                warn!("⚠️ wkhtmltopdf工具未安装: {}", e);
                Err(anyhow::anyhow!("wkhtmltopdf工具未安装: {}", e))
            }
        }
    }

    /// 优化PDF设置（用于大文件）
    pub async fn html_to_pdf_optimized(
        html_content: &str, 
        output_path: &Path,
        compress: bool
    ) -> Result<()> {
        info!("开始优化PDF转换: {}", output_path.display());
        
        let temp_html_path = output_path.with_extension("temp.html");
        tokio::fs::write(&temp_html_path, html_content).await?;

        let mut args = vec![
            "--page-size", "A4",
            "--margin-top", "15mm",
            "--margin-bottom", "15mm", 
            "--margin-left", "10mm",
            "--margin-right", "10mm",
            "--encoding", "UTF-8",
            "--print-media-type",
            "--disable-smart-shrinking",
        ];

        if compress {
            args.extend_from_slice(&[
                "--lowquality",
                "--image-quality", "50",
                "--image-dpi", "150",
            ]);
        }

        args.push(temp_html_path.to_str().unwrap_or_default());
        args.push(output_path.to_str().unwrap_or_default());

        let status = Command::new("wkhtmltopdf")
            .args(&args)
            .status()?;

        // 清理临时文件
        let _ = tokio::fs::remove_file(&temp_html_path).await;

        if !status.success() {
            return Err(anyhow::anyhow!("优化PDF转换失败"));
        }

        Ok(())
    }
}