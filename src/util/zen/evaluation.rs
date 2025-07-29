//! OCR评估模块
//! 处理OCR材料评估和预审逻辑

use crate::model::evaluation::{PreviewEvaluationResult, MaterialEvaluationResult};
use crate::model::preview::Preview;
use ocr_conn::ocr::Extractor;
use std::io::Write;
use tracing::{error, info, warn};

/// 评估器结构体
pub struct PreviewEvaluator {
    pub preview: Preview,
}

/// 简化的材料评估结果
#[derive(Debug, Clone)]
struct SimpleEvaluationResult {
    code: u64,
    message: String,
    description: String,
}

impl PreviewEvaluator {
    /// 创建新的评估器实例
    pub fn new(preview: Preview) -> Self {
        Self { preview }
    }

    /// 执行预审评估
    pub async fn evaluate(self) -> anyhow::Result<PreviewEvaluationResult> {
        info!("=== 开始预审评估 ===");
        info!("事项ID: {}", self.preview.matter_id);
        info!("事项名称: {}", self.preview.matter_name);
        info!("主题ID: {:?}", self.preview.theme_id);

        // 构建基础信息
        let basic_info = crate::model::evaluation::BasicInfo {
            applicant_name: "申请人".to_string(),
            applicant_id: self.preview.agent_info.user_id.clone(),
            agent_name: "经办人".to_string(),
            agent_id: self.preview.agent_info.user_id.clone(),
            matter_name: self.preview.matter_name.clone(),
            matter_id: self.preview.matter_id.clone(),
            matter_type: self.preview.matter_type.clone(),
            request_id: self.preview.request_id.clone(),
            sequence_no: self.preview.sequence_no.clone(),
            theme_id: self.preview.theme_id.clone().unwrap_or_default(),
            theme_name: "".to_string(),
        };

        let mut evaluation_result = PreviewEvaluationResult::new(basic_info);

        // 如果没有材料数据，返回基本的评估结果
        if self.preview.material_data.is_empty() {
            warn!("没有材料数据，返回基本评估结果");
            evaluation_result.set_overall_result("无材料数据", "warning");
            return Ok(evaluation_result);
        }

        info!("开始处理材料数据，材料数量: {}", self.preview.material_data.len());

        // 处理每个材料
        for (index, material) in self.preview.material_data.iter().enumerate() {
            info!("=== 处理材料 {} ===", index + 1);

            // 从MaterialValue结构中提取材料信息
            let material_code = &material.code;
            info!("材料代码: {}", material_code);

            // 处理附件列表
            for (att_index, attachment) in material.attachment_list.iter().enumerate() {
                info!("处理附件 {}: {}", att_index + 1, attachment.attach_name);

                // 处理每个附件
                match self.process_material_content(&attachment.attach_url, material_code, &attachment.attach_name).await {
                    Ok(material_result) => {
                        info!("✅ 材料处理成功: {}", attachment.attach_name);
                        evaluation_result.add_material_result(material_result);
                    }
                    Err(e) => {
                        error!("❌ 材料处理失败: {} - {}", attachment.attach_name, e);
                        
                        // 添加失败的材料结果
                        let error_result = MaterialEvaluationResult::new_simple(
                            material_code.clone(),
                            attachment.attach_name.clone(),
                            "".to_string(),
                            500,
                            format!("材料处理失败: {}", e),
                            vec![],
                        );
                        evaluation_result.add_material_result(error_result);
                    }
                }
            }

            // 如果材料没有附件
            if material.attachment_list.is_empty() {
                warn!("材料没有附件，跳过处理: {}", material_code);
                
                // 添加无附件的材料结果
                let no_attachment_result = MaterialEvaluationResult::new_simple(
                    material_code.clone(),
                    format!("材料代码: {}", material_code),
                    "".to_string(),
                    400,
                    "材料缺少附件信息".to_string(),
                    vec![],
                );
                evaluation_result.add_material_result(no_attachment_result);
            }
        }

        info!("=== 预审评估完成 ===");
        Ok(evaluation_result)
    }

    /// 处理单个材料内容
    async fn process_material_content(
        &self,
        material_url: &str,
        material_code: &str,
        material_name: &str,
    ) -> anyhow::Result<MaterialEvaluationResult> {
        info!("开始处理材料内容: {}", material_name);

        // 下载文件内容
        let file_content = super::downloader::download_file_content(material_url).await?;
        info!("文件下载成功，大小: {} bytes", file_content.len());

        // 进行OCR识别
        let ocr_result = self.perform_ocr_recognition(&file_content).await?;
        info!("OCR识别完成，内容长度: {}", ocr_result.len());

        // 简化评估（不使用复杂的规则引擎）
        let evaluation = evaluate_material_simple(material_code, &ocr_result);
        
        // 构建材料结果
        let material_result = MaterialEvaluationResult::new_simple(
            material_code.to_string(),
            material_name.to_string(),
            ocr_result.clone(),
            evaluation.code,
            evaluation.message,
            self.extract_review_points(&ocr_result),
        );

        Ok(material_result)
    }

    /// 执行OCR识别
    async fn perform_ocr_recognition(&self, file_content: &[u8]) -> anyhow::Result<String> {
        info!("开始OCR识别，文件大小: {} bytes", file_content.len());

        // 创建临时文件
        let mut temp_file = std::env::temp_dir();
        temp_file.push(format!("ocr_temp_{}.tmp", uuid::Uuid::new_v4()));
        
        // 写入临时文件
        {
            let mut file = std::fs::File::create(&temp_file)?;
            file.write_all(file_content)?;
            file.flush()?;
        }

        // 进行OCR识别
        let mut extractor = Extractor::new()?;
        let contents = extractor.ocr_and_parse(file_content.to_vec().into())
            .map_err(|e| anyhow::anyhow!("OCR识别失败: {}", e))?;

        // 清理临时文件
        let _ = std::fs::remove_file(&temp_file);

        // 提取文本内容
        let text_content = contents.into_iter()
            .map(|content| content.text)
            .collect::<Vec<_>>()
            .join("\n");

        info!("OCR识别完成，提取文本长度: {}", text_content.len());
        Ok(text_content)
    }

    /// 从OCR内容中提取审核要点
    fn extract_review_points(&self, ocr_content: &str) -> Vec<String> {
        let mut review_points = Vec::new();

        // 基于关键词提取审核要点
        let keywords = [
            ("身份证", "身份证件有效期检查"),
            ("营业执照", "营业执照有效性验证"),
            ("日期", "证件有效期确认"),
            ("签名", "签名完整性检查"),
            ("印章", "印章清晰度验证"),
        ];

        for (keyword, point) in &keywords {
            if ocr_content.contains(keyword) {
                review_points.push(point.to_string());
            }
        }

        // 如果没有找到特定关键词，添加通用审核要点
        if review_points.is_empty() {
            review_points.push("材料完整性检查".to_string());
        }

        review_points
    }
}

/// 简化的材料评估函数
fn evaluate_material_simple(material_code: &str, content: &str) -> SimpleEvaluationResult {
    info!("简化评估 - 材料代码: {}, 内容长度: {}", material_code, content.len());

    // 基于材料代码的简单规则匹配
    match material_code {
        code if code.contains("16570147206221001") => {
            // 杭州市工程渣土准运证核准申请表
            if content.is_empty() {
                SimpleEvaluationResult {
                    code: 500,
                    message: "没有材料".to_string(),
                    description: "杭州市工程渣土准运证核准申请表".to_string(),
                }
            } else {
                SimpleEvaluationResult {
                    code: 200,
                    message: "材料检查通过".to_string(),
                    description: "杭州市工程渣土准运证核准申请表".to_string(),
                }
            }
        }
        code if code.contains("105100813") => {
            // 申请单位营业执照
            SimpleEvaluationResult {
                code: 200,
                message: "申请单位营业执照".to_string(),
                description: "申请单位营业执照".to_string(),
            }
        }
        code if code.contains("105100001") => {
            // 委托代理人身份证
            SimpleEvaluationResult {
                code: 200,
                message: "委托代理人身份证（检查有效期）".to_string(),
                description: "委托代理人身份证".to_string(),
            }
        }
        _ => {
            // 默认通过
            SimpleEvaluationResult {
                code: 200,
                message: "材料检查通过".to_string(),
                description: format!("材料代码: {}", material_code),
            }
        }
    }
}

/// 获取模拟用户名（用于测试）
pub async fn get_user_name_by_id(user_id: &str) -> Option<String> {
    // 在实际环境中，这里应该调用真实的用户服务API
    // 现在返回模拟数据用于测试
    get_mock_user_name(user_id)
}

/// 获取模拟用户名
fn get_mock_user_name(user_id: &str) -> Option<String> {
    let mock_users = [
        ("user001", "张三"),
        ("user002", "李四"),
        ("test_user_001", "测试用户"),
        ("admin", "管理员"),
    ];
    
    for (id, name) in &mock_users {
        if *id == user_id {
            return Some(name.to_string());
        }
    }
    
    // 如果没有找到，生成一个基于用户ID的名称
    Some(format!("用户{}", &user_id[user_id.len().saturating_sub(3)..]))
}