//! HTML报告生成模块
//! 专门负责将评估数据转换为HTML格式的报告

use crate::model::evaluation::*;
use build_html::{Html, HtmlContainer, HtmlPage, Table};
use chrono::{FixedOffset, Utc};
use regex::Regex;
use std::sync::OnceLock;

/// HTML报告生成器
pub struct HtmlReportGenerator;

impl HtmlReportGenerator {
    /// 生成标准预审报告HTML
    pub fn generate_standard_report(result: &PreviewEvaluationResult) -> String {
        let mut html = HtmlPage::new()
            .with_title("预审报告")
            .with_meta(vec![("charset", "utf-8")])
            .with_style(super::styles::get_report_css());

        // 1. 报告标题 + 基础信息放入同一版块，避免空白封面页
        html.add_raw("<div class=\"section report-header\">");
        html.add_raw("<h1 class=\"report-title\">预审结果报告</h1>");
        html.add_raw("<h2>基础信息</h2>");
        html.add_table(Self::build_basic_info_table(&result.basic_info));
        html.add_raw("</div>");

        // 3. 评估摘要
        html.add_raw("<div class=\"section\">");
        html.add_raw("<h2>评估摘要</h2>");
        html.add_raw(&Self::build_summary_html(&result.evaluation_summary));
        html.add_raw("</div>");

        // 4. 材料详细评估结果
        html.add_raw("<div class=\"section\">");
        html.add_raw("<h2>材料评估详情</h2>");
        html.add_raw(&Self::build_materials_html(&result.material_results));
        html.add_raw("</div>");

        // 5. 报告尾部
        html.add_raw("<div class=\"footer\">");
        let offset = FixedOffset::east_opt(8 * 3600).expect("valid east offset");
        let beijing_time = result.evaluation_time.with_timezone(&offset);
        html.add_raw(&format!(
            "<p>报告生成时间（北京时间）: {}</p>",
            beijing_time.format("%Y年%m月%d日 %H:%M:%S %:z")
        ));
        html.add_raw("<p>本报告由智能预审系统自动生成</p>");
        html.add_raw("</div>");

        html.to_html_string()
    }

    /// 生成简化版HTML（用于快速预览）
    pub fn generate_simple_preview(
        matter_name: &str,
        request_id: &str,
        materials: &[String],
    ) -> String {
        let mut html = HtmlPage::new()
            .with_title("预审预览")
            .with_meta(vec![("charset", "utf-8")])
            .with_style(super::styles::get_simple_css());

        html.add_raw("<h1>预审预览</h1>");
        html.add_raw(&format!(
            "<p><strong>事项名称:</strong> {}</p>",
            matter_name
        ));
        html.add_raw(&format!("<p><strong>申请编号:</strong> {}</p>", request_id));

        html.add_raw("<h2>提交材料</h2>");
        html.add_raw("<ul>");
        for material in materials {
            html.add_raw(&format!("<li>{}</li>", material));
        }
        html.add_raw("</ul>");

        html.to_html_string()
    }

    /// 生成错误报告HTML
    pub fn generate_error_report(request_id: &str, message: &str) -> String {
        let mut html = HtmlPage::new()
            .with_title("预审报告 - 异常")
            .with_meta(vec![("charset", "utf-8")])
            .with_style(super::styles::get_report_css());

        html.add_raw("<h1 class=\"report-title\">预审报告暂不可用</h1>");
        html.add_raw("<div class=\"section\">");
        html.add_raw("<h2>基础信息</h2>");
        let table = Table::from([[
            "预审编号".to_string(),
            request_id.to_string(),
            "状态".to_string(),
            "生成失败".to_string(),
        ]]);
        html.add_table(table);
        html.add_raw("</div>");

        html.add_raw("<div class=\"section\">");
        html.add_raw("<h2>错误详情</h2>");
        let safe_message = message
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        html.add_raw(&format!(
            "<div class=\"error-message\"><p>{}</p><p>请稍后重试或联系运维人员处理。</p></div>",
            safe_message
        ));
        html.add_raw("</div>");

        html.add_raw("<div class=\"footer\">");
        let offset = FixedOffset::east_opt(8 * 3600).expect("valid east offset");
        let beijing_now = Utc::now().with_timezone(&offset);
        html.add_raw(&format!(
            "<p>报告生成时间（北京时间）: {}</p>",
            beijing_now.format("%Y年%m月%d日 %H:%M:%S %:z")
        ));
        html.add_raw("<p>本报告由智能预审系统自动生成</p>");
        html.add_raw("</div>");

        html.to_html_string()
    }

    /// 生成材料对比HTML（用于材料差异分析）
    pub fn generate_material_comparison(
        original_materials: &[MaterialEvaluationResult],
        updated_materials: &[MaterialEvaluationResult],
    ) -> String {
        let mut html = HtmlPage::new()
            .with_title("材料对比报告")
            .with_meta(vec![("charset", "utf-8")])
            .with_style(super::styles::get_comparison_css());

        html.add_raw("<h1>材料变更对比报告</h1>");

        html.add_raw("<div class=\"comparison-container\">");
        html.add_raw("<div class=\"original-section\">");
        html.add_raw("<h2>原始材料</h2>");
        html.add_raw(&Self::build_materials_html(original_materials));
        html.add_raw("</div>");

        html.add_raw("<div class=\"updated-section\">");
        html.add_raw("<h2>更新材料</h2>");
        html.add_raw(&Self::build_materials_html(updated_materials));
        html.add_raw("</div>");
        html.add_raw("</div>");

        html.to_html_string()
    }

    /// 构建基础信息表格
    fn build_basic_info_table(basic_info: &BasicInfo) -> Table {
        let display = |value: &Option<String>| {
            value
                .as_deref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("-")
                .to_string()
        };

        Table::from([
            [
                "申请人".to_string(),
                basic_info.applicant_name.clone(),
                "经办人".to_string(),
                basic_info.agent_name.clone(),
            ],
            [
                "申请人证件号".to_string(),
                display(&basic_info.applicant_certificate_number),
                "经办人证件号".to_string(),
                display(&basic_info.agent_certificate_number),
            ],
            [
                "申请人联系方式".to_string(),
                display(&basic_info.applicant_phone),
                "经办人联系方式".to_string(),
                display(&basic_info.agent_phone),
            ],
            [
                "申请人单位".to_string(),
                display(&basic_info.applicant_org),
                "经办人单位".to_string(),
                display(&basic_info.agent_org),
            ],
            [
                "事项名称".to_string(),
                basic_info.matter_name.clone(),
                "事项类型".to_string(),
                basic_info.matter_type.clone(),
            ],
            [
                "办件流水号".to_string(),
                basic_info.request_id.clone(),
                "预审编号".to_string(),
                basic_info.sequence_no.clone(),
            ],
            [
                "使用规则".to_string(),
                format!("{} ({})", basic_info.theme_name, basic_info.theme_id),
                "评估状态".to_string(),
                "已完成".to_string(),
            ],
        ])
    }

    /// 构建评估摘要HTML
    fn build_summary_html(summary: &EvaluationSummary) -> String {
        let result_class = match summary.overall_result {
            OverallResult::Passed => "result-success",
            OverallResult::PassedWithSuggestions => "result-warning",
            OverallResult::Failed | OverallResult::RequiresAdditionalMaterials => "result-error",
        };

        let result_text = match summary.overall_result {
            OverallResult::Passed => "[ok] 预审通过",
            OverallResult::PassedWithSuggestions => "[warn] 预审通过（有建议）",
            OverallResult::Failed => "[fail] 预审不通过",
            OverallResult::RequiresAdditionalMaterials => "[clipboard] 需要补充材料",
        };

        format!(
            r#"
            <div class="summary-box">
                <div class="summary-result {}">
                    <h3>{}</h3>
                </div>
                <div class="summary-stats">
                    <p>总材料数: <strong>{}</strong></p>
                    <p>通过材料: <strong class="text-success">{}</strong></p>
                    <p>不通过材料: <strong class="text-error">{}</strong></p>
                    <p>有警告材料: <strong class="text-warning">{}</strong></p>
                </div>
                {}
            </div>
            "#,
            result_class,
            result_text,
            summary.total_materials,
            summary.passed_materials,
            summary.failed_materials,
            summary.warning_materials,
            if !summary.overall_suggestions.is_empty() {
                format!(
                    "<div class=\"suggestions\"><h4>总体建议:</h4><ul>{}</ul></div>",
                    summary
                        .overall_suggestions
                        .iter()
                        .map(|s| format!("<li>{}</li>", Self::sanitize_and_escape(s)))
                        .collect::<Vec<_>>()
                        .join("")
                )
            } else {
                String::new()
            }
        )
    }

    /// 构建材料评估结果HTML
    fn build_materials_html(materials: &[MaterialEvaluationResult]) -> String {
        let mut html = String::new();

        for (index, material) in materials.iter().enumerate() {
            let status_class = if material.rule_evaluation.status_code == 200 {
                "material-success"
            } else {
                "material-error"
            };

            let status_icon = if material.rule_evaluation.status_code == 200 {
                "[ok]"
            } else {
                "[fail]"
            };

            let attachments_html = if !material.attachments.is_empty() {
                let mut items = String::new();
                items.push_str("<div class=\"attachments\">");

                for attachment in &material.attachments {
                    let name = Self::escape_html(&attachment.file_name);
                    let link_url = attachment.file_url.trim();
                    let meta = Self::build_attachment_meta(attachment);
                    let preview = Self::attachment_preview_src(attachment)
                        .map(|src| {
                            format!(
                                r#"<div class="attachment-preview"><img src="{src}" alt="{alt}" loading="lazy" /></div>"#,
                                src = src,
                                alt = name
                            )
                        })
                        .unwrap_or_default();

                    items.push_str(&format!(
                        r#"<div class="attachment-item">
                                <a class="attachment-name attachment-link" href="{url}" target="_blank" rel="noopener">{name}</a>
                                {meta}{preview}
                           </div>"#,
                        url = link_url,
                        name = name,
                        meta = meta,
                        preview = preview
                    ));
                }

                items.push_str("</div>");
                items
            } else {
                String::new()
            };

            let suggestions_html = if !material.rule_evaluation.suggestions.is_empty() {
                format!(
                    "<div class=\"suggestions\"><strong>建议:</strong><ul>{}</ul></div>",
                    material
                        .rule_evaluation
                        .suggestions
                        .iter()
                        .map(|s| format!("<li>{}</li>", Self::sanitize_and_escape(s)))
                        .collect::<Vec<_>>()
                        .join("")
                )
            } else {
                String::new()
            };

            let summary_text = Self::material_summary_text(material);
            let detail_text = Self::material_detail_text(material);
            let raw_details_html = Self::build_raw_details_html(material);

            html.push_str(&format!(
                r#"
                <div class="material-item {}">
                    <h3>{} {} - {}</h3>
                    <div class="material-details">
                        <p><strong>材料代码:</strong> {}</p>
                        <p><strong>评估结果:</strong> {}</p>
                        <p><strong>详细说明:</strong> {}</p>
                        {attachments}
                        {suggestions}
                        {raw_details}
                    </div>
                </div>
                "#,
                status_class,
                status_icon,
                index + 1,
                material.material_name,
                material.material_code,
                summary_text,
                detail_text,
                attachments = attachments_html,
                suggestions = suggestions_html,
                raw_details = raw_details_html
            ));
        }

        html
    }

    fn escape_html(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    fn sanitize_and_escape(value: &str) -> String {
        Self::escape_html(&Self::sanitize_text(value))
    }

    fn sanitize_text(value: &str) -> String {
        static PATH_RE: OnceLock<Regex> = OnceLock::new();
        static MATERIAL_RE: OnceLock<Regex> = OnceLock::new();
        static INDEX_RE: OnceLock<Regex> = OnceLock::new();

        let mut text = value.replace("\r\n", "\n");

        let path_re = PATH_RE
            .get_or_init(|| Regex::new(r"(/app/\S+|/tmp/\S+|/var/\S+|/home/\S+|/opt/\S+)").unwrap());
        text = path_re.replace_all(&text, "[路径已省略]").into_owned();

        let material_re = MATERIAL_RE
            .get_or_init(|| Regex::new(r"material=[^,;\s]+").unwrap());
        text = material_re.replace_all(&text, "材料").into_owned();

        let index_re = INDEX_RE.get_or_init(|| Regex::new(r"index=\d+").unwrap());
        text = index_re.replace_all(&text, "").into_owned();

        text.split_whitespace().collect::<Vec<_>>().join(" ")
    }

    fn build_attachment_meta(attachment: &AttachmentInfo) -> String {
        let mut meta_parts = Vec::new();

        if let Some(size) = attachment.file_size {
            meta_parts.push(Self::format_file_size(size));
        }

        if let Some(pages) = attachment.page_count {
            meta_parts.push(format!("{}页", pages));
        }

        if attachment.is_cloud_share {
            meta_parts.push("云端文件".to_string());
        }

        if !attachment.ocr_success {
            meta_parts.push("OCR识别失败".to_string());
        }

        if meta_parts.is_empty() {
            String::new()
        } else {
            format!(
                r#"<div class="attachment-meta">{}</div>"#,
                meta_parts.join(" · ")
            )
        }
    }

    fn format_file_size(size: u64) -> String {
        const KB: f64 = 1024.0;
        const MB: f64 = KB * 1024.0;
        const GB: f64 = MB * 1024.0;

        let size_f = size as f64;
        if size_f >= GB {
            format!("{:.2} GB", size_f / GB)
        } else if size_f >= MB {
            format!("{:.1} MB", size_f / MB)
        } else if size_f >= KB {
            format!("{:.0} KB", size_f / KB)
        } else {
            format!("{} B", size)
        }
    }

    fn attachment_preview_src(attachment: &AttachmentInfo) -> Option<&str> {
        attachment
            .preview_url
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| {
                attachment
                    .thumbnail_url
                    .as_deref()
                    .filter(|s| !s.is_empty())
            })
            .or_else(|| {
                let url = attachment.file_url.trim();
                if !url.is_empty() && Self::is_image_url(url, attachment) {
                    Some(url)
                } else {
                    None
                }
            })
    }

    fn material_summary_text(material: &MaterialEvaluationResult) -> String {
        if let Some(summary) = material
            .display_summary
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return Self::sanitize_and_escape(summary);
        }

        if material.rule_evaluation.status_code == 200 {
            return "系统自动核验通过".to_string();
        }

        if let Some(first) = material.rule_evaluation.suggestions.first() {
            return Self::sanitize_and_escape(first);
        }

        if material.rule_evaluation.message.trim().is_empty() {
            "请人工复核".to_string()
        } else {
            Self::sanitize_and_escape(&material.rule_evaluation.message)
        }
    }

    fn material_detail_text(material: &MaterialEvaluationResult) -> String {
        if let Some(detail) = material
            .display_detail
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            return Self::sanitize_and_escape(detail);
        }

        match &material.processing_status {
            ProcessingStatus::Success => "材料内容与规则匹配，系统自动通过".to_string(),
            ProcessingStatus::PartialSuccess { warnings } => {
                if warnings.is_empty() {
                    "部分字段需要人工确认".to_string()
                } else {
                    warnings
                        .iter()
                        .map(|w| Self::sanitize_and_escape(w))
                        .collect::<Vec<_>>()
                        .join("；")
                }
            }
            ProcessingStatus::Failed { error } => {
                if error.trim().is_empty() {
                    "系统自动校验未通过，请人工复核".to_string()
                } else {
                    Self::sanitize_and_escape(error)
                }
            }
        }
    }

    fn build_raw_details_html(material: &MaterialEvaluationResult) -> String {
        let mut sections = Vec::new();

        if !material.ocr_content.trim().is_empty() {
            sections.push(format!(
                r#"<div class="ocr-text"><strong>OCR识别文本:</strong><pre>{}</pre></div>"#,
                Self::sanitize_and_escape(&material.ocr_content)
            ));
        }

        if !material.rule_evaluation.message.trim().is_empty() {
            sections.push(format!(
                r#"<div class="rule-message"><strong>系统说明:</strong><p>{}</p></div>"#,
                Self::sanitize_and_escape(&material.rule_evaluation.message)
            ));
        }

        if sections.is_empty() {
            return String::new();
        }

        format!(
            r#"<details class="raw-evidence"><summary>查看识别详情</summary>{}</details>"#,
            sections.join("")
        )
    }

    fn is_image_url(url: &str, attachment: &AttachmentInfo) -> bool {
        if let Some(mime) = attachment
            .mime_type
            .as_deref()
            .map(|m| m.to_ascii_lowercase())
        {
            if mime.starts_with("image/") {
                return true;
            }
        }

        if let Some(ext) = attachment
            .file_type
            .as_deref()
            .map(|e| e.to_ascii_lowercase())
        {
            if matches!(
                ext.as_str(),
                "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "svg"
            ) {
                return true;
            }
        }

        let lowered = url.to_ascii_lowercase();
        lowered.ends_with(".jpg")
            || lowered.ends_with(".jpeg")
            || lowered.ends_with(".png")
            || lowered.ends_with(".gif")
            || lowered.ends_with(".bmp")
            || lowered.ends_with(".webp")
            || lowered.ends_with(".svg")
    }
}
