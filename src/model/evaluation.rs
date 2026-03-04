use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEvaluationResult {
    pub basic_info: BasicInfo,
    pub material_results: Vec<MaterialEvaluationResult>,
    pub evaluation_summary: EvaluationSummary,
    pub evaluation_time: DateTime<Local>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicInfo {
    pub applicant_name: String,
    pub applicant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicant_org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicant_phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub applicant_certificate_number: Option<String>,
    pub agent_name: String,
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_org: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_phone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_certificate_number: Option<String>,
    pub matter_name: String,
    pub matter_id: String,
    pub matter_type: String,
    pub request_id: String,
    pub sequence_no: String,
    pub theme_id: String,
    pub theme_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialEvaluationResult {
    pub material_code: String,
    pub material_name: String,
    pub attachments: Vec<AttachmentInfo>,
    pub ocr_content: String,
    pub rule_evaluation: RuleEvaluationResult,
    pub processing_status: ProcessingStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_detail: Option<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleEvaluationResult {
    pub status_code: u64,
    pub message: String,
    pub description: String,
    pub suggestions: Vec<String>,
    pub rule_details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessingStatus {
    Success,
    PartialSuccess { warnings: Vec<String> },
    Failed { error: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationSummary {
    pub total_materials: usize,
    pub passed_materials: usize,
    pub failed_materials: usize,
    pub warning_materials: usize,
    pub overall_result: OverallResult,
    pub overall_suggestions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OverallResult {
    Passed,
    PassedWithSuggestions,
    Failed,
    RequiresAdditionalMaterials,
}

impl PreviewEvaluationResult {
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

    pub fn add_material_result(&mut self, result: MaterialEvaluationResult) {
        self.material_results.push(result);
        self.update_summary();
    }

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
            attachments: vec![],
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
    pub fn set_overall_result(&mut self, message: &str, _status: &str) {
        self.evaluation_summary
            .overall_suggestions
            .push(message.to_string());
    }
}
