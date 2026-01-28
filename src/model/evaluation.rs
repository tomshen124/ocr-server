use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 预审评估结果 - 纯数据结构，不包含任何展示逻辑
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEvaluationResult {
    /// 基础信息
    pub basic_info: BasicInfo,
    /// 材料评估结果
    pub material_results: Vec<MaterialEvaluationResult>,
    /// 评估摘要
    pub evaluation_summary: EvaluationSummary,
    /// 评估时间
    pub evaluation_time: DateTime<Local>,
}

/// 基础信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicInfo {
    /// 申请人信息
    pub applicant_name: String,
    pub applicant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicant_org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicant_phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicant_certificate_number: Option<String>,
    /// 经办人信息
    pub agent_name: String,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_certificate_number: Option<String>,
    /// 事项信息
    pub matter_name: String,
    pub matter_id: String,
    pub matter_type: String,
    /// 流水号信息
    pub request_id: String,
    pub sequence_no: String,
    /// 使用的规则主题
    pub theme_id: String,
    pub theme_name: String,
}

/// 单个材料的评估结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialEvaluationResult {
    /// 材料基本信息
    pub material_code: String,
    pub material_name: String,
    /// 附件信息
    pub attachments: Vec<AttachmentInfo>,
    /// OCR识别结果
    pub ocr_content: String,
    /// 规则评估结果
    pub rule_evaluation: RuleEvaluationResult,
    /// 处理状态
    pub processing_status: ProcessingStatus,
    /// 面向用户的摘要
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_summary: Option<String>,
    /// 面向用户的详细提示
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_detail: Option<String>,
}

/// 附件信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AttachmentInfo {
    pub file_name: String,
    pub file_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub is_cloud_share: bool,
    pub ocr_success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// 规则评估结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEvaluationResult {
    /// 评估状态码 (200=通过, 500=不通过)
    pub status_code: u64,
    /// 评估消息
    pub message: String,
    /// 详细描述
    pub description: String,
    /// 建议或要求
    pub suggestions: Vec<String>,
    /// 规则匹配详情
    pub rule_details: Option<serde_json::Value>,
}

/// 处理状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingStatus {
    /// 处理成功
    Success,
    /// 部分成功（如部分文件OCR失败）
    PartialSuccess { warnings: Vec<String> },
    /// 处理失败
    Failed { error: String },
}

/// 评估摘要
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationSummary {
    /// 总材料数
    pub total_materials: usize,
    /// 通过的材料数
    pub passed_materials: usize,
    /// 不通过的材料数
    pub failed_materials: usize,
    /// 有警告的材料数
    pub warning_materials: usize,
    /// 整体评估结果
    pub overall_result: OverallResult,
    /// 总体建议
    pub overall_suggestions: Vec<String>,
}

/// 整体评估结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverallResult {
    /// 预审通过
    Passed,
    /// 预审通过但有建议
    PassedWithSuggestions,
    /// 预审不通过
    Failed,
    /// 需要补充材料
    RequiresAdditionalMaterials,
}

impl PreviewEvaluationResult {
    /// 创建新的评估结果
    pub fn new(basic_info: BasicInfo) -> Self {
        Self {
            basic_info,
            material_results: Vec::new(),
            evaluation_summary: EvaluationSummary {
                total_materials: 0,
                passed_materials: 0,
                failed_materials: 0,
                warning_materials: 0,
                overall_result: OverallResult::Passed,
                overall_suggestions: Vec::new(),
            },
            evaluation_time: Local::now(),
        }
    }

    /// 添加材料评估结果
    pub fn add_material_result(&mut self, result: MaterialEvaluationResult) {
        self.material_results.push(result);
        self.update_summary();
    }

    /// 更新评估摘要
    fn update_summary(&mut self) {
        self.evaluation_summary.total_materials = self.material_results.len();
        self.evaluation_summary.passed_materials = self
            .material_results
            .iter()
            .filter(|r| r.rule_evaluation.status_code == 200)
            .count();
        self.evaluation_summary.failed_materials = self
            .material_results
            .iter()
            .filter(|r| r.rule_evaluation.status_code != 200)
            .count();
        self.evaluation_summary.warning_materials = self
            .material_results
            .iter()
            .filter(|r| matches!(r.processing_status, ProcessingStatus::PartialSuccess { .. }))
            .count();

        // 确定整体结果
        self.evaluation_summary.overall_result = if self.evaluation_summary.failed_materials > 0 {
            OverallResult::Failed
        } else if self.evaluation_summary.warning_materials > 0 {
            OverallResult::PassedWithSuggestions
        } else {
            OverallResult::Passed
        };
    }
}

impl MaterialEvaluationResult {
    /// 创建简单的材料评估结果
    pub fn new_simple(
        material_code: String,
        material_name: String,
        ocr_content: String,
        status_code: u64,
        message: String,
        suggestions: Vec<String>,
    ) -> Self {
        Self {
            material_code,
            material_name,
            attachments: vec![], // 可以后续添加
            ocr_content,
            rule_evaluation: RuleEvaluationResult {
                status_code,
                message: message.clone(),
                description: message.clone(),
                suggestions,
                rule_details: None,
            },
            processing_status: if status_code == 200 {
                ProcessingStatus::Success
            } else {
                ProcessingStatus::Failed { error: message }
            },
            display_summary: None,
            display_detail: None,
        }
    }
}

impl PreviewEvaluationResult {
    /// 设置整体结果
    pub fn set_overall_result(&mut self, message: &str, _status: &str) {
        self.evaluation_summary
            .overall_suggestions
            .push(message.to_string());
    }
}
