use crate::db::traits::Database as DbTrait;
use crate::model::evaluation::{
    BasicInfo, MaterialEvaluationResult, OverallResult, PreviewEvaluationResult, ProcessingStatus,
    RuleEvaluationResult,
};
use crate::model::{Goto, PreviewInfo};
use crate::storage::Storage;
use crate::util::material_cache;
use crate::util::report::pdf::PdfGenerator;
use crate::util::rules::MatterRuleDefinition;
use crate::util::zen::evaluation::PreviewEvaluator;
use crate::util::WebResult;
use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::Response;
use chrono::Local;
use ocr_conn::CURRENT_DIR;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::task;
use tokio_util::io::ReaderStream;
use tracing::{debug, info, warn};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreviewBody {
    #[serde(rename = "userId")]
    pub user_id: String,
    pub preview: Preview,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_definition: Option<serde_json::Value>,
    #[serde(skip)]
    pub parsed_rule_definition: Option<Arc<MatterRuleDefinition>>,
}

#[derive(Debug, Clone)]
pub struct PreviewExecutionOutput {
    pub web_result: WebResult,
    pub evaluation_result: Option<PreviewEvaluationResult>,
    pub generated_report: Option<GeneratedReport>,
}

#[derive(Debug, Clone)]
pub struct GeneratedReport {
    pub preview_url: String,
    pub html_path: PathBuf,
    pub pdf_path: Option<PathBuf>,
    pub remote_html_url: Option<String>,
    pub remote_pdf_url: Option<String>,
    pub html_size: Option<u64>,
    pub pdf_size: Option<u64>,
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
    #[serde(rename = "requestId", alias = "request_id", default)]
    pub request_id: Option<String>,
    #[serde(
        rename = "thirdPartyRequestId",
        alias = "third_party_request_id",
        default
    )]
    pub third_party_request_id: Option<String>,
    #[serde(rename = "sequenceNo")]
    pub sequence_no: String,
    // 预留sceneData字段支持，等待第三方实际数据确认
    #[serde(rename = "sceneData", default)]
    pub scene_data: Option<Vec<SceneValue>>,
}

impl ProductionPreviewRequest {
    /// 转换为标准的PreviewBody格式
    pub fn to_preview_body(self) -> PreviewBody {
        let third_party_request_id = self
            .request_id
            .clone()
            .or(self.third_party_request_id.clone())
            .unwrap_or_else(|| self.sequence_no.clone());

        PreviewBody {
            user_id: self.agent_info.user_id.clone(),
            preview: Preview {
                matter_id: self.matter_id,
                matter_type: self.matter_type,
                matter_name: self.matter_name,
                copy: self.copy,
                channel: self.channel,
                request_id: third_party_request_id,
                sequence_no: self.sequence_no,
                form_data: self.form_data,
                material_data: self.material_data,
                agent_info: self.agent_info,
                subject_info: self.subject_info,
                theme_id: None,              // 将在后续处理中设置
                scene_data: self.scene_data, // 传递sceneData数据
            },
            rule_definition: None,
            parsed_rule_definition: None,
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
    // 预留sceneData字段支持，等待第三方实际数据确认
    #[serde(rename = "sceneData", default)]
    pub scene_data: Option<Vec<SceneValue>>,
}

impl Preview {
    /// 执行预审评估
    pub async fn evaluate(
        self,
    ) -> anyhow::Result<crate::model::evaluation::PreviewEvaluationResult> {
        use crate::util::zen::evaluation::PreviewEvaluator;

        let mut evaluator = PreviewEvaluator::new(self);
        evaluator.evaluate_complete().await
    }

    /// 执行预审评估（带存储支持）
    pub async fn evaluate_with_storage(
        self,
        storage: Arc<dyn crate::storage::Storage>,
        database: Option<Arc<dyn DbTrait>>,
    ) -> anyhow::Result<crate::model::evaluation::PreviewEvaluationResult> {
        use crate::util::zen::evaluation::PreviewEvaluator;

        let mut evaluator = PreviewEvaluator::new_with_resources(self, Some(storage), database);
        evaluator.evaluate_complete().await
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
    pub fn to_log_snapshot(&self) -> Value {
        let preview = &self.preview;
        let agent = &preview.agent_info;
        let subject = &preview.subject_info;

        let total_attachments: usize = preview
            .material_data
            .iter()
            .map(|material| material.attachment_list.len())
            .sum();

        let materials: Vec<Value> = preview
            .material_data
            .iter()
            .map(|material| {
                let attachments: Vec<Value> = material
                    .attachment_list
                    .iter()
                    .map(|attachment| {
                        json!({
                            "附件名称": attachment.attach_name,
                            "来源云盘": attachment.is_cloud_share
                        })
                    })
                    .collect();

                json!({
                    "材料编码": &material.code,
                    "材料名称": material.name.as_deref().unwrap_or("未命名材料"),
                    "附件数量": attachments.len(),
                    "附件详情": attachments,
                })
            })
            .collect();

        json!({
            "请求概况": {
                "预审编号": &preview.request_id,
                "申报渠道": &preview.channel,
                "申报流水号": &preview.sequence_no,
                "使用模板复用": preview.copy,
                "表单字段数": preview.form_data.len(),
                "场景字段数": preview.scene_data.as_ref().map(|s| s.len()).unwrap_or(0),
                "携带内置规则": self.rule_definition.is_some(),
                "材料数量": preview.material_data.len(),
                "附件总数": total_attachments
            },
            "事项信息": {
                "事项编号": &preview.matter_id,
                "事项名称": &preview.matter_name,
                "事项类型": &preview.matter_type,
                "主题ID": preview.theme_id.as_deref()
            },
            "经办人": {
                "用户ID": &agent.user_id,
                "姓名": agent.user_name.as_deref(),
                "所属单位": agent.organization_name.as_deref(),
                "联系方式": agent.phone_number.as_deref()
            },
            "申请人": {
                "用户ID": &subject.user_id,
                "姓名": subject.user_name.as_deref(),
                "证件号码": subject.certificate_number.as_deref()
            },
            "材料清单": materials
        })
    }

    pub async fn preview(self) -> anyhow::Result<WebResult> {
        self.preview_with_storage(None).await
    }

    pub async fn execute_preview(
        self,
        storage: Option<Arc<dyn crate::storage::Storage>>,
        database: Option<Arc<dyn DbTrait>>,
    ) -> anyhow::Result<PreviewExecutionOutput> {
        self.execute_preview_with_options(storage, database, true)
            .await
    }

    pub async fn execute_preview_with_options(
        self,
        storage: Option<Arc<dyn crate::storage::Storage>>,
        database: Option<Arc<dyn DbTrait>>,
        generate_report: bool,
    ) -> anyhow::Result<PreviewExecutionOutput> {
        info!(
            "=== 开始预审处理 === 模式: {}",
            if storage.is_some() {
                "完整OCR处理"
            } else {
                "快速响应模式"
            }
        );
        let total_materials = self.preview.material_data.len();
        let total_attachments: usize = self
            .preview
            .material_data
            .iter()
            .map(|m| m.attachment_list.len())
            .sum();
        let request_id = self.preview.request_id.clone();
        info!(
            request_id = %request_id,
            matter_id = %self.preview.matter_id,
            materials = total_materials,
            attachments = total_attachments,
            "预审任务接入"
        );
        if let Ok(pretty_payload) = serde_json::to_string_pretty(&self.to_log_snapshot()) {
            info!(
                request_id = %request_id,
                matter_id = %self.preview.matter_id,
                payload = %pretty_payload,
                "预审请求数据"
            );
        }
        debug!("预审请求详情: {:?}", &self);
        let user_id_check = &self.preview.agent_info.user_id;
        if user_id_check.ne(&self.user_id) {
            info!("User: {} not match", self.user_id);
            return Ok(PreviewExecutionOutput {
                web_result: WebResult::ok("User no match"),
                evaluation_result: None,
                generated_report: None,
            });
        }

        let embedded_rule_definition = if let Some(definition) = self.parsed_rule_definition.clone()
        {
            Some(definition)
        } else {
            match self.rule_definition.clone() {
                Some(value) => match serde_json::from_value::<MatterRuleDefinition>(value) {
                    Ok(definition) => Some(Arc::new(definition)),
                    Err(err) => {
                        warn!(
                            matter_id = %self.preview.matter_id,
                            error = %err,
                            "内嵌事项规则解析失败，将回退到默认规则"
                        );
                        None
                    }
                },
                None => None,
            }
        };

        // 使用新的数据与展示分离架构
        let mut evaluator = PreviewEvaluator::new_with_resources(
            self.preview.clone(),
            storage.clone(),
            database.clone(),
        );
        evaluator.set_embedded_rule_definition(embedded_rule_definition);
        let evaluation_attempt = evaluator.evaluate_complete().await;

        let (evaluation_result, html) = match evaluation_attempt {
            Ok(result) => {
                let html = crate::util::report::PreviewReportGenerator::generate_html(&result);
                (Some(result), html)
            }
            Err(e) => {
                tracing::error!("预审评估失败: {}", e);
                let fallback = self.build_fallback_evaluation(&e.to_string());
                let html = crate::util::report::PreviewReportGenerator::generate_html(&fallback);
                (Some(fallback), html)
            }
        };

        let mut preview_url = format!("/api/preview/view/{}", request_id);
        let mut approve_pdf_file: Option<String> = None;

        let mut generated_report = None;
        if generate_report {
            let generated = Self::persist_report_files(&request_id, &html, storage.clone()).await?;
            preview_url = generated.preview_url.clone();
            if crate::CONFIG.report_export.enable_approve_pdf {
                // 只暴露PDF URL，不再回退到HTML；若上传失败则用本地下载接口
                let mut preferred_pdf = generated
                    .remote_pdf_url
                    .clone()
                    .or_else(|| crate::util::callbacks::build_default_download_url(&request_id));
                // 如果仍没有PDF直链，则不返回字段
                if preferred_pdf.is_some() {
                    approve_pdf_file = preferred_pdf.take();
                }
            }
            generated_report = Some(generated);
        }

        let preview_info = PreviewInfo {
            user_id: self.user_id.clone(),
            preview_url,
            approve_pdf_file,
        };

        if let Some(result) = &evaluation_result {
            let summary = &result.evaluation_summary;
            tracing::info!(
                "[stats] 预审结果摘要: 总材料={}，通过={}，警告={}，不通过={}，整体结论={:?}",
                summary.total_materials,
                summary.passed_materials,
                summary.warning_materials,
                summary.failed_materials,
                summary.overall_result
            );

            debug!(
                "材料评估详情: {:?}",
                result
                    .material_results
                    .iter()
                    .map(|material| {
                        (
                            material.material_code.as_str(),
                            material.rule_evaluation.status_code,
                            material.rule_evaluation.message.as_str(),
                        )
                    })
                    .collect::<Vec<_>>()
            );
        }

        Ok(PreviewExecutionOutput {
            web_result: WebResult::ok(preview_info),
            evaluation_result,
            generated_report,
        })
    }

    pub async fn preview_with_storage(
        self,
        storage: Option<Arc<dyn crate::storage::Storage>>,
    ) -> anyhow::Result<WebResult> {
        self.execute_preview(storage, None)
            .await
            .map(|output| output.web_result)
    }

    fn build_fallback_evaluation(&self, error: &str) -> PreviewEvaluationResult {
        let preview = &self.preview;
        let applicant_name = preview
            .subject_info
            .user_name
            .clone()
            .unwrap_or_else(|| preview.subject_info.user_id.clone());
        let agent_name = preview
            .agent_info
            .user_name
            .clone()
            .unwrap_or_else(|| preview.agent_info.user_id.clone());

        let basic_info = BasicInfo {
            applicant_name,
            applicant_id: preview.subject_info.user_id.clone(),
            applicant_org: preview.subject_info.organization_name.clone(),
            applicant_phone: preview.subject_info.phone_number.clone(),
            applicant_certificate_number: preview.subject_info.certificate_number.clone(),
            agent_name,
            agent_id: preview.agent_info.user_id.clone(),
            agent_org: preview.agent_info.organization_name.clone(),
            agent_phone: preview.agent_info.phone_number.clone(),
            agent_certificate_number: preview.agent_info.certificate_number.clone(),
            matter_name: preview.matter_name.clone(),
            matter_id: preview.matter_id.clone(),
            matter_type: preview.matter_type.clone(),
            request_id: preview.request_id.clone(),
            sequence_no: preview.sequence_no.clone(),
            theme_id: preview
                .theme_id
                .clone()
                .unwrap_or_else(|| "default-theme".to_string()),
            theme_name: preview
                .theme_id
                .clone()
                .unwrap_or_else(|| "默认主题".to_string()),
        };

        let mut fallback = PreviewEvaluationResult::new(basic_info);

        let material = MaterialEvaluationResult {
            material_code: "SYSTEM".to_string(),
            material_name: "系统处理".to_string(),
            attachments: Vec::new(),
            ocr_content: String::new(),
            rule_evaluation: RuleEvaluationResult {
                status_code: 500,
                message: "报告生成失败".to_string(),
                description: error.to_string(),
                suggestions: vec!["请稍后重试或联系系统运维".to_string()],
                rule_details: None,
            },
            processing_status: ProcessingStatus::Failed {
                error: error.to_string(),
            },
            display_summary: Some("系统暂时无法生成报告".to_string()),
            display_detail: Some("请稍后重试或联系运维人员".to_string()),
        };

        fallback.material_results.push(material);
        fallback.evaluation_summary.total_materials = fallback.material_results.len();
        fallback.evaluation_summary.passed_materials = 0;
        fallback.evaluation_summary.failed_materials = fallback.material_results.len();
        fallback.evaluation_summary.warning_materials = 0;
        fallback.evaluation_summary.overall_result = OverallResult::Failed;
        fallback.evaluation_summary.overall_suggestions =
            vec!["系统自动生成报告失败，请联系运维人员排查。".to_string()];
        fallback.evaluation_time = Local::now();

        fallback
    }

    pub(crate) async fn persist_report_files(
        request_id: &str,
        html: &str,
        storage: Option<Arc<dyn Storage>>,
    ) -> anyhow::Result<GeneratedReport> {
        let preview_dir = CURRENT_DIR.join("preview");
        let images_dir = CURRENT_DIR.join("images");

        tracing::info!("[files] 准备创建预览目录: {}", preview_dir.display());
        tracing::info!("[files] 当前工作目录: {}", CURRENT_DIR.display());

        if fs::metadata(&preview_dir).await.is_err() {
            tracing::info!("[folder] 预览目录不存在，正在创建...");
            fs::create_dir_all(&preview_dir).await.map_err(|e| {
                tracing::error!(
                    "[fail] 创建预览目录失败: {} - 错误: {}",
                    preview_dir.display(),
                    e
                );
                anyhow::anyhow!("无法创建预览目录 {}: {}", preview_dir.display(), e)
            })?;
            tracing::info!("[ok] 预览目录创建成功: {}", preview_dir.display());
        }

        if fs::metadata(&images_dir).await.is_err() {
            tracing::info!("[folder] 图像目录不存在，正在创建...");
            fs::create_dir_all(&images_dir).await.map_err(|e| {
                tracing::error!(
                    "[fail] 创建图像目录失败: {} - 错误: {}",
                    images_dir.display(),
                    e
                );
                anyhow::anyhow!("无法创建图像目录 {}: {}", images_dir.display(), e)
            })?;
            tracing::info!("[ok] 图像目录创建成功: {}", images_dir.display());
        }

        let file_path_html = preview_dir.join(format!("{}.html", request_id));
        let file_path_pdf = preview_dir.join(format!("{}.pdf", request_id));

        tracing::info!("[doc] 准备创建HTML文件: {}", file_path_html.display());

        let mut file = fs::File::options()
            .write(true)
            .truncate(true)
            .create(true)
            .open(&file_path_html)
            .await
            .map_err(|e| {
                tracing::error!(
                    "[fail] 创建HTML文件失败: {} - 错误: {}",
                    file_path_html.display(),
                    e
                );
                anyhow::anyhow!("无法创建HTML文件 {}: {}", file_path_html.display(), e)
            })?;

        tracing::info!(
            "[note] 准备写入HTML内容 (长度: {} bytes) 到文件: {}",
            html.len(),
            file_path_html.display()
        );

        file.write_all(html.as_bytes()).await.map_err(|e| {
            tracing::error!("[fail] HTML内容写入失败: {}", e);
            anyhow::anyhow!("写入HTML文件失败: {}", e)
        })?;
        file.flush().await.map_err(|e| {
            tracing::error!("[fail] 文件缓冲区刷新失败: {}", e);
            anyhow::anyhow!("刷新文件缓冲区失败: {}", e)
        })?;

        info!("Save result to html: {}", file_path_html.display());
        let html_size = html.len() as u64;

        let (html_for_pdf, resolved_tokens, missing_tokens) =
            Self::resolve_worker_cache_links_for_pdf(html).await;
        if !resolved_tokens.is_empty() {
            tracing::info!(
                resolved = %resolved_tokens.len(),
                tokens = ?resolved_tokens,
                "[sanitizer] 已将 worker-cache 资源转换为本地文件URL，供wkhtmltopdf使用"
            );
        }
        if !missing_tokens.is_empty() {
            tracing::warn!(
                missing = %missing_tokens.len(),
                tokens = ?missing_tokens,
                "[sanitizer] 存在未能解析的 worker-cache 资源，PDF 内图片可能缺失"
            );
        }

        tracing::info!("[tool] 尝试执行wkhtmltopdf命令进行PDF转换");

        let command_result = PdfGenerator::html_to_pdf(&html_for_pdf, &file_path_pdf).await;

        // 安全加固：预览地址始终指向受保护的预审查看页，不暴露本地路径或存储公网URL
        let mut preview_url = format!("/api/preview/view/{}", request_id);
        let mut pdf_path = None;
        let mut pdf_size = None;

        match command_result {
            Ok(()) => {
                tracing::info!(
                    "[ok] wkhtmltopdf执行成功，PDF转换完成: {}",
                    file_path_pdf.display()
                );
                pdf_size = tokio::fs::metadata(&file_path_pdf)
                    .await
                    .ok()
                    .map(|meta| meta.len());
                pdf_path = Some(file_path_pdf.clone());
            }
            Err(err) => {
                tracing::warn!("[warn] wkhtmltopdf执行失败: {}", err);
                tracing::warn!("   [ok] HTML文件已成功保存: {}", file_path_html.display());
                tracing::warn!("   [fail] PDF文件生成失败: {}", file_path_pdf.display());
                tracing::warn!("   [loop] 预审任务继续执行");
            }
        }

        let mut remote_html_url = None;
        let mut remote_pdf_url = None;

        if let Some(storage) = storage.as_ref() {
            let html_key = format!("previews/{}.html", request_id);
            let pdf_key = format!("previews/{}.pdf", request_id);

            if let Err(err) = storage.put(&html_key, html.as_bytes()).await {
                tracing::error!("[fail] 上传HTML到存储失败: key={}, error={}", html_key, err);
            }

            if let Some(pdf_path_actual) = pdf_path.as_ref() {
                match tokio::fs::read(pdf_path_actual).await {
                    Ok(bytes) => {
                        pdf_size = Some(bytes.len() as u64);
                        if let Err(err) = storage.put(&pdf_key, &bytes).await {
                            tracing::error!(
                                "[fail] 上传PDF到存储失败: key={}, error={}",
                                pdf_key,
                                err
                            );
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            "[warn] 读取生成的PDF文件失败，无法上传: path={}, error={}",
                            pdf_path_actual.display(),
                            err
                        );
                    }
                }
            }
        }

        Ok(GeneratedReport {
            preview_url,
            html_path: file_path_html,
            pdf_path,
            remote_html_url,
            remote_pdf_url,
            html_size: Some(html_size),
            pdf_size,
        })
    }

    async fn resolve_worker_cache_links_for_pdf(html: &str) -> (String, Vec<String>, Vec<String>) {
        let re = match Regex::new(r"worker-cache://([A-Za-z0-9_-]+)") {
            Ok(r) => r,
            Err(_) => return (html.to_string(), Vec::new(), Vec::new()),
        };

        let mut matches = Vec::new();
        let mut tokens = HashSet::new();
        for caps in re.captures_iter(html) {
            if let (Some(m), Some(token)) = (caps.get(0), caps.get(1)) {
                matches.push((m.start(), m.end(), token.as_str().to_string()));
                tokens.insert(token.as_str().to_string());
            }
        }

        if matches.is_empty() {
            return (html.to_string(), Vec::new(), Vec::new());
        }

        let mut resolved_map: HashMap<String, Option<String>> = HashMap::new();
        let mut resolved_tokens = Vec::new();
        let mut missing_tokens = Vec::new();

        for token in tokens {
            let resolved = match material_cache::get_material_path(&token).await {
                Some(path) => match url::Url::from_file_path(&path) {
                    Ok(url) => Some(url.to_string()),
                    Err(_) => Some(format!("file://{}", path.to_string_lossy())),
                },
                None => None,
            };

            if resolved.is_some() {
                resolved_tokens.push(token.clone());
            } else {
                missing_tokens.push(token.clone());
            }

            resolved_map.insert(token, resolved);
        }

        let mut out = String::with_capacity(html.len());
        let mut last = 0usize;
        for (start, end, token) in matches {
            out.push_str(&html[last..start]);
            let replacement = resolved_map
                .get(&token)
                .and_then(|opt| opt.clone())
                .unwrap_or_else(|| "about:blank".to_string());
            out.push_str(&replacement);
            last = end;
        }
        if last < html.len() {
            out.push_str(&html[last..]);
        }

        (out, resolved_tokens, missing_tokens)
    }

    pub async fn download(goto: Goto) -> anyhow::Result<Response> {
        // 统一下载API，直接调用download_local
        Self::download_local(goto).await
    }

    pub async fn download_local(goto: Goto) -> anyhow::Result<Response> {
        // Basic extension whitelist
        let allowed_exts = ["pdf", "html", "jpg", "jpeg", "png", "txt"];
        let path = Path::new(&goto.goto);
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| allowed_exts.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false);
        if !ext_ok {
            return Err(anyhow::anyhow!("不允许的文件类型"));
        }

        // Resolve canonical path and ensure it is under allowed directories
        let req_abs = fs::canonicalize(&path)
            .await
            .map_err(|e| anyhow::anyhow!("非法下载路径: {}", e))?;

        let base_preview = CURRENT_DIR.join("preview");
        let base_fallback = CURRENT_DIR
            .join("runtime")
            .join("fallback")
            .join("storage")
            .join("previews");
        let base_storage = CURRENT_DIR.join("storage").join("previews");

        let bases = [base_preview, base_fallback, base_storage];
        let mut allowed = false;
        for base in bases.iter() {
            let base_abs = match fs::canonicalize(base).await {
                Ok(p) => p,
                Err(_) => continue, // skip non-existent bases
            };
            if req_abs.starts_with(&base_abs) {
                allowed = true;
                break;
            }
        }
        if !allowed {
            return Err(anyhow::anyhow!("下载路径不被允许"));
        }

        let buf = fs::read(&req_abs).await?;
        let content_type = mime_guess::from_path(&goto.goto)
            .first_or_octet_stream()
            .to_string();
        let response = Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .header(
                header::CONTENT_DISPOSITION,
                format!(
                    "attachment; filename={}",
                    urlencoding::encode(
                        req_abs
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("download")
                    )
                ),
            )
            .body(Body::from_stream(ReaderStream::new(Cursor::new(buf))))?;
        info!("Download file: {}", req_abs.display());
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
    #[serde(
        rename = "name",
        alias = "materialName",
        alias = "material_name",
        alias = "displayName",
        default
    )]
    pub name: Option<String>,
    #[serde(rename = "attachmentList")]
    pub attachment_list: Vec<Attachment>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Attachment {
    #[serde(rename = "attaName", alias = "attachName", alias = "name")]
    pub attach_name: String,
    #[serde(rename = "attaUrl", alias = "attachUrl", alias = "url")]
    pub attach_url: String,
    #[serde(rename = "isCloudShare", alias = "cloudShare", default)]
    pub is_cloud_share: bool,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserInfo {
    #[serde(rename = "userId")]
    pub user_id: String,
    #[serde(rename = "certificateType")]
    pub certificate_type: String,
    // 扩展用户信息字段
    #[serde(rename = "userName", alias = "name", default)]
    pub user_name: Option<String>,
    #[serde(rename = "nickName", default)]
    pub nick_name: Option<String>,
    #[serde(
        rename = "certificateNumber",
        alias = "idNumber",
        alias = "certNumber",
        default
    )]
    pub certificate_number: Option<String>,
    #[serde(rename = "phoneNumber", alias = "mobile", default)]
    pub phone_number: Option<String>,
    #[serde(rename = "email", alias = "emailAddress", default)]
    pub email: Option<String>,
    #[serde(
        rename = "organizationName",
        alias = "companyName",
        alias = "company_name",
        alias = "orgName",
        default
    )]
    pub organization_name: Option<String>,
    #[serde(
        rename = "organizationCode",
        alias = "creditCode",
        alias = "organizationNumber",
        alias = "credit_code",
        default
    )]
    pub organization_code: Option<String>,
    #[serde(rename = "address", alias = "companyAddress", default)]
    pub address: Option<String>,
    #[serde(rename = "authLevel", default)]
    pub auth_level: Option<String>,
    #[serde(rename = "userType", default)]
    pub user_type: Option<String>,
    #[serde(rename = "loginType", default)]
    pub login_type: Option<String>,
    #[serde(rename = "extInfos", default)]
    pub ext_infos: Option<Value>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn production_preview_request_without_request_id_uses_sequence_no() {
        let raw = json!({
            "agentInfo": {
                "userId": "7609052",
                "certificateType": "ID_CARD",
                "certificateNumber": "330682199001241434",
                "phoneNumber": "15906690637",
                "name": "沈刚",
                "nickName": "xiyue1",
                "companyName": "浙江熙唐智慧城市科技有限公司",
                "creditCode": "91330106MA2HXFY97H",
                "address": "浙江省杭州市西湖区紫荆花路108号251室",
                "authLevel": "3",
                "userType": "LEGAL_UAA",
                "loginType": "zlbscan",
                "extInfos": { "CompanyLegRep": "奚跃" }
            },
            "subjectInfo": {
                "userId": "7609052",
                "certificateType": "ID_CARD",
                "certificateNumber": "330682199001241434",
                "phoneNumber": "15906690637",
                "name": "沈刚",
                "companyName": "浙江熙唐智慧城市科技有限公司",
                "creditCode": "91330106MA2HXFY97H"
            },
            "channel": "pc",
            "copy": true,
            "formData": [],
            "materialData": [
                {
                    "code": "self.105100813",
                    "name": "营业执照（社会统一信用代码证）",
                    "attachmentList": [
                        {
                            "attaName": "license.jpg",
                            "attaUrl": "https://example.com/license.jpg",
                            "isCloudShare": true
                        }
                    ],
                    "required": true
                }
            ],
            "matterId": "101104353",
            "matterName": "工程渣土准运证核准",
            "matterType": "powerDirectory",
            "sequenceNo": "SEQ123456789"
        });

        let request: ProductionPreviewRequest =
            serde_json::from_value(raw).expect("parse production payload");
        assert!(request.request_id.is_none());
        assert_eq!(request.third_party_request_id, None);

        let preview_body = request.to_preview_body();

        assert_eq!(preview_body.preview.request_id, "SEQ123456789");
        assert_eq!(preview_body.preview.sequence_no, "SEQ123456789");
        assert_eq!(
            preview_body.preview.agent_info.organization_name.as_deref(),
            Some("浙江熙唐智慧城市科技有限公司")
        );
        assert_eq!(
            preview_body.preview.agent_info.auth_level.as_deref(),
            Some("3")
        );
        assert_eq!(
            preview_body.preview.agent_info.nick_name.as_deref(),
            Some("xiyue1")
        );
        assert_eq!(
            preview_body
                .preview
                .agent_info
                .ext_infos
                .as_ref()
                .and_then(|ext| ext.get("CompanyLegRep"))
                .and_then(|v| v.as_str()),
            Some("奚跃")
        );

        assert_eq!(preview_body.preview.material_data.len(), 1);
        let material = &preview_body.preview.material_data[0];
        assert_eq!(
            material.name.as_deref(),
            Some("营业执照（社会统一信用代码证）")
        );
        assert_eq!(
            material
                .extra
                .get("required")
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(material.attachment_list.len(), 1);
        let attachment = &material.attachment_list[0];
        assert_eq!(attachment.attach_name, "license.jpg");
        assert_eq!(attachment.attach_url, "https://example.com/license.jpg");
        assert!(attachment.extra.is_empty());
    }
}
