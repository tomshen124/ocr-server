//! HTML报告生成模块
//! 专门负责将评估数据转换为HTML格式的报告

use crate::model::evaluation::*;
use build_html::{Html, HtmlContainer, HtmlPage, Table};

/// HTML报告生成器
pub struct HtmlReportGenerator;

impl HtmlReportGenerator {
    /// 生成标准预审报告HTML
    pub fn generate_standard_report(result: &PreviewEvaluationResult) -> String {
        let mut html = HtmlPage::new()
            .with_title("预审报告")
            .with_meta(vec![("charset", "utf-8")])
            .with_style(super::styles::get_report_css());

        // 1. 报告标题
        html.add_raw("<h1 class=\"report-title\">预审结果报告</h1>");

        // 2. 基础信息表格
        html.add_raw("<div class=\"section\">");
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
        html.add_raw(&format!(
            "<p>报告生成时间: {}</p>",
            result.evaluation_time.format("%Y年%m月%d日 %H:%M:%S")
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
        html.add_raw(&format!("<p><strong>事项名称:</strong> {}</p>", matter_name));
        html.add_raw(&format!("<p><strong>申请编号:</strong> {}</p>", request_id));
        
        html.add_raw("<h2>提交材料</h2>");
        html.add_raw("<ul>");
        for material in materials {
            html.add_raw(&format!("<li>{}</li>", material));
        }
        html.add_raw("</ul>");

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
        Table::from([
            [
                "申请人".to_string(),
                basic_info.applicant_name.clone(),
                "经办人".to_string(),
                basic_info.agent_name.clone(),
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
            OverallResult::Passed => "✅ 预审通过",
            OverallResult::PassedWithSuggestions => "⚠️ 预审通过（有建议）",
            OverallResult::Failed => "❌ 预审不通过",
            OverallResult::RequiresAdditionalMaterials => "📋 需要补充材料",
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
                    summary.overall_suggestions
                        .iter()
                        .map(|s| format!("<li>{}</li>", s))
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
                "✅"
            } else {
                "❌"
            };

            html.push_str(&format!(
                r#"
                <div class="material-item {}">
                    <h3>{} {} - {}</h3>
                    <div class="material-details">
                        <p><strong>材料代码:</strong> {}</p>
                        <p><strong>评估结果:</strong> {}</p>
                        <p><strong>详细说明:</strong> {}</p>
                        {}
                        {}
                    </div>
                </div>
                "#,
                status_class,
                status_icon,
                index + 1,
                material.material_name,
                material.material_code,
                material.rule_evaluation.message,
                material.rule_evaluation.description,
                if !material.attachments.is_empty() {
                    format!(
                        "<p><strong>附件:</strong> {}</p>",
                        material.attachments
                            .iter()
                            .map(|a| a.file_name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    String::new()
                },
                if !material.rule_evaluation.suggestions.is_empty() {
                    format!(
                        "<div class=\"suggestions\"><strong>建议:</strong><ul>{}</ul></div>",
                        material.rule_evaluation.suggestions
                            .iter()
                            .map(|s| format!("<li>{}</li>", s))
                            .collect::<Vec<_>>()
                            .join("")
                    )
                } else {
                    String::new()
                }
            ));
        }

        html
    }
}