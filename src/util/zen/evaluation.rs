//! OCR预审评估器 - 传统版本
//!
//! 这是原有的PreviewEvaluator实现，用于向后兼容
//! 新的多阶段并发控制功能在 enhanced_evaluator.rs 中实现

use crate::db::traits::{Database as DbTrait, MaterialFileFilter, MaterialFileRecord};
use crate::model::evaluation::ProcessingStatus;
use crate::model::preview::{Attachment, MaterialValue, Preview, UserInfo};
use crate::storage::Storage;
use crate::util::logging::runtime::ATTACHMENT_LOGGING_RUNTIME;
use crate::util::logging::standards::events;
use crate::util::processing::multi_stage_controller::MULTI_STAGE_CONTROLLER;
use crate::util::processing::TaskResourcePredictor;
use crate::util::system_info::get_memory_usage;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use crate::util::tracing::request_tracker::{LogLevel, RequestTracker, TraceStatus};
use crate::util::worker;
use crate::CONFIG;
use anyhow::{anyhow, Result};
use ocr_conn::ocr::{OcrEngineOptions, GLOBAL_POOL};
use ocr_conn::{pdf_page_count, pdf_render_jpg_range};
use serde_json::{to_value, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, error, info, warn};
use urlencoding::encode;

use crate::util::extract::{self, ExtractedData};
use crate::util::processing::optimized_pipeline::OPTIMIZED_PIPELINE;
use crate::util::rules::{
    compute_definition_fingerprint, MaterialRule, MaterialScope, MaterialValidity,
    MatterRuleConfig, MatterRuleDefinition, RuleMode, RuleRepository,
};
use ocr_conn::CURRENT_DIR;
use std::path::PathBuf;
use tokio::fs;

/// 传统的OCR预审评估器
pub struct PreviewEvaluator {
    pub preview: Preview,
    pub storage: Option<Arc<dyn Storage>>,
    pub database: Option<Arc<dyn DbTrait>>,
    pub tracker: Option<RequestTracker>,
    rule_config: Option<Arc<MatterRuleConfig>>,
    embedded_rule_definition: Option<Arc<MatterRuleDefinition>>,
    rule_fingerprint: Option<String>,
    material_rule_index: HashMap<String, MaterialRule>,
    extracted_map: HashMap<String, ExtractedData>,
}

struct AttachmentDownload {
    bytes: Vec<u8>,
    record_id: Option<String>,
    checksum: Option<String>,
    size_bytes: usize,
    is_pdf: bool,
    page_count: Option<u32>,
    local_path: Option<PathBuf>,
}

impl PreviewEvaluator {
    async fn process_material_attachment(
        &mut self,
        material: &MaterialValue,
        attachment: &crate::model::preview::Attachment,
        attachment_index: usize,
    ) -> Result<String> {
        let attachment_span = tracing::info_span!(
            "material_attachment",
            preview_id = %self.preview.request_id,
            material_code = %material.code,
            attachment_index = attachment_index
        );
        let _attachment_guard = attachment_span.enter();

        let download = self
            .download_attachment_content(material, attachment, attachment_index)
            .await?;

        self.apply_prediction_throttling(material, attachment_index, &download)
            .await;

        if let Some(text) = self
            .try_reuse_existing_attachment(material, attachment_index, &download)
            .await?
        {
            return Ok(text);
        }

        let ocr_start = Instant::now();
        let ocr_text = if download.is_pdf && download.local_path.is_some() {
            // 使用优化后的磁盘PDF处理流水线
            let path = download.local_path.as_ref().unwrap();
            let results = OPTIMIZED_PIPELINE
                .process_pdf_optimized(
                    path.clone(),
                    self.preview.request_id.clone(),
                    material.code.clone(),
                    self.storage.clone(),
                )
                .await?;

            results.join("\n\n")
        } else {
            // 传统内存处理 (图片或无路径PDF)
            self.process_ocr(
                &download.bytes,
                &material.code,
                download.record_id.as_deref(),
            )
            .await?
        };
        let ocr_duration = ocr_start.elapsed();
        debug!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_OCR_COMPLETE,
            material_code = %material.code,
            attachment_index,
            duration_ms = ocr_duration.as_millis() as u64,
            text_length = ocr_text.len()
        );

        // 清理临时PDF文件（仅删除我们生成的临时文件）
        if let Some(path) = download.local_path.as_ref() {
            let temp_root = CURRENT_DIR.join("runtime").join("temp_pdfs");
            if path.starts_with(&temp_root) {
                if let Err(err) = tokio::fs::remove_file(path).await {
                    warn!(
                        preview_id = %self.preview.request_id,
                        material_code = %material.code,
                        attachment_index,
                        path = %path.display(),
                        error = %err,
                        "清理临时PDF文件失败"
                    );
                }
            }
        }

        Ok(ocr_text)
    }

    async fn download_attachment_content(
        &mut self,
        material: &MaterialValue,
        attachment: &Attachment,
        attachment_index: usize,
    ) -> Result<AttachmentDownload> {
        let url = &attachment.attach_url;
        let download_start = Instant::now();

        let mut bytes = Vec::new();
        let mut local_path: Option<PathBuf> = None;
        let mut source_label: Option<String> = None;

        // 1. 尝试获取本地路径（优先）
        if let Some(path_result) = worker::fetch_material_path(
            url,
            Some(&self.preview.request_id),
            Some(&material.code),
            Some(&attachment.attach_name),
        )
        .await
        {
            match path_result {
                Ok(path) => {
                    bytes = fs::read(&path).await?; // 读取内容以兼容后续流程
                    local_path = Some(path);
                    source_label = Some("worker_path".to_string());
                    METRICS_COLLECTOR.record_preview_download(
                        true,
                        download_start.elapsed(),
                        "worker_path",
                    );
                }
                Err(e) => {
                    METRICS_COLLECTOR.record_preview_download(
                        false,
                        download_start.elapsed(),
                        "worker_path",
                    );
                    return Err(e);
                }
            }
        } else if let Some(proxy_result) = worker::fetch_material_via_proxy(
            url,
            Some(&self.preview.request_id),
            Some(&material.code),
            Some(&attachment.attach_name),
        )
        .await
        {
            // 2. Fallback to proxy if path fetch not supported or failed
            match proxy_result {
                Ok(buf) => {
                    bytes = buf;
                    source_label = Some("worker_proxy".to_string());
                    METRICS_COLLECTOR.record_preview_download(
                        true,
                        download_start.elapsed(),
                        "worker_proxy",
                    );
                }
                Err(err) => {
                    METRICS_COLLECTOR.record_preview_download(
                        false,
                        download_start.elapsed(),
                        "worker_proxy",
                    );
                    return Err(err);
                }
            }
        }

        if attachment.attach_url.trim().is_empty() {
            METRICS_COLLECTOR.record_preview_download(
                false,
                download_start.elapsed(),
                "missing_url",
            );
            return Err(anyhow!("附件没有URL"));
        }

        let (file_content, source_label) = if let Some(label) = source_label {
            (bytes, label)
        } else if attachment.attach_url.starts_with("data:") {
            match self.decode_base64_content(&attachment.attach_url) {
                Ok(bytes) => (bytes, "data_url".to_string()),
                Err(e) => {
                    METRICS_COLLECTOR.record_preview_download(
                        false,
                        download_start.elapsed(),
                        "data_url",
                    );
                    return Err(e);
                }
            }
        } else {
            let source = classify_download_source(&attachment.attach_url).to_string();
            match self.download_from_url(&attachment.attach_url).await {
                Ok(bytes) => (bytes, source),
                Err(e) => {
                    METRICS_COLLECTOR.record_preview_download(
                        false,
                        download_start.elapsed(),
                        &source,
                    );
                    return Err(e);
                }
            }
        };

        if file_content.is_empty() {
            METRICS_COLLECTOR.record_preview_download(
                false,
                download_start.elapsed(),
                &source_label,
            );
            return Err(anyhow!(
                "下载内容为空，无法进行OCR (material={}, attachment_index={})",
                material.code,
                attachment_index
            ));
        }

        let download_duration = download_start.elapsed();
        METRICS_COLLECTOR.record_preview_download(true, download_duration, &source_label);
        debug!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_DOWNLOAD_COMPLETE,
            material_code = %material.code,
            attachment_index,
            source = %source_label,
            size_kb = file_content.len() / 1024,
            duration_ms = download_duration.as_millis() as u64
        );

        let result = self
            .finalize_download(
                material,
                attachment,
                attachment_index,
                file_content,
                local_path,
            )
            .await?;
        self.log_attachment_profile(
            material,
            attachment,
            attachment_index,
            &result,
            download_duration,
            &source_label,
        );
        Ok(result)
    }

    async fn finalize_download(
        &mut self,
        material: &MaterialValue,
        attachment: &crate::model::preview::Attachment,
        attachment_index: usize,
        file_content: Vec<u8>,
        existing_local_path: Option<PathBuf>,
    ) -> Result<AttachmentDownload> {
        use sha2::{Digest, Sha256};

        let guessed = mime_guess::from_path(&attachment.attach_url).first_or_octet_stream();
        debug!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_DOWNLOAD_START,
            url = %attachment.attach_url,
            mime = %guessed.essence_str()
        );

        let size_bytes = file_content.len();
        let (is_pdf, page_count) = if size_bytes > 4 && &file_content[0..4] == b"%PDF" {
            let estimated = pdf_page_count(&file_content)
                .ok()
                .or_else(|| estimate_pdf_pages(&file_content).map(|p| p as u32));
            (true, estimated)
        } else {
            (false, Some(1))
        };

        let mut local_path = existing_local_path;
        if is_pdf && local_path.is_none() {
            local_path = self
                .persist_pdf_to_disk(&file_content, &material.code, attachment_index)
                .await;
        }

        let mut record_id = None;
        let mut checksum = None;

        let mut stored_original_key = None;
        if let Some(storage) = &self.storage {
            let preview_id = self.preview.request_id.clone();
            let material_code = material.code.clone();
            let ext = guessed.essence_str().split('/').last().unwrap_or("bin");
            let safe_name = sanitize_name(&attachment.attach_name);
            let filename = if safe_name.is_empty() {
                format!("original-{}.{ext}", attachment_index)
            } else {
                format!("{}-{}.{ext}", safe_name, attachment_index)
            };
            let key = format!(
                "uploads/{}/{}/original/{}",
                preview_id, material_code, filename
            );

            if let Err(e) = storage.put(&key, &file_content).await {
                warn!("保存原始附件失败: {}", e);
                METRICS_COLLECTOR.record_preview_persistence_failure("storage_put_original");
            } else {
                debug!(
                    target: "attachment.pipeline",
                    event = events::ATTACHMENT_COMPLETE,
                    action = "stored_original",
                    storage_key = %key
                );
                stored_original_key = Some(key);
            }
        }

        if let Some(db) = &self.database {
            let mut hasher = Sha256::new();
            hasher.update(&file_content);
            let digest = hex::encode(hasher.finalize());
            checksum = Some(digest.clone());

            let now = chrono::Utc::now();
            let rec_id = uuid::Uuid::new_v4().to_string();
            let safe_name = sanitize_name(&attachment.attach_name);
            let record = MaterialFileRecord {
                id: rec_id.clone(),
                preview_id: self.preview.request_id.clone(),
                material_code: material.code.clone(),
                attachment_name: if safe_name.is_empty() {
                    None
                } else {
                    Some(safe_name)
                },
                source_url: Some(attachment.attach_url.clone()),
                stored_original_key: stored_original_key.clone().unwrap_or_default(),
                stored_processed_keys: None,
                mime_type: Some(guessed.essence_str().to_string()),
                size_bytes: Some(size_bytes as i64),
                checksum_sha256: Some(digest),
                ocr_text_key: None,
                ocr_text_length: None,
                status: "downloaded".to_string(),
                error_message: None,
                created_at: now,
                updated_at: now,
            };

            if let Err(e) = db.save_material_file_record(&record).await {
                warn!(
                    "保存材料文件记录失败（可能主库未实现，已由failover兜底）: {}",
                    e
                );
                METRICS_COLLECTOR.record_preview_persistence_failure("db_save_material_file");
            } else {
                debug!(
                    target: "attachment.pipeline",
                    event = events::ATTACHMENT_COMPLETE,
                    material_code = %material.code,
                    attachment_index,
                    action = "stored_raw",
                    record_id = %rec_id
                );
                record_id = Some(rec_id);
            }
        }

        Ok(AttachmentDownload {
            bytes: if is_pdf && local_path.is_some() {
                Vec::new()
            } else {
                file_content
            },
            record_id,
            checksum,
            size_bytes,
            is_pdf,
            page_count,
            local_path,
        })
    }

    async fn persist_pdf_to_disk(
        &self,
        file_content: &[u8],
        material_code: &str,
        attachment_index: usize,
    ) -> Option<PathBuf> {
        let temp_root = resolve_temp_pdf_dir();
        if let Err(err) = fs::create_dir_all(&temp_root).await {
            warn!(
                preview_id = %self.preview.request_id,
                material_code = %material_code,
                attachment_index,
                error = %err,
                "创建临时PDF目录失败"
            );
            return None;
        }

        let filename = format!(
            "{}-{}-{}.pdf",
            sanitize_name(&self.preview.request_id),
            sanitize_name(material_code),
            attachment_index
        );
        let path = temp_root.join(filename);

        if let Err(err) = fs::write(&path, file_content).await {
            warn!(
                preview_id = %self.preview.request_id,
                material_code = %material_code,
                attachment_index,
                path = %path.display(),
                error = %err,
                "写入临时PDF失败"
            );
            return None;
        }

        info!(
            target: "attachment.pipeline",
            event = "temp_pdf_written",
            preview_id = %self.preview.request_id,
            material_code = %material_code,
            attachment_index,
            path = %path.display()
        );

        // 异步清理过期临时文件，避免磁盘持续膨胀
        let cleanup_root = temp_root.clone();
        let ttl_hours = CONFIG.master.temp_pdf_ttl_hours;
        tokio::spawn(async move {
            cleanup_expired_temp_pdfs(cleanup_root, ttl_hours).await;
        });

        Some(path)
    }

    fn log_attachment_profile(
        &self,
        material: &MaterialValue,
        attachment: &crate::model::preview::Attachment,
        attachment_index: usize,
        download: &AttachmentDownload,
        duration: Duration,
        source: &str,
    ) {
        info!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_PROFILE,
            preview_id = %self.preview.request_id,
            material_code = %material.code,
            attachment_index,
            attachment_name = %attachment.attach_name,
            size_bytes = download.size_bytes as u64,
            size_kb = download.size_bytes / 1024,
            is_pdf = download.is_pdf,
            pages = download.page_count.unwrap_or(0),
            download_ms = duration.as_millis() as u64,
            source = %source
        );
    }

    async fn apply_prediction_throttling(
        &self,
        material: &MaterialValue,
        attachment_index: usize,
        download: &AttachmentDownload,
    ) {
        let file_type = if download.is_pdf { "PDF" } else { "IMAGE" };
        let profile = TaskResourcePredictor::predict_task_resources(download.size_bytes, file_type);
        info!(
            target: "processing.adaptive",
            event = events::PIPELINE_STAGE,
            stage = "resource_prediction",
            preview_id = %self.preview.request_id,
            material_code = %material.code,
            attachment_index,
            file_type = %file_type,
            size_mb = format!("{:.2}", profile.file_size_mb),
            predicted_pages = profile.estimated_pages,
            peak_memory_mb = profile.peak_memory_mb,
            recommendation = ?profile.execution_recommendation,
            risk = ?profile.risk_level
        );

        let baseline_mb = CONFIG
            .concurrency
            .as_ref()
            .map(|cfg| cfg.resource_limits.max_memory_per_task as f64)
            .unwrap_or(4096.0);
        let predicted_percent =
            ((profile.peak_memory_mb as f64) / baseline_mb * 100.0).clamp(25.0, 200.0);
        MULTI_STAGE_CONTROLLER
            .adaptive_tune_once(predicted_percent)
            .await;
    }

    async fn try_reuse_existing_attachment(
        &mut self,
        material: &MaterialValue,
        attachment_index: usize,
        download: &AttachmentDownload,
    ) -> Result<Option<String>> {
        let (db, storage) = match (&self.database, &self.storage) {
            (Some(db), Some(storage)) => (db, storage),
            _ => return Ok(None),
        };

        let checksum = match download.checksum.as_deref() {
            Some(c) => c,
            None => return Ok(None),
        };

        let record_id = match download.record_id.as_deref() {
            Some(id) => id,
            None => return Ok(None),
        };

        let filter = MaterialFileFilter {
            preview_id: Some(self.preview.request_id.clone()),
            material_code: Some(material.code.clone()),
        };

        if let Ok(records) = db.list_material_files(&filter).await {
            if let Some(prev) = records
                .into_iter()
                .filter(|r| r.id != record_id && r.checksum_sha256.as_deref() == Some(checksum))
                .rev()
                .next()
            {
                if let Some(ocr_key) = &prev.ocr_text_key {
                    if let Ok(Some(bytes)) = storage.get(ocr_key).await {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        if let Err(err) = db
                            .update_material_file_processing(
                                record_id,
                                prev.stored_processed_keys.as_deref(),
                                Some(ocr_key),
                                prev.ocr_text_length,
                            )
                            .await
                        {
                            METRICS_COLLECTOR.record_preview_persistence_failure(
                                "db_update_material_processing",
                            );
                            warn!("更新材料处理信息失败: {}", err);
                        }
                        if let Err(err) = db
                            .update_material_file_status(record_id, "reused", None)
                            .await
                        {
                            METRICS_COLLECTOR
                                .record_preview_persistence_failure("db_update_material_status");
                            warn!("更新材料状态失败: {}", err);
                        }
                        debug!(
                            target: "attachment.pipeline",
                            event = events::ATTACHMENT_REUSED,
                            material_code = %material.code,
                            attachment_index,
                            ocr_key = %ocr_key
                        );
                        return Ok(Some(text));
                    }
                }
            }
        }

        Ok(None)
    }
    /// 创建新的评估器实例
    pub fn new(preview: Preview) -> Self {
        Self {
            preview,
            storage: None,
            database: None,
            tracker: None,
            rule_config: None,
            embedded_rule_definition: None,
            rule_fingerprint: None,
            material_rule_index: HashMap::new(),
            extracted_map: HashMap::new(),
        }
    }

    /// 创建带存储的评估器实例
    pub fn new_with_storage(preview: Preview, storage: Arc<dyn Storage>) -> Self {
        Self {
            preview,
            storage: Some(storage),
            database: None,
            tracker: None,
            rule_config: None,
            embedded_rule_definition: None,
            rule_fingerprint: None,
            material_rule_index: HashMap::new(),
            extracted_map: HashMap::new(),
        }
    }

    /// 创建带追踪器的评估器实例
    pub fn new_with_tracker(
        preview: Preview,
        storage: Option<Arc<dyn Storage>>,
        database: Option<Arc<dyn DbTrait>>,
        tracker: RequestTracker,
    ) -> Self {
        Self {
            preview,
            storage,
            database,
            tracker: Some(tracker),
            rule_config: None,
            embedded_rule_definition: None,
            rule_fingerprint: None,
            material_rule_index: HashMap::new(),
            extracted_map: HashMap::new(),
        }
    }

    /// 创建带存储和数据库的评估器实例（不启用追踪）
    pub fn new_with_resources(
        preview: Preview,
        storage: Option<Arc<dyn Storage>>,
        database: Option<Arc<dyn DbTrait>>,
    ) -> Self {
        Self {
            preview,
            storage,
            database,
            tracker: None,
            rule_config: None,
            embedded_rule_definition: None,
            rule_fingerprint: None,
            material_rule_index: HashMap::new(),
            extracted_map: HashMap::new(),
        }
    }

    pub fn set_embedded_rule_definition(&mut self, definition: Option<Arc<MatterRuleDefinition>>) {
        self.embedded_rule_definition = definition;

        if self.rule_config.is_some() {
            return;
        }

        if let Some(definition) = self.embedded_rule_definition.clone() {
            self.update_rule_fingerprint(&definition);
            if self.material_rule_index.is_empty() {
                self.material_rule_index = definition
                    .materials
                    .iter()
                    .map(|rule| (rule.id.clone(), rule.clone()))
                    .collect();
            }
        }
    }

    fn update_rule_fingerprint(&mut self, definition: &MatterRuleDefinition) {
        match compute_definition_fingerprint(definition) {
            Ok(fingerprint) => self.rule_fingerprint = Some(fingerprint),
            Err(err) => {
                warn!(
                    matter_id = %self.preview.matter_id,
                    error = %err,
                    "计算事项规则指纹失败"
                );
                self.rule_fingerprint = None;
            }
        }
    }

    fn current_rule_mode(&self) -> &str {
        if let Some(config) = &self.rule_config {
            return config.mode.as_str();
        }

        if let Some(definition) = &self.embedded_rule_definition {
            return definition.mode.as_str();
        }

        "default"
    }

    fn current_rule_source(&self) -> &'static str {
        if self.rule_config.is_some() {
            "database"
        } else if self.embedded_rule_definition.is_some() {
            "embedded"
        } else {
            "default"
        }
    }

    fn log_rule_audit(
        &self,
        material: &MaterialValue,
        rule: Option<&MaterialRule>,
        status_code: u16,
        message: &str,
        errors: &[String],
        warnings: &[String],
        suggestions: &[String],
    ) {
        let attachment_names: Vec<String> = material
            .attachment_list
            .iter()
            .map(|a| a.attach_name.clone())
            .collect();

        let (rule_id, rule_name, required, mode_name) = if let Some(rule) = rule {
            (
                rule.id.as_str(),
                rule.name.as_deref().unwrap_or(""),
                Some(rule.required),
                rule.scope.clone(),
            )
        } else {
            ("default", "默认规则", None, MaterialScope::Global)
        };

        let rule_scope = match mode_name {
            MaterialScope::Global => "global",
            MaterialScope::PerVehicle => "perVehicle",
            MaterialScope::Custom(ref value) => value.as_str(),
        };

        tracing::info!(
            target = "rule.audit",
            event = "rule_evaluation",
            preview_id = %self.preview.request_id,
            matter_id = %self.preview.matter_id,
            matter_name = %self.preview.matter_name,
            matter_type = %self.preview.matter_type,
            material_code = %material.code,
            material_name = %material.name.as_deref().unwrap_or(""),
            rule_id = %rule_id,
            rule_name = %rule_name,
            rule_required = required,
            rule_scope = %rule_scope,
            rule_mode = %self.current_rule_mode(),
            rule_source = %self.current_rule_source(),
            fingerprint = self.rule_fingerprint.as_deref().unwrap_or(""),
            status_code,
            message = %message,
            errors = ?errors,
            warnings = ?warnings,
            suggestions = ?suggestions,
            attachment_total = attachment_names.len(),
            attachments = ?attachment_names
        );
    }

    /// 简化的评估入口方法 - 调用evaluate_all
    pub async fn evaluate(&mut self) -> Result<Vec<MaterialEvaluationResult>> {
        if let Err(err) = self.ensure_rule_config().await {
            warn!(
                "加载事项规则配置失败: matter_id={}, error={}",
                self.preview.matter_id, err
            );
        }

        let result = self.evaluate_all().await;

        let (status, message, level) = match &result {
            Ok(_) => (
                TraceStatus::Success,
                format!(
                    "预审评估完成，共处理{}份材料",
                    self.preview.material_data.len()
                ),
                LogLevel::Info,
            ),
            Err(err) => (
                TraceStatus::Failed,
                format!("预审评估失败: {}", err),
                LogLevel::Error,
            ),
        };

        self.finalize_tracker(status, message, level).await;
        result
    }

    async fn ensure_rule_config(&mut self) -> Result<()> {
        if self.rule_config.is_some() {
            return Ok(());
        }

        let mut attempted_db_lookup = false;

        if let Some(db) = &self.database {
            attempted_db_lookup = true;
            let repo = RuleRepository::new(Arc::clone(db));
            match repo.fetch(&self.preview.matter_id).await {
                Ok(Some(config)) => {
                    info!(
                        "已加载事项规则配置: matter_id={}, mode={}",
                        config.matter_id(),
                        config.mode.as_str()
                    );
                    let arc_config = Arc::new(config);
                    self.material_rule_index = arc_config
                        .definition
                        .materials
                        .iter()
                        .map(|rule| (rule.id.clone(), rule.clone()))
                        .collect();

                    if let Some(checksum) = arc_config.record.checksum.clone() {
                        self.rule_fingerprint = Some(checksum);
                    } else {
                        self.update_rule_fingerprint(&arc_config.definition);
                    }

                    self.rule_config = Some(Arc::clone(&arc_config));
                    return Ok(());
                }
                Ok(None) => {
                    info!(
                        "事项 {} 未找到专属规则配置，尝试使用任务携带的规则定义",
                        self.preview.matter_id
                    );
                }
                Err(err) => {
                    warn!(
                        "加载事项规则配置异常: matter_id={}, error={}",
                        self.preview.matter_id, err
                    );
                }
            }
        }

        if let Some(definition) = &self.embedded_rule_definition {
            if self.material_rule_index.is_empty() {
                self.material_rule_index = definition
                    .materials
                    .iter()
                    .map(|rule| (rule.id.clone(), rule.clone()))
                    .collect();
            }
            info!(
                "使用任务携带的事项规则: matter_id={}, mode={}",
                self.preview.matter_id,
                definition.mode.as_str()
            );
            return Ok(());
        }

        if !attempted_db_lookup {
            info!(
                "未配置数据库，事项 {} 使用默认规则流程",
                self.preview.matter_id
            );
        }

        Ok(())
    }

    fn lookup_material_rule(&self, material_code: &str) -> Option<&MaterialRule> {
        self.material_rule_index.get(material_code)
    }

    fn should_check_missing_materials(&self) -> bool {
        if let Some(config) = &self.rule_config {
            return config.mode != RuleMode::PresentOnly;
        }

        if let Some(definition) = &self.embedded_rule_definition {
            return definition.mode != RuleMode::PresentOnly;
        }

        false
    }

    /// 评估并返回完整的预审结果
    pub async fn evaluate_complete(
        &mut self,
    ) -> Result<crate::model::evaluation::PreviewEvaluationResult> {
        use crate::model::evaluation::{
            BasicInfo, EvaluationSummary, OverallResult, PreviewEvaluationResult,
        };
        use chrono::Local;

        let material_results_local = self.evaluate().await?;

        // 预先构建材料URL映射（material_code -> 公网URL）
        let material_url_map = self.build_material_url_map().await;

        // 转换MaterialEvaluationResult为模型类型
        let material_results = material_results_local
            .into_iter()
            .map(|r| self.convert_material_result(r, &material_url_map))
            .collect::<Vec<_>>();

        // 构建基础信息
        let subject = &self.preview.subject_info;
        let agent = &self.preview.agent_info;
        let applicant_name = subject
            .user_name
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(&subject.user_id)
            .to_string();
        let agent_name = agent
            .user_name
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .unwrap_or(&agent.user_id)
            .to_string();
        let basic_info = BasicInfo {
            applicant_name,
            applicant_id: subject.user_id.clone(),
            applicant_org: subject.organization_name.clone(),
            applicant_phone: subject.phone_number.clone(),
            applicant_certificate_number: subject.certificate_number.clone(),
            agent_name,
            agent_id: agent.user_id.clone(),
            agent_org: agent.organization_name.clone(),
            agent_phone: agent.phone_number.clone(),
            agent_certificate_number: agent.certificate_number.clone(),
            matter_id: self.preview.matter_id.clone(),
            matter_name: self.preview.matter_name.clone(),
            matter_type: self.preview.matter_type.clone(),
            request_id: self.preview.request_id.clone(),
            sequence_no: self.preview.sequence_no.clone(),
            theme_id: self
                .preview
                .theme_id
                .clone()
                .unwrap_or_else(|| self.preview.matter_id.clone()),
            theme_name: self.preview.matter_name.clone(),
        };

        // 构建评估摘要
        let total_materials = material_results.len();
        let passed_materials = material_results
            .iter()
            .filter(|r| r.rule_evaluation.status_code == 200)
            .count();
        let failed_materials = total_materials - passed_materials;

        let overall_result = if failed_materials == 0 {
            OverallResult::Passed
        } else if passed_materials > failed_materials {
            OverallResult::PassedWithSuggestions
        } else {
            OverallResult::Failed
        };

        let evaluation_summary = EvaluationSummary {
            total_materials,
            passed_materials,
            failed_materials,
            warning_materials: 0, // 简化版本暂时为0
            overall_result,
            overall_suggestions: Vec::new(),
        };

        Ok(PreviewEvaluationResult {
            basic_info,
            material_results,
            evaluation_summary,
            evaluation_time: Local::now(),
        })
    }

    async fn finalize_tracker(&mut self, status: TraceStatus, message: String, level: LogLevel) {
        if let Some(mut tracker) = self.tracker.take() {
            tracker.record_event(&message, level);
            tracker.finish_span_with_status(status);
            tracker.finish().await;
        }
    }

    /// 构建材料URL映射，从数据库查询stored_original_key并生成公网URL
    async fn build_material_url_map(&self) -> HashMap<String, String> {
        let mut url_map = HashMap::new();

        let Some(db) = &self.database else {
            return url_map;
        };
        let Some(storage) = &self.storage else {
            return url_map;
        };

        let filter = MaterialFileFilter {
            preview_id: Some(self.preview.request_id.clone()),
            material_code: None,
        };

        let records = match db.list_material_files(&filter).await {
            Ok(records) => records,
            Err(e) => {
                warn!("查询材料文件记录失败: {}", e);
                return url_map;
            }
        };

        for record in records {
            if record.stored_original_key.is_empty() {
                continue;
            }

            // 使用 material_code + attachment_name 作为复合 key
            let lookup_key = if let Some(attach_name) = &record.attachment_name {
                format!("{}:{}", record.material_code, attach_name)
            } else {
                record.material_code.clone()
            };

            // 安全加固：统一使用受保护的存储代理地址，不暴露存储公网URL
            let base = CONFIG.base_url();
            let proxy_url = format!(
                "{}/api/storage/files/{}",
                base.trim_end_matches('/'),
                encode(record.stored_original_key.trim_start_matches('/'))
            );
            url_map.insert(lookup_key, proxy_url);
        }

        debug!(
            preview_id = %self.preview.request_id,
            url_count = url_map.len(),
            "构建材料URL映射完成"
        );

        url_map
    }

    /// 转换本地MaterialEvaluationResult为模型类型
    fn convert_material_result(
        &self,
        local_result: MaterialEvaluationResult,
        material_url_map: &HashMap<String, String>,
    ) -> crate::model::evaluation::MaterialEvaluationResult {
        use crate::model::evaluation::{
            AttachmentInfo, MaterialEvaluationResult as ModelResult, ProcessingStatus,
            RuleEvaluationResult,
        };

        let evaluation_message = local_result.evaluation_message.clone();

        let (material_name, attachment_infos) = self
            .preview
            .material_data
            .iter()
            .find(|material| material.code == local_result.material_code)
            .map(|material| {
                let attachments = material
                    .attachment_list
                    .iter()
                    .map(|attachment| {
                        let file_type = self.infer_file_extension(attachment);
                        let mime_type = self.infer_mime_type(attachment, file_type.as_deref());
                        let file_size = self.extract_u64_multi(
                            &attachment.extra,
                            &["fileSize", "size", "file_size"],
                        );
                        let page_count = self
                            .extract_u64_multi(
                                &attachment.extra,
                                &["pageCount", "pages", "page_count"],
                            )
                            .map(|v| v as u32);

                        // 优先从预构建的URL映射中获取OSS公网URL
                        let composite_key = format!("{}:{}", material.code, attachment.attach_name);
                        let oss_public_url = material_url_map
                            .get(&composite_key)
                            .or_else(|| material_url_map.get(&material.code))
                            .cloned();

                        // 如果有OSS URL则使用，否则回退到原始URL
                        let resolved_file_url = oss_public_url
                            .clone()
                            .unwrap_or_else(|| attachment.attach_url.clone());

                        // preview_url 优先使用 extra 中的配置，其次使用 OSS URL
                        let preview_url = self
                            .extract_string_multi(
                                &attachment.extra,
                                &["previewUrl", "preview_url", "preview"],
                            )
                            .or(oss_public_url);

                        let thumbnail_url = self.extract_string_multi(
                            &attachment.extra,
                            &["thumbnailUrl", "thumbnail_url", "thumbnail"],
                        );
                        let extra = if attachment.extra.is_empty() {
                            None
                        } else {
                            to_value(&attachment.extra).ok().filter(|v| !v.is_null())
                        };

                        AttachmentInfo {
                            file_name: attachment.attach_name.clone(),
                            file_url: resolved_file_url,
                            file_type,
                            mime_type,
                            file_size,
                            page_count,
                            preview_url,
                            thumbnail_url,
                            is_cloud_share: attachment.is_cloud_share,
                            ocr_success: local_result.is_success,
                            extra,
                        }
                    })
                    .collect::<Vec<_>>();

                (
                    material
                        .name
                        .clone()
                        .unwrap_or_else(|| material.code.clone()),
                    attachments,
                )
            })
            .unwrap_or_else(|| (local_result.material_code.clone(), Vec::new()));

        let status_code = match local_result.evaluation_status.as_str() {
            "success" => 200,
            "warning" => 206,
            _ => 500,
        };

        let suggestions = local_result.extracted_info.clone();

        let processing_status = match local_result.evaluation_status.as_str() {
            "success" => ProcessingStatus::Success,
            "warning" => ProcessingStatus::PartialSuccess {
                warnings: suggestions.clone(),
            },
            _ => ProcessingStatus::Failed {
                error: evaluation_message.clone(),
            },
        };

        let friendly_summary =
            Self::build_display_summary_text(status_code, &evaluation_message, &suggestions);
        let friendly_detail =
            Self::build_display_detail_text(&processing_status, &suggestions, &evaluation_message);

        ModelResult {
            material_code: local_result.material_code,
            material_name,
            attachments: attachment_infos,
            ocr_content: local_result.ocr_content.unwrap_or_default(),
            rule_evaluation: RuleEvaluationResult {
                status_code,
                message: evaluation_message.clone(),
                description: "评估完成".to_string(),
                suggestions: suggestions.clone(),
                rule_details: None,
            },
            processing_status,
            display_summary: Some(friendly_summary),
            display_detail: Some(friendly_detail),
        }
    }

    fn build_display_summary_text(
        status_code: u64,
        message: &str,
        suggestions: &[String],
    ) -> String {
        if status_code == 200 {
            return "系统自动核验通过".to_string();
        }

        if let Some(first) = Self::first_non_empty(suggestions) {
            return first.to_string();
        }

        let trimmed = message.trim();
        if trimmed.is_empty() {
            "请人工复核".to_string()
        } else {
            trimmed.to_string()
        }
    }

    fn build_display_detail_text(
        status: &ProcessingStatus,
        suggestions: &[String],
        fallback_message: &str,
    ) -> String {
        match status {
            ProcessingStatus::Success => "材料信息与规则匹配，系统自动通过".to_string(),
            ProcessingStatus::PartialSuccess { warnings } => {
                if let Some(joined) = Self::first_non_empty(warnings).map(|s| s.to_string()) {
                    joined
                } else if let Some(joined) = Self::join_non_empty(suggestions) {
                    joined
                } else {
                    "部分字段需要人工确认".to_string()
                }
            }
            ProcessingStatus::Failed { error } => {
                let text = if !error.trim().is_empty() {
                    error.trim()
                } else if let Some(joined) = Self::join_non_empty(suggestions) {
                    return joined;
                } else {
                    fallback_message.trim()
                };

                if text.is_empty() {
                    "系统自动校验未通过，请人工复核".to_string()
                } else {
                    text.to_string()
                }
            }
        }
    }

    fn first_non_empty(items: &[String]) -> Option<&str> {
        items.iter().map(|s| s.trim()).find(|s| !s.is_empty())
    }

    fn join_non_empty(items: &[String]) -> Option<String> {
        let filtered: Vec<_> = items
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        if filtered.is_empty() {
            None
        } else {
            Some(filtered.join("；"))
        }
    }

    fn extract_value<'a>(&self, extra: &'a HashMap<String, Value>, key: &str) -> Option<&'a Value> {
        if let Some(value) = extra.get(key) {
            return Some(value);
        }

        let key_lower = key.to_ascii_lowercase();
        extra.iter().find_map(|(k, v)| {
            if k.eq_ignore_ascii_case(key) || k.to_ascii_lowercase() == key_lower {
                Some(v)
            } else {
                None
            }
        })
    }

    fn extract_string_multi(
        &self,
        extra: &HashMap<String, Value>,
        keys: &[&str],
    ) -> Option<String> {
        for key in keys {
            if let Some(value) = self.extract_string(extra, key) {
                if !value.trim().is_empty() {
                    return Some(value);
                }
            }
        }
        None
    }

    fn extract_string(&self, extra: &HashMap<String, Value>, key: &str) -> Option<String> {
        let value = self.extract_value(extra, key)?;
        match value {
            Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
            Value::Number(n) => Some(n.to_string()),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    fn extract_u64_multi(&self, extra: &HashMap<String, Value>, keys: &[&str]) -> Option<u64> {
        for key in keys {
            if let Some(value) = self.extract_u64(extra, key) {
                return Some(value);
            }
        }
        None
    }

    fn extract_u64(&self, extra: &HashMap<String, Value>, key: &str) -> Option<u64> {
        let value = self.extract_value(extra, key)?;
        match value {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.trim().parse::<u64>().ok(),
            _ => None,
        }
    }

    fn infer_file_extension(&self, attachment: &Attachment) -> Option<String> {
        self.extract_string_multi(
            &attachment.extra,
            &[
                "fileExtension",
                "fileExt",
                "suffix",
                "fileSuffix",
                "extension",
            ],
        )
        .and_then(|raw| Self::normalize_extension(&raw))
        .or_else(|| Self::extension_from_candidate(&attachment.attach_name))
        .or_else(|| Self::extension_from_candidate(&attachment.attach_url))
    }

    fn infer_mime_type(&self, attachment: &Attachment, ext: Option<&str>) -> Option<String> {
        self.extract_string_multi(
            &attachment.extra,
            &["mimeType", "contentType", "content_type"],
        )
        .or_else(|| ext.and_then(Self::mime_from_extension))
        .or_else(|| Self::mime_from_path(&attachment.attach_name))
        .or_else(|| Self::mime_from_path(&attachment.attach_url))
    }

    fn normalize_extension(raw: &str) -> Option<String> {
        let trimmed = raw.trim().trim_start_matches('.');
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_ascii_lowercase())
        }
    }

    fn extension_from_candidate(candidate: &str) -> Option<String> {
        let candidate = candidate.split('#').next().unwrap_or(candidate);
        let candidate = candidate.split('?').next().unwrap_or(candidate);
        let segment = candidate.rsplit('/').next().unwrap_or(candidate);
        if segment.is_empty() || segment.ends_with('.') {
            return None;
        }
        segment
            .rsplit_once('.')
            .map(|(_, ext)| ext.to_ascii_lowercase())
    }

    fn mime_from_extension(ext: &str) -> Option<String> {
        mime_guess::from_ext(ext).first_raw().map(|m| m.to_string())
    }

    fn mime_from_path(path: &str) -> Option<String> {
        mime_guess::from_path(path)
            .first_raw()
            .map(|m| m.to_string())
    }

    /// 评估所有材料
    pub async fn evaluate_all(&mut self) -> Result<Vec<MaterialEvaluationResult>> {
        let evaluation_start = Instant::now();
        let material_total = self.preview.material_data.len();
        let preview_id = self.preview.request_id.clone();

        info!(
            target: "attachment.pipeline",
            event = events::MATERIAL_BATCH_START,
            preview_id = %preview_id,
            material_total
        );

        let mut results = Vec::new();
        let mut processed_codes: HashSet<String> = HashSet::new();
        let material_data = self.preview.material_data.clone();

        for (index, material) in material_data.iter().enumerate() {
            debug!(
                target: "attachment.pipeline",
                event = events::MATERIAL_START,
                preview_id = %preview_id,
                material_code = %material.code,
                material_index = index,
                material_total,
                attachment_total = material.attachment_list.len()
            );

            match self.evaluate_single_material(material, index).await {
                Ok(result) => {
                    processed_codes.insert(material.code.clone());
                    results.push(result);
                }
                Err(e) => {
                    error!(
                        target: "attachment.pipeline",
                        event = events::MATERIAL_ERROR,
                        preview_id = %preview_id,
                        material_code = %material.code,
                        error = %e
                    );
                    let error_result = MaterialEvaluationResult::new_with_error(
                        material.code.clone(),
                        format!("处理失败: {}", e),
                    );
                    processed_codes.insert(material.code.clone());
                    results.push(error_result);
                }
            }
        }

        if self.should_check_missing_materials() {
            for (code, rule) in &self.material_rule_index {
                if rule.required && !processed_codes.contains(code) {
                    let mut missing = MaterialEvaluationResult::new(code.clone());
                    let name = rule
                        .name
                        .as_deref()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| code.clone());
                    let message = format!("缺少必传材料：{}", name);
                    missing.set_evaluation_result(message.clone(), "error");
                    missing.set_extracted_info(vec![message]);
                    results.push(missing);
                }
            }
        }

        let total_duration = evaluation_start.elapsed();
        info!(
            target: "attachment.pipeline",
            event = events::MATERIAL_BATCH_COMPLETE,
            preview_id = %preview_id,
            material_total = results.len(),
            duration_ms = total_duration.as_millis() as u64
        );

        let success_count = results.iter().filter(|r| r.is_success()).count();
        let warning_count = results
            .iter()
            .filter(|r| r.evaluation_status == "warning")
            .count();
        let error_count = results.len().saturating_sub(success_count + warning_count);
        info!(
            target: "attachment.pipeline",
            event = events::MATERIAL_STATS,
            preview_id = %preview_id,
            success = success_count as u32,
            warning = warning_count as u32,
            error = error_count as u32
        );

        Ok(results)
    }

    /// 评估单个材料
    async fn evaluate_single_material(
        &mut self,
        material: &MaterialValue,
        _material_index: usize,
    ) -> Result<MaterialEvaluationResult> {
        if material.attachment_list.is_empty() {
            return Err(anyhow!("材料{}缺少附件", material.code));
        }

        let preview_id = self.preview.request_id.clone();
        let attachment_settings = attachment_log_settings();
        let material_start = Instant::now();

        let mut attachment_texts = Vec::new();
        for (idx, attachment) in material.attachment_list.iter().enumerate() {
            let attachment_start = Instant::now();
            let sample_logging = attachment_settings.should_sample(idx);

            if sample_logging {
                debug!(
                    target: "attachment.pipeline",
                    event = events::ATTACHMENT_START,
                    preview_id = %preview_id,
                    material_code = %material.code,
                    attachment_index = idx,
                    attachment_name = %attachment.attach_name
                );
            }

            let text = self
                .process_material_attachment(material, attachment, idx)
                .await
                .map_err(|err| {
                    anyhow!(
                        "附件处理失败 (material={}, index={}): {}",
                        material.code,
                        idx,
                        err
                    )
                })?;
            let elapsed = attachment_start.elapsed();
            let duration_ms = elapsed.as_millis() as u64;

            if duration_ms >= attachment_settings.slow_threshold_ms {
                warn!(
                    target: "attachment.pipeline",
                    event = events::ATTACHMENT_SLOW,
                    preview_id = %preview_id,
                    material_code = %material.code,
                    attachment_index = idx,
                    duration_ms,
                    threshold_ms = attachment_settings.slow_threshold_ms
                );
            } else if sample_logging {
                debug!(
                    target: "attachment.pipeline",
                    event = events::ATTACHMENT_COMPLETE,
                    preview_id = %preview_id,
                    material_code = %material.code,
                    attachment_index = idx,
                    duration_ms,
                    text_length = text.len()
                );
            }
            attachment_texts.push(text);
        }

        let combined_text = attachment_texts.join("\n\n");

        let result = self
            .process_evaluation_result(combined_text, material, material_start)
            .await?;

        let total_duration = material_start.elapsed();
        debug!(
            target: "attachment.pipeline",
            event = events::MATERIAL_COMPLETE,
            preview_id = %preview_id,
            material_code = %material.code,
            status = %result.evaluation_status,
            duration_ms = total_duration.as_millis() as u64
        );

        Ok(result)
    }

    /// 解码Base64内容
    fn decode_base64_content(&self, data_url: &str) -> Result<Vec<u8>> {
        let data_part = data_url
            .split(',')
            .nth(1)
            .ok_or_else(|| anyhow!("无效的Base64数据URL"))?;

        use base64::{engine::general_purpose, Engine as _};
        let content = general_purpose::STANDARD
            .decode(data_part)
            .map_err(|e| anyhow!("Base64解码失败: {}", e))?;

        Ok(content)
    }

    /// 从URL下载文件
    async fn download_from_url(&self, url: &str) -> Result<Vec<u8>> {
        // 使用通用下载器，支持 http/https/file 协议
        debug!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_DOWNLOAD_START,
            url = %url
        );
        let bytes = crate::util::zen::downloader::download_file_content(url)
            .await
            .map_err(|e| anyhow!("URL下载失败: {}", e))?;
        debug!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_DOWNLOAD_COMPLETE,
            url = %url,
            bytes = bytes.len()
        );
        Ok(bytes)
    }

    /// 处理OCR
    async fn process_ocr(
        &mut self,
        file_content: &[u8],
        material_code: &str,
        record_id: Option<&str>,
    ) -> Result<String> {
        let ocr_start = Instant::now();
        // 构建本地引擎启动参数（自动探测 + 可选配置）
        let engine_opts = if let Some(cfg) = &crate::CONFIG.ocr_engine {
            let work_dir = cfg.work_dir.as_ref().map(|s| std::path::PathBuf::from(s));
            let binary = cfg.binary.as_ref().map(|s| std::path::PathBuf::from(s));
            let lib_path = cfg.lib_path.as_ref().map(|s| std::path::PathBuf::from(s));
            OcrEngineOptions {
                work_dir,
                binary,
                lib_path,
                timeout_secs: cfg.timeout_secs,
            }
        } else {
            OcrEngineOptions::default()
        };
        // 配置全局池的启动参数（仅首次生效）
        GLOBAL_POOL.set_options_if_empty(engine_opts);
        // 简易自适应：在任务开始前做一次内存水位调节
        let mem = get_memory_usage();
        MULTI_STAGE_CONTROLLER
            .adaptive_tune_once(mem.usage_percent as f64)
            .await;

        // 简单魔数检测PDF: 以 %PDF- 开头
        let is_pdf = file_content.len() > 4 && &file_content[0..4] == b"%PDF";
        let text_content = if is_pdf {
            debug!(
                target: "attachment.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "pdf_detect",
                size_kb = file_content.len() / 1024
            );
            let limits = &crate::CONFIG.download_limits;
            // 页数与策略
            let total_pages = pdf_page_count(file_content)
                .unwrap_or_else(|_| estimate_pdf_pages(file_content).unwrap_or(0) as u32);
            let allowed_pages = total_pages.min(limits.pdf_max_pages);
            debug!(
                target: "attachment.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "pdf_stats",
                total_pages,
                allowed_pages,
                limit_pages = limits.pdf_max_pages
            );
            if total_pages > limits.pdf_max_pages {
                tracing::error!(
                    " PDF页数超限被拒绝: {} > {}",
                    total_pages,
                    limits.pdf_max_pages
                );
                return Err(anyhow!(
                    "PDF页数超限: {} > {}",
                    total_pages,
                    limits.pdf_max_pages
                ));
            }

            // 每请求OCR水位（默认20页）
            let window = crate::CONFIG
                .concurrency
                .as_ref()
                .map(|c| c.queue_monitoring.max_queue_length.max(1))
                .unwrap_or(20) as u32;

            let mut all_text = Vec::new();
            let mut start = 1u32;
            let pdf_processing_start = Instant::now();
            debug!(
                target: "attachment.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "pdf_batch_init",
                allowed_pages,
                batch_size = window
            );
            while start <= allowed_pages {
                let end = (start + window - 1).min(allowed_pages);
                let batch_start = Instant::now();
                debug!(
                    target: "attachment.pipeline",
                    event = events::PIPELINE_STAGE,
                    stage = "pdf_batch",
                    page_start = start,
                    page_end = end,
                    allowed_pages
                );

                // PDF转换许可
                let _pdf_permit = MULTI_STAGE_CONTROLLER
                    .acquire_pdf_convert_permit()
                    .await
                    .map_err(|e| anyhow!("获取PDF转换许可失败: {}", e))?;

                let lim = &crate::CONFIG.download_limits;
                let material_label = material_code.to_string();
                let batch_label = format!("{}-{}", start, end);
                let render_start = Instant::now();
                let image_paths = pdf_render_jpg_range(
                    "upload.pdf",
                    file_content,
                    start,
                    end,
                    limits.max_pdf_mb as usize,
                    lim.pdf_render_dpi,
                    Some(lim.pdf_jpeg_quality),
                )
                .map_err(|e| {
                    let duration = render_start.elapsed();
                    let mut labels = HashMap::new();
                    labels.insert("material".to_string(), material_label.clone());
                    labels.insert("batch".to_string(), batch_label.clone());
                    let err_msg = e.to_string();
                    METRICS_COLLECTOR.record_pipeline_stage(
                        "pdf_render",
                        false,
                        duration,
                        Some(labels),
                        Some(&err_msg),
                    );
                    anyhow!("PDF渲染失败: {}", e)
                })?;
                {
                    let mut labels = HashMap::new();
                    labels.insert("material".to_string(), material_label.clone());
                    labels.insert("batch".to_string(), batch_label.clone());
                    METRICS_COLLECTOR.record_pipeline_stage(
                        "pdf_render",
                        true,
                        render_start.elapsed(),
                        Some(labels),
                        None,
                    );
                }

                if image_paths.is_empty() {
                    tracing::warn!("页面 {}-{} 渲染为空，跳过", start, end);
                    start = end + 1;
                    continue;
                }

                let mut converted_keys: Vec<String> = Vec::new();
                for (offset, image) in image_paths.iter().enumerate() {
                    // 记录图片尺寸/大小，辅助DPI调参
                    if crate::CONFIG.ocr_tuning.logging_detail {
                        match image::image_dimensions(image) {
                            Ok((w, h)) => {
                                if let Ok(meta) = std::fs::metadata(image) {
                                    debug!(
                                        target: "attachment.pipeline",
                                        event = events::PIPELINE_STAGE,
                                        stage = "pdf_slice",
                                        page = start as usize + offset,
                                        width = w,
                                        height = h,
                                        bytes = meta.len(),
                                        dpi = crate::CONFIG.download_limits.pdf_render_dpi,
                                        quality = crate::CONFIG.download_limits.pdf_jpeg_quality
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!("无法读取图片尺寸: {} -> {}", image.display(), e)
                            }
                        }
                    }
                    // 注意：OCR许可已在上层(preview.rs)获取，这里不需要重复获取
                    // 避免双重信号量导致死锁
                    let mut engine = GLOBAL_POOL
                        .acquire()
                        .await
                        .map_err(|e| anyhow::anyhow!("获取OCR引擎失败: {}", e))?;
                    let ocr_started = Instant::now();
                    let ocr_result = engine.ocr_and_parse(std::path::PathBuf::from(&image).into());
                    let duration = ocr_started.elapsed();
                    METRICS_COLLECTOR.record_ocr_invocation(ocr_result.is_ok(), duration);

                    let abs_page = start as usize + offset;
                    let mut stage_labels = HashMap::new();
                    stage_labels.insert("material".to_string(), material_label.clone());
                    stage_labels.insert("page".to_string(), abs_page.to_string());

                    match ocr_result {
                        Ok(contents) => {
                            METRICS_COLLECTOR.record_pipeline_stage(
                                "ocr",
                                true,
                                duration,
                                Some(stage_labels.clone()),
                                None,
                            );
                            // 质量评估与日志
                            let mut total_score = 0.0f64;
                            let mut scored = 0usize;
                            let mut chars = 0usize;
                            for c in contents.iter() {
                                total_score += c.score;
                                scored += 1;
                                chars += c.text.len();
                            }
                            let avg_score = if scored > 0 {
                                total_score / scored as f64
                            } else {
                                0.0
                            };
                            if crate::CONFIG.ocr_tuning.logging_detail {
                                debug!(
                                    target: "attachment.pipeline",
                                    event = events::PIPELINE_STAGE,
                                    stage = "ocr_quality",
                                    page = start as usize + offset,
                                    avg_score,
                                    chars,
                                    score_threshold = crate::CONFIG.ocr_tuning.low_confidence_threshold,
                                    char_threshold = crate::CONFIG.ocr_tuning.min_char_threshold
                                );
                                if avg_score < crate::CONFIG.ocr_tuning.low_confidence_threshold
                                    || chars < crate::CONFIG.ocr_tuning.min_char_threshold
                                {
                                    tracing::warn!(
                                        " OCR质量偏低: 建议提高DPI到{} 或增加质量至{} (当前 dpi={} quality={})",
                                        (crate::CONFIG.download_limits.pdf_render_dpi + crate::CONFIG.ocr_tuning.retry_dpi_step)
                                            .min(crate::CONFIG.ocr_tuning.max_dpi),
                                        (crate::CONFIG.download_limits.pdf_jpeg_quality + 5).min(95),
                                        crate::CONFIG.download_limits.pdf_render_dpi,
                                        crate::CONFIG.download_limits.pdf_jpeg_quality
                                    );
                                }
                            }

                            let page_text = contents
                                .into_iter()
                                .map(|c| c.text)
                                .collect::<Vec<_>>()
                                .join("\n");
                            all_text.push(page_text);
                            // 可选：上传转换后的图片
                            if let Some(storage) = &self.storage {
                                if let Ok(bytes) = std::fs::read(&image) {
                                    let key = format!(
                                        "uploads/{}/{}/converted/page-{}.jpg",
                                        self.preview.request_id, material_code, abs_page
                                    );
                                    let upload_started = Instant::now();
                                    match storage.put(&key, &bytes).await {
                                        Ok(_) => {
                                            let mut upload_labels = HashMap::new();
                                            upload_labels.insert(
                                                "material".to_string(),
                                                material_label.clone(),
                                            );
                                            upload_labels
                                                .insert("page".to_string(), abs_page.to_string());
                                            METRICS_COLLECTOR.record_pipeline_stage(
                                                "upload",
                                                true,
                                                upload_started.elapsed(),
                                                Some(upload_labels),
                                                None,
                                            );
                                            converted_keys.push(key);
                                        }
                                        Err(e) => {
                                            let err_msg = e.to_string();
                                            let mut upload_labels = HashMap::new();
                                            upload_labels.insert(
                                                "material".to_string(),
                                                material_label.clone(),
                                            );
                                            upload_labels
                                                .insert("page".to_string(), abs_page.to_string());
                                            METRICS_COLLECTOR.record_pipeline_stage(
                                                "upload",
                                                false,
                                                upload_started.elapsed(),
                                                Some(upload_labels),
                                                Some(&err_msg),
                                            );
                                            warn!("上传转换后的图片失败: {:?} -> {}", image, e);
                                            METRICS_COLLECTOR.record_preview_persistence_failure(
                                                "storage_put_converted_image",
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let err_msg = e.to_string();
                            METRICS_COLLECTOR.record_pipeline_stage(
                                "ocr",
                                false,
                                duration,
                                Some(stage_labels.clone()),
                                Some(&err_msg),
                            );
                            if err_msg.contains("超时")
                                || err_msg.contains("无响应")
                                || err_msg.to_ascii_lowercase().contains("timeout")
                            {
                                METRICS_COLLECTOR.record_preview_ocr_timeout(material_code);
                            }
                            tracing::warn!("OCR失败 页{}: {}", start as usize + offset, err_msg);
                        }
                    }
                }

                // 记录转换后的keys
                if let (Some(db), Some(id)) = (&self.database, record_id) {
                    if !converted_keys.is_empty() {
                        let keys_json =
                            serde_json::to_string(&converted_keys).unwrap_or("[]".to_string());
                        if let Err(err) = db
                            .update_material_file_processing(id, Some(&keys_json), None, None)
                            .await
                        {
                            METRICS_COLLECTOR.record_preview_persistence_failure(
                                "db_update_material_processing",
                            );
                            warn!("更新材料处理信息失败: {}", err);
                        }
                        if let Err(err) =
                            db.update_material_file_status(id, "converted", None).await
                        {
                            METRICS_COLLECTOR
                                .record_preview_persistence_failure("db_update_material_status");
                            warn!("更新材料状态失败: {}", err);
                        }
                    }
                }

                let batch_elapsed = batch_start.elapsed();
                debug!(
                    target: "attachment.pipeline",
                    event = events::PIPELINE_STAGE,
                    stage = "pdf_batch_complete",
                    page_start = start,
                    page_end = end,
                    duration_ms = batch_elapsed.as_millis() as u64,
                    text_chars = all_text.iter().map(|s| s.len()).sum::<usize>()
                );

                start = end + 1;
            }
            let pdf_total_elapsed = pdf_processing_start.elapsed();
            debug!(
                target: "attachment.pipeline",
                event = events::PIPELINE_STAGE,
                stage = "pdf_processing_complete",
                allowed_pages,
                duration_ms = pdf_total_elapsed.as_millis() as u64,
                text_chars = all_text.iter().map(|s| s.len()).sum::<usize>()
            );
            all_text.join("\n\n")
        } else {
            // 非PDF：优先尝试 base64；失败则落盘为临时文件，改用 image_path 模式
            // 非PDF：一次性借用引擎
            // 注意：OCR许可已在上层(preview.rs)获取，这里不需要重复获取
            // 避免双重信号量导致死锁
            let mut engine = GLOBAL_POOL
                .acquire()
                .await
                .map_err(|e| anyhow::anyhow!("获取OCR引擎失败: {}", e))?;
            let ocr_started = Instant::now();
            let ocr_result = engine.ocr_and_parse(file_content.to_vec().into());
            let duration = ocr_started.elapsed();
            METRICS_COLLECTOR.record_ocr_invocation(ocr_result.is_ok(), duration);
            match ocr_result {
                Ok(contents) => contents
                    .into_iter()
                    .map(|content| content.text)
                    .collect::<Vec<_>>()
                    .join("\n"),
                Err(e) => {
                    let err_msg = e.to_string();
                    if err_msg.contains("超时")
                        || err_msg.contains("无响应")
                        || err_msg.to_ascii_lowercase().contains("timeout")
                    {
                        METRICS_COLLECTOR.record_preview_ocr_timeout(material_code);
                    }
                    warn!("Base64 OCR失败，切换为路径模式: {}", err_msg);
                    // 落盘：CURRENT_DIR/images/tmp-<material>-<uuid>.bin
                    let tmp_dir = ocr_conn::CURRENT_DIR.join("images");
                    let _ = std::fs::create_dir_all(&tmp_dir);
                    let tmp_path = tmp_dir.join(format!(
                        "tmp-{}-{}.img",
                        material_code,
                        uuid::Uuid::new_v4()
                    ));
                    if let Err(w) = std::fs::write(&tmp_path, file_content) {
                        METRICS_COLLECTOR.record_preview_persistence_failure("write_tmp_image");
                        return Err(anyhow!("写入临时文件失败: {}", w));
                    }
                    // 使用路径模式再次尝试
                    // 再借用一次引擎
                    let mut engine = GLOBAL_POOL
                        .acquire()
                        .await
                        .map_err(|e| anyhow::anyhow!("获取OCR引擎失败: {}", e))?;
                    let ocr_started = Instant::now();
                    let ocr_result = engine.ocr_and_parse(tmp_path.clone().into());
                    let duration = ocr_started.elapsed();
                    METRICS_COLLECTOR.record_ocr_invocation(ocr_result.is_ok(), duration);
                    match ocr_result {
                        Ok(contents) => {
                            // 清理临时文件（忽略错误）
                            let _ = std::fs::remove_file(&tmp_path);
                            contents
                                .into_iter()
                                .map(|content| content.text)
                                .collect::<Vec<_>>()
                                .join("\n")
                        }
                        Err(e2) => {
                            // 保留文件用于排查
                            let err_msg = e2.to_string();
                            if err_msg.contains("超时")
                                || err_msg.contains("无响应")
                                || err_msg.to_ascii_lowercase().contains("timeout")
                            {
                                METRICS_COLLECTOR.record_preview_ocr_timeout(material_code);
                            }
                            return Err(anyhow!(
                                "OCR处理失败（路径模式）: {}，临时文件: {}",
                                err_msg,
                                tmp_path.display()
                            ));
                        }
                    }
                }
            }
        };

        let ocr_duration = ocr_start.elapsed();
        debug!(
            target: "attachment.pipeline",
            event = events::ATTACHMENT_OCR_COMPLETE,
            material_code = %material_code,
            duration_ms = ocr_duration.as_millis() as u64,
            text_length = text_content.len()
        );

        // 将OCR文本持久化（可选）
        if let Some(storage) = &self.storage {
            let key = format!(
                "uploads/{}/{}/ocr/{}.txt",
                self.preview.request_id,
                material_code,
                uuid::Uuid::new_v4().to_string()
            );
            if let Err(e) = storage.put(&key, text_content.as_bytes()).await {
                warn!("保存OCR文本失败: {}", e);
                METRICS_COLLECTOR.record_preview_persistence_failure("storage_put_ocr_text");
            } else if let (Some(db), Some(id)) = (&self.database, record_id) {
                if let Err(err) = db
                    .update_material_file_processing(
                        id,
                        None,
                        Some(&key),
                        Some(text_content.len() as i64),
                    )
                    .await
                {
                    METRICS_COLLECTOR
                        .record_preview_persistence_failure("db_update_material_processing");
                    warn!("更新材料处理信息失败: {}", err);
                }
                if let Err(err) = db
                    .update_material_file_status(id, "ocr_completed", None)
                    .await
                {
                    METRICS_COLLECTOR
                        .record_preview_persistence_failure("db_update_material_status");
                    warn!("更新材料状态失败: {}", err);
                }
            }
        }

        Ok(text_content)
    }

    /// 处理评估结果
    async fn process_evaluation_result(
        &mut self,
        ocr_text: String,
        material: &MaterialValue,
        material_start: Instant,
    ) -> Result<MaterialEvaluationResult> {
        let process_start = Instant::now();

        // 创建材料评估结果
        let mut material_result = MaterialEvaluationResult::new(material.code.clone());

        // 结构化抽取 + 关键信息摘要
        let extracted_struct = extract::extract_all(&ocr_text);
        self.extracted_map
            .insert(material.code.clone(), extracted_struct.clone());

        let mut extracted_info = self.extract_key_information(&ocr_text);
        extracted_info.extend(self.describe_extracted_fields(&extracted_struct));
        material_result.set_ocr_content(ocr_text.clone());

        // 执行规则匹配评估
        let rule_start = Instant::now();
        let evaluation = self.evaluate_material_with_rules(&ocr_text, material).await;
        let rule_duration = rule_start.elapsed();
        METRICS_COLLECTOR.record_preview_rule_execution(rule_duration, evaluation.is_ok());

        let mut status_label: String;
        let mut evaluation_message: String;

        match evaluation {
            Ok(evaluation) => {
                status_label = if evaluation.code == 200 {
                    "success".to_string()
                } else if evaluation.code >= 500 {
                    "error".to_string()
                } else {
                    "warning".to_string()
                };
                evaluation_message = evaluation.message.clone();
                extracted_info.extend(evaluation.suggestions.clone());
                debug!(
                    target: "attachment.pipeline",
                    event = events::PIPELINE_STAGE,
                    stage = "rule_eval",
                    material_code = %material.code,
                    status_code = evaluation.code,
                    message = %evaluation.message
                );
            }
            Err(e) => {
                warn!("规则评估失败: {}", e);
                evaluation_message = "规则评估失败，请人工复核".to_string();
                status_label = "error".to_string();
                extracted_info.push("系统未能完成规则校验，请人工复核相关材料。".to_string());
            }
        }

        // 一致性校验（申请人/经办人/合同/证照）
        let (consistency_notes, consistency_tags, severe) =
            self.run_consistency_checks(&extracted_struct, material.code.as_str());
        if !consistency_notes.is_empty() {
            extracted_info.extend(consistency_notes);
        }
        if severe {
            let mut review_tags = consistency_tags;
            if review_tags.len() > 3 {
                review_tags.truncate(3);
            }
            let review_hint = if review_tags.is_empty() {
                "需要人工审核（关键信息不一致）".to_string()
            } else {
                format!(
                    "需要人工审核：{}{}",
                    review_tags.join("，"),
                    if review_tags.len() >= 3 { "…" } else { "" }
                )
            };

            match status_label.as_str() {
                "success" | "warning" => {
                    status_label = "warning".to_string();
                    evaluation_message = review_hint;
                }
                _ => {
                    evaluation_message = format!("{review_hint}；{evaluation_message}");
                }
            }
        }

        material_result.set_evaluation_result(evaluation_message, &status_label);
        material_result.set_extracted_info(extracted_info);

        let total_material_duration = material_start.elapsed();
        let process_duration = process_start.elapsed();

        debug!(
            target: "attachment.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "rule_eval_summary",
            material_code = %material.code,
            status = %material_result.evaluation_status,
            message = %material_result.evaluation_message,
            duration_ms = rule_duration.as_millis() as u64
        );

        debug!(
            target: "attachment.pipeline",
            event = events::PIPELINE_STAGE,
            stage = "material_post_process",
            material_code = %material.code,
            processing_duration_ms = process_duration.as_millis() as u64,
            total_material_duration_ms = total_material_duration.as_millis() as u64
        );

        Ok(material_result)
    }

    /// 从OCR内容中提取关键信息
    fn extract_key_information(&self, ocr_text: &str) -> Vec<String> {
        // 简化的信息提取逻辑
        let mut extracted = Vec::new();

        // 提取关键词
        if ocr_text.contains("身份证") {
            extracted.push("发现身份证信息".to_string());
        }
        if ocr_text.contains("营业执照") {
            extracted.push("发现营业执照信息".to_string());
        }

        extracted
    }

    fn describe_extracted_fields(&self, extracted: &ExtractedData) -> Vec<String> {
        let mut notes = Vec::new();
        let mut render_fields = |label: &str, fields: &[(&str, Option<&str>)]| {
            let parts: Vec<String> = fields
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .map(|v| v.trim())
                        .filter(|v| !v.is_empty())
                        .map(|v| format!("{name}={v}"))
                })
                .collect();
            if !parts.is_empty() {
                notes.push(format!("{label}: {}", parts.join("，")));
            }
        };

        if let Some(id) = &extracted.id_card {
            render_fields(
                "提取到身份证信息",
                &[
                    ("姓名", id.name.as_deref()),
                    ("证件号", id.id_number.as_deref()),
                    ("住址", id.address.as_deref()),
                ],
            );
        }
        if let Some(lic) = &extracted.biz_license {
            render_fields(
                "提取到营业执照",
                &[
                    ("名称", lic.company_name.as_deref()),
                    ("信用代码", lic.credit_code.as_deref()),
                    ("法人", lic.legal_person.as_deref()),
                ],
            );
        }
        if let Some(contract) = &extracted.contract {
            render_fields(
                "提取到合同关键信息",
                &[
                    ("甲方", contract.party_a.as_deref()),
                    ("乙方", contract.party_b.as_deref()),
                    ("地址", contract.address.as_deref()),
                ],
            );
        }
        notes
    }

    fn run_consistency_checks(
        &self,
        extracted: &ExtractedData,
        material_code: &str,
    ) -> (Vec<String>, Vec<String>, bool) {
        let matches_any = |value: &str, candidates: &[String]| -> bool {
            let val = value.trim();
            if val.is_empty() {
                return false;
            }
            candidates
                .iter()
                .any(|c| !c.trim().is_empty() && c.trim() == val)
        };

        let mut notes = Vec::new();
        let mut tags = Vec::new();
        let mut severe = false;

        let applicant_name = self
            .preview
            .subject_info
            .user_name
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let applicant_id = self
            .preview
            .subject_info
            .certificate_number
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let agent_name = self
            .preview
            .agent_info
            .user_name
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let agent_id = self
            .preview
            .agent_info
            .certificate_number
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let request_address = self
            .preview
            .subject_info
            .address
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_string();
        let name_candidates: Vec<String> = [applicant_name.clone(), agent_name.clone()]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
        let id_candidates: Vec<String> = [applicant_id.clone(), agent_id.clone()]
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();

        if let Some(id) = &extracted.id_card {
            if let Some(name) = &id.name {
                if !name_candidates.is_empty() && !matches_any(name, &name_candidates) {
                    notes.push(format!(
                        "身份证姓名与申请人/经办人不一致: {} (材料 {})",
                        name, material_code
                    ));
                    tags.push("身份证姓名不一致".to_string());
                    severe = true;
                }
            }
            if let Some(id_no) = &id.id_number {
                if !id_candidates.is_empty() && !matches_any(id_no, &id_candidates) {
                    notes.push(format!(
                        "身份证号码与申请人/经办人不一致: {} (材料 {})",
                        id_no, material_code
                    ));
                    tags.push("身份证号码不一致".to_string());
                    severe = true;
                }
            }
        }

        if let Some(lic) = &extracted.biz_license {
            if let Some(code) = &lic.credit_code {
                if let Some(req_code) = self.preview.subject_info.organization_code.as_deref() {
                    if !req_code.trim().is_empty() && code != req_code {
                        notes.push(format!(
                            "营业执照信用代码与申请信息不一致: {} vs {}",
                            code, req_code
                        ));
                        tags.push("营业执照信用代码不一致".to_string());
                        severe = true;
                    }
                }
            }
        }

        if let Some(contract) = &extracted.contract {
            // 甲方/出租人 ↔ 申请人 或 法人；乙方/承租人 ↔ 经办人/申请人
            if let Some(party_a) = &contract.party_a {
                if !name_candidates.is_empty() && !matches_any(party_a, &name_candidates) {
                    notes.push(format!(
                        "合同甲方/出租人姓名与申请人/经办人不一致: {}",
                        party_a
                    ));
                    tags.push("合同甲方姓名不一致".to_string());
                    severe = true;
                }
            }
            if let Some(party_b) = &contract.party_b {
                if !name_candidates.is_empty() && !matches_any(party_b, &name_candidates) {
                    notes.push(format!(
                        "合同乙方/承租人姓名与申请人/经办人不一致: {}",
                        party_b
                    ));
                    tags.push("合同乙方姓名不一致".to_string());
                    severe = true;
                }
            }
            if let Some(addr) = &contract.address {
                if !request_address.is_empty()
                    && !addr.contains(&request_address)
                    && !request_address.contains(addr)
                {
                    notes.push(format!(
                        "合同地址与申请地址存在差异: 合同地址='{}' 申请地址='{}'",
                        addr, request_address
                    ));
                    tags.push("合同地址不一致".to_string());
                    severe = true;
                }
            }
            if let (Some(start), Some(end)) = (&contract.start_date, &contract.end_date) {
                notes.push(format!("合同有效期: {} 至 {}", start, end));
            }
            if let Some(rent) = &contract.rent {
                notes.push(format!("合同租金/金额: {}", rent));
            }
        }

        (notes, tags, severe)
    }

    /// 使用规则引擎评估材料
    async fn evaluate_material_with_rules(
        &self,
        _ocr_text: &str,
        material: &MaterialValue,
    ) -> Result<RuleEvaluationResult> {
        if let Some(rule) = self.lookup_material_rule(&material.code) {
            let mut errors = Vec::new();
            let mut warnings = Vec::new();
            let mut suggestions = Vec::new();

            let attachment_count = material.attachment_list.len();

            if attachment_count == 0 {
                if rule.required {
                    errors.push("未上传任何附件".to_string());
                } else {
                    warnings.push("未检测到附件，建议人工确认".to_string());
                }
            }

            if let Some(min_files) = rule.min_files {
                if attachment_count < min_files as usize {
                    errors.push(format!(
                        "附件数量不足：至少需要 {} 份，当前 {} 份",
                        min_files, attachment_count
                    ));
                }
            }

            if let Some(max_files) = rule.max_files {
                if attachment_count > max_files as usize {
                    warnings.push(format!(
                        "附件数量超过上限 {} 份，当前 {} 份",
                        max_files, attachment_count
                    ));
                }
            }

            if !rule.allowed_types.is_empty() && attachment_count > 0 {
                let allowed: HashSet<String> = rule
                    .allowed_types
                    .iter()
                    .map(|t| t.to_ascii_lowercase())
                    .collect();
                for attachment in &material.attachment_list {
                    match self.infer_file_extension(attachment) {
                        Some(ext) => {
                            if !allowed.contains(&ext) {
                                errors.push(format!(
                                    "附件 {} 类型 {} 不在允许范围 ({})",
                                    attachment.attach_name,
                                    ext,
                                    rule.allowed_types.join(", ")
                                ));
                            }
                        }
                        None => warnings
                            .push(format!("附件 {} 无法识别文件类型", attachment.attach_name)),
                    }
                }
            }

            if let Some(validity) = &rule.validity {
                match validity {
                    MaterialValidity::None => {}
                    MaterialValidity::ExpiryField { field } => {
                        warnings.push(format!("需人工确认有效期字段 {} 的准确性", field))
                    }
                    MaterialValidity::IssuePlusDays { days } => {
                        warnings.push(format!("需核实签发日期与有效期（+{} 天）", days))
                    }
                }
            }

            if let Some(checks) = &rule.checks {
                if checks.must_have_seal {
                    warnings.push("需人工确认是否加盖公章".to_string());
                }
                if checks.must_have_signature {
                    warnings.push("需人工确认是否具备签字".to_string());
                }
                if !checks.matches.is_empty() {
                    warnings.push("存在字段比对规则，需人工核对 OCR 结果".to_string());
                }
            }

            if let Some(pairing) = &rule.pairing {
                if !pairing.required_angles.is_empty() {
                    warnings.push(format!(
                        "需确认附件角度齐全：{}",
                        pairing.required_angles.join(", ")
                    ));
                } else {
                    warnings.push("需确认附件配对规则".to_string());
                }
            }

            if rule.scope != MaterialScope::Global {
                warnings.push(format!("材料作用域为 {:?}，暂不自动校验", rule.scope));
            }

            if let Some(repeat) = &rule.repeat {
                warnings.push(format!(
                    "材料与案例列表 {} 存在重复校验要求，需人工确认 (caseKeyField={}, ocrKeyField={})",
                    repeat.case_list, repeat.case_key_field, repeat.ocr_key_field
                ));
            }

            if let Some(notes) = &rule.notes {
                suggestions.push(notes.clone());
            }

            let code;
            let message;

            if !errors.is_empty() {
                code = 500;
                message = errors.join("；");
            } else if !warnings.is_empty() {
                code = 206;
                message = warnings.join("；");
            } else {
                code = 200;
                message = "材料符合配置要求".to_string();
            }

            let mut combined_suggestions = warnings.clone();
            combined_suggestions.extend(suggestions);
            if combined_suggestions.is_empty() && code == 200 && !rule.allowed_types.is_empty() {
                combined_suggestions
                    .push(format!("允许的文件类型：{}", rule.allowed_types.join(", ")));
            }

            self.log_rule_audit(
                material,
                Some(rule),
                code,
                &message,
                &errors,
                &warnings,
                &combined_suggestions,
            );

            Ok(RuleEvaluationResult {
                code,
                message,
                suggestions: combined_suggestions,
            })
        } else {
            self.log_rule_audit(
                material,
                None,
                200,
                "未配置专项规则，默认通过",
                &[],
                &[],
                &[],
            );
            Ok(RuleEvaluationResult {
                code: 200,
                message: "未配置专项规则，默认通过".to_string(),
                suggestions: Vec::new(),
            })
        }
    }
}

#[derive(Clone, Copy)]
struct AttachmentLogSettings {
    enabled: bool,
    sampling_rate: u32,
    slow_threshold_ms: u64,
}

impl AttachmentLogSettings {
    fn should_sample(&self, index: usize) -> bool {
        if !self.enabled {
            return false;
        }
        let rate = self.sampling_rate.max(1);
        rate == 1 || index % rate as usize == 0
    }
}

fn attachment_log_settings() -> AttachmentLogSettings {
    let snapshot = ATTACHMENT_LOGGING_RUNTIME.snapshot();
    AttachmentLogSettings {
        enabled: snapshot.enabled,
        sampling_rate: snapshot.sampling_rate,
        slow_threshold_ms: snapshot.slow_threshold_ms,
    }
}
/// 简单文件名清洗，保留字母数字、下划线、连字符和点，其他替换为下划线
fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' {
            out.push(c);
        } else if c.is_whitespace() {
            out.push('_');
        }
    }
    out.trim_matches('.').to_string()
}

fn resolve_temp_pdf_dir() -> PathBuf {
    let configured = PathBuf::from(&CONFIG.master.temp_pdf_dir);
    if configured.is_absolute() {
        configured
    } else {
        CURRENT_DIR.join(configured)
    }
}

async fn cleanup_expired_temp_pdfs(root: PathBuf, ttl_hours: u64) {
    if ttl_hours == 0 {
        return;
    }

    let ttl_secs = ttl_hours.saturating_mul(3600);
    let cutoff_time = match SystemTime::now().checked_sub(Duration::from_secs(ttl_secs)) {
        Some(t) => t,
        None => return,
    };

    let mut entries = match fs::read_dir(&root).await {
        Ok(it) => it,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        let Ok(meta) = entry.metadata().await else {
            continue;
        };

        if !meta.is_file() {
            continue;
        }

        let Ok(modified) = meta.modified() else {
            continue;
        };

        if modified < cutoff_time {
            let _ = fs::remove_file(path).await;
        }
    }
}

fn classify_download_source(url: &str) -> &str {
    if url.starts_with("https://") {
        "https"
    } else if url.starts_with("http://") {
        "http"
    } else if url.starts_with("oss://") {
        "oss"
    } else if url.starts_with("file://") {
        "file"
    } else if url.starts_with("ftp://") {
        "ftp"
    } else {
        "other"
    }
}

/// 材料评估结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct MaterialEvaluationResult {
    pub material_code: String,
    pub ocr_content: Option<String>,
    pub extracted_info: Vec<String>,
    pub evaluation_message: String,
    pub evaluation_status: String,
    pub is_success: bool,
}

/// 估算PDF页数（简单扫描）
fn estimate_pdf_pages(data: &[u8]) -> Option<usize> {
    if data.len() < 8 {
        return None;
    }
    let s = if data.len() > 4 * 1024 * 1024 {
        // 最多扫描前4MB
        &data[..4 * 1024 * 1024]
    } else {
        data
    };
    let hay = std::str::from_utf8(s).ok()?;
    // 粗略统计 '/Type /Page' 出现次数
    Some(hay.matches("/Type /Page").count())
}

impl MaterialEvaluationResult {
    pub fn new(material_code: String) -> Self {
        Self {
            material_code,
            ocr_content: None,
            extracted_info: Vec::new(),
            evaluation_message: "未处理".to_string(),
            evaluation_status: "pending".to_string(),
            is_success: false,
        }
    }

    pub fn new_with_error(material_code: String, error_msg: String) -> Self {
        Self {
            material_code,
            ocr_content: None,
            extracted_info: Vec::new(),
            evaluation_message: error_msg,
            evaluation_status: "error".to_string(),
            is_success: false,
        }
    }

    pub fn set_ocr_content(&mut self, content: String) {
        self.ocr_content = Some(content);
    }

    pub fn set_extracted_info(&mut self, info: Vec<String>) {
        self.extracted_info = info;
    }

    pub fn set_evaluation_result(&mut self, message: String, status: &str) {
        self.evaluation_message = message;
        self.evaluation_status = status.to_string();
        self.is_success = status == "success";
    }

    pub fn is_success(&self) -> bool {
        self.is_success
    }

    pub fn is_passed(&self) -> bool {
        self.is_success
    }
}

/// 规则评估结果
#[derive(Debug)]
struct RuleEvaluationResult {
    pub code: u16,
    pub message: String,
    pub suggestions: Vec<String>,
}
