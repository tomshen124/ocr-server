//! SQLite数据库表结构定义
//! 包含所有表的CREATE语句和索引定义

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid;

use crate::api::monitor_auth::DEFAULT_MONITOR_ADMIN_PASSWORD;

/// 数据库表结构管理器
pub struct SchemaManager;

impl SchemaManager {
    fn default_monitor_admin_password_hash() -> String {
        #[cfg(feature = "monitoring")]
        {
            use bcrypt::{hash, DEFAULT_COST};
            hash(DEFAULT_MONITOR_ADMIN_PASSWORD, DEFAULT_COST)
                .unwrap_or_else(|_| DEFAULT_MONITOR_ADMIN_PASSWORD.to_string())
        }
        #[cfg(not(feature = "monitoring"))]
        {
            DEFAULT_MONITOR_ADMIN_PASSWORD.to_string()
        }
    }

    /// 创建所有表结构
    pub async fn create_all_tables(pool: &SqlitePool) -> Result<()> {
        Self::create_preview_requests_table(pool).await?;
        Self::create_preview_records_table(pool).await?;
        Self::create_preview_material_results_table(pool).await?;
        Self::create_preview_rule_results_table(pool).await?;
        Self::create_preview_task_payloads_table(pool).await?;
        Self::create_api_stats_table(pool).await?;
        Self::create_preview_material_files_table(pool).await?;
        Self::create_cached_materials_table(pool).await?;
        Self::create_matter_rule_configs_table(pool).await?;
        // 新增监控系统表
        Self::create_monitor_tables(pool).await?;
        // 新增用户登录记录表
        Self::create_user_login_records_table(pool).await?;
        // 新增：回灌Outbox事件表（用于DM恢复后的回灌）
        Self::create_db_outbox_table(pool).await?;
        // 新增：Worker结果异步处理队列
        Self::create_worker_results_queue_table(pool).await?;
        Ok(())
    }

    async fn create_worker_results_queue_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS worker_results_queue (
                id TEXT PRIMARY KEY,
                preview_id TEXT NOT NULL,
                payload TEXT NOT NULL,
                status TEXT DEFAULT 'pending',
                attempts INTEGER DEFAULT 0,
                last_error TEXT,
                created_at TEXT DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_wrq_preview
            ON worker_results_queue(preview_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_wrq_status
            ON worker_results_queue(status, created_at)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 创建材料缓存状态表
    async fn create_cached_materials_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS cached_materials (
                id TEXT PRIMARY KEY,
                preview_id TEXT NOT NULL,
                material_code TEXT NOT NULL,
                attachment_index INTEGER NOT NULL,
                token TEXT NOT NULL,
                local_path TEXT NOT NULL,
                upload_status TEXT NOT NULL,
                oss_key TEXT,
                last_error TEXT,
                file_size INTEGER,
                checksum_sha256 TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_cached_materials_preview
            ON cached_materials(preview_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_cached_materials_status
            ON cached_materials(upload_status, preview_id)
            "#,
        )
        .execute(pool)
        .await?;

        let alter_statements = [
            "ALTER TABLE cached_materials ADD COLUMN attachment_index INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE cached_materials ADD COLUMN file_size INTEGER",
            "ALTER TABLE cached_materials ADD COLUMN checksum_sha256 TEXT",
        ];

        for statement in alter_statements {
            let _ = sqlx::query(statement).execute(pool).await;
        }

        Ok(())
    }

    /// 创建预审材料评估结果表
    async fn create_preview_material_results_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS preview_material_results (
                id TEXT PRIMARY KEY,
                preview_id TEXT NOT NULL,
                material_code TEXT NOT NULL,
                material_name TEXT,
                status TEXT NOT NULL,
                status_code INTEGER NOT NULL,
                processing_status TEXT,
                issues_count INTEGER NOT NULL DEFAULT 0,
                warnings_count INTEGER NOT NULL DEFAULT 0,
                attachments_json TEXT,
                summary_json TEXT,
                schema_version INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_material_results_preview
            ON preview_material_results(preview_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_material_results_material
            ON preview_material_results(preview_id, material_code)
            "#,
        )
        .execute(pool)
        .await?;

        let alter_statements = [
            "ALTER TABLE preview_material_results ADD COLUMN material_name TEXT",
            "ALTER TABLE preview_material_results ADD COLUMN processing_status TEXT",
            "ALTER TABLE preview_material_results ADD COLUMN issues_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE preview_material_results ADD COLUMN warnings_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE preview_material_results ADD COLUMN attachments_json TEXT",
            "ALTER TABLE preview_material_results ADD COLUMN summary_json TEXT",
            "ALTER TABLE preview_material_results ADD COLUMN schema_version INTEGER NOT NULL DEFAULT 1",
            "ALTER TABLE preview_material_results ADD COLUMN created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
            "ALTER TABLE preview_material_results ADD COLUMN updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
        ];

        for statement in alter_statements {
            let _ = sqlx::query(statement).execute(pool).await;
        }

        Ok(())
    }

    /// 创建预审规则评估结果表
    async fn create_preview_rule_results_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS preview_rule_results (
                id TEXT PRIMARY KEY,
                preview_id TEXT NOT NULL,
                material_result_id TEXT,
                material_code TEXT,
                rule_id TEXT,
                rule_code TEXT,
                rule_name TEXT,
                engine TEXT,
                severity TEXT,
                status TEXT,
                message TEXT,
                suggestions_json TEXT,
                evidence_json TEXT,
                extra_json TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_rule_results_preview
            ON preview_rule_results(preview_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_rule_results_material
            ON preview_rule_results(material_result_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_rule_results_code
            ON preview_rule_results(rule_code)
            "#,
        )
        .execute(pool)
        .await?;

        let alter_statements = [
            "ALTER TABLE preview_rule_results ADD COLUMN material_code TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN rule_id TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN rule_code TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN rule_name TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN engine TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN severity TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN status TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN message TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN suggestions_json TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN evidence_json TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN extra_json TEXT",
            "ALTER TABLE preview_rule_results ADD COLUMN updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP",
        ];

        for statement in alter_statements {
            let _ = sqlx::query(statement).execute(pool).await;
        }

        Ok(())
    }

    /// 创建预审请求表
    async fn create_preview_requests_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS preview_requests (
                id TEXT PRIMARY KEY,
                third_party_request_id TEXT UNIQUE,
                user_id TEXT NOT NULL,
                user_info_json TEXT,
                matter_id TEXT NOT NULL,
                matter_type TEXT NOT NULL,
                matter_name TEXT NOT NULL,
                channel TEXT NOT NULL,
                sequence_no TEXT NOT NULL,
                agent_info_json TEXT,
                subject_info_json TEXT,
                form_data_json TEXT,
                scene_data_json TEXT,
                material_data_json TEXT,
                latest_preview_id TEXT,
                latest_status TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_requests_user_id
            ON preview_requests(user_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_requests_matter
            ON preview_requests(matter_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_requests_created_at
            ON preview_requests(created_at)
            "#,
        )
        .execute(pool)
        .await?;

        let _ = sqlx::query(r#"ALTER TABLE preview_requests ADD COLUMN user_info_json TEXT"#)
            .execute(pool)
            .await;

        Ok(())
    }

    /// 创建预审材料文件记录表
    async fn create_preview_material_files_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS preview_material_files (
                id TEXT PRIMARY KEY,
                preview_id TEXT NOT NULL,
                material_code TEXT NOT NULL,
                attachment_name TEXT,
                source_url TEXT,
                stored_original_key TEXT NOT NULL,
                stored_processed_keys TEXT,
                mime_type TEXT,
                size_bytes INTEGER,
                checksum_sha256 TEXT,
                ocr_text_key TEXT,
                ocr_text_length INTEGER,
                status TEXT NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        // 索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_material_files_preview 
            ON preview_material_files(preview_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_material_files_material_code 
            ON preview_material_files(material_code)
            "#,
        )
        .execute(pool)
        .await?;
        Ok(())
    }

    /// 创建事项规则配置表
    async fn create_matter_rule_configs_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS matter_rule_configs (
                id TEXT PRIMARY KEY,
                matter_id TEXT NOT NULL,
                matter_name TEXT,
                spec_version TEXT NOT NULL,
                mode TEXT NOT NULL DEFAULT 'presentOnly',
                rule_payload TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'active',
                description TEXT,
                checksum TEXT,
                updated_by TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_matter_rule_configs_matter
            ON matter_rule_configs(matter_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_matter_rule_configs_status
            ON matter_rule_configs(status)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 创建预审记录表
    async fn create_preview_records_table(pool: &SqlitePool) -> Result<()> {
        // 创建预审记录表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS preview_records (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                user_info_json TEXT,
                file_name TEXT NOT NULL,
                ocr_text TEXT NOT NULL,
                theme_id TEXT,
                evaluation_result TEXT,
                preview_url TEXT NOT NULL,
                preview_view_url TEXT,
                preview_download_url TEXT,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                third_party_request_id TEXT,
                queued_at TEXT,
                processing_started_at TEXT,
                retry_count INTEGER NOT NULL DEFAULT 0,
                last_worker_id TEXT,
                last_attempt_id TEXT,
                failure_reason TEXT,
                ocr_stderr_summary TEXT,
                failure_context TEXT,
                last_error_code TEXT,
                slow_attachment_info_json TEXT,
                callback_url TEXT,
                callback_status TEXT,
                callback_attempts INTEGER NOT NULL DEFAULT 0,
                callback_successes INTEGER NOT NULL DEFAULT 0,
                callback_failures INTEGER NOT NULL DEFAULT 0,
                last_callback_at TEXT,
                last_callback_status_code INTEGER,
                last_callback_response TEXT,
                last_callback_error TEXT,
                callback_payload TEXT,
                next_callback_after TEXT
            )
            "#,
        )
        .execute(pool)
        .await?;

        // 尝试为旧表补充新增列
        let alter_statements = [
            "ALTER TABLE preview_records ADD COLUMN user_info_json TEXT",
            "ALTER TABLE preview_records ADD COLUMN queued_at TEXT",
            "ALTER TABLE preview_records ADD COLUMN processing_started_at TEXT",
            "ALTER TABLE preview_records ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE preview_records ADD COLUMN last_worker_id TEXT",
            "ALTER TABLE preview_records ADD COLUMN last_attempt_id TEXT",
            "ALTER TABLE preview_records ADD COLUMN preview_view_url TEXT",
            "ALTER TABLE preview_records ADD COLUMN preview_download_url TEXT",
            "ALTER TABLE preview_records ADD COLUMN failure_reason TEXT",
            "ALTER TABLE preview_records ADD COLUMN ocr_stderr_summary TEXT",
            "ALTER TABLE preview_records ADD COLUMN failure_context TEXT",
            "ALTER TABLE preview_records ADD COLUMN last_error_code TEXT",
            "ALTER TABLE preview_records ADD COLUMN slow_attachment_info_json TEXT",
            "ALTER TABLE preview_records ADD COLUMN callback_url TEXT",
            "ALTER TABLE preview_records ADD COLUMN callback_status TEXT",
            "ALTER TABLE preview_records ADD COLUMN callback_attempts INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE preview_records ADD COLUMN callback_successes INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE preview_records ADD COLUMN callback_failures INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE preview_records ADD COLUMN last_callback_at TEXT",
            "ALTER TABLE preview_records ADD COLUMN last_callback_status_code INTEGER",
            "ALTER TABLE preview_records ADD COLUMN last_callback_response TEXT",
            "ALTER TABLE preview_records ADD COLUMN last_callback_error TEXT",
            "ALTER TABLE preview_records ADD COLUMN callback_payload TEXT",
            "ALTER TABLE preview_records ADD COLUMN next_callback_after TEXT",
        ];
        for stmt in alter_statements {
            let _ = sqlx::query(stmt).execute(pool).await;
        }

        Self::create_preview_records_indexes(pool).await?;
        Ok(())
    }

    /// 创建预审记录表索引
    async fn create_preview_records_indexes(pool: &SqlitePool) -> Result<()> {
        // 创建用户ID索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_user_id 
            ON preview_records(user_id)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建第三方请求ID索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_third_party_id 
            ON preview_records(third_party_request_id)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建处理时间索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_processing_started 
            ON preview_records(processing_started_at)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_status_retry
            ON preview_records(status, retry_count)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建状态索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_status 
            ON preview_records(status)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建创建时间索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_created_at 
            ON preview_records(created_at)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_callback_status
            ON preview_records(callback_status, next_callback_after)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_next_callback
            ON preview_records(next_callback_after)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    async fn create_preview_task_payloads_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS preview_task_payloads (
                preview_id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_task_payloads_updated_at 
            ON preview_task_payloads(updated_at)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 创建API统计表
    async fn create_api_stats_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS api_stats (
                id TEXT PRIMARY KEY,
                endpoint TEXT NOT NULL,
                method TEXT NOT NULL,
                client_id TEXT,
                user_id TEXT,
                status_code INTEGER NOT NULL,
                response_time_ms INTEGER NOT NULL,
                request_size INTEGER NOT NULL,
                response_size INTEGER NOT NULL,
                error_message TEXT,
                created_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        Self::create_api_stats_indexes(pool).await?;
        Ok(())
    }

    /// 创建API统计表索引
    async fn create_api_stats_indexes(pool: &SqlitePool) -> Result<()> {
        // 创建时间索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_stats_created_at 
            ON api_stats(created_at)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建端点索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_stats_endpoint 
            ON api_stats(endpoint)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建客户端ID索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_stats_client_id 
            ON api_stats(client_id)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建用户ID索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_stats_user_id 
            ON api_stats(user_id)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 创建监控系统相关表
    async fn create_monitor_tables(pool: &SqlitePool) -> Result<()> {
        Self::create_monitor_users_table(pool).await?;
        Self::create_monitor_sessions_table(pool).await?;
        Self::insert_default_monitor_users(pool).await?;
        Ok(())
    }

    /// 创建监控用户表
    async fn create_monitor_users_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS monitor_users (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'readonly',
                last_login_at TEXT,
                login_count INTEGER DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                is_active INTEGER DEFAULT 1
            )
            "#,
        )
        .execute(pool)
        .await?;

        Self::create_monitor_users_indexes(pool).await?;

        // 初始化默认管理员用户
        Self::init_default_admin_user(pool).await?;

        Ok(())
    }

    /// 初始化默认管理员用户
    async fn init_default_admin_user(pool: &SqlitePool) -> Result<()> {
        // 检查是否已存在管理员用户
        let existing_admin =
            sqlx::query("SELECT COUNT(*) as count FROM monitor_users WHERE role = 'admin'")
                .fetch_one(pool)
                .await?;

        let admin_count: i64 = existing_admin.get("count");

        if admin_count == 0 {
            tracing::info!("初始化默认管理员用户...");

            let admin_id = uuid::Uuid::new_v4().to_string();
            let current_time = chrono::Utc::now().to_rfc3339();
            let password_hash = Self::default_monitor_admin_password_hash();

            sqlx::query(
                r#"
                INSERT INTO monitor_users (
                    id, username, password_hash, role, 
                    login_count, created_at, updated_at, is_active
                ) VALUES (?, 'admin', ?, 'admin', 0, ?, ?, 1)
                "#,
            )
            .bind(&admin_id)
            .bind(&password_hash)
            .bind(&current_time)
            .bind(&current_time)
            .execute(pool)
            .await?;

            tracing::info!(
                "[ok] 默认管理员用户创建成功 (用户名: admin, 密码: {})",
                DEFAULT_MONITOR_ADMIN_PASSWORD
            );
            tracing::warn!("[warn]  请在生产环境中修改默认密码！");
        }

        Ok(())
    }

    /// 创建监控用户表索引
    async fn create_monitor_users_indexes(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_monitor_users_username 
            ON monitor_users(username)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_monitor_users_role 
            ON monitor_users(role)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_monitor_users_created_at 
            ON monitor_users(created_at)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 创建监控会话表
    async fn create_monitor_sessions_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS monitor_sessions (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                ip_address TEXT,
                user_agent TEXT,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                last_activity TEXT NOT NULL,
                is_active INTEGER DEFAULT 1
            )
            "#,
        )
        .execute(pool)
        .await?;

        Self::create_monitor_sessions_indexes(pool).await?;
        Ok(())
    }

    /// 创建监控会话表索引
    async fn create_monitor_sessions_indexes(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_monitor_sessions_user_id 
            ON monitor_sessions(user_id)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_monitor_sessions_expires_at 
            ON monitor_sessions(expires_at)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 插入默认监控用户
    async fn insert_default_monitor_users(pool: &SqlitePool) -> Result<()> {
        // 检查是否已存在admin用户
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM monitor_users WHERE username = 'admin'",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if exists == 0 {
            let admin_id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let password_hash = Self::default_monitor_admin_password_hash();

            sqlx::query(
                r#"
                INSERT INTO monitor_users (
                    id, username, password_hash, role, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&admin_id)
            .bind("admin")
            .bind(&password_hash)
            .bind("admin")
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;

            tracing::info!(
                "[ok] 默认监控管理员账户已创建: admin/{}",
                DEFAULT_MONITOR_ADMIN_PASSWORD
            );
        }

        Ok(())
    }

    /// 创建用户登录记录表
    async fn create_user_login_records_table(pool: &SqlitePool) -> Result<()> {
        // 创建用户登录记录表
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_login_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id TEXT NOT NULL,
                user_name TEXT,
                certificate_type TEXT,
                certificate_number TEXT,
                phone_number TEXT,
                email TEXT,
                organization_name TEXT,
                organization_code TEXT,
                login_type TEXT NOT NULL,
                login_time TEXT NOT NULL,
                client_ip TEXT NOT NULL,
                user_agent TEXT NOT NULL,
                referer TEXT,
                cookie_info TEXT,
                raw_data TEXT,
                created_at TEXT NOT NULL
            )
            "#,
        )
        .execute(pool)
        .await?;

        Self::create_user_login_records_indexes(pool).await?;
        Ok(())
    }

    /// 创建用户登录记录表索引
    async fn create_user_login_records_indexes(pool: &SqlitePool) -> Result<()> {
        // 用户ID索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_user_login_records_user_id 
            ON user_login_records(user_id)
            "#,
        )
        .execute(pool)
        .await?;

        // 登录类型索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_user_login_records_login_type 
            ON user_login_records(login_type)
            "#,
        )
        .execute(pool)
        .await?;

        // 创建时间索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_user_login_records_created_at 
            ON user_login_records(created_at)
            "#,
        )
        .execute(pool)
        .await?;

        // 客户端IP索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_user_login_records_client_ip 
            ON user_login_records(client_ip)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 创建回灌Outbox事件表（SQLite）
    async fn create_db_outbox_table(pool: &SqlitePool) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS db_outbox (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                table_name TEXT NOT NULL,
                op_type TEXT NOT NULL,
                pk_value TEXT NOT NULL,
                idempotency_key TEXT NOT NULL UNIQUE,
                payload TEXT NOT NULL,
                created_at TEXT NOT NULL,
                applied_at TEXT,
                retries INTEGER DEFAULT 0,
                last_error TEXT
            )
            "#,
        )
        .execute(pool)
        .await?;

        // 索引：表名、创建时间、应用时间
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_db_outbox_table_name 
            ON db_outbox(table_name)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_db_outbox_created_at 
            ON db_outbox(created_at)
            "#,
        )
        .execute(pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_db_outbox_applied_at 
            ON db_outbox(applied_at)
            "#,
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}
