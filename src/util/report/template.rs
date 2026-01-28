//! 报告模板管理模块
//! 提供可配置的报告模板和数据绑定功能

use crate::model::evaluation::*;
use serde_json::{json, Value};
use std::collections::HashMap;

/// 报告模板引擎
pub struct TemplateEngine {
    templates: HashMap<String, String>,
}

impl TemplateEngine {
    /// 创建新的模板引擎
    pub fn new() -> Self {
        let mut engine = Self {
            templates: HashMap::new(),
        };

        // 加载默认模板
        engine.load_default_templates();
        engine
    }

    /// 加载默认模板
    fn load_default_templates(&mut self) {
        // 标准报告模板
        self.templates
            .insert("standard_report".to_string(), Self::get_standard_template());

        // 简化预览模板
        self.templates
            .insert("simple_preview".to_string(), Self::get_simple_template());

        // 政务格式模板
        self.templates.insert(
            "government_format".to_string(),
            Self::get_government_template(),
        );

        // 统计报告模板
        self.templates.insert(
            "statistics_report".to_string(),
            Self::get_statistics_template(),
        );
    }

    /// 使用模板生成报告
    pub fn render_template(
        &self,
        template_name: &str,
        data: &PreviewEvaluationResult,
    ) -> Result<String, String> {
        let template = self
            .templates
            .get(template_name)
            .ok_or_else(|| format!("模板不存在: {}", template_name))?;

        // 将评估结果转换为模板数据
        let template_data = self.convert_to_template_data(data);

        // 简单的模板替换（实际项目中可以使用handlebars或tera等模板引擎）
        Ok(self.simple_template_replace(template, &template_data))
    }

    /// 注册自定义模板
    pub fn register_template(&mut self, name: String, template: String) {
        self.templates.insert(name, template);
    }

    /// 列出所有可用模板
    pub fn list_templates(&self) -> Vec<&String> {
        self.templates.keys().collect()
    }

    /// 将评估结果转换为模板数据
    fn convert_to_template_data(&self, result: &PreviewEvaluationResult) -> Value {
        json!({
            "basic_info": {
                "applicant_name": result.basic_info.applicant_name,
                "agent_name": result.basic_info.agent_name,
                "matter_name": result.basic_info.matter_name,
                "matter_type": result.basic_info.matter_type,
                "request_id": result.basic_info.request_id,
                "sequence_no": result.basic_info.sequence_no,
                "theme_id": result.basic_info.theme_id,
                "theme_name": result.basic_info.theme_name,
            },
            "evaluation_summary": {
                "total_materials": result.evaluation_summary.total_materials,
                "passed_materials": result.evaluation_summary.passed_materials,
                "failed_materials": result.evaluation_summary.failed_materials,
                "warning_materials": result.evaluation_summary.warning_materials,
                "overall_result": match result.evaluation_summary.overall_result {
                    OverallResult::Passed => "通过",
                    OverallResult::PassedWithSuggestions => "通过（有建议）",
                    OverallResult::Failed => "不通过",
                    OverallResult::RequiresAdditionalMaterials => "需要补充材料",
                },
                "overall_result_class": match result.evaluation_summary.overall_result {
                    OverallResult::Passed => "result-success",
                    OverallResult::PassedWithSuggestions => "result-warning",
                    OverallResult::Failed | OverallResult::RequiresAdditionalMaterials => "result-error",
                },
                "suggestions": result.evaluation_summary.overall_suggestions,
            },
            "materials": result.material_results.iter().enumerate().map(|(index, material)| {
                json!({
                    "index": index + 1,
                    "material_code": material.material_code,
                    "material_name": material.material_name,
                    "status_icon": if material.rule_evaluation.status_code == 200 { "[ok]" } else { "[fail]" },
                    "status_class": if material.rule_evaluation.status_code == 200 { "material-success" } else { "material-error" },
                    "message": material.rule_evaluation.message,
                    "description": material.rule_evaluation.description,
                    "suggestions": material.rule_evaluation.suggestions,
                    "attachments": material.attachments.iter().map(|a| a.file_name.as_str()).collect::<Vec<_>>(),
                })
            }).collect::<Vec<_>>(),
            "evaluation_time": result.evaluation_time.format("%Y年%m月%d日 %H:%M:%S").to_string(),
            "current_time": chrono::Local::now().format("%Y年%m月%d日 %H:%M:%S").to_string(),
        })
    }

    /// 简单的模板变量替换
    fn simple_template_replace(&self, template: &str, data: &Value) -> String {
        let mut result = template.to_string();

        // 替换基础信息变量
        if let Some(basic_info) = data.get("basic_info") {
            result = result.replace(
                "{{applicant_name}}",
                basic_info["applicant_name"].as_str().unwrap_or(""),
            );
            result = result.replace(
                "{{agent_name}}",
                basic_info["agent_name"].as_str().unwrap_or(""),
            );
            result = result.replace(
                "{{matter_name}}",
                basic_info["matter_name"].as_str().unwrap_or(""),
            );
            result = result.replace(
                "{{matter_type}}",
                basic_info["matter_type"].as_str().unwrap_or(""),
            );
            result = result.replace(
                "{{request_id}}",
                basic_info["request_id"].as_str().unwrap_or(""),
            );
            result = result.replace(
                "{{sequence_no}}",
                basic_info["sequence_no"].as_str().unwrap_or(""),
            );
        }

        // 替换评估摘要变量
        if let Some(summary) = data.get("evaluation_summary") {
            result = result.replace(
                "{{total_materials}}",
                &summary["total_materials"].to_string(),
            );
            result = result.replace(
                "{{passed_materials}}",
                &summary["passed_materials"].to_string(),
            );
            result = result.replace(
                "{{failed_materials}}",
                &summary["failed_materials"].to_string(),
            );
            result = result.replace(
                "{{overall_result}}",
                summary["overall_result"].as_str().unwrap_or(""),
            );
            result = result.replace(
                "{{overall_result_class}}",
                summary["overall_result_class"].as_str().unwrap_or(""),
            );
        }

        // 替换时间变量
        result = result.replace(
            "{{evaluation_time}}",
            data["evaluation_time"].as_str().unwrap_or(""),
        );
        result = result.replace(
            "{{current_time}}",
            data["current_time"].as_str().unwrap_or(""),
        );

        result
    }

    /// 标准报告模板
    fn get_standard_template() -> String {
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <title>预审报告</title>
            <style>
                /* 这里会插入CSS样式 */
            </style>
        </head>
        <body>
            <h1 class="report-title">预审结果报告</h1>
            
            <div class="section">
                <h2>基础信息</h2>
                <table>
                    <tr><td>申请人</td><td>{{applicant_name}}</td><td>经办人</td><td>{{agent_name}}</td></tr>
                    <tr><td>事项名称</td><td>{{matter_name}}</td><td>事项类型</td><td>{{matter_type}}</td></tr>
                    <tr><td>办件流水号</td><td>{{request_id}}</td><td>预审编号</td><td>{{sequence_no}}</td></tr>
                </table>
            </div>
            
            <div class="section">
                <h2>评估摘要</h2>
                <div class="summary-box">
                    <div class="summary-result {{overall_result_class}}">
                        <h3>{{overall_result}}</h3>
                    </div>
                    <div class="summary-stats">
                        <p>总材料数: <strong>{{total_materials}}</strong></p>
                        <p>通过材料: <strong class="text-success">{{passed_materials}}</strong></p>
                        <p>不通过材料: <strong class="text-error">{{failed_materials}}</strong></p>
                    </div>
                </div>
            </div>
            
            <div class="footer">
                <p>报告生成时间: {{evaluation_time}}</p>
                <p>本报告由智能预审系统自动生成</p>
            </div>
        </body>
        </html>
        "#.to_string()
    }

    /// 简化预览模板
    fn get_simple_template() -> String {
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <title>预审预览</title>
        </head>
        <body>
            <h1>预审预览</h1>
            <p><strong>事项名称:</strong> {{matter_name}}</p>
            <p><strong>申请编号:</strong> {{request_id}}</p>
            <p><strong>整体结果:</strong> {{overall_result}}</p>
            <p><strong>生成时间:</strong> {{current_time}}</p>
        </body>
        </html>
        "#
        .to_string()
    }

    /// 政务格式模板
    fn get_government_template() -> String {
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <title>智能预审报告</title>
            <style>
                body { font-family: '仿宋', serif; line-height: 1.8; }
                .header { text-align: center; font-size: 18px; font-weight: bold; margin-bottom: 30px; }
                .content { padding: 20px; }
                .footer { text-align: right; margin-top: 40px; }
            </style>
        </head>
        <body>
            <div class="header">
                <h1>智能预审报告</h1>
                <p>编号: {{sequence_no}}</p>
            </div>
            
            <div class="content">
                <p>申请人: {{applicant_name}}</p>
                <p>经办人: {{agent_name}}</p>
                <p>事项名称: {{matter_name}}</p>
                <p>办件流水号: {{request_id}}</p>
                
                <h3>预审结果</h3>
                <p>{{overall_result}}</p>
                
                <h3>材料统计</h3>
                <p>提交材料总数: {{total_materials}} 份</p>
                <p>审核通过: {{passed_materials}} 份</p>
                <p>需要补正: {{failed_materials}} 份</p>
            </div>
            
            <div class="footer">
                <p>预审时间: {{evaluation_time}}</p>
                <p>智能预审系统</p>
            </div>
        </body>
        </html>
        "#.to_string()
    }

    /// 统计报告模板
    fn get_statistics_template() -> String {
        r#"
        <!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <title>预审统计报告</title>
        </head>
        <body>
            <h1>预审统计报告</h1>
            
            <div class="statistics">
                <h2>统计概览</h2>
                <div class="stat-item">
                    <label>事项名称:</label>
                    <span>{{matter_name}}</span>
                </div>
                <div class="stat-item">
                    <label>材料总数:</label>
                    <span>{{total_materials}}</span>
                </div>
                <div class="stat-item">
                    <label>通过率:</label>
                    <span>{{pass_rate}}%</span>
                </div>
                <div class="stat-item">
                    <label>统计时间:</label>
                    <span>{{current_time}}</span>
                </div>
            </div>
        </body>
        </html>
        "#
        .to_string()
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new()
    }
}
