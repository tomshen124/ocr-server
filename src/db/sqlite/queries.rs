//! SQLite数据库查询操作
//! 包含所有数据库查询和操作的实现

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};

use crate::db::traits::*;

/// 预审请求查询操作
pub struct PreviewRequestQueries;

impl PreviewRequestQueries {
    pub async fn upsert(pool: &SqlitePool, request: &PreviewRequestRecord) -> Result<()> {
        let latest_status = request.latest_status.as_ref().map(|s| s.as_str());

        sqlx::query(
            r#"
            INSERT INTO preview_requests (
                id, third_party_request_id, user_id, user_info_json, matter_id, matter_type, matter_name,
                channel, sequence_no, agent_info_json, subject_info_json, form_data_json,
                scene_data_json, material_data_json, latest_preview_id, latest_status,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                third_party_request_id = excluded.third_party_request_id,
                user_id = excluded.user_id,
                user_info_json = excluded.user_info_json,
                matter_id = excluded.matter_id,
                matter_type = excluded.matter_type,
                matter_name = excluded.matter_name,
                channel = excluded.channel,
                sequence_no = excluded.sequence_no,
                agent_info_json = excluded.agent_info_json,
                subject_info_json = excluded.subject_info_json,
                form_data_json = excluded.form_data_json,
                scene_data_json = excluded.scene_data_json,
                material_data_json = excluded.material_data_json,
                latest_preview_id = excluded.latest_preview_id,
                latest_status = excluded.latest_status,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&request.id)
        .bind(&request.third_party_request_id)
        .bind(&request.user_id)
        .bind(&request.user_info_json)
        .bind(&request.matter_id)
        .bind(&request.matter_type)
        .bind(&request.matter_name)
        .bind(&request.channel)
        .bind(&request.sequence_no)
        .bind(&request.agent_info_json)
        .bind(&request.subject_info_json)
        .bind(&request.form_data_json)
        .bind(&request.scene_data_json)
        .bind(&request.material_data_json)
        .bind(&request.latest_preview_id)
        .bind(latest_status)
        .bind(request.created_at.to_rfc3339())
        .bind(request.updated_at.to_rfc3339())
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn update_latest_state(
        pool: &SqlitePool,
        request_id: &str,
        latest_preview_id: Option<&str>,
        latest_status: Option<PreviewStatus>,
    ) -> Result<()> {
        let latest_status = latest_status.map(|s| s.as_str());
        sqlx::query(
            r#"
            UPDATE preview_requests
            SET latest_preview_id = ?, latest_status = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(latest_preview_id)
        .bind(latest_status)
        .bind(Utc::now().to_rfc3339())
        .bind(request_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<PreviewRequestRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, third_party_request_id, user_id, user_info_json, matter_id, matter_type, matter_name,
                   channel, sequence_no, agent_info_json, subject_info_json, form_data_json,
                   scene_data_json, material_data_json, latest_preview_id, latest_status,
                   created_at, updated_at
            FROM preview_requests
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

    pub async fn get_by_third_party(
        pool: &SqlitePool,
        third_party_id: &str,
    ) -> Result<Option<PreviewRequestRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, third_party_request_id, user_id, user_info_json, matter_id, matter_type, matter_name,
                   channel, sequence_no, agent_info_json, subject_info_json, form_data_json,
                   scene_data_json, material_data_json, latest_preview_id, latest_status,
                   created_at, updated_at
            FROM preview_requests
            WHERE third_party_request_id = ?
            "#,
        )
        .bind(third_party_id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(Self::row_to_record(row)?))
        } else {
            Ok(None)
        }
    }

    pub async fn list(
        pool: &SqlitePool,
        filter: &PreviewRequestFilter,
    ) -> Result<Vec<PreviewRequestRecord>> {
        let mut builder = QueryBuilder::<sqlx::Sqlite>::new(
            "SELECT id, third_party_request_id, user_id, user_info_json, matter_id, matter_type, matter_name, \
                    channel, sequence_no, agent_info_json, subject_info_json, form_data_json, \
                    scene_data_json, material_data_json, latest_preview_id, latest_status, \
                    created_at, updated_at FROM preview_requests WHERE 1=1",
        );

        if let Some(user_id) = &filter.user_id {
            builder.push(" AND user_id = ").push_bind(user_id);
        }

        if let Some(matter_id) = &filter.matter_id {
            builder.push(" AND matter_id = ").push_bind(matter_id);
        }

        if let Some(channel) = &filter.channel {
            builder.push(" AND channel = ").push_bind(channel);
        }

        if let Some(sequence_no) = &filter.sequence_no {
            builder.push(" AND sequence_no = ").push_bind(sequence_no);
        }

        if let Some(third_party) = &filter.third_party_request_id {
            builder
                .push(" AND third_party_request_id = ")
                .push_bind(third_party);
        }

        if let Some(status) = &filter.latest_status {
            builder
                .push(" AND latest_status = ")
                .push_bind(status.as_str());
        }

        if let Some(start) = filter.created_from {
            builder
                .push(" AND created_at >= ")
                .push_bind(start.to_rfc3339());
        }

        if let Some(end) = filter.created_to {
            builder
                .push(" AND created_at <= ")
                .push_bind(end.to_rfc3339());
        }

        if let Some(search) = &filter.search {
            let pattern = format!("%{}%", search);
            builder
                .push(" AND (id LIKE ")
                .push_bind(pattern.clone())
                .push(" OR third_party_request_id LIKE ")
                .push_bind(pattern)
                .push(")");
        }

        builder.push(" ORDER BY updated_at DESC");

        if let Some(limit) = filter.limit {
            builder.push(" LIMIT ").push_bind(limit as i64);
        }

        if let Some(offset) = filter.offset {
            builder.push(" OFFSET ").push_bind(offset as i64);
        }

        let query = builder.build();
        let rows = query.fetch_all(pool).await?;
        rows.into_iter().map(Self::row_to_record).collect()
    }

    fn row_to_record(row: sqlx::sqlite::SqliteRow) -> Result<PreviewRequestRecord> {
        let latest_status = row.try_get::<Option<String>, _>("latest_status")?;
        let latest_status = latest_status
            .as_deref()
            .map(Self::string_to_status)
            .transpose()?;

        Ok(PreviewRequestRecord {
            id: row.try_get("id")?,
            third_party_request_id: row.try_get("third_party_request_id")?,
            user_id: row.try_get("user_id")?,
            user_info_json: row.try_get("user_info_json")?,
            matter_id: row.try_get("matter_id")?,
            matter_type: row.try_get("matter_type")?,
            matter_name: row.try_get("matter_name")?,
            channel: row.try_get("channel")?,
            sequence_no: row.try_get("sequence_no")?,
            agent_info_json: row.try_get("agent_info_json")?,
            subject_info_json: row.try_get("subject_info_json")?,
            form_data_json: row.try_get("form_data_json")?,
            scene_data_json: row.try_get("scene_data_json")?,
            material_data_json: row.try_get("material_data_json")?,
            latest_preview_id: row.try_get("latest_preview_id")?,
            latest_status,
            created_at: DateTime::parse_from_rfc3339(
                row.try_get::<String, _>("created_at")?.as_str(),
            )
            .map(|dt| dt.with_timezone(&Utc))?,
            updated_at: DateTime::parse_from_rfc3339(
                row.try_get::<String, _>("updated_at")?.as_str(),
            )
            .map(|dt| dt.with_timezone(&Utc))?,
        })
    }

    fn string_to_status(value: &str) -> Result<PreviewStatus> {
        match value {
            "pending" => Ok(PreviewStatus::Pending),
            "queued" => Ok(PreviewStatus::Queued),
            "processing" => Ok(PreviewStatus::Processing),
            "completed" => Ok(PreviewStatus::Completed),
            "failed" => Ok(PreviewStatus::Failed),
            other => Err(anyhow!("unknown preview status: {}", other)),
        }
    }
}

/// 材料缓存记录查询
pub struct CachedMaterialQueries;

impl CachedMaterialQueries {
    pub async fn upsert(pool: &SqlitePool, record: &CachedMaterialRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO cached_materials (
                id, preview_id, material_code, attachment_index, token, local_path,
                upload_status, oss_key, last_error, file_size, checksum_sha256,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                preview_id = excluded.preview_id,
                material_code = excluded.material_code,
                attachment_index = excluded.attachment_index,
                token = excluded.token,
                local_path = excluded.local_path,
                upload_status = excluded.upload_status,
                oss_key = excluded.oss_key,
                last_error = excluded.last_error,
                file_size = excluded.file_size,
                checksum_sha256 = excluded.checksum_sha256,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&record.id)
        .bind(&record.preview_id)
        .bind(&record.material_code)
        .bind(record.attachment_index)
        .bind(&record.token)
        .bind(&record.local_path)
        .bind(record.upload_status.as_str())
        .bind(&record.oss_key)
        .bind(&record.last_error)
        .bind(record.file_size)
        .bind(&record.checksum_sha256)
        .bind(record.created_at.to_rfc3339())
        .bind(record.updated_at.to_rfc3339())
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn update_status(
        pool: &SqlitePool,
        id: &str,
        status: CachedMaterialStatus,
        oss_key: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE cached_materials
            SET upload_status = ?,
                oss_key = ?,
                last_error = ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status.as_str())
        .bind(oss_key)
        .bind(last_error)
        .bind(Utc::now().to_rfc3339())
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list(
        pool: &SqlitePool,
        filter: &CachedMaterialFilter,
    ) -> Result<Vec<CachedMaterialRecord>> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "SELECT id, preview_id, material_code, attachment_index, token, local_path, \
                    upload_status, oss_key, last_error, file_size, checksum_sha256, \
                    created_at, updated_at FROM cached_materials WHERE 1=1",
        );

        if let Some(preview_id) = &filter.preview_id {
            builder.push(" AND preview_id = ").push_bind(preview_id);
        }

        if let Some(status) = &filter.status {
            builder
                .push(" AND upload_status = ")
                .push_bind(status.as_str());
        }

        builder.push(" ORDER BY updated_at DESC");

        if let Some(limit) = filter.limit {
            builder.push(" LIMIT ").push_bind(limit as i64);
        }

        let rows = builder.build().fetch_all(pool).await?;
        rows.into_iter().map(Self::row_to_record).collect()
    }

    pub async fn delete_by_id(pool: &SqlitePool, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM cached_materials WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn delete_by_preview(pool: &SqlitePool, preview_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM cached_materials WHERE preview_id = ?")
            .bind(preview_id)
            .execute(pool)
            .await?;
        Ok(())
    }

    fn row_to_record(row: sqlx::sqlite::SqliteRow) -> Result<CachedMaterialRecord> {
        let status: String = row.try_get("upload_status")?;
        let created_at_str: String = row.try_get("created_at")?;
        let updated_at_str: String = row.try_get("updated_at")?;
        Ok(CachedMaterialRecord {
            id: row.try_get("id")?,
            preview_id: row.try_get("preview_id")?,
            material_code: row.try_get("material_code")?,
            attachment_index: row.try_get("attachment_index")?,
            token: row.try_get("token")?,
            local_path: row.try_get("local_path")?,
            upload_status: CachedMaterialStatus::from_str(&status),
            oss_key: row.try_get("oss_key")?,
            last_error: row.try_get("last_error")?,
            file_size: row.try_get("file_size")?,
            checksum_sha256: row.try_get("checksum_sha256")?,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc),
        })
    }
}

/// 材料评估结果查询
pub struct MaterialResultQueries;

impl MaterialResultQueries {
    pub async fn replace(
        pool: &SqlitePool,
        preview_id: &str,
        records: &[PreviewMaterialResultRecord],
    ) -> Result<()> {
        let _ = pool;
        let _ = preview_id;
        let _ = records;
        Ok(())
    }
}

/// 规则评估结果查询
pub struct RuleResultQueries;

impl RuleResultQueries {
    pub async fn replace(
        pool: &SqlitePool,
        preview_id: &str,
        records: &[PreviewRuleResultRecord],
    ) -> Result<()> {
        let _ = pool;
        let _ = preview_id;
        let _ = records;
        Ok(())
    }
}

/// 预审记录查询操作
pub struct PreviewQueries;

impl PreviewQueries {
    /// 保存预审记录
    pub async fn save_record(pool: &SqlitePool, record: &PreviewRecord) -> Result<()> {
        let status_str = Self::status_to_string(&record.status);

        let queued_at = record.queued_at.as_ref().map(|dt| dt.to_rfc3339());
        let processing_started_at = record
            .processing_started_at
            .as_ref()
            .map(|dt| dt.to_rfc3339());
        let last_callback_at = record.last_callback_at.as_ref().map(|dt| dt.to_rfc3339());
        let next_callback_after = record
            .next_callback_after
            .as_ref()
            .map(|dt| dt.to_rfc3339());

        sqlx::query(
            r#"
            INSERT INTO preview_records (
                id, user_id, user_info_json, file_name, ocr_text, theme_id,
                evaluation_result, preview_url, preview_view_url, preview_download_url, status,
                created_at, updated_at, third_party_request_id,
                queued_at, processing_started_at, retry_count,
                last_worker_id, last_attempt_id, failure_reason, ocr_stderr_summary,
                failure_context, last_error_code, slow_attachment_info_json,
                callback_url, callback_status, callback_attempts, callback_successes, callback_failures,
                last_callback_at, last_callback_status_code, last_callback_response,
                last_callback_error, callback_payload, next_callback_after
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&record.id)
        .bind(&record.user_id)
        .bind(&record.user_info_json)
        .bind(&record.file_name)
        .bind(&record.ocr_text)
        .bind(&record.theme_id)
        .bind(&record.evaluation_result)
        .bind(&record.preview_url)
        .bind(record.preview_view_url.as_deref())
        .bind(record.preview_download_url.as_deref())
        .bind(status_str)
        .bind(record.created_at.to_rfc3339())
        .bind(record.updated_at.to_rfc3339())
        .bind(&record.third_party_request_id)
        .bind(queued_at)
        .bind(processing_started_at)
        .bind(record.retry_count)
        .bind(&record.last_worker_id)
        .bind(&record.last_attempt_id)
        .bind(&record.failure_reason)
        .bind(&record.ocr_stderr_summary)
        .bind(&record.failure_context)
        .bind(&record.last_error_code)
        .bind(&record.slow_attachment_info_json)
        .bind(&record.callback_url)
        .bind(&record.callback_status)
        .bind(record.callback_attempts)
        .bind(record.callback_successes)
        .bind(record.callback_failures)
        .bind(last_callback_at)
        .bind(&record.last_callback_status_code)
        .bind(&record.last_callback_response)
        .bind(&record.last_callback_error)
        .bind(&record.callback_payload)
        .bind(next_callback_after)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 根据ID获取预审记录
    pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<PreviewRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, user_id, user_info_json, file_name, ocr_text, theme_id, 
                   evaluation_result, preview_url, preview_view_url, preview_download_url, status,
                   created_at, updated_at, third_party_request_id,
                   queued_at, processing_started_at, retry_count, last_worker_id, last_attempt_id,
                   failure_reason, ocr_stderr_summary, failure_context, last_error_code,
                   slow_attachment_info_json, callback_url, callback_status, callback_attempts,
                   callback_successes, callback_failures, last_callback_at, last_callback_status_code,
                   last_callback_response, last_callback_error, callback_payload, next_callback_after
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
        let now = Utc::now().to_rfc3339();

        match status {
            PreviewStatus::Pending => {
                sqlx::query(
                    r#"
                    UPDATE preview_records 
                    SET status = ?, updated_at = ?, queued_at = NULL, processing_started_at = NULL,
                        retry_count = 0, last_worker_id = NULL, last_attempt_id = NULL
                    WHERE id = ?
                    "#,
                )
                .bind(status_str)
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?;
            }
            PreviewStatus::Queued => {
                sqlx::query(
                    r#"
                    UPDATE preview_records 
                    SET status = ?, updated_at = ?, queued_at = ?, processing_started_at = NULL
                    WHERE id = ?
                    "#,
                )
                .bind(status_str)
                .bind(&now)
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?;
            }
            PreviewStatus::Processing => {
                sqlx::query(
                    r#"
                    UPDATE preview_records 
                    SET status = ?, updated_at = ?, processing_started_at = ?, retry_count = retry_count + 1
                    WHERE id = ?
                    "#,
                )
                .bind(status_str)
                .bind(&now)
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?;
            }
            PreviewStatus::Completed | PreviewStatus::Failed => {
                sqlx::query(
                    r#"
                    UPDATE preview_records 
                    SET status = ?, updated_at = ?
                    WHERE id = ?
                    "#,
                )
                .bind(status_str)
                .bind(&now)
                .bind(id)
                .execute(pool)
                .await?;
            }
        }

        Ok(())
    }

    /// 标记任务进入Processing状态并记录worker信息
    pub async fn mark_processing(
        pool: &SqlitePool,
        id: &str,
        worker_id: &str,
        attempt_id: &str,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            UPDATE preview_records
            SET status = 'processing',
                updated_at = ?,
                processing_started_at = ?,
                retry_count = retry_count + 1,
                last_worker_id = ?,
                last_attempt_id = ?
            WHERE id = ?
            "#,
        )
        .bind(&now)
        .bind(&now)
        .bind(worker_id)
        .bind(attempt_id)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 更新预审的evaluation_result字段
    pub async fn update_evaluation_result(
        pool: &SqlitePool,
        id: &str,
        evaluation_result: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE preview_records SET evaluation_result = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(evaluation_result)
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    pub async fn update_artifacts(
        pool: &SqlitePool,
        id: &str,
        file_name: &str,
        preview_url: &str,
        preview_view_url: Option<&str>,
        preview_download_url: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE preview_records SET file_name = ?, preview_url = ?, preview_view_url = ?, preview_download_url = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(file_name)
        .bind(preview_url)
        .bind(preview_view_url)
        .bind(preview_download_url)
        .bind(id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 根据过滤条件列出预审记录
    pub async fn list_with_filter(
        pool: &SqlitePool,
        filter: &PreviewFilter,
    ) -> Result<Vec<PreviewRecord>> {
        let mut query = String::from(concat!(
            "SELECT id, user_id, user_info_json, file_name, ocr_text, theme_id, ",
            "evaluation_result, preview_url, preview_view_url, preview_download_url, status, ",
            "created_at, updated_at, third_party_request_id, ",
            "queued_at, processing_started_at, retry_count, last_worker_id, last_attempt_id, ",
            "failure_reason, ocr_stderr_summary, failure_context, last_error_code, ",
            "slow_attachment_info_json, callback_url, callback_status, callback_attempts, ",
            "callback_successes, callback_failures, last_callback_at, last_callback_status_code, ",
            "last_callback_response, last_callback_error, callback_payload, next_callback_after ",
            "FROM preview_records WHERE 1=1"
        ));

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

    /// 统计各状态数量
    pub async fn count_by_status(pool: &SqlitePool) -> Result<PreviewStatusCounts> {
        let rows = sqlx::query(
            r#"
            SELECT status, COUNT(*) AS cnt
            FROM preview_records
            GROUP BY status
            "#,
        )
        .fetch_all(pool)
        .await?;

        let mut counts = PreviewStatusCounts::default();
        for row in rows {
            let status_str: String = row.get("status");
            let cnt: i64 = row.get("cnt");
            let status = Self::string_to_status(&status_str);
            let add = cnt.max(0) as u64;
            counts.total += add;
            match status {
                PreviewStatus::Completed => counts.completed += add,
                PreviewStatus::Processing => counts.processing += add,
                PreviewStatus::Failed => counts.failed += add,
                PreviewStatus::Pending => counts.pending += add,
                PreviewStatus::Queued => counts.queued += add,
            }
        }

        Ok(counts)
    }

    /// 根据第三方请求ID查找预审记录
    pub async fn find_by_third_party_id(
        pool: &SqlitePool,
        third_party_id: &str,
        user_id: &str,
    ) -> Result<Option<PreviewRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, user_id, user_info_json, file_name, ocr_text, theme_id, 
                   evaluation_result, preview_url, preview_view_url, preview_download_url, status,
                   created_at, updated_at, third_party_request_id,
                   queued_at, processing_started_at, retry_count, last_worker_id, last_attempt_id,
                   failure_reason, ocr_stderr_summary, failure_context, last_error_code,
                   slow_attachment_info_json, callback_url, callback_status, callback_attempts,
                   callback_successes, callback_failures, last_callback_at, last_callback_status_code,
                   last_callback_response, last_callback_error, callback_payload, next_callback_after
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
    fn apply_filter_conditions(
        query: &mut String,
        bindings: &mut Vec<String>,
        filter: &PreviewFilter,
    ) {
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

        if let Some(tp_id) = &filter.third_party_request_id {
            query.push_str(" AND third_party_request_id = ?");
            bindings.push(tp_id.clone());
        }

        if let Some(start) = filter.start_date {
            query.push_str(" AND created_at >= ?");
            bindings.push(start.to_rfc3339());
        }

        if let Some(end) = filter.end_date {
            query.push_str(" AND created_at <= ?");
            bindings.push(end.to_rfc3339());
        }
    }

    /// 更新第三方回调状态
    pub async fn update_callback_state(
        pool: &SqlitePool,
        update: &PreviewCallbackUpdate,
    ) -> Result<()> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "UPDATE preview_records SET updated_at = CURRENT_TIMESTAMP",
        );

        if let Some(callback_url_opt) = &update.callback_url {
            builder.push(", callback_url = ");
            match callback_url_opt {
                Some(url) => {
                    builder.push_bind(url);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(callback_status_opt) = &update.callback_status {
            builder.push(", callback_status = ");
            match callback_status_opt {
                Some(status) => {
                    builder.push_bind(status);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(attempts) = update.callback_attempts {
            builder.push(", callback_attempts = ");
            builder.push_bind(attempts);
        }

        if let Some(successes) = update.callback_successes {
            builder.push(", callback_successes = ");
            builder.push_bind(successes);
        }

        if let Some(failures) = update.callback_failures {
            builder.push(", callback_failures = ");
            builder.push_bind(failures);
        }

        if let Some(last_at_opt) = &update.last_callback_at {
            builder.push(", last_callback_at = ");
            match last_at_opt {
                Some(dt) => {
                    builder.push_bind(dt.to_rfc3339());
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(status_code_opt) = &update.last_callback_status_code {
            builder.push(", last_callback_status_code = ");
            match status_code_opt {
                Some(code) => {
                    builder.push_bind(*code);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(response_opt) = &update.last_callback_response {
            builder.push(", last_callback_response = ");
            match response_opt {
                Some(resp) => {
                    builder.push_bind(resp);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(error_opt) = &update.last_callback_error {
            builder.push(", last_callback_error = ");
            match error_opt {
                Some(err) => {
                    builder.push_bind(err);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(payload_opt) = &update.callback_payload {
            builder.push(", callback_payload = ");
            match payload_opt {
                Some(payload) => {
                    builder.push_bind(payload);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(next_after_opt) = &update.next_callback_after {
            builder.push(", next_callback_after = ");
            match next_after_opt {
                Some(dt) => {
                    builder.push_bind(dt.to_rfc3339());
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        builder.push(" WHERE id = ");
        builder.push_bind(&update.preview_id);

        builder.build().execute(pool).await?;
        Ok(())
    }

    pub async fn update_failure_context(
        pool: &SqlitePool,
        update: &PreviewFailureUpdate,
    ) -> Result<()> {
        let mut builder = QueryBuilder::<Sqlite>::new(
            "UPDATE preview_records SET updated_at = CURRENT_TIMESTAMP",
        );

        if let Some(reason_opt) = &update.failure_reason {
            builder.push(", failure_reason = ");
            match reason_opt {
                Some(reason) => {
                    builder.push_bind(reason);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(context_opt) = &update.failure_context {
            builder.push(", failure_context = ");
            match context_opt {
                Some(context) => {
                    builder.push_bind(context);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(code_opt) = &update.last_error_code {
            builder.push(", last_error_code = ");
            match code_opt {
                Some(code) => {
                    builder.push_bind(code);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(slow_opt) = &update.slow_attachment_info_json {
            builder.push(", slow_attachment_info_json = ");
            match slow_opt {
                Some(json) => {
                    builder.push_bind(json);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        if let Some(ocr_opt) = &update.ocr_stderr_summary {
            builder.push(", ocr_stderr_summary = ");
            match ocr_opt {
                Some(summary) => {
                    builder.push_bind(summary);
                }
                None => {
                    builder.push("NULL");
                }
            }
        }

        builder.push(" WHERE id = ");
        builder.push_bind(&update.preview_id);

        builder.build().execute(pool).await?;
        Ok(())
    }

    /// 列出需要执行第三方回调的预审记录
    pub async fn list_due_callbacks(pool: &SqlitePool, limit: u32) -> Result<Vec<PreviewRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, user_id, user_info_json, file_name, ocr_text, theme_id,
                   evaluation_result, preview_url, preview_view_url, preview_download_url, status,
                   created_at, updated_at, third_party_request_id,
                   queued_at, processing_started_at, retry_count, last_worker_id, last_attempt_id,
                   failure_reason, ocr_stderr_summary, failure_context, last_error_code,
                   slow_attachment_info_json, callback_url, callback_status, callback_attempts,
                   callback_successes, callback_failures, last_callback_at, last_callback_status_code,
                   last_callback_response, last_callback_error, callback_payload, next_callback_after
            FROM preview_records
            WHERE callback_url IS NOT NULL
              AND callback_url <> ''
              AND callback_payload IS NOT NULL
              AND callback_status IN ('scheduled', 'retrying')
              AND (next_callback_after IS NULL OR next_callback_after <= CURRENT_TIMESTAMP)
            ORDER BY COALESCE(next_callback_after, updated_at) ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(pool)
        .await?
        .into_iter()
        .map(Self::row_to_record)
        .collect::<Result<Vec<_>>>()?;

        Ok(rows)
    }

    /// 将数据库行转换为PreviewRecord
    fn row_to_record(row: sqlx::sqlite::SqliteRow) -> Result<PreviewRecord> {
        let status_str: String = row.get("status");
        let status = Self::string_to_status(&status_str);

        let created_at_str: String = row.get("created_at");
        let updated_at_str: String = row.get("updated_at");

        let queued_at = row
            .try_get::<String, _>("queued_at")
            .ok()
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let processing_started_at = row
            .try_get::<String, _>("processing_started_at")
            .ok()
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let retry_count = row.try_get::<i64, _>("retry_count").unwrap_or(0) as i32;
        let last_worker_id = row.try_get::<String, _>("last_worker_id").ok();
        let last_attempt_id = row.try_get::<String, _>("last_attempt_id").ok();
        let preview_view_url = row.try_get::<String, _>("preview_view_url").ok();
        let preview_download_url = row.try_get::<String, _>("preview_download_url").ok();
        let failure_reason = row.try_get::<String, _>("failure_reason").ok();
        let ocr_stderr_summary = row.try_get::<String, _>("ocr_stderr_summary").ok();
        let failure_context = row.try_get::<String, _>("failure_context").ok();
        let last_error_code = row.try_get::<String, _>("last_error_code").ok();
        let slow_attachment_info_json = row.try_get::<String, _>("slow_attachment_info_json").ok();
        let callback_url = row.try_get::<String, _>("callback_url").ok();
        let callback_status = row.try_get::<String, _>("callback_status").ok();
        let callback_attempts = row.try_get::<i64, _>("callback_attempts").unwrap_or(0) as i32;
        let callback_successes = row.try_get::<i64, _>("callback_successes").unwrap_or(0) as i32;
        let callback_failures = row.try_get::<i64, _>("callback_failures").unwrap_or(0) as i32;
        let last_callback_at = row
            .try_get::<String, _>("last_callback_at")
            .ok()
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));
        let last_callback_status_code = row
            .try_get::<i64, _>("last_callback_status_code")
            .ok()
            .map(|v| v as i32);
        let last_callback_response = row.try_get::<String, _>("last_callback_response").ok();
        let last_callback_error = row.try_get::<String, _>("last_callback_error").ok();
        let callback_payload = row.try_get::<String, _>("callback_payload").ok();
        let next_callback_after = row
            .try_get::<String, _>("next_callback_after")
            .ok()
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(PreviewRecord {
            id: row.get("id"),
            user_id: row.get("user_id"),
            user_info_json: row.try_get("user_info_json").ok(),
            file_name: row.get("file_name"),
            ocr_text: row.get("ocr_text"),
            theme_id: row.get("theme_id"),
            evaluation_result: row.get("evaluation_result"),
            preview_url: row.get("preview_url"),
            preview_view_url,
            preview_download_url,
            status,
            created_at: DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at_str)?.with_timezone(&Utc),
            third_party_request_id: row.get("third_party_request_id"),
            queued_at,
            processing_started_at,
            retry_count,
            last_worker_id,
            last_attempt_id,
            failure_reason,
            ocr_stderr_summary,
            failure_context,
            last_error_code,
            slow_attachment_info_json,
            callback_url,
            callback_status,
            callback_attempts,
            callback_successes,
            callback_failures,
            last_callback_at,
            last_callback_status_code,
            last_callback_response,
            last_callback_error,
            callback_payload,
            next_callback_after,
        })
    }

    /// 状态枚举转字符串
    fn status_to_string(status: &PreviewStatus) -> &'static str {
        match status {
            PreviewStatus::Pending => "pending",
            PreviewStatus::Queued => "queued",
            PreviewStatus::Processing => "processing",
            PreviewStatus::Completed => "completed",
            PreviewStatus::Failed => "failed",
        }
    }

    /// 字符串转状态枚举
    fn string_to_status(status_str: &str) -> PreviewStatus {
        match status_str {
            "pending" => PreviewStatus::Pending,
            "queued" => PreviewStatus::Queued,
            "processing" => PreviewStatus::Processing,
            "completed" => PreviewStatus::Completed,
            "failed" => PreviewStatus::Failed,
            _ => PreviewStatus::Pending,
        }
    }
}

/// 材料文件记录查询操作
pub struct MaterialFileQueries;

impl MaterialFileQueries {
    pub async fn insert(pool: &SqlitePool, rec: &MaterialFileRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO preview_material_files (
                id, preview_id, material_code, attachment_name, source_url,
                stored_original_key, stored_processed_keys, mime_type, size_bytes,
                checksum_sha256, ocr_text_key, ocr_text_length, status, error_message,
                created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&rec.id)
        .bind(&rec.preview_id)
        .bind(&rec.material_code)
        .bind(&rec.attachment_name)
        .bind(&rec.source_url)
        .bind(&rec.stored_original_key)
        .bind(&rec.stored_processed_keys)
        .bind(&rec.mime_type)
        .bind(&rec.size_bytes)
        .bind(&rec.checksum_sha256)
        .bind(&rec.ocr_text_key)
        .bind(&rec.ocr_text_length)
        .bind(&rec.status)
        .bind(&rec.error_message)
        .bind(rec.created_at.to_rfc3339())
        .bind(rec.updated_at.to_rfc3339())
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn update_status(
        pool: &SqlitePool,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE preview_material_files
            SET status = ?, error_message = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(status)
        .bind(error.unwrap_or(""))
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn update_processing(
        pool: &SqlitePool,
        id: &str,
        processed_keys_json: Option<&str>,
        ocr_text_key: Option<&str>,
        ocr_text_length: Option<i64>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE preview_material_files
            SET stored_processed_keys = COALESCE(?, stored_processed_keys),
                ocr_text_key = COALESCE(?, ocr_text_key),
                ocr_text_length = COALESCE(?, ocr_text_length),
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(processed_keys_json)
        .bind(ocr_text_key)
        .bind(ocr_text_length)
        .bind(chrono::Utc::now().to_rfc3339())
        .bind(id)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn list(
        pool: &SqlitePool,
        filter: &MaterialFileFilter,
    ) -> Result<Vec<MaterialFileRecord>> {
        let mut query = String::from(
            "SELECT id, preview_id, material_code, attachment_name, source_url, stored_original_key, stored_processed_keys, mime_type, size_bytes, checksum_sha256, ocr_text_key, ocr_text_length, status, error_message, created_at, updated_at FROM preview_material_files WHERE 1=1"
        );
        let mut params: Vec<String> = Vec::new();
        if let Some(preview_id) = &filter.preview_id {
            query.push_str(" AND preview_id = ?");
            params.push(preview_id.clone());
        }
        if let Some(material_code) = &filter.material_code {
            query.push_str(" AND material_code = ?");
            params.push(material_code.clone());
        }
        query.push_str(" ORDER BY created_at ASC");

        let mut q = sqlx::query(&query);
        for p in params {
            q = q.bind(p);
        }
        let rows = q.fetch_all(pool).await?;
        let mut out = Vec::new();
        for row in rows {
            let created_at_str: String = row.get("created_at");
            let updated_at_str: String = row.get("updated_at");
            out.push(MaterialFileRecord {
                id: row.get("id"),
                preview_id: row.get("preview_id"),
                material_code: row.get("material_code"),
                attachment_name: row.get("attachment_name"),
                source_url: row.get("source_url"),
                stored_original_key: row.get("stored_original_key"),
                stored_processed_keys: row.get("stored_processed_keys"),
                mime_type: row.get("mime_type"),
                size_bytes: row.get("size_bytes"),
                checksum_sha256: row.get("checksum_sha256"),
                ocr_text_key: row.get("ocr_text_key"),
                ocr_text_length: row.get("ocr_text_length"),
                status: row.get("status"),
                error_message: row.get("error_message"),
                created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)?
                    .with_timezone(&Utc),
                updated_at: chrono::DateTime::parse_from_rfc3339(&updated_at_str)?
                    .with_timezone(&Utc),
            });
        }
        Ok(out)
    }
}

/// 事项规则配置查询操作
pub struct MatterRuleConfigQueries;

impl MatterRuleConfigQueries {
    pub async fn upsert(pool: &SqlitePool, config: &MatterRuleConfigRecord) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO matter_rule_configs (
                id, matter_id, matter_name, spec_version, mode, rule_payload,
                status, description, checksum, updated_by, created_at, updated_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(matter_id) DO UPDATE SET
                id = excluded.id,
                matter_name = excluded.matter_name,
                spec_version = excluded.spec_version,
                mode = excluded.mode,
                rule_payload = excluded.rule_payload,
                status = excluded.status,
                description = excluded.description,
                checksum = excluded.checksum,
                updated_by = excluded.updated_by,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(&config.id)
        .bind(&config.matter_id)
        .bind(&config.matter_name)
        .bind(&config.spec_version)
        .bind(&config.mode)
        .bind(&config.rule_payload)
        .bind(&config.status)
        .bind(&config.description)
        .bind(&config.checksum)
        .bind(&config.updated_by)
        .bind(config.created_at.to_rfc3339())
        .bind(config.updated_at.to_rfc3339())
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn get_by_matter_id(
        pool: &SqlitePool,
        matter_id: &str,
    ) -> Result<Option<MatterRuleConfigRecord>> {
        let row = sqlx::query(
            r#"
            SELECT id, matter_id, matter_name, spec_version, mode, rule_payload,
                   status, description, checksum, updated_by, created_at, updated_at
            FROM matter_rule_configs
            WHERE matter_id = ?
            "#,
        )
        .bind(matter_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(Self::row_to_record).transpose()?)
    }

    pub async fn list(
        pool: &SqlitePool,
        status: Option<&str>,
    ) -> Result<Vec<MatterRuleConfigRecord>> {
        let mut query = String::from(
            "SELECT id, matter_id, matter_name, spec_version, mode, rule_payload, status, description, checksum, updated_by, created_at, updated_at FROM matter_rule_configs WHERE 1=1",
        );
        let mut params: Vec<String> = Vec::new();

        if let Some(status) = status {
            query.push_str(" AND status = ?");
            params.push(status.to_string());
        }

        query.push_str(" ORDER BY updated_at DESC");

        let mut stmt = sqlx::query(&query);
        for p in params {
            stmt = stmt.bind(p);
        }

        let rows = stmt.fetch_all(pool).await?;
        rows.into_iter()
            .map(Self::row_to_record)
            .collect::<Result<Vec<_>>>()
    }

    fn row_to_record(row: sqlx::sqlite::SqliteRow) -> Result<MatterRuleConfigRecord> {
        let created_at: String = row.get("created_at");
        let updated_at: String = row.get("updated_at");
        Ok(MatterRuleConfigRecord {
            id: row.get("id"),
            matter_id: row.get("matter_id"),
            matter_name: row.try_get("matter_name").ok(),
            spec_version: row.get("spec_version"),
            mode: row.get("mode"),
            rule_payload: row.get("rule_payload"),
            status: row.get("status"),
            description: row.try_get("description").ok(),
            checksum: row.try_get("checksum").ok(),
            updated_by: row.try_get("updated_by").ok(),
            created_at: DateTime::parse_from_rfc3339(&created_at)?.with_timezone(&Utc),
            updated_at: DateTime::parse_from_rfc3339(&updated_at)?.with_timezone(&Utc),
        })
    }
}

/// 任务payload查询操作
pub struct TaskPayloadQueries;

impl TaskPayloadQueries {
    pub async fn upsert(pool: &SqlitePool, preview_id: &str, payload: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO preview_task_payloads (preview_id, payload, created_at, updated_at)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(preview_id) DO UPDATE SET payload = excluded.payload, updated_at = excluded.updated_at
            "#,
        )
        .bind(preview_id)
        .bind(payload)
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn load(pool: &SqlitePool, preview_id: &str) -> Result<Option<String>> {
        let row = sqlx::query(
            r#"
            SELECT payload FROM preview_task_payloads WHERE preview_id = ?
            "#,
        )
        .bind(preview_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.map(|r| r.get::<String, _>("payload")))
    }

    pub async fn delete(pool: &SqlitePool, preview_id: &str) -> Result<()> {
        sqlx::query(
            r#"
            DELETE FROM preview_task_payloads WHERE preview_id = ?
            "#,
        )
        .bind(preview_id)
        .execute(pool)
        .await?;
        Ok(())
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
    pub async fn get_stats_with_filter(
        pool: &SqlitePool,
        filter: &StatsFilter,
    ) -> Result<Vec<ApiStats>> {
        let mut query = String::from(
            "SELECT id, endpoint, method, client_id, user_id,
                    status_code, response_time_ms, request_size, response_size,
                    error_message, created_at
             FROM api_stats WHERE 1=1",
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
            "#,
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
            avg_response_time_ms: row
                .get::<Option<f64>, _>("avg_response_time_ms")
                .unwrap_or(0.0),
            total_request_size: row.get::<i64, _>("total_request_size") as u64,
            total_response_size: row.get::<i64, _>("total_response_size") as u64,
        })
    }

    /// 应用统计过滤条件
    fn apply_stats_filter_conditions(
        query: &mut String,
        bindings: &mut Vec<String>,
        filter: &StatsFilter,
    ) {
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

/// Outbox事件查询
pub struct OutboxQueries;

impl OutboxQueries {
    pub async fn insert(pool: &SqlitePool, event: &NewOutboxEvent) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            r#"
            INSERT INTO db_outbox (
                table_name, op_type, pk_value, idempotency_key, payload,
                created_at, applied_at, retries, last_error
            ) VALUES (?, ?, ?, ?, ?, ?, NULL, 0, NULL)
            ON CONFLICT(idempotency_key) DO NOTHING
            "#,
        )
        .bind(&event.table_name)
        .bind(&event.op_type)
        .bind(&event.pk_value)
        .bind(&event.idempotency_key)
        .bind(&event.payload)
        .bind(&now)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn fetch_pending(pool: &SqlitePool, limit: u32) -> Result<Vec<OutboxEvent>> {
        let rows = sqlx::query(
            r#"
            SELECT id, table_name, op_type, pk_value, idempotency_key, payload,
                   created_at, applied_at, retries, last_error
            FROM db_outbox
            WHERE applied_at IS NULL
            ORDER BY created_at ASC
            LIMIT ?
            "#,
        )
        .bind(limit as i64)
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(Self::row_to_event).collect()
    }

    pub async fn mark_applied(pool: &SqlitePool, event_id: &str) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let id_num = event_id
            .parse::<i64>()
            .map_err(|e| anyhow!("无效的Outbox事件ID: {}", e))?;
        sqlx::query(
            r#"
            UPDATE db_outbox
            SET applied_at = ?, last_error = NULL
            WHERE id = ?
            "#,
        )
        .bind(&now)
        .bind(id_num)
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(pool: &SqlitePool, event_id: &str, error: &str) -> Result<()> {
        let id_num = event_id
            .parse::<i64>()
            .map_err(|e| anyhow!("无效的Outbox事件ID: {}", e))?;
        sqlx::query(
            r#"
            UPDATE db_outbox
            SET retries = retries + 1,
                last_error = ?,
                applied_at = NULL
            WHERE id = ?
            "#,
        )
        .bind(error)
        .bind(id_num)
        .execute(pool)
        .await?;
        Ok(())
    }

    fn row_to_event(row: sqlx::sqlite::SqliteRow) -> Result<OutboxEvent> {
        let created_at_str: String = row.get("created_at");
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)?.with_timezone(&Utc);
        let applied_at = row
            .get::<Option<String>, _>("applied_at")
            .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(OutboxEvent {
            id: row.get::<i64, _>("id").to_string(),
            table_name: row.get("table_name"),
            op_type: row.get("op_type"),
            pk_value: row.get("pk_value"),
            idempotency_key: row.get("idempotency_key"),
            payload: row.get("payload"),
            created_at,
            applied_at,
            retries: row.get::<i64, _>("retries") as i32,
            last_error: row.get("last_error"),
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
