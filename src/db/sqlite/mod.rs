//! SQLite模块
//! 重构后的模块化SQLite数据库实现

pub mod connection;
pub mod monitor_queries;
pub mod queries;
pub mod schemas;

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use super::traits::*;
use crate::db::models::{MonitorSession, MonitorUser};
use connection::ConnectionManager;
use monitor_queries::MonitorQueries;
use queries::{
    ApiStatsQueries, CachedMaterialQueries, HealthQueries, MaterialFileQueries,
    MaterialResultQueries, MatterRuleConfigQueries, OutboxQueries, PreviewQueries,
    PreviewRequestQueries, RuleResultQueries, TaskPayloadQueries,
};
use schemas::SchemaManager;

/// SQLite数据库实现
pub struct SqliteDatabase {
    pool: SqlitePool,
}

impl SqliteDatabase {
    /// 创建新的SQLite数据库实例
    pub async fn new(db_path: &str) -> Result<Self> {
        let pool = ConnectionManager::create_pool(db_path).await?;
        Ok(Self { pool })
    }

    /// 获取连接池引用
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// 获取连接池信息
    pub fn pool_info(&self) -> connection::PoolInfo {
        ConnectionManager::get_pool_info(&self.pool)
    }
}

#[async_trait]
impl Database for SqliteDatabase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    async fn save_preview_request(&self, request: &PreviewRequestRecord) -> Result<()> {
        PreviewRequestQueries::upsert(&self.pool, request).await
    }

    async fn get_preview_request(&self, id: &str) -> Result<Option<PreviewRequestRecord>> {
        PreviewRequestQueries::get_by_id(&self.pool, id).await
    }

    async fn find_preview_request_by_third_party(
        &self,
        third_party_request_id: &str,
    ) -> Result<Option<PreviewRequestRecord>> {
        PreviewRequestQueries::get_by_third_party(&self.pool, third_party_request_id).await
    }

    async fn update_preview_request_latest(
        &self,
        request_id: &str,
        latest_preview_id: Option<&str>,
        latest_status: Option<PreviewStatus>,
    ) -> Result<()> {
        PreviewRequestQueries::update_latest_state(
            &self.pool,
            request_id,
            latest_preview_id,
            latest_status,
        )
        .await
    }

    async fn list_preview_requests(
        &self,
        filter: &PreviewRequestFilter,
    ) -> Result<Vec<PreviewRequestRecord>> {
        PreviewRequestQueries::list(&self.pool, filter).await
    }

    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()> {
        PreviewQueries::save_record(&self.pool, record).await
    }

    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>> {
        PreviewQueries::get_by_id(&self.pool, id).await
    }

    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()> {
        PreviewQueries::update_status(&self.pool, id, status).await
    }

    async fn update_preview_evaluation_result(
        &self,
        id: &str,
        evaluation_result: &str,
    ) -> Result<()> {
        PreviewQueries::update_evaluation_result(&self.pool, id, evaluation_result).await
    }

    async fn mark_preview_processing(
        &self,
        id: &str,
        worker_id: &str,
        attempt_id: &str,
    ) -> Result<()> {
        PreviewQueries::mark_processing(&self.pool, id, worker_id, attempt_id).await
    }

    async fn update_preview_artifacts(
        &self,
        id: &str,
        file_name: &str,
        preview_url: &str,
        preview_view_url: Option<&str>,
        preview_download_url: Option<&str>,
    ) -> Result<()> {
        PreviewQueries::update_artifacts(
            &self.pool,
            id,
            file_name,
            preview_url,
            preview_view_url,
            preview_download_url,
        )
        .await
    }

    async fn replace_preview_material_results(
        &self,
        preview_id: &str,
        records: &[PreviewMaterialResultRecord],
    ) -> Result<()> {
        MaterialResultQueries::replace(&self.pool, preview_id, records).await
    }

    async fn replace_preview_rule_results(
        &self,
        preview_id: &str,
        records: &[PreviewRuleResultRecord],
    ) -> Result<()> {
        RuleResultQueries::replace(&self.pool, preview_id, records).await
    }

    async fn update_preview_failure_context(&self, update: &PreviewFailureUpdate) -> Result<()> {
        PreviewQueries::update_failure_context(&self.pool, update).await
    }

    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>> {
        PreviewQueries::list_with_filter(&self.pool, filter).await
    }

    async fn check_and_update_preview_dedup(
        &self,
        _fingerprint: &str,
        _preview_id: &str,
        _meta: &PreviewDedupMeta,
        _limit: i32,
    ) -> Result<PreviewDedupDecision> {
        // SQLite 路径不做持久化去重，默认允许
        Ok(PreviewDedupDecision::Allowed { repeat_count: 1 })
    }

    async fn get_preview_status_counts(&self) -> Result<PreviewStatusCounts> {
        PreviewQueries::count_by_status(&self.pool).await
    }

    async fn find_preview_by_third_party_id(
        &self,
        third_party_id: &str,
        user_id: &str,
    ) -> Result<Option<PreviewRecord>> {
        PreviewQueries::find_by_third_party_id(&self.pool, third_party_id, user_id).await
    }

    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()> {
        ApiStatsQueries::save_stats(&self.pool, stats).await
    }

    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>> {
        ApiStatsQueries::get_stats_with_filter(&self.pool, filter).await
    }

    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary> {
        ApiStatsQueries::get_summary(&self.pool, filter).await
    }

    async fn health_check(&self) -> Result<bool> {
        HealthQueries::check_health(&self.pool).await
    }

    async fn initialize(&self) -> Result<()> {
        SchemaManager::create_all_tables(&self.pool).await?;
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
        let sql = r#"
            INSERT INTO user_login_records (
                user_id, user_name, certificate_type, certificate_number,
                phone_number, email, organization_name, organization_code,
                login_type, login_time, client_ip, user_agent, referer,
                cookie_info, raw_data, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#;

        let now = chrono::Utc::now().to_rfc3339();

        sqlx::query(sql)
            .bind(user_id)
            .bind(user_name.unwrap_or(""))
            .bind(certificate_type)
            .bind(certificate_number.unwrap_or(""))
            .bind(phone_number.unwrap_or(""))
            .bind(email.unwrap_or(""))
            .bind(organization_name.unwrap_or(""))
            .bind(organization_code.unwrap_or(""))
            .bind(login_type)
            .bind(login_time)
            .bind(client_ip)
            .bind(user_agent)
            .bind(referer)
            .bind(cookie_info)
            .bind(raw_data)
            .bind(&now)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    async fn upsert_cached_material_record(&self, record: &CachedMaterialRecord) -> Result<()> {
        CachedMaterialQueries::upsert(&self.pool, record).await
    }

    async fn update_cached_material_status(
        &self,
        id: &str,
        status: CachedMaterialStatus,
        oss_key: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<()> {
        CachedMaterialQueries::update_status(&self.pool, id, status, oss_key, last_error).await
    }

    async fn list_cached_material_records(
        &self,
        filter: &CachedMaterialFilter,
    ) -> Result<Vec<CachedMaterialRecord>> {
        CachedMaterialQueries::list(&self.pool, filter).await
    }

    async fn delete_cached_material_record(&self, id: &str) -> Result<()> {
        CachedMaterialQueries::delete_by_id(&self.pool, id).await
    }

    async fn delete_cached_materials_by_preview(&self, preview_id: &str) -> Result<()> {
        CachedMaterialQueries::delete_by_preview(&self.pool, preview_id).await
    }

    async fn save_material_file_record(&self, record: &MaterialFileRecord) -> Result<()> {
        MaterialFileQueries::insert(&self.pool, record).await
    }

    async fn update_material_file_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        MaterialFileQueries::update_status(&self.pool, id, status, error).await
    }

    async fn update_material_file_processing(
        &self,
        id: &str,
        processed_keys_json: Option<&str>,
        ocr_text_key: Option<&str>,
        ocr_text_length: Option<i64>,
    ) -> Result<()> {
        MaterialFileQueries::update_processing(
            &self.pool,
            id,
            processed_keys_json,
            ocr_text_key,
            ocr_text_length,
        )
        .await
    }

    async fn list_material_files(
        &self,
        filter: &MaterialFileFilter,
    ) -> Result<Vec<MaterialFileRecord>> {
        MaterialFileQueries::list(&self.pool, filter).await
    }

    async fn save_task_payload(&self, preview_id: &str, payload: &str) -> Result<()> {
        TaskPayloadQueries::upsert(&self.pool, preview_id, payload).await
    }

    async fn load_task_payload(&self, preview_id: &str) -> Result<Option<String>> {
        TaskPayloadQueries::load(&self.pool, preview_id).await
    }

    async fn delete_task_payload(&self, preview_id: &str) -> Result<()> {
        TaskPayloadQueries::delete(&self.pool, preview_id).await
    }

    async fn update_preview_callback_state(&self, update: &PreviewCallbackUpdate) -> Result<()> {
        PreviewQueries::update_callback_state(&self.pool, update).await
    }

    async fn list_due_callbacks(&self, limit: u32) -> Result<Vec<PreviewRecord>> {
        PreviewQueries::list_due_callbacks(&self.pool, limit).await
    }

    async fn enqueue_outbox_event(&self, event: &NewOutboxEvent) -> Result<()> {
        OutboxQueries::insert(&self.pool, event).await
    }

    async fn fetch_pending_outbox_events(&self, limit: u32) -> Result<Vec<OutboxEvent>> {
        OutboxQueries::fetch_pending(&self.pool, limit).await
    }

    async fn mark_outbox_event_applied(&self, event_id: &str) -> Result<()> {
        OutboxQueries::mark_applied(&self.pool, event_id).await
    }

    async fn mark_outbox_event_failed(&self, event_id: &str, error: &str) -> Result<()> {
        OutboxQueries::mark_failed(&self.pool, event_id, error).await
    }

    async fn get_matter_rule_config(
        &self,
        matter_id: &str,
    ) -> Result<Option<MatterRuleConfigRecord>> {
        MatterRuleConfigQueries::get_by_matter_id(&self.pool, matter_id).await
    }

    async fn upsert_matter_rule_config(&self, config: &MatterRuleConfigRecord) -> Result<()> {
        MatterRuleConfigQueries::upsert(&self.pool, config).await
    }

    async fn list_matter_rule_configs(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<MatterRuleConfigRecord>> {
        MatterRuleConfigQueries::list(&self.pool, status).await
    }

    // 监控系统数据库方法
    /// 根据用户名查找监控用户
    async fn find_monitor_user_by_username(&self, username: &str) -> Result<Option<MonitorUser>> {
        MonitorQueries::find_user_by_username(&self.pool, username).await
    }

    /// 获取监控用户密码哈希
    async fn get_monitor_user_password_hash(&self, user_id: &str) -> Result<String> {
        MonitorQueries::get_user_password_hash(&self.pool, user_id).await
    }

    /// 创建监控会话
    async fn create_monitor_session(
        &self,
        session_id: &str,
        user_id: &str,
        ip: &str,
        user_agent: &str,
        created_at: &str,
        expires_at: &str,
    ) -> Result<()> {
        MonitorQueries::create_session(
            &self.pool, session_id, user_id, ip, user_agent, created_at, expires_at,
        )
        .await
    }

    /// 根据会话ID查找监控会话
    async fn find_monitor_session_by_id(&self, session_id: &str) -> Result<Option<MonitorSession>> {
        MonitorQueries::find_session_by_id(&self.pool, session_id).await
    }

    /// 更新监控用户登录信息
    async fn update_monitor_login_info(&self, user_id: &str, now: &str) -> Result<()> {
        MonitorQueries::update_login_info(&self.pool, user_id, now).await
    }

    /// 更新监控会话活动时间
    async fn update_monitor_session_activity(&self, session_id: &str, now: &str) -> Result<()> {
        MonitorQueries::update_session_activity(&self.pool, session_id, now).await
    }

    /// 删除监控会话
    async fn delete_monitor_session(&self, session_id: &str) -> Result<()> {
        MonitorQueries::delete_session(&self.pool, session_id).await
    }

    /// 清理过期监控会话
    async fn cleanup_expired_monitor_sessions(&self, now: &str) -> Result<u64> {
        MonitorQueries::cleanup_expired_sessions(&self.pool, now).await
    }

    /// 获取活跃监控会话数量
    async fn get_active_monitor_sessions_count(&self, now: &str) -> Result<i64> {
        MonitorQueries::get_active_sessions_count(&self.pool, now).await
    }

    /// 列出监控用户
    async fn list_monitor_users(&self) -> Result<Vec<MonitorUser>> {
        MonitorQueries::list_users(&self.pool).await
    }

    /// 创建监控用户
    async fn create_monitor_user(
        &self,
        id: &str,
        username: &str,
        password_hash: &str,
        role: &str,
        now: &str,
    ) -> Result<()> {
        MonitorQueries::create_user(&self.pool, id, username, password_hash, role, now).await
    }

    /// 更新监控用户角色
    async fn update_monitor_user_role(&self, user_id: &str, role: &str, now: &str) -> Result<()> {
        MonitorQueries::update_user_role(&self.pool, user_id, role, now).await
    }

    /// 更新监控用户密码
    async fn update_monitor_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
        now: &str,
    ) -> Result<()> {
        MonitorQueries::update_user_password(&self.pool, user_id, password_hash, now).await
    }

    /// 设置监控用户是否启用
    async fn set_monitor_user_active(
        &self,
        user_id: &str,
        is_active: bool,
        now: &str,
    ) -> Result<()> {
        MonitorQueries::set_user_active(&self.pool, user_id, is_active, now).await
    }

    /// 统计活跃管理员数量
    async fn count_active_monitor_admins(&self) -> Result<i64> {
        MonitorQueries::count_active_admins(&self.pool).await
    }

    /// 根据ID查找监控用户
    async fn find_monitor_user_by_id(&self, user_id: &str) -> Result<Option<MonitorUser>> {
        MonitorQueries::find_user_by_id(&self.pool, user_id).await
    }

    // Worker结果异步处理队列相关方法
    async fn enqueue_worker_result(&self, preview_id: &str, payload: &str) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO worker_results_queue (id, preview_id, payload, status, attempts, created_at, updated_at)
            VALUES (?, ?, ?, 'pending', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            ON CONFLICT(preview_id) DO UPDATE SET
                payload=excluded.payload,
                status='pending',
                attempts=0,
                last_error=NULL,
                updated_at=CURRENT_TIMESTAMP
            "#,
        )
        .bind(Uuid::new_v4().to_string())
        .bind(preview_id)
        .bind(payload)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn fetch_pending_worker_results(
        &self,
        limit: u32,
    ) -> Result<Vec<WorkerResultQueueRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, preview_id, payload, status, attempts, last_error, created_at, updated_at
            FROM worker_results_queue
            WHERE status = 'pending'
            ORDER BY created_at ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await?;

        let mut results = Vec::with_capacity(rows.len());
        for row in rows {
            let parse_time = |s: String| -> DateTime<Utc> {
                NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                    .or_else(|_| NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S%.f"))
                    .map(|dt| DateTime::<Utc>::from_utc(dt, Utc))
                    .unwrap_or_else(|_| Utc::now())
            };

            let created_at: String = row.get("created_at");
            let updated_at: String = row.get("updated_at");

            results.push(WorkerResultQueueRecord {
                id: row.get("id"),
                preview_id: row.get("preview_id"),
                payload: row.get("payload"),
                status: row.get("status"),
                attempts: row.get::<i64, _>("attempts") as i32,
                last_error: row.get::<Option<String>, _>("last_error"),
                created_at: parse_time(created_at),
                updated_at: parse_time(updated_at),
            });
        }

        Ok(results)
    }

    async fn update_worker_result_status(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<()> {
        if let Some(err) = last_error {
            sqlx::query(
                r#"
                UPDATE worker_results_queue
                SET status = ?, last_error = ?, attempts = attempts + 1, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                "#,
            )
            .bind(status)
            .bind(err)
            .bind(id)
            .execute(&self.pool)
            .await?;
        } else {
            sqlx::query(
                r#"
                UPDATE worker_results_queue
                SET status = ?, updated_at = CURRENT_TIMESTAMP
                WHERE id = ?
                "#,
            )
            .bind(status)
            .bind(id)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    // Material Download Queue (SQLite not supported)
    async fn enqueue_material_download(&self, _preview_id: &str, _payload: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "SQLite backend does not support material download queue"
        ))
    }

    async fn fetch_pending_material_downloads(
        &self,
        _limit: u32,
    ) -> Result<Vec<MaterialDownloadQueueRecord>> {
        Err(anyhow::anyhow!(
            "SQLite backend does not support material download queue"
        ))
    }

    async fn update_material_download_status(
        &self,
        _id: &str,
        _status: &str,
        _last_error: Option<&str>,
    ) -> Result<()> {
        Err(anyhow::anyhow!(
            "SQLite backend does not support material download queue"
        ))
    }

    async fn update_material_download_payload(&self, _id: &str, _payload: &str) -> Result<()> {
        Err(anyhow::anyhow!(
            "SQLite backend does not support material download queue"
        ))
    }

    async fn get_download_cache_token(
        &self,
        _url: &str,
    ) -> Result<Option<crate::db::traits::MaterialDownloadCacheEntry>> {
        Ok(None)
    }

    async fn upsert_download_cache_token(
        &self,
        _url: &str,
        _token: &str,
        _ttl_secs: i64,
    ) -> Result<()> {
        Ok(())
    }
}
