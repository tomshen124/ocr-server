//! SQLite数据库表结构定义
//! 包含所有表的CREATE语句和索引定义

use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{SqlitePool, Row};
use uuid;

/// 数据库表结构管理器
pub struct SchemaManager;

impl SchemaManager {
    /// 创建所有表结构
    pub async fn create_all_tables(pool: &SqlitePool) -> Result<()> {
        Self::create_preview_records_table(pool).await?;
        Self::create_api_stats_table(pool).await?;
        // 新增监控系统表
        Self::create_monitor_tables(pool).await?;
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
                file_name TEXT NOT NULL,
                ocr_text TEXT NOT NULL,
                theme_id TEXT,
                evaluation_result TEXT,
                preview_url TEXT NOT NULL,
                status TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                third_party_request_id TEXT
            )
            "#,
        )
        .execute(pool)
        .await?;
        
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
        let existing_admin = sqlx::query(
            "SELECT COUNT(*) as count FROM monitor_users WHERE role = 'admin'"
        )
        .fetch_one(pool)
        .await?;
        
        let admin_count: i64 = existing_admin.get("count");
        
        if admin_count == 0 {
            tracing::info!("初始化默认管理员用户...");
            
            let admin_id = uuid::Uuid::new_v4().to_string();
            let current_time = chrono::Utc::now().to_rfc3339();
            
            // 默认密码：admin123，生产环境请修改
            #[cfg(feature = "monitoring")]
            let password_hash = {
                use bcrypt::{hash, DEFAULT_COST};
                hash("admin123", DEFAULT_COST).unwrap_or_else(|_| "admin123".to_string())
            };
            #[cfg(not(feature = "monitoring"))]
            let password_hash = "admin123"; // 简单模式下直接存储明文
            
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
            
            tracing::info!("✅ 默认管理员用户创建成功 (用户名: admin, 密码: admin123)");
            tracing::warn!("⚠️  请在生产环境中修改默认密码！");
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
            "SELECT COUNT(*) FROM monitor_users WHERE username = 'admin'"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        
        if exists == 0 {
            // 生成UUID
            let admin_id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
            
            // 插入默认管理员 (密码: admin!@#123)
            sqlx::query(
                r#"
                INSERT INTO monitor_users (
                    id, username, password_hash, role, created_at, updated_at
                ) VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(&admin_id)
            .bind("admin")
            .bind("$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/LeWJ6VzKTZ8ELAGFm") // bcrypt hash of 'admin!@#123'
            .bind("admin")
            .bind(&now)
            .bind(&now)
            .execute(pool)
            .await?;
            
            tracing::info!("✅ 默认监控管理员账户已创建: admin/admin!@#123");
        }
        
        Ok(())
    }
}