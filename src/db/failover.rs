use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info, warn};

use super::factory;
use super::traits::*;
use crate::util::config::DatabaseFailoverConfig;

/// 数据库故障转移状态
#[derive(Debug, Clone, PartialEq)]
enum FailoverState {
    /// 使用主数据库
    Primary,
    /// 使用本地降级数据库
    Fallback,
    /// 正在尝试恢复到主数据库
    Recovering,
}

/// 带故障转移功能的数据库包装器
pub struct FailoverDatabase {
    /// 主数据库
    primary: Arc<dyn Database>,
    /// 降级数据库（本地SQLite）
    fallback: Arc<dyn Database>,
    /// 当前状态
    state: Arc<RwLock<FailoverState>>,
    /// 配置
    config: DatabaseFailoverConfig,
    /// 最后一次健康检查时间
    last_health_check: Arc<RwLock<DateTime<Utc>>>,
    /// 健康检查互斥锁（防止并发健康检查）
    health_check_lock: Arc<Mutex<()>>,
    /// 状态切换计数器（用于检测异常循环）
    state_transition_counter: Arc<AtomicU32>,
}

impl FailoverDatabase {
    pub async fn new(primary: Arc<dyn Database>, config: DatabaseFailoverConfig) -> Result<Self> {
        // 创建本地降级数据库
        let fallback_config = factory::DatabaseConfig {
            db_type: factory::DatabaseType::Sqlite,
            sqlite: Some(factory::SqliteConfig {
                path: format!("{}/fallback.db", config.local_data_dir),
            }),
            dm: None,
        };

        // 确保降级目录存在
        std::fs::create_dir_all(&config.local_data_dir)
            .context("Failed to create fallback database directory")?;

        let fallback = factory::create_database(&fallback_config).await?;

        // 初始化降级数据库
        fallback.initialize().await?;

        Ok(Self {
            primary,
            fallback: fallback.into(),
            state: Arc::new(RwLock::new(FailoverState::Primary)),
            config,
            last_health_check: Arc::new(RwLock::new(Utc::now())),
            health_check_lock: Arc::new(Mutex::new(())),
            state_transition_counter: Arc::new(AtomicU32::new(0)),
        })
    }

    /// 原子状态切换 - 使用CAS操作避免竞态
    /// 返回是否成功切换状态
    async fn try_transition_state(&self, from: FailoverState, to: FailoverState) -> bool {
        let mut state = self.state.write().await;
        if *state == from {
            *state = to.clone();

            // 增加状态切换计数器
            let counter = self
                .state_transition_counter
                .fetch_add(1, Ordering::Relaxed);

            // 检测异常循环（超过max_retries * 4的状态切换视为异常）
            let threshold = (self.config.max_retries * 4) as u32;
            if counter > threshold {
                error!(
                    "[warn] 检测到异常状态切换循环: {} 次切换超过阈值 {}",
                    counter, threshold
                );
                // 强制进入fallback状态
                *state = FailoverState::Fallback;
            }

            info!("[ok] 状态切换成功: {:?} → {:?}", from, state);
            true
        } else {
            warn!("[warn] 状态切换失败: 期望 {:?} 但当前为 {:?}", from, *state);
            false
        }
    }

    /// 获取当前活动的数据库
    async fn get_active_db(&self) -> Arc<dyn Database> {
        let state = self.state.read().await;
        match *state {
            FailoverState::Primary => self.primary.clone(),
            FailoverState::Fallback | FailoverState::Recovering => self.fallback.clone(),
        }
    }

    /// 执行带重试的数据库操作
    async fn execute_with_failover<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn(
            Arc<dyn Database>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
    {
        // 检查是否需要健康检查
        self.check_health_if_needed().await;

        let mut retries = 0;
        let max_retries = self.config.max_retries;
        // 硬限制：防止死循环，最多重试 max_retries * 2
        let hard_limit = max_retries * 2;

        loop {
            // 硬限制检查
            if retries >= hard_limit {
                error!(
                    "[fail] 达到硬限制 ({} 次重试)，终止操作以防止死循环",
                    hard_limit
                );
                return Err(anyhow::anyhow!(
                    "Database operation exceeded hard retry limit ({})",
                    hard_limit
                ));
            }

            let db = self.get_active_db().await;
            let state = self.state.read().await.clone();

            match operation(db.clone()).await {
                Ok(result) => {
                    // 如果当前在恢复状态且操作成功，尝试切换回主数据库
                    if state == FailoverState::Recovering {
                        self.try_transition_state(
                            FailoverState::Recovering,
                            FailoverState::Primary,
                        )
                        .await;
                    }
                    // 重置切换计数器（操作成功）
                    self.state_transition_counter.store(0, Ordering::Relaxed);
                    return Ok(result);
                }
                Err(e) => {
                    retries += 1;
                    let msg = e.to_string();
                    if msg.to_lowercase().contains("not implemented") {
                        warn!(
                            "Primary op not implemented; falling back for this call: {}",
                            msg
                        );
                        return operation(self.fallback.clone()).await;
                    }

                    // 如果是主数据库失败，尝试切换到降级数据库
                    if state == FailoverState::Primary && self.config.fallback_to_local {
                        warn!(
                            "Primary database failed (retry {}/{}): {}, switching to fallback",
                            retries, max_retries, e
                        );

                        // 使用原子切换，避免竞态
                        if self
                            .try_transition_state(FailoverState::Primary, FailoverState::Fallback)
                            .await
                        {
                            // 切换成功，重试使用降级数据库
                            if retries <= max_retries {
                                tokio::time::sleep(Duration::from_millis(self.config.retry_delay))
                                    .await;
                                continue;
                            }
                        } else {
                            // 状态已被其他线程切换，继续重试
                            if retries <= max_retries {
                                tokio::time::sleep(Duration::from_millis(self.config.retry_delay))
                                    .await;
                                continue;
                            }
                        }
                    }

                    // 如果已经在使用降级数据库或重试次数超限，返回错误
                    if retries > max_retries {
                        error!(
                            "Database operation failed after {} retries (max: {}): {}",
                            retries, max_retries, e
                        );
                        return Err(e);
                    }

                    // 继续重试
                    warn!("Retrying database operation ({}/{})", retries, max_retries);
                    tokio::time::sleep(Duration::from_millis(self.config.retry_delay)).await;
                }
            }
        }
    }

    /// 检查是否需要进行健康检查
    async fn check_health_if_needed(&self) {
        if !self.config.enabled {
            return;
        }

        let now = Utc::now();
        let last_check = *self.last_health_check.read().await;
        let interval = Duration::from_secs(self.config.health_check_interval);

        if now
            .signed_duration_since(last_check)
            .to_std()
            .unwrap_or(Duration::ZERO)
            < interval
        {
            return;
        }

        // [locked] 使用互斥锁防止并发健康检查
        // try_lock: 如果已有线程在执行健康检查，直接返回
        let lock_result = self.health_check_lock.try_lock();
        if lock_result.is_err() {
            // 已有其他线程在执行健康检查
            return;
        }
        let _guard = lock_result.unwrap();

        // 再次检查时间（double-check locking pattern）
        let last_check = *self.last_health_check.read().await;
        if now
            .signed_duration_since(last_check)
            .to_std()
            .unwrap_or(Duration::ZERO)
            < interval
        {
            return;
        }

        // 更新最后检查时间
        *self.last_health_check.write().await = now;

        // 如果当前在降级状态，尝试恢复主数据库
        let state = self.state.read().await.clone();
        if state == FailoverState::Fallback {
            info!("[loop] 尝试恢复主数据库连接");

            // 使用原子切换进入Recovering状态
            if self
                .try_transition_state(FailoverState::Fallback, FailoverState::Recovering)
                .await
            {
                // 在后台尝试健康检查
                let primary = self.primary.clone();
                let state_clone = self.state.clone();
                let state_transition_counter = self.state_transition_counter.clone();

                tokio::spawn(async move {
                    match primary.health_check().await {
                        Ok(true) => {
                            info!("[ok] 主数据库健康检查通过");
                            // 健康检查通过，但不立即切换，等下次操作成功后再切换
                            // 状态会保持在Recovering，直到execute_with_failover中操作成功
                        }
                        Ok(false) | Err(_) => {
                            warn!("[warn] 主数据库仍不健康，保持fallback模式");
                            // 切换回Fallback状态
                            let mut state = state_clone.write().await;
                            *state = FailoverState::Fallback;
                            // 重置状态切换计数器
                            state_transition_counter.store(0, Ordering::Relaxed);
                        }
                    }
                });
            } else {
                // 状态切换失败（可能已被其他线程切换），不执行健康检查
                warn!("[warn] 状态切换失败，跳过健康检查");
            }
        }
    }
}

#[async_trait]
impl Database for FailoverDatabase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn save_preview_request(&self, request: &PreviewRequestRecord) -> Result<()> {
        self.execute_with_failover(|db| {
            let request = request.clone();
            Box::pin(async move { db.save_preview_request(&request).await })
        })
        .await
    }

    async fn get_preview_request(&self, id: &str) -> Result<Option<PreviewRequestRecord>> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            Box::pin(async move { db.get_preview_request(&id).await })
        })
        .await
    }

    async fn find_preview_request_by_third_party(
        &self,
        third_party_request_id: &str,
    ) -> Result<Option<PreviewRequestRecord>> {
        self.execute_with_failover(|db| {
            let third_party_request_id = third_party_request_id.to_string();
            Box::pin(async move {
                db.find_preview_request_by_third_party(&third_party_request_id)
                    .await
            })
        })
        .await
    }

    async fn update_preview_request_latest(
        &self,
        request_id: &str,
        latest_preview_id: Option<&str>,
        latest_status: Option<PreviewStatus>,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let request_id = request_id.to_string();
            let latest_preview_id = latest_preview_id.map(|s| s.to_string());
            let latest_status = latest_status.clone();
            Box::pin(async move {
                db.update_preview_request_latest(
                    &request_id,
                    latest_preview_id.as_deref(),
                    latest_status,
                )
                .await
            })
        })
        .await
    }

    async fn list_preview_requests(
        &self,
        filter: &PreviewRequestFilter,
    ) -> Result<Vec<PreviewRequestRecord>> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.list_preview_requests(&filter).await })
        })
        .await
    }

    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()> {
        self.execute_with_failover(|db| {
            let record = record.clone();
            Box::pin(async move { db.save_preview_record(&record).await })
        })
        .await
    }

    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            Box::pin(async move { db.get_preview_record(&id).await })
        })
        .await
    }

    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let status = status.clone();
            Box::pin(async move { db.update_preview_status(&id, status).await })
        })
        .await
    }

    async fn update_preview_evaluation_result(
        &self,
        id: &str,
        evaluation_result: &str,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let evaluation_result = evaluation_result.to_string();
            Box::pin(async move {
                db.update_preview_evaluation_result(&id, &evaluation_result)
                    .await
            })
        })
        .await
    }

    async fn mark_preview_processing(
        &self,
        id: &str,
        worker_id: &str,
        attempt_id: &str,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let worker_id = worker_id.to_string();
            let attempt_id = attempt_id.to_string();
            Box::pin(async move {
                db.mark_preview_processing(&id, &worker_id, &attempt_id)
                    .await
            })
        })
        .await
    }

    async fn update_preview_artifacts(
        &self,
        id: &str,
        file_name: &str,
        preview_url: &str,
        preview_view_url: Option<&str>,
        preview_download_url: Option<&str>,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let file_name = file_name.to_string();
            let preview_url = preview_url.to_string();
            let preview_view_url = preview_view_url.map(|s| s.to_string());
            let preview_download_url = preview_download_url.map(|s| s.to_string());
            Box::pin(async move {
                db.update_preview_artifacts(
                    &id,
                    &file_name,
                    &preview_url,
                    preview_view_url.as_deref(),
                    preview_download_url.as_deref(),
                )
                .await
            })
        })
        .await
    }

    async fn replace_preview_material_results(
        &self,
        preview_id: &str,
        records: &[PreviewMaterialResultRecord],
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let preview_id = preview_id.to_string();
            let records = records.to_vec();
            Box::pin(async move {
                db.replace_preview_material_results(&preview_id, &records)
                    .await
            })
        })
        .await
    }

    async fn replace_preview_rule_results(
        &self,
        preview_id: &str,
        records: &[PreviewRuleResultRecord],
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let preview_id = preview_id.to_string();
            let records = records.to_vec();
            Box::pin(async move { db.replace_preview_rule_results(&preview_id, &records).await })
        })
        .await
    }

    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.list_preview_records(&filter).await })
        })
        .await
    }

    async fn check_and_update_preview_dedup(
        &self,
        fingerprint: &str,
        preview_id: &str,
        meta: &PreviewDedupMeta,
        limit: i32,
    ) -> Result<PreviewDedupDecision> {
        self.execute_with_failover(|db| {
            let fingerprint = fingerprint.to_string();
            let preview_id = preview_id.to_string();
            let meta = meta.clone();
            Box::pin(async move {
                db.check_and_update_preview_dedup(&fingerprint, &preview_id, &meta, limit)
                    .await
            })
        })
        .await
    }

    async fn get_preview_status_counts(&self) -> Result<PreviewStatusCounts> {
        self.execute_with_failover(|db| {
            Box::pin(async move { db.get_preview_status_counts().await })
        })
        .await
    }

    async fn find_preview_by_third_party_id(
        &self,
        third_party_id: &str,
        user_id: &str,
    ) -> Result<Option<PreviewRecord>> {
        self.execute_with_failover(|db| {
            let third_party_id = third_party_id.to_string();
            let user_id = user_id.to_string();
            Box::pin(async move {
                db.find_preview_by_third_party_id(&third_party_id, &user_id)
                    .await
            })
        })
        .await
    }

    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()> {
        self.execute_with_failover(|db| {
            let stats = stats.clone();
            Box::pin(async move { db.save_api_stats(&stats).await })
        })
        .await
    }

    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.get_api_stats(&filter).await })
        })
        .await
    }

    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.get_api_summary(&filter).await })
        })
        .await
    }

    async fn health_check(&self) -> Result<bool> {
        // 总是返回true，因为我们有降级机制
        Ok(true)
    }

    async fn initialize(&self) -> Result<()> {
        // 初始化两个数据库
        self.primary.initialize().await.ok(); // 忽略主数据库初始化失败
        self.fallback.initialize().await?;
        Ok(())
    }

    async fn save_user_login_record(
        &self,
        user_id: &str,
        user_name: Option<&str>,
        certificate_type: &str,
        certificate_number: Option<&str>,
        phone_number: Option<&str>,
        email: Option<&str>,
        organization_name: Option<&str>,
        organization_code: Option<&str>,
        login_type: &str,
        login_time: &str,
        client_ip: &str,
        user_agent: &str,
        referer: &str,
        cookie_info: &str,
        raw_data: &str,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let user_id = user_id.to_string();
            let user_name = user_name.map(|s| s.to_string());
            let certificate_type = certificate_type.to_string();
            let certificate_number = certificate_number.map(|s| s.to_string());
            let phone_number = phone_number.map(|s| s.to_string());
            let email = email.map(|s| s.to_string());
            let organization_name = organization_name.map(|s| s.to_string());
            let organization_code = organization_code.map(|s| s.to_string());
            let login_type = login_type.to_string();
            let login_time = login_time.to_string();
            let client_ip = client_ip.to_string();
            let user_agent = user_agent.to_string();
            let referer = referer.to_string();
            let cookie_info = cookie_info.to_string();
            let raw_data = raw_data.to_string();

            Box::pin(async move {
                db.save_user_login_record(
                    &user_id,
                    user_name.as_deref(),
                    &certificate_type,
                    certificate_number.as_deref(),
                    phone_number.as_deref(),
                    email.as_deref(),
                    organization_name.as_deref(),
                    organization_code.as_deref(),
                    &login_type,
                    &login_time,
                    &client_ip,
                    &user_agent,
                    &referer,
                    &cookie_info,
                    &raw_data,
                )
                .await
            })
        })
        .await
    }

    async fn save_material_file_record(
        &self,
        record: &crate::db::traits::MaterialFileRecord,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let rec = record.clone();
            Box::pin(async move { db.save_material_file_record(&rec).await })
        })
        .await
    }

    async fn update_material_file_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let status = status.to_string();
            let err = error.map(|s| s.to_string());
            Box::pin(async move {
                db.update_material_file_status(&id, &status, err.as_deref())
                    .await
            })
        })
        .await
    }

    async fn update_material_file_processing(
        &self,
        id: &str,
        processed_keys_json: Option<&str>,
        ocr_text_key: Option<&str>,
        ocr_text_length: Option<i64>,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let keys = processed_keys_json.map(|s| s.to_string());
            let ocr_key = ocr_text_key.map(|s| s.to_string());
            let len = ocr_text_length;
            Box::pin(async move {
                db.update_material_file_processing(&id, keys.as_deref(), ocr_key.as_deref(), len)
                    .await
            })
        })
        .await
    }

    async fn list_material_files(
        &self,
        filter: &crate::db::traits::MaterialFileFilter,
    ) -> Result<Vec<crate::db::traits::MaterialFileRecord>> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.list_material_files(&filter).await })
        })
        .await
    }

    async fn save_task_payload(&self, preview_id: &str, payload: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = preview_id.to_string();
            let payload = payload.to_string();
            Box::pin(async move { db.save_task_payload(&id, &payload).await })
        })
        .await
    }

    async fn load_task_payload(&self, preview_id: &str) -> Result<Option<String>> {
        self.execute_with_failover(|db| {
            let id = preview_id.to_string();
            Box::pin(async move { db.load_task_payload(&id).await })
        })
        .await
    }

    async fn delete_task_payload(&self, preview_id: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = preview_id.to_string();
            Box::pin(async move { db.delete_task_payload(&id).await })
        })
        .await
    }

    async fn update_preview_callback_state(&self, update: &PreviewCallbackUpdate) -> Result<()> {
        self.execute_with_failover(|db| {
            let update = update.clone();
            Box::pin(async move { db.update_preview_callback_state(&update).await })
        })
        .await
    }

    async fn list_due_callbacks(&self, limit: u32) -> Result<Vec<PreviewRecord>> {
        self.execute_with_failover(|db| Box::pin(async move { db.list_due_callbacks(limit).await }))
            .await
    }

    async fn update_preview_failure_context(&self, update: &PreviewFailureUpdate) -> Result<()> {
        self.execute_with_failover(|db| {
            let update = update.clone();
            Box::pin(async move { db.update_preview_failure_context(&update).await })
        })
        .await
    }

    async fn enqueue_outbox_event(&self, event: &NewOutboxEvent) -> Result<()> {
        self.execute_with_failover(|db| {
            let event = event.clone();
            Box::pin(async move { db.enqueue_outbox_event(&event).await })
        })
        .await
    }

    async fn fetch_pending_outbox_events(&self, limit: u32) -> Result<Vec<OutboxEvent>> {
        self.execute_with_failover(|db| {
            Box::pin(async move { db.fetch_pending_outbox_events(limit).await })
        })
        .await
    }

    async fn mark_outbox_event_applied(&self, event_id: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let event_id = event_id.to_string();
            Box::pin(async move { db.mark_outbox_event_applied(&event_id).await })
        })
        .await
    }

    async fn mark_outbox_event_failed(&self, event_id: &str, error: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let event_id = event_id.to_string();
            let error = error.to_string();
            Box::pin(async move { db.mark_outbox_event_failed(&event_id, &error).await })
        })
        .await
    }

    async fn get_matter_rule_config(
        &self,
        matter_id: &str,
    ) -> Result<Option<MatterRuleConfigRecord>> {
        self.execute_with_failover(|db| {
            let matter_id = matter_id.to_string();
            Box::pin(async move { db.get_matter_rule_config(&matter_id).await })
        })
        .await
    }

    async fn upsert_matter_rule_config(&self, config: &MatterRuleConfigRecord) -> Result<()> {
        self.execute_with_failover(|db| {
            let config = config.clone();
            Box::pin(async move { db.upsert_matter_rule_config(&config).await })
        })
        .await
    }

    async fn list_matter_rule_configs(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<MatterRuleConfigRecord>> {
        self.execute_with_failover(|db| {
            let status = status.map(|s| s.to_string());
            Box::pin(async move { db.list_matter_rule_configs(status.as_deref()).await })
        })
        .await
    }

    // 监控系统相关方法

    async fn find_monitor_user_by_username(&self, username: &str) -> Result<Option<MonitorUser>> {
        self.execute_with_failover(|db| {
            let username = username.to_string();
            Box::pin(async move { db.find_monitor_user_by_username(&username).await })
        })
        .await
    }

    async fn get_monitor_user_password_hash(&self, user_id: &str) -> Result<String> {
        self.execute_with_failover(|db| {
            let user_id = user_id.to_string();
            Box::pin(async move { db.get_monitor_user_password_hash(&user_id).await })
        })
        .await
    }

    async fn create_monitor_session(
        &self,
        session_id: &str,
        user_id: &str,
        ip: &str,
        user_agent: &str,
        created_at: &str,
        expires_at: &str,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let session_id = session_id.to_string();
            let user_id = user_id.to_string();
            let ip = ip.to_string();
            let user_agent = user_agent.to_string();
            let created_at = created_at.to_string();
            let expires_at = expires_at.to_string();
            Box::pin(async move {
                db.create_monitor_session(
                    &session_id,
                    &user_id,
                    &ip,
                    &user_agent,
                    &created_at,
                    &expires_at,
                )
                .await
            })
        })
        .await
    }

    async fn find_monitor_session_by_id(&self, session_id: &str) -> Result<Option<MonitorSession>> {
        self.execute_with_failover(|db| {
            let session_id = session_id.to_string();
            Box::pin(async move { db.find_monitor_session_by_id(&session_id).await })
        })
        .await
    }

    async fn update_monitor_login_info(&self, user_id: &str, now: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let user_id = user_id.to_string();
            let now = now.to_string();
            Box::pin(async move { db.update_monitor_login_info(&user_id, &now).await })
        })
        .await
    }

    async fn update_monitor_session_activity(&self, session_id: &str, now: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let session_id = session_id.to_string();
            let now = now.to_string();
            Box::pin(async move { db.update_monitor_session_activity(&session_id, &now).await })
        })
        .await
    }

    async fn delete_monitor_session(&self, session_id: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let session_id = session_id.to_string();
            Box::pin(async move { db.delete_monitor_session(&session_id).await })
        })
        .await
    }

    async fn cleanup_expired_monitor_sessions(&self, now: &str) -> Result<u64> {
        self.execute_with_failover(|db| {
            let now = now.to_string();
            Box::pin(async move { db.cleanup_expired_monitor_sessions(&now).await })
        })
        .await
    }

    async fn get_active_monitor_sessions_count(&self, now: &str) -> Result<i64> {
        self.execute_with_failover(|db| {
            let now = now.to_string();
            Box::pin(async move { db.get_active_monitor_sessions_count(&now).await })
        })
        .await
    }

    async fn list_monitor_users(&self) -> Result<Vec<MonitorUser>> {
        self.execute_with_failover(|db| Box::pin(async move { db.list_monitor_users().await }))
            .await
    }

    async fn create_monitor_user(
        &self,
        id: &str,
        username: &str,
        password_hash: &str,
        role: &str,
        now: &str,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let username = username.to_string();
            let password_hash = password_hash.to_string();
            let role = role.to_string();
            let now = now.to_string();
            Box::pin(async move {
                db.create_monitor_user(&id, &username, &password_hash, &role, &now)
                    .await
            })
        })
        .await
    }

    async fn update_monitor_user_role(&self, user_id: &str, role: &str, now: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let user_id = user_id.to_string();
            let role = role.to_string();
            let now = now.to_string();
            Box::pin(async move { db.update_monitor_user_role(&user_id, &role, &now).await })
        })
        .await
    }

    async fn update_monitor_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
        now: &str,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let user_id = user_id.to_string();
            let password_hash = password_hash.to_string();
            let now = now.to_string();
            Box::pin(async move {
                db.update_monitor_user_password(&user_id, &password_hash, &now)
                    .await
            })
        })
        .await
    }

    async fn set_monitor_user_active(
        &self,
        user_id: &str,
        is_active: bool,
        now: &str,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let user_id = user_id.to_string();
            let now = now.to_string();
            Box::pin(async move { db.set_monitor_user_active(&user_id, is_active, &now).await })
        })
        .await
    }

    async fn count_active_monitor_admins(&self) -> Result<i64> {
        self.execute_with_failover(|db| {
            Box::pin(async move { db.count_active_monitor_admins().await })
        })
        .await
    }

    async fn find_monitor_user_by_id(&self, user_id: &str) -> Result<Option<MonitorUser>> {
        self.execute_with_failover(|db| {
            let user_id = user_id.to_string();
            Box::pin(async move { db.find_monitor_user_by_id(&user_id).await })
        })
        .await
    }

    // Worker Result Queue methods
    async fn enqueue_worker_result(&self, preview_id: &str, payload: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let preview_id = preview_id.to_string();
            let payload = payload.to_string();
            Box::pin(async move { db.enqueue_worker_result(&preview_id, &payload).await })
        })
        .await
    }

    async fn fetch_pending_worker_results(
        &self,
        limit: u32,
    ) -> Result<Vec<WorkerResultQueueRecord>> {
        self.execute_with_failover(|db| {
            Box::pin(async move { db.fetch_pending_worker_results(limit).await })
        })
        .await
    }

    async fn update_worker_result_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let status = status.to_string();
            let error = error.map(|s| s.to_string());
            Box::pin(async move {
                db.update_worker_result_status(&id, &status, error.as_deref())
                    .await
            })
        })
        .await
    }

    async fn get_worker_result_by_preview_id(
        &self,
        preview_id: &str,
    ) -> Result<Option<WorkerResultQueueRecord>> {
        self.execute_with_failover(|db| {
            let preview_id = preview_id.to_string();
            Box::pin(async move { db.get_worker_result_by_preview_id(&preview_id).await })
        })
        .await
    }

    // Material Download Queue methods
    async fn enqueue_material_download(&self, preview_id: &str, payload: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let preview_id = preview_id.to_string();
            let payload = payload.to_string();
            Box::pin(async move { db.enqueue_material_download(&preview_id, &payload).await })
        })
        .await
    }

    async fn fetch_pending_material_downloads(
        &self,
        limit: u32,
    ) -> Result<Vec<crate::db::traits::MaterialDownloadQueueRecord>> {
        self.execute_with_failover(|db| {
            Box::pin(async move { db.fetch_pending_material_downloads(limit).await })
        })
        .await
    }

    async fn update_material_download_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let status = status.to_string();
            let error = error.map(|s| s.to_string());
            Box::pin(async move {
                db.update_material_download_status(&id, &status, error.as_deref())
                    .await
            })
        })
        .await
    }

    async fn update_material_download_payload(&self, id: &str, payload: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let payload = payload.to_string();
            Box::pin(async move { db.update_material_download_payload(&id, &payload).await })
        })
        .await
    }

    async fn get_download_cache_token(
        &self,
        url: &str,
    ) -> Result<Option<crate::db::traits::MaterialDownloadCacheEntry>> {
        self.execute_with_failover(|db| {
            let url = url.to_string();
            Box::pin(async move { db.get_download_cache_token(&url).await })
        })
        .await
    }

    async fn upsert_download_cache_token(
        &self,
        url: &str,
        token: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let url = url.to_string();
            let token = token.to_string();
            Box::pin(async move { db.upsert_download_cache_token(&url, &token, ttl_secs).await })
        })
        .await
    }

    async fn create_preview_share_token(
        &self,
        preview_id: &str,
        token: &str,
        format: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        self.execute_with_failover(|db| {
            let preview_id = preview_id.to_string();
            let token = token.to_string();
            let format = format.to_string();
            Box::pin(async move {
                db.create_preview_share_token(&preview_id, &token, &format, ttl_secs)
                    .await
            })
        })
        .await
    }

    async fn consume_preview_share_token(
        &self,
        token: &str,
    ) -> Result<Option<crate::db::traits::PreviewShareTokenRecord>> {
        self.execute_with_failover(|db| {
            let token = token.to_string();
            Box::pin(async move { db.consume_preview_share_token(&token).await })
        })
        .await
    }
}
