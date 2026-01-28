//! API工具函数模块
//! 包含API处理中使用的共享工具函数

use chrono::Utc;
use nanoid::nanoid;
use regex::Regex;
use std::sync::OnceLock;

use crate::model::evaluation::{PreviewEvaluationResult, ProcessingStatus};

const PREVIEW_ID_RANDOM_ALPHABET: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'J',
    'K', 'L', 'M', 'N', 'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z',
];
const PREVIEW_ID_RANDOM_LEN: usize = 8;

/// 生成安全的预审ID
///
/// 格式：{13位毫秒时间戳}{8位大写随机码}
/// 总长度：21 字符，首位必为数字
pub fn generate_secure_preview_id() -> String {
    let timestamp = format!("{:013}", Utc::now().timestamp_millis().abs());
    let random = nanoid!(PREVIEW_ID_RANDOM_LEN, PREVIEW_ID_RANDOM_ALPHABET);
    format!("{}{}", timestamp, random)
}

/// 根据材料名称和状态获取对应的图片路径
pub fn get_material_image_path(material_name: &str, status: &str) -> String {
    let base_path = "/static/images/";

    // 根据状态优先选择
    match status {
        "approved" | "passed" => {
            if material_name.contains("章程") {
                format!("{}智能预审_已通过材料1.3.png", base_path)
            } else {
                format!("{}预审通过1.3.png", base_path)
            }
        }
        "rejected" | "failed" => {
            format!("{}智能预审异常提示1.3.png", base_path)
        }
        "pending" | "reviewing" => {
            if material_name.contains("合同") || material_name.contains("协议") {
                format!("{}智能预审_有审查点1.3.png", base_path)
            } else if material_name.contains("申请") || material_name.contains("登记") {
                format!("{}智能预审_审核依据材料1.3.png", base_path)
            } else {
                format!("{}智能预审_审核依据材料1.3.png", base_path)
            }
        }
        "no_reference" => {
            format!("{}智能预审_无审核依据材料1.3.png", base_path)
        }
        _ => {
            format!("{}智能预审_审核依据材料1.3.png", base_path)
        }
    }
}

/// 清洗评估结果中的系统内部信息（路径/调试字段），用于 API/页面展示
pub fn sanitize_evaluation_result(result: &mut PreviewEvaluationResult) {
    // 总体建议
    result.evaluation_summary.overall_suggestions = result
        .evaluation_summary
        .overall_suggestions
        .iter()
        .map(|s| sanitize_system_text(s))
        .collect();

    for material in &mut result.material_results {
        material.rule_evaluation.message = sanitize_system_text(&material.rule_evaluation.message);
        material.rule_evaluation.description =
            sanitize_system_text(&material.rule_evaluation.description);
        material.rule_evaluation.suggestions = material
            .rule_evaluation
            .suggestions
            .iter()
            .map(|s| sanitize_system_text(s))
            .collect();

        material.display_summary = material
            .display_summary
            .as_ref()
            .map(|s| sanitize_system_text(s));
        material.display_detail = material
            .display_detail
            .as_ref()
            .map(|s| sanitize_system_text(s));

        match &mut material.processing_status {
            ProcessingStatus::PartialSuccess { warnings } => {
                *warnings = warnings.iter().map(|w| sanitize_system_text(w)).collect();
            }
            ProcessingStatus::Failed { error } => {
                *error = sanitize_system_text(error);
            }
            ProcessingStatus::Success => {}
        }
    }
}

fn sanitize_system_text(value: &str) -> String {
    static PATH_RE: OnceLock<Regex> = OnceLock::new();
    static MATERIAL_RE: OnceLock<Regex> = OnceLock::new();
    static INDEX_RE: OnceLock<Regex> = OnceLock::new();

    let mut text = value.replace("\r\n", "\n");

    let path_re = PATH_RE.get_or_init(|| {
        Regex::new(r"(/app/\S+|/tmp/\S+|/var/\S+|/home/\S+|/opt/\S+)").unwrap()
    });
    text = path_re.replace_all(&text, "[路径已省略]").into_owned();

    let material_re = MATERIAL_RE.get_or_init(|| Regex::new(r"material=[^,;\s]+").unwrap());
    text = material_re.replace_all(&text, "材料").into_owned();

    let index_re = INDEX_RE.get_or_init(|| Regex::new(r"index=\d+").unwrap());
    text = index_re.replace_all(&text, "").into_owned();

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_secure_preview_id() {
        let id = generate_secure_preview_id();
        assert!(id.chars().next().unwrap().is_ascii_digit());
        assert_eq!(id.len(), 13 + PREVIEW_ID_RANDOM_LEN);
    }

    #[test]
    fn test_get_material_image_path() {
        assert_eq!(
            get_material_image_path("公司章程", "approved"),
            "/static/images/智能预审_已通过材料1.3.png"
        );

        assert_eq!(
            get_material_image_path("测试文件", "failed"),
            "/static/images/智能预审异常提示1.3.png"
        );

        assert_eq!(
            get_material_image_path("合同文件", "pending"),
            "/static/images/智能预审_有审查点1.3.png"
        );
    }
}
