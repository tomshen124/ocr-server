use crate::model::{Goto, PreviewInfo};
use crate::util::WebResult;
use crate::storage::Storage;
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use ocr_conn::CURRENT_DIR;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio_util::io::ReaderStream;
use tracing::info;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreviewBody {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub preview: Preview,
}

// 生产环境数据格式（直接格式，无包装层）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProductionPreviewRequest {
    #[serde(rename = "agentInfo")]
    pub agent_info: UserInfo,
    #[serde(rename = "subjectInfo")]
    pub subject_info: UserInfo,
    pub channel: String,
    pub copy: bool,
    #[serde(rename = "formData")]
    pub form_data: Vec<Value>,
    #[serde(rename = "materialData")]
    pub material_data: Vec<MaterialValue>,
    #[serde(rename = "matterId")]
    pub matter_id: String,
    #[serde(rename = "matterName")]
    pub matter_name: String,
    #[serde(rename = "matterType")]
    pub matter_type: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "sequenceNo")]
    pub sequence_no: String,
}

impl ProductionPreviewRequest {
    /// 转换为标准的PreviewBody格式
    pub fn to_preview_body(self) -> PreviewBody {
        PreviewBody {
            user_id: self.agent_info.user_id.clone(),
            preview: Preview {
                matter_id: self.matter_id,
                matter_type: self.matter_type,
                matter_name: self.matter_name,
                copy: self.copy,
                channel: self.channel,
                request_id: self.request_id,
                sequence_no: self.sequence_no,
                form_data: self.form_data,
                material_data: self.material_data,
                agent_info: self.agent_info,
                subject_info: self.subject_info,
                theme_id: None, // 将在后续处理中设置
            },
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Preview {
    #[serde(rename = "matterId")]
    pub matter_id: String,
    #[serde(rename = "matterType")]
    pub matter_type: String,
    #[serde(rename = "matterName")]
    pub matter_name: String,
    pub copy: bool,
    pub channel: String,
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "sequenceNo")]
    pub sequence_no: String,
    #[serde(rename = "formData")]
    pub form_data: Vec<Value>,
    #[serde(rename = "materialData")]
    pub material_data: Vec<MaterialValue>,
    #[serde(rename = "agentInfo")]
    pub agent_info: UserInfo,
    #[serde(rename = "subjectInfo")]
    pub subject_info: UserInfo,
    // 新增主题ID字段，用于选择对应的规则文件
    #[serde(rename = "themeId", default)]
    pub theme_id: Option<String>,
}

impl Preview {
    /// 执行预审评估
    pub async fn evaluate(self) -> anyhow::Result<crate::model::evaluation::PreviewEvaluationResult> {
        use crate::util::zen::evaluation::PreviewEvaluator;
        
        let evaluator = PreviewEvaluator::new(self);
        evaluator.evaluate().await
    }

    pub async fn generate_html(&self) -> anyhow::Result<String> {
        // 这是一个简单的HTML生成示例
        // 实际实现应该根据业务需求生成相应的HTML内容
        let html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>Preview - {}</title>
    <style>
        body {{ font-family: Arial, sans-serif; margin: 20px; }}
        .header {{ border-bottom: 1px solid #ccc; padding-bottom: 10px; }}
        .content {{ margin-top: 20px; }}
        .material {{ margin: 10px 0; padding: 10px; border: 1px solid #eee; }}
    </style>
</head>
<body>
    <div class="header">
        <h1>Matter: {}</h1>
        <p>Type: {}</p>
        <p>Request ID: {}</p>
    </div>
    <div class="content">
        <h2>Agent Info</h2>
        <p>User ID: {}</p>
        <h2>Subject Info</h2>
        <p>User ID: {}</p>
        <h2>Materials</h2>
        {}
    </div>
</body>
</html>"#,
            self.matter_name,
            self.matter_name,
            self.matter_type,
            self.request_id,
            self.agent_info.user_id,
            self.subject_info.user_id,
            self.generate_materials_html()
        );
        Ok(html)
    }

    fn generate_materials_html(&self) -> String {
        self.material_data
            .iter()
            .map(|material| {
                format!(
                    r#"<div class="material">
                        <h3>Code: {}</h3>
                        <ul>
                            {}
                        </ul>
                    </div>"#,
                    material.code,
                    material
                        .attachment_list
                        .iter()
                        .map(|att| format!("<li>{} - {}</li>", att.attach_name, att.attach_url))
                        .collect::<Vec<_>>()
                        .join("")
                )
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

impl PreviewBody {
    pub async fn preview(self) -> anyhow::Result<WebResult> {
        info!("Executing preview: {:?}", &self);
        let user_id_check = &self.preview.agent_info.user_id;
        if user_id_check.ne(&self.user_id) {
            info!("User: {} not match", self.user_id);
            return Ok(WebResult::ok("User no match"));
        }

        let path = CURRENT_DIR.join("preview");
        let images_dir = CURRENT_DIR.join("images");
        if !path.is_dir() {
            fs::create_dir_all(&path).await?;
        }
        if !images_dir.is_dir() {
            fs::create_dir_all(&images_dir).await?;
        }

        let request_id = &self.preview.request_id;
        let file_name_html = format!("{}.html", request_id);
        let file_path_html = path.join(&file_name_html);
        let file_path_pdf = path.join(format!("{}.pdf", request_id));

        let mut file = fs::File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&file_path_html)
            .await?;

        // 使用新的数据与展示分离架构
        let html = match self.preview.clone().evaluate().await {
            Ok(evaluation_result) => {
                // 使用专门的报告生成器生成HTML
                crate::util::report::PreviewReportGenerator::generate_html(&evaluation_result)
            }
            Err(e) => {
                tracing::error!("预审评估失败: {}", e);
                // 降级到简化HTML生成
                let materials: Vec<String> = self.preview.material_data.iter()
                    .map(|m| format!("{} ({})", m.code, m.attachment_list.len()))
                    .collect();
                crate::util::report::PreviewReportGenerator::generate_simple_html(
                    &self.preview.matter_name,
                    &self.preview.request_id,
                    &materials
                )
            }
        };
        file.write_all(html.as_bytes()).await?;
        file.flush().await?;

        info!("Save result to html: {}", file_path_html.display());

        let status = Command::new("wkhtmltopdf")
            .args([
                file_path_html.to_str().unwrap_or_default(),
                file_path_pdf.to_str().unwrap_or_default(),
            ])
            .status()?;

        if !status.success() {
            info!("wkhtmltopdf conversion failed for {}", file_path_html.display());
            return Err(anyhow::anyhow!("PDF conversion failed"));
        }

        info!(
            "Convert html to pdf as local file: {}",
            file_path_pdf.display()
        );

        let preview_info = PreviewInfo {
            user_id: self.user_id.clone(),
            preview_url: format!(
                "/api/download?goto={}",
                file_path_pdf.display()
            ),
        };
        Ok(WebResult::ok(preview_info))
    }

    /// 使用存储抽象层的预览方法
    pub async fn preview_with_storage(self, storage: &Arc<dyn Storage>) -> anyhow::Result<WebResult> {
        info!("Executing preview with storage: {:?}", &self);
        let user_id_check = &self.preview.agent_info.user_id;
        if user_id_check.ne(&self.user_id) {
            info!("User: {} not match", self.user_id);
            return Ok(WebResult::ok("User no match"));
        }

        let request_id = &self.preview.request_id;
        
        // 执行预审评估并生成HTML内容
        let (html, evaluation_result_json) = match self.preview.clone().evaluate().await {
            Ok(evaluation_result) => {
                // 将evaluation_result序列化为JSON字符串用于数据库存储
                let evaluation_json = match serde_json::to_string(&evaluation_result) {
                    Ok(json) => Some(json),
                    Err(e) => {
                        tracing::error!("序列化evaluation_result失败: {}", e);
                        None
                    }
                };
                
                // 使用专门的报告生成器生成HTML
                let html = crate::util::report::PreviewReportGenerator::generate_html(&evaluation_result);
                (html, evaluation_json)
            }
            Err(e) => {
                tracing::error!("预审评估失败: {}", e);
                // 降级到简化HTML生成
                let materials: Vec<String> = self.preview.material_data.iter()
                    .map(|m| format!("{} ({})", m.code, m.attachment_list.len()))
                    .collect();
                let html = crate::util::report::PreviewReportGenerator::generate_simple_html(
                    &self.preview.matter_name,
                    &self.preview.request_id,
                    &materials
                );
                (html, None)
            }
        };

        // 保存HTML文件到存储
        let html_key = format!("previews/{}.html", request_id);
        storage.put(&html_key, html.as_bytes()).await?;
        
        info!("Save HTML to storage: {}", html_key);

        // 生成PDF（如果需要）
        let temp_dir = std::env::temp_dir();
        let temp_html_path = temp_dir.join(format!("{}.html", request_id));
        let temp_pdf_path = temp_dir.join(format!("{}.pdf", request_id));

        // 写入临时HTML文件
        fs::write(&temp_html_path, html.as_bytes()).await?;

        // 转换为PDF
        let status = Command::new("wkhtmltopdf")
            .args([
                temp_html_path.to_str().unwrap_or_default(),
                temp_pdf_path.to_str().unwrap_or_default(),
            ])
            .status()?;

        // TODO: 需要将evaluation_result_json保存到数据库
        // 这需要通过参数传递database实例或其他方式来实现
        // 目前先记录日志，表示需要保存这个数据
        if let Some(ref eval_json) = evaluation_result_json {
            tracing::info!("需要保存evaluation_result到数据库: request_id={}, 数据长度={}", request_id, eval_json.len());
            tracing::debug!("evaluation_result内容预览: {}", 
                if eval_json.len() > 200 { 
                    format!("{}...", &eval_json[..200]) 
                } else { 
                    eval_json.clone() 
                }
            );
        }

        if status.success() {
            // 读取生成的PDF并保存到存储
            let pdf_content = fs::read(&temp_pdf_path).await?;
            let pdf_key = format!("previews/{}.pdf", request_id);
            storage.put(&pdf_key, &pdf_content).await?;
            
            info!("Save PDF to storage: {}", pdf_key);
            
            // 清理临时文件
            let _ = fs::remove_file(&temp_html_path).await;
            let _ = fs::remove_file(&temp_pdf_path).await;
            
            let preview_info = PreviewInfo {
                user_id: self.user_id.clone(),
                preview_url: format!("/api/download?goto=storage/{}", pdf_key),
            };
            Ok(WebResult::ok(preview_info))
        } else {
            info!("wkhtmltopdf conversion failed for {}", temp_html_path.display());
            
            // 清理临时文件
            let _ = fs::remove_file(&temp_html_path).await;
            
            // 即使PDF转换失败，也返回HTML文件的URL
            let preview_info = PreviewInfo {
                user_id: self.user_id.clone(),
                preview_url: format!("/api/download?goto=storage/{}", html_key),
            };
            Ok(WebResult::ok(preview_info))
        }
    }

    pub async fn download(goto: Goto) -> anyhow::Result<Response> {
        // 统一下载API，直接调用download_local
        Self::download_local(goto).await
    }

    pub async fn download_local(goto: Goto) -> anyhow::Result<Response> {
        let buf = fs::read(&goto.goto).await?;
        let path = Path::new(&goto.goto);
        let content_type = mime_guess::from_path(&goto.goto)
            .first_or_octet_stream()
            .to_string();
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(
                header::CONTENT_TYPE,
                format!(
                    "attachment; filename={}",
                    urlencoding::encode(path.file_name().unwrap().to_str().unwrap())
                ),
            )
            .body(Body::from_stream(ReaderStream::new(Cursor::new(buf))))?;
        info!("Download file: {}", goto.goto);
        Ok(response)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SceneValue {
    #[serde(rename = "questionCode")]
    pub question_code: String,
    #[serde(rename = "optionList")]
    pub option_list: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MaterialValue {
    pub code: String,
    #[serde(rename = "attachmentList")]
    pub attachment_list: Vec<Attachment>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(rename = "attaName")]
    pub attach_name: String,
    #[serde(rename = "attaUrl")]
    pub attach_url: String,
    #[serde(rename = "isCloudShare")]
    pub is_cloud_share: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserInfo {
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "certificateType")]
    pub certificate_type: String,
    // 扩展用户信息字段
    #[serde(rename = "userName", default)]
    pub user_name: Option<String>,
    #[serde(rename = "certificateNumber", default)]
    pub certificate_number: Option<String>,
    #[serde(rename = "phoneNumber", default)]
    pub phone_number: Option<String>,
    #[serde(rename = "email", default)]
    pub email: Option<String>,
    #[serde(rename = "organizationName", default)]
    pub organization_name: Option<String>,
    #[serde(rename = "organizationCode", default)]
    pub organization_code: Option<String>,
}
