//! 报告生成模块
//! 
//! 这个模块提供了完整的报告生成功能，包括：
//! - HTML报告生成 (html.rs)
//! - PDF报告生成 (pdf.rs)  
//! - CSS样式管理 (styles.rs)
//! - 报告模板管理 (template.rs)
//! 
//! 使用示例：
//! ```rust
//! use crate::util::report::PreviewReportGenerator;
//! 
//! // 生成HTML报告
//! let html = PreviewReportGenerator::generate_html(&evaluation_result);
//! 
//! // 生成PDF报告
//! PreviewReportGenerator::html_to_pdf(&html, &output_path).await?;
//! ```

pub mod html;
pub mod pdf;
pub mod styles;
pub mod template;

// 重新导出主要组件，保持向后兼容
pub use html::HtmlReportGenerator;
pub use pdf::PdfGenerator;
pub use styles::CssStyleManager;
pub use template::TemplateEngine;

use crate::model::evaluation::*;
use anyhow::Result;
use std::path::Path;

/// 预审报告生成器 - 主要接口
/// 提供统一的报告生成入口，保持与原有代码的兼容性
pub struct PreviewReportGenerator;

impl PreviewReportGenerator {
    /// 生成标准预审报告HTML（兼容性接口）
    pub fn generate_html(result: &PreviewEvaluationResult) -> String {
        HtmlReportGenerator::generate_standard_report(result)
    }

    /// 生成简化版HTML（兼容性接口）
    pub fn generate_simple_html(
        matter_name: &str,
        request_id: &str,
        materials: &[String],
    ) -> String {
        HtmlReportGenerator::generate_simple_preview(matter_name, request_id, materials)
    }

    /// 将HTML转换为PDF（兼容性接口）
    pub async fn html_to_pdf(html_content: &str, output_path: &Path) -> Result<()> {
        PdfGenerator::html_to_pdf(html_content, output_path).await
    }

    /// 生成带水印的PDF报告
    pub async fn generate_pdf_with_watermark(
        result: &PreviewEvaluationResult,
        output_path: &Path,
        watermark_text: &str,
    ) -> Result<()> {
        let html_content = Self::generate_html(result);
        PdfGenerator::html_to_pdf_with_watermark(&html_content, output_path, watermark_text).await
    }

    /// 使用模板生成报告
    pub fn generate_from_template(
        template_name: &str,
        result: &PreviewEvaluationResult,
    ) -> Result<String> {
        let template_engine = TemplateEngine::new();
        template_engine.render_template(template_name, result)
            .map_err(|e| anyhow::anyhow!("模板生成失败: {}", e))
    }

    /// 生成移动端友好的HTML报告
    pub fn generate_mobile_html(result: &PreviewEvaluationResult) -> String {
        let mut html_content = HtmlReportGenerator::generate_standard_report(result);
        
        // 替换CSS为移动端样式
        html_content = html_content.replace(
            styles::get_report_css(),
            styles::get_mobile_css()
        );
        
        html_content
    }

    /// 生成深色主题HTML报告
    pub fn generate_dark_theme_html(result: &PreviewEvaluationResult) -> String {
        let mut html_content = HtmlReportGenerator::generate_standard_report(result);
        
        // 替换CSS为深色主题样式
        html_content = html_content.replace(
            styles::get_report_css(),
            styles::get_dark_theme_css()
        );
        
        html_content
    }

    /// 批量生成报告
    pub async fn batch_generate_reports(
        reports: Vec<(&PreviewEvaluationResult, &Path)>,
        format: ReportFormat,
    ) -> Result<Vec<Result<(), anyhow::Error>>> {
        let mut results = Vec::new();
        
        for (result, output_path) in reports {
            let generation_result = match format {
                ReportFormat::Html => {
                    let html_content = Self::generate_html(result);
                    tokio::fs::write(output_path, html_content).await
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

    /// 检查PDF生成工具是否可用
    pub fn check_dependencies() -> Result<()> {
        PdfGenerator::check_pdf_tools()
    }
}

/// 报告生成格式
#[derive(Debug, Clone)]
pub enum ReportFormat {
    /// HTML格式
    Html,
    /// PDF格式
    Pdf,
    /// 带水印的PDF格式
    PdfWithWatermark(String),
}

/// 报告生成配置
#[derive(Debug, Clone)]
pub struct ReportConfig {
    /// 报告格式
    pub format: ReportFormat,
    /// 是否启用移动端样式
    pub mobile_friendly: bool,
    /// 是否使用深色主题
    pub dark_theme: bool,
    /// 自定义模板名称
    pub template_name: Option<String>,
    /// PDF压缩选项
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
    /// 使用配置生成报告
    pub async fn generate_with_config(
        result: &PreviewEvaluationResult,
        output_path: &Path,
        config: &ReportConfig,
    ) -> Result<()> {
        // 生成HTML内容
        let html_content = if let Some(template_name) = &config.template_name {
            Self::generate_from_template(template_name, result)?
        } else if config.mobile_friendly {
            Self::generate_mobile_html(result)
        } else if config.dark_theme {
            Self::generate_dark_theme_html(result)
        } else {
            Self::generate_html(result)
        };

        // 根据配置生成不同格式
        match &config.format {
            ReportFormat::Html => {
                tokio::fs::write(output_path, html_content).await
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
                PdfGenerator::html_to_pdf_with_watermark(&html_content, output_path, watermark).await?;
            }
        }

        Ok(())
    }
}