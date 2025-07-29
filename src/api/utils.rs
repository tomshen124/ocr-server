//! API工具函数模块
//! 包含API处理中使用的共享工具函数

use chrono::Utc;
use uuid::Uuid;

/// 生成安全的预审ID
/// 
/// 格式：PV{时间戳}{UUID前12位}{UUID的12-18位大写}
/// 总长度：2 + 14 + 12 + 6 = 34位
/// PV = Preview（预审）
pub fn generate_secure_preview_id() -> String {
    // 组合方案：时间戳 + UUID，确保唯一性和安全性
    let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let uuid = Uuid::new_v4().to_string().replace("-", "");

    // 使用UUID的另一部分作为随机后缀，避免额外依赖
    let random_suffix = &uuid[12..18].to_uppercase();

    format!("PV{}{}{}", timestamp, &uuid[..12].to_uppercase(), random_suffix)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_secure_preview_id() {
        let id = generate_secure_preview_id();
        assert!(id.starts_with("PV"));
        assert_eq!(id.len(), 34);
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