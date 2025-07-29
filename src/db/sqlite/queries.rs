//! SQLite数据库查询操作
//! 包含所有数据库查询和操作的实现

use anyhow::Result;
use sqlx::{SqlitePool, Row};
use chrono::{DateTime, Utc};

use crate::db::traits::*;

/// 预审记录查询操作
pub struct PreviewQueries;

impl PreviewQueries {
    /// 保存预审记录
    pub async fn save_record(pool: &SqlitePool, record: &PreviewRecord) -> Result<()> {
        let status_str = Self::status_to_string(&record.status);
        
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
        .execute(pool)
        .await?;
        
        Ok(())
    }
    
    /// 根据ID获取预审记录
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<PreviewRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, user_id, file_name, ocr_text, theme_id, 
                   evaluation_result, preview_url, status, created_at, updated_at, third_party_request_id
            FROM preview_records
            WHERE id = ?
            "#,
        )
        .bind(id)
        .fetch_optional(pool)
        .await?;
        
        if let Some(row) = row {
            Ok(Some(Self::row_to_record(row)?))
        } else {
            Ok(None)
        }
    }
    
    /// 更新预审状态
    pub async fn update_status(pool: &SqlitePool, id: &str, status: PreviewStatus) -> Result<()> {
        let status_str = Self::status_to_string(&status);
        
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
        .execute(pool)
        .await?;
        
        Ok(())
    }

    /// 更新预审的evaluation_result字段
    pub async fn update_evaluation_result(pool: &SqlitePool, id: &str, evaluation_result: &str) -> Result<()> {
        sqlx::query("UPDATE preview_records SET evaluation_result = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(evaluation_result)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }
    
    /// 根据过滤条件列出预审记录
    pub async fn list_with_filter(pool: &SqlitePool, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>> {
        let mut query = String::from(
            "SELECT id, user_id, file_name, ocr_text, theme_id, 
                    evaluation_result, preview_url, status, created_at, updated_at, third_party_request_id
             FROM preview_records WHERE 1=1"
        );
        
        let mut bindings = Vec::new();
        
        Self::apply_filter_conditions(&mut query, &mut bindings, filter);
        
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
        
        let rows = sql_query.fetch_all(pool).await?;
        
        let mut records = Vec::new();
        for row in rows {
            records.push(Self::row_to_record(row)?);
        }
        
        Ok(records)
    }
    
    /// 根据第三方请求ID查找预审记录
    pub async fn find_by_third_party_id(
        pool: &SqlitePool, 
        third_party_id: &str, 
        user_id: &str
    ) -> Result<Option<PreviewRecord>> {
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
        .fetch_optional(pool)
        .await?;
        
        if let Some(row) = row {
            Ok(Some(Self::row_to_record(row)?))
        } else {
            Ok(None)
        }
    }
    
    /// 应用过滤条件到查询语句
    fn apply_filter_conditions(query: &mut String, bindings: &mut Vec<String>, filter: &PreviewFilter) {
        if let Some(user_id) = &filter.user_id {
            query.push_str(" AND user_id = ?");
            bindings.push(user_id.clone());
        }
        
        if let Some(status) = &filter.status {
            let status_str = Self::status_to_string(status);
            query.push_str(" AND status = ?");
            bindings.push(status_str.to_string());
        }
        
        if let Some(theme_id) = &filter.theme_id {
            query.push_str(" AND theme_id = ?");
            bindings.push(theme_id.clone());
        }
    }
    
    /// 将数据库行转换为PreviewRecord
    fn row_to_record(row: sqlx::sqlite::SqliteRow) -> Result<PreviewRecord> {
        let status_str: String = row.get("status");
        let status = Self::string_to_status(&status_str);
        
        let created_at_str: String = row.get("created_at");
        let updated_at_str: String = row.get("updated_at");
        
        Ok(PreviewRecord {
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
        })
    }
    
    /// 状态枚举转字符串
    fn status_to_string(status: &PreviewStatus) -> &'static str {
        match status {
            PreviewStatus::Pending => "pending",
            PreviewStatus::Processing => "processing",
            PreviewStatus::Completed => "completed",
            PreviewStatus::Failed => "failed",
        }
    }
    
    /// 字符串转状态枚举
    fn string_to_status(status_str: &str) -> PreviewStatus {
        match status_str {
            "pending" => PreviewStatus::Pending,
            "processing" => PreviewStatus::Processing,
            "completed" => PreviewStatus::Completed,
            "failed" => PreviewStatus::Failed,
            _ => PreviewStatus::Pending,
        }
    }
}

/// API统计查询操作
pub struct ApiStatsQueries;

impl ApiStatsQueries {
    /// 保存API统计
    pub async fn save_stats(pool: &SqlitePool, stats: &ApiStats) -> Result<()> {
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
        .execute(pool)
        .await?;
        
        Ok(())
    }
    
    /// 根据过滤条件获取API统计
    pub async fn get_stats_with_filter(pool: &SqlitePool, filter: &StatsFilter) -> Result<Vec<ApiStats>> {
        let mut query = String::from(
            "SELECT id, endpoint, method, client_id, user_id,
                    status_code, response_time_ms, request_size, response_size,
                    error_message, created_at
             FROM api_stats WHERE 1=1"
        );
        
        let mut bindings: Vec<String> = Vec::new();
        
        Self::apply_stats_filter_conditions(&mut query, &mut bindings, filter);
        
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
        
        let rows = sql_query.fetch_all(pool).await?;
        
        let mut stats_list = Vec::new();
        for row in rows {
            stats_list.push(Self::row_to_stats(row)?);
        }
        
        Ok(stats_list)
    }
    
    /// 获取API统计摘要
    pub async fn get_summary(pool: &SqlitePool, filter: &StatsFilter) -> Result<ApiSummary> {
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
        
        Self::apply_stats_filter_conditions(&mut query, &mut bindings, filter);
        
        let mut sql_query = sqlx::query(&query);
        for binding in bindings {
            sql_query = sql_query.bind(binding);
        }
        
        let row = sql_query.fetch_one(pool).await?;
        
        Ok(ApiSummary {
            total_calls: row.get::<i64, _>("total_calls") as u64,
            success_calls: row.get::<i64, _>("success_calls") as u64,
            failed_calls: row.get::<i64, _>("failed_calls") as u64,
            avg_response_time_ms: row.get::<Option<f64>, _>("avg_response_time_ms").unwrap_or(0.0),
            total_request_size: row.get::<i64, _>("total_request_size") as u64,
            total_response_size: row.get::<i64, _>("total_response_size") as u64,
        })
    }
    
    /// 应用统计过滤条件
    fn apply_stats_filter_conditions(query: &mut String, bindings: &mut Vec<String>, filter: &StatsFilter) {
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
    }
    
    /// 将数据库行转换为ApiStats
    fn row_to_stats(row: sqlx::sqlite::SqliteRow) -> Result<ApiStats> {
        let created_at_str: String = row.get("created_at");
        
        Ok(ApiStats {
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
        })
    }
}

/// 数据库健康检查查询
pub struct HealthQueries;

impl HealthQueries {
    /// 执行健康检查
    pub async fn check_health(pool: &SqlitePool) -> Result<bool> {
        sqlx::query("SELECT 1")
            .fetch_one(pool)
            .await
            .map(|_| true)
            .map_err(|e| e.into())
    }
}