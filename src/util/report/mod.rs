//!
//!
//! ```rust
//! use crate::util::report::PreviewReportGenerator;
//!
//! let html = PreviewReportGenerator::generate_html(&evaluation_result);
//!
//! PreviewReportGenerator::html_to_pdf(&html, &output_path).await?;
//! ```

pub mod html;
pub mod pdf;
pub mod styles;
pub mod template;

pub use html::HtmlReportGenerator;
pub use pdf::PdfGenerator;
pub use styles::CssStyleManager;
pub use template::TemplateEngine;

use crate::model::evaluation::*;
use anyhow::Result;
use std::path::Path;

use crate::AppState;
use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::warn;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportFile {
    pub file_type: String,
    pub view_url: String,
    pub download_url: String,
}

pub struct PreviewReportGenerator {
    app_state: AppState,
}

impl PreviewReportGenerator {
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn generate_and_persist_reports(&self, preview_id: &str) -> Result<Vec<ReportFile>> {
        let database = &self.app_state.database;
        let storage = &self.app_state.storage;

        // 1. Get record
        let record = database
            .get_preview_record(preview_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Preview record not found"))?;

        // 2. Parse result
        let result_json = record
            .evaluation_result
            .ok_or_else(|| anyhow::anyhow!("Evaluation result not found"))?;
        let mut result: PreviewEvaluationResult = serde_json::from_str(&result_json)?;

        // 2.5. Enrich attachment URLs with OSS public URLs (for historical records)
        if let Err(e) = crate::api::worker_proxy::enrich_preview_attachments(
            database,
            storage,
            preview_id,
            &mut result,
        )
        .await
        {
            warn!(
                preview_id = %preview_id,
                error = %e,
                "Failed to enrich attachment URLs, continuing with original URLs"
            );
        }

        // 3. Generate HTML
        let html_content = Self::generate_html(&result);

        // 4. Upload HTML
        let html_filename = format!("{}_report.html", preview_id);
        let html_path = format!("previews/{}/{}", preview_id, html_filename);
        storage.put(&html_path, html_content.as_bytes()).await?;

        let html_download_url = format!("/api/preview/download/{}?format=html", preview_id);
        let html_view_url = html_download_url.clone();

        let mut files = vec![ReportFile {
            file_type: "html".to_string(),
            view_url: html_view_url,
            download_url: html_download_url,
        }];

        // 5. Generate PDF (best-effort)
        let pdf_filename = format!("{}_report.pdf", preview_id);
        let pdf_storage_path = format!("previews/{}/{}", preview_id, pdf_filename);
        let temp_dir = std::path::PathBuf::from(&self.app_state.config.master.temp_pdf_dir);
        if let Err(e) = fs::create_dir_all(&temp_dir).await {
            warn!(
                preview_id = %preview_id,
                error = %e,
                "Failed to create temp PDF directory"
            );
        } else {
            let local_pdf_path = temp_dir.join(&pdf_filename);
            match PdfGenerator::html_to_pdf(&html_content, &local_pdf_path).await {
                Ok(_) => match fs::read(&local_pdf_path).await {
                    Ok(bytes) => {
                        if let Err(e) = storage.put(&pdf_storage_path, &bytes).await {
                            warn!(
                                preview_id = %preview_id,
                                error = %e,
                                "Failed to upload PDF report"
                            );
                        }
                    }
                    Err(e) => warn!(
                        preview_id = %preview_id,
                        error = %e,
                        "Failed to read generated PDF"
                    ),
                },
                Err(e) => warn!(
                    preview_id = %preview_id,
                    error = %e,
                    "Failed to generate PDF report"
                ),
            }

            if let Err(e) = fs::remove_file(&local_pdf_path).await {
                warn!(
                    preview_id = %preview_id,
                    error = %e,
                    "Failed to remove temp PDF file"
                );
            }
        }

        let pdf_download_url = format!("/api/preview/download/{}?format=pdf", preview_id);
        files.push(ReportFile {
            file_type: "pdf".to_string(),
            view_url: pdf_download_url.clone(),
            download_url: pdf_download_url,
        });

        Ok(files)
    }

    pub fn generate_html(result: &PreviewEvaluationResult) -> String {
        HtmlReportGenerator::generate_standard_report(result)
    }

    pub fn generate_simple_html(
        matter_name: &str,
        request_id: &str,
        materials: &[String],
    ) -> String {
        HtmlReportGenerator::generate_simple_preview(matter_name, request_id, materials)
    }

    pub fn generate_error_html(request_id: &str, message: &str) -> String {
        HtmlReportGenerator::generate_error_report(request_id, message)
    }

    pub async fn html_to_pdf(html_content: &str, output_path: &Path) -> Result<()> {
        PdfGenerator::html_to_pdf(html_content, output_path).await
    }

    pub async fn generate_pdf_with_watermark(
        result: &PreviewEvaluationResult,
        output_path: &Path,
        watermark_text: &str,
    ) -> Result<()> {
        let html_content = Self::generate_html(result);
        PdfGenerator::html_to_pdf_with_watermark(&html_content, output_path, watermark_text).await
    }

    pub fn generate_from_template(
        template_name: &str,
        result: &PreviewEvaluationResult,
    ) -> Result<String> {
        let template_engine = TemplateEngine::new();
        template_engine
            .render_template(template_name, result)
            .map_err(|e| anyhow::anyhow!("模板生成失败: {}", e))
    }

    pub fn generate_mobile_html(result: &PreviewEvaluationResult) -> String {
        let mut html_content = HtmlReportGenerator::generate_standard_report(result);

        html_content = html_content.replace(styles::get_report_css(), styles::get_mobile_css());

        html_content
    }

    pub fn generate_dark_theme_html(result: &PreviewEvaluationResult) -> String {
        let mut html_content = HtmlReportGenerator::generate_standard_report(result);

        html_content = html_content.replace(styles::get_report_css(), styles::get_dark_theme_css());

        html_content
    }

    pub async fn batch_generate_reports(
        reports: Vec<(&PreviewEvaluationResult, &Path)>,
        format: ReportFormat,
    ) -> Result<Vec<Result<(), anyhow::Error>>> {
        let mut results = Vec::new();

        for (result, output_path) in reports {
            let generation_result = match format {
                ReportFormat::Html => {
                    let html_content = Self::generate_html(result);
                    tokio::fs::write(output_path, html_content)
                        .await
                        .map_err(|e| anyhow::anyhow!("写入HTML文件失败: {}", e))
                }
                ReportFormat::Pdf => {
                    let html_content = Self::generate_html(result);
                    Self::html_to_pdf(&html_content, output_path).await
                }
                ReportFormat::PdfWithWatermark(ref watermark) => {
                    Self::generate_pdf_with_watermark(result, output_path, watermark).await
                }
            };

            results.push(generation_result);
        }

        Ok(results)
    }

    pub fn check_dependencies() -> Result<()> {
        PdfGenerator::check_pdf_tools()
    }
}

#[derive(Debug, Clone)]
pub enum ReportFormat {
    Html,
    Pdf,
    PdfWithWatermark(String),
}

#[derive(Debug, Clone)]
pub struct ReportConfig {
    pub format: ReportFormat,
    pub mobile_friendly: bool,
    pub dark_theme: bool,
    pub template_name: Option<String>,
    pub compress_pdf: bool,
}

impl Default for ReportConfig {
    fn default() -> Self {
        Self {
            format: ReportFormat::Html,
            mobile_friendly: false,
            dark_theme: false,
            template_name: None,
            compress_pdf: false,
        }
    }
}

impl PreviewReportGenerator {
    pub async fn generate_with_config(
        result: &PreviewEvaluationResult,
        output_path: &Path,
        config: &ReportConfig,
    ) -> Result<()> {
        let html_content = if let Some(template_name) = &config.template_name {
            Self::generate_from_template(template_name, result)?
        } else if config.mobile_friendly {
            Self::generate_mobile_html(result)
        } else if config.dark_theme {
            Self::generate_dark_theme_html(result)
        } else {
            Self::generate_html(result)
        };

        match &config.format {
            ReportFormat::Html => {
                tokio::fs::write(output_path, html_content)
                    .await
                    .map_err(|e| anyhow::anyhow!("写入HTML文件失败: {}", e))?;
            }
            ReportFormat::Pdf => {
                if config.compress_pdf {
                    PdfGenerator::html_to_pdf_optimized(&html_content, output_path, true).await?;
                } else {
                    PdfGenerator::html_to_pdf(&html_content, output_path).await?;
                }
            }
            ReportFormat::PdfWithWatermark(watermark) => {
                PdfGenerator::html_to_pdf_with_watermark(&html_content, output_path, watermark)
                    .await?;
            }
        }

        Ok(())
    }
}
