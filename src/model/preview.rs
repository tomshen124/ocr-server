use crate::model::{Goto, PreviewInfo};
use crate::util::WebResult;
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use ocr_conn::CURRENT_DIR;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;
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
                crate::util::report_generator::PreviewReportGenerator::generate_html(&evaluation_result)
            }
            Err(e) => {
                tracing::error!("预审评估失败: {}", e);
                // 降级到简化HTML生成
                let materials: Vec<String> = self.preview.material_data.iter()
                    .map(|m| format!("{} ({})", m.code, m.attachment_list.len()))
                    .collect();
                crate::util::report_generator::PreviewReportGenerator::generate_simple_html(
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
