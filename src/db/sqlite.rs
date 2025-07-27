use async_trait::async_trait;
use anyhow::Result;
use sqlx::{sqlite::SqlitePool, Row};
use std::path::Path;
use chrono::{DateTime, Utc};

use super::traits::*;

pub struct SqliteDatabase {
    pool: SqlitePool,
}

impl SqliteDatabase {
    pub async fn new(db_path: &str) -> Result<Self> {
        // 确保数据库目录存在
        if let Some(parent) = Path::new(db_path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 如果数据库文件不存在，创建空文件
        // 这确保SQLite连接池能够正常初始化
        if !Path::new(db_path).exists() {
            std::fs::File::create(db_path)?;
            tracing::info!("Created SQLite database file: {}", db_path);
        }

        // 创建连接池
        let connection_string = format!("sqlite:{}", db_path);
        let pool = SqlitePool::connect(&connection_string).await?;

        Ok(Self { pool })
    }
    
    /// 创建表结构
    async fn create_tables(&self) -> Result<()> {
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
        .execute(&self.pool)
        .await?;
        
        // 创建用户ID索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_user_id 
            ON preview_records(user_id)
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // 创建第三方请求ID索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_preview_records_third_party_id 
            ON preview_records(third_party_request_id)
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // 创建API统计表
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
        .execute(&self.pool)
        .await?;
        
        // 创建时间索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_stats_created_at 
            ON api_stats(created_at)
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        // 创建端点索引
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_api_stats_endpoint 
            ON api_stats(endpoint)
            "#,
        )
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
}

#[async_trait]
impl Database for SqliteDatabase {
    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()> {
        let status_str = match record.status {
            PreviewStatus::Pending => "pending",
            PreviewStatus::Processing => "processing",
            PreviewStatus::Completed => "completed",
            PreviewStatus::Failed => "failed",
        };
        
        sqlx::query(
            r#"
            INSERT INTO preview_records (
                id, user_id, file_name, ocr_text, theme_id, 
                evaluation_result, preview_url, status, created_at, updated_at, third_party_request_id
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&record.id)
        .bind(&record.user_id)
        .bind(&record.file_name)
        .bind(&record.ocr_text)
        .bind(&record.theme_id)
        .bind(&record.evaluation_result)
        .bind(&record.preview_url)
        .bind(status_str)
        .bind(record.created_at.to_rfc3339())
        .bind(record.updated_at.to_rfc3339())
        .bind(&record.third_party_request_id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, user_id, file_name, ocr_text, theme_id, 
                   evaluation_result, preview_url, status, created_at, updated_at, third_party_request_id
            FROM preview_records
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(row) = row {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "pending" => PreviewStatus::Pending,
                "processing" => PreviewStatus::Processing,
                "completed" => PreviewStatus::Completed,
                "failed" => PreviewStatus::Failed,
                _ => PreviewStatus::Pending,
            };
            
            let created_at_str: String = row.get("created_at");
            let updated_at_str: String = row.get("updated_at");
            
            Ok(Some(PreviewRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                file_name: row.get("file_name"),
                ocr_text: row.get("ocr_text"),
                theme_id: row.get("theme_id"),
                evaluation_result: row.get("evaluation_result"),
                preview_url: row.get("preview_url"),
                status,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc),
                third_party_request_id: row.get("third_party_request_id"),
            }))
        } else {
            Ok(None)
        }
    }
    
    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()> {
        let status_str = match status {
            PreviewStatus::Pending => "pending",
            PreviewStatus::Processing => "processing",
            PreviewStatus::Completed => "completed",
            PreviewStatus::Failed => "failed",
        };
        
        sqlx::query(
            r#"
            UPDATE preview_records 
            SET status = ?, updated_at = ? 
            WHERE id = ?
            "#,
        )
        .bind(status_str)
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>> {
        let mut query = String::from(
            "SELECT id, user_id, file_name, ocr_text, theme_id, 
                    evaluation_result, preview_url, status, created_at, updated_at, third_party_request_id
             FROM preview_records WHERE 1=1"
        );
        
        let mut bindings = Vec::new();
        
        if let Some(user_id) = &filter.user_id {
            query.push_str(" AND user_id = ?");
            bindings.push(user_id.clone());
        }
        
        if let Some(status) = &filter.status {
            let status_str = match status {
                PreviewStatus::Pending => "pending",
                PreviewStatus::Processing => "processing",
                PreviewStatus::Completed => "completed",
                PreviewStatus::Failed => "failed",
            };
            query.push_str(" AND status = ?");
            bindings.push(status_str.to_string());
        }
        
        if let Some(theme_id) = &filter.theme_id {
            query.push_str(" AND theme_id = ?");
            bindings.push(theme_id.clone());
        }
        
        query.push_str(" ORDER BY created_at DESC");
        
        if let Some(limit) = filter.limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }
        
        if let Some(offset) = filter.offset {
            query.push_str(&format!(" OFFSET {}", offset));
        }
        
        let mut sql_query = sqlx::query(&query);
        for binding in bindings {
            sql_query = sql_query.bind(binding);
        }
        
        let rows = sql_query.fetch_all(&self.pool).await?;
        
        let mut records = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "pending" => PreviewStatus::Pending,
                "processing" => PreviewStatus::Processing,
                "completed" => PreviewStatus::Completed,
                "failed" => PreviewStatus::Failed,
                _ => PreviewStatus::Pending,
            };
            
            let created_at_str: String = row.get("created_at");
            let updated_at_str: String = row.get("updated_at");
            
            records.push(PreviewRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                file_name: row.get("file_name"),
                ocr_text: row.get("ocr_text"),
                theme_id: row.get("theme_id"),
                evaluation_result: row.get("evaluation_result"),
                preview_url: row.get("preview_url"),
                status,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc),
                third_party_request_id: row.get("third_party_request_id"),
            });
        }
        
        Ok(records)
    }
    
    async fn find_preview_by_third_party_id(&self, third_party_id: &str, user_id: &str) -> Result<Option<PreviewRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, user_id, file_name, ocr_text, theme_id, 
                   evaluation_result, preview_url, status, created_at, updated_at, third_party_request_id
            FROM preview_records 
            WHERE user_id = ? AND third_party_request_id = ?
            ORDER BY created_at DESC
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(third_party_id)
        .fetch_optional(&self.pool)
        .await?;
        
        if let Some(row) = row {
            let status_str: String = row.get("status");
            let status = match status_str.as_str() {
                "pending" => PreviewStatus::Pending,
                "processing" => PreviewStatus::Processing,
                "completed" => PreviewStatus::Completed,
                "failed" => PreviewStatus::Failed,
                _ => PreviewStatus::Pending,
            };
            
            let created_at_str: String = row.get("created_at");
            let updated_at_str: String = row.get("updated_at");
            
            Ok(Some(PreviewRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                file_name: row.get("file_name"),
                ocr_text: row.get("ocr_text"),
                theme_id: row.get("theme_id"),
                evaluation_result: row.get("evaluation_result"),
                preview_url: row.get("preview_url"),
                status,
                created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
                updated_at: DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc),
                third_party_request_id: row.get("third_party_request_id"),
            }))
        } else {
            Ok(None)
        }
    }
    
    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO api_stats (
                id, endpoint, method, client_id, user_id,
                status_code, response_time_ms, request_size, response_size,
                error_message, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&stats.id)
        .bind(&stats.endpoint)
        .bind(&stats.method)
        .bind(&stats.client_id)
        .bind(&stats.user_id)
        .bind(stats.status_code)
        .bind(stats.response_time_ms)
        .bind(stats.request_size)
        .bind(stats.response_size)
        .bind(&stats.error_message)
        .bind(stats.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;
        
        Ok(())
    }
    
    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>> {
        let mut query = String::from(
            "SELECT id, endpoint, method, client_id, user_id,
                    status_code, response_time_ms, request_size, response_size,
                    error_message, created_at
             FROM api_stats WHERE 1=1"
        );
        
        let mut bindings: Vec<String> = Vec::new();
        
        if let Some(endpoint) = &filter.endpoint {
            query.push_str(" AND endpoint = ?");
            bindings.push(endpoint.clone());
        }
        
        if let Some(client_id) = &filter.client_id {
            query.push_str(" AND client_id = ?");
            bindings.push(client_id.clone());
        }
        
        if let Some(user_id) = &filter.user_id {
            query.push_str(" AND user_id = ?");
            bindings.push(user_id.clone());
        }
        
        if let Some(start_date) = &filter.start_date {
            query.push_str(" AND created_at >= ?");
            bindings.push(start_date.to_rfc3339());
        }
        
        if let Some(end_date) = &filter.end_date {
            query.push_str(" AND created_at <= ?");
            bindings.push(end_date.to_rfc3339());
        }
        
        query.push_str(" ORDER BY created_at DESC");
        
        if let Some(limit) = filter.limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }
        
        if let Some(offset) = filter.offset {
            query.push_str(&format!(" OFFSET {}", offset));
        }
        
        let mut sql_query = sqlx::query(&query);
        for binding in bindings {
            sql_query = sql_query.bind(binding);
        }
        
        let rows = sql_query.fetch_all(&self.pool).await?;
        
        let mut stats_list = Vec::new();
        for row in rows {
            let created_at_str: String = row.get("created_at");
            
            stats_list.push(ApiStats {
                id: row.get("id"),
                endpoint: row.get("endpoint"),
                method: row.get("method"),
                client_id: row.get("client_id"),
                user_id: row.get("user_id"),
                status_code: row.get("status_code"),
                response_time_ms: row.get("response_time_ms"),
                request_size: row.get("request_size"),
                response_size: row.get("response_size"),
                error_message: row.get("error_message"),
                created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
            });
        }
        
        Ok(stats_list)
    }
    
    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary> {
        let mut query = String::from(
            r#"
            SELECT 
                COUNT(*) as total_calls,
                SUM(CASE WHEN status_code >= 200 AND status_code < 300 THEN 1 ELSE 0 END) as success_calls,
                SUM(CASE WHEN status_code >= 400 THEN 1 ELSE 0 END) as failed_calls,
                AVG(response_time_ms) as avg_response_time_ms,
                SUM(request_size) as total_request_size,
                SUM(response_size) as total_response_size
            FROM api_stats WHERE 1=1
            "#
        );
        
        let mut bindings: Vec<String> = Vec::new();
        
        if let Some(endpoint) = &filter.endpoint {
            query.push_str(" AND endpoint = ?");
            bindings.push(endpoint.clone());
        }
        
        if let Some(client_id) = &filter.client_id {
            query.push_str(" AND client_id = ?");
            bindings.push(client_id.clone());
        }
        
        if let Some(start_date) = &filter.start_date {
            query.push_str(" AND created_at >= ?");
            bindings.push(start_date.to_rfc3339());
        }
        
        if let Some(end_date) = &filter.end_date {
            query.push_str(" AND created_at <= ?");
            bindings.push(end_date.to_rfc3339());
        }
        
        let mut sql_query = sqlx::query(&query);
        for binding in bindings {
            sql_query = sql_query.bind(binding);
        }
        
        let row = sql_query.fetch_one(&self.pool).await?;
        
        Ok(ApiSummary {
            total_calls: row.get::<i64, _>("total_calls") as u64,
            success_calls: row.get::<i64, _>("success_calls") as u64,
            failed_calls: row.get::<i64, _>("failed_calls") as u64,
            avg_response_time_ms: row.get::<Option<f64>, _>("avg_response_time_ms").unwrap_or(0.0),
            total_request_size: row.get::<i64, _>("total_request_size") as u64,
            total_response_size: row.get::<i64, _>("total_response_size") as u64,
        })
    }
    
    async fn health_check(&self) -> Result<bool> {
        sqlx::query("SELECT 1")
            .fetch_one(&self.pool)
            .await
            .map(|_| true)
            .map_err(|e| e.into())
    }
    
    async fn initialize(&self) -> Result<()> {
        self.create_tables().await?;
        Ok(())
    }
}