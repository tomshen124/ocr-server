use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use chrono::{Duration as ChronoDuration, Timelike, Utc};
use tokio::time::interval;
use tracing::{debug, info, warn};

use crate::db::traits::{
    CachedMaterialFilter, CachedMaterialRecord, CachedMaterialStatus, Database,
};
use crate::storage::Storage;
use crate::util::config::types::DeploymentRole;
use crate::AppState;

const CACHE_UPLOAD_INTERVAL_SECS: u64 = 30;
const CACHE_UPLOAD_BATCH: u32 = 5;
const CACHE_CLEANUP_INTERVAL_SECS: u64 = 600;
const CACHE_CLEANUP_MIN_AGE_HOURS: i64 = 24;
// 每天 0 点附近执行一次兜底清理（仅清理达到保留期的已上传/失败记录）
const CACHE_CLEANUP_CRON_HOUR_UTC: u32 = 16; // 0 点北京时间 = 16 UTC
const CACHE_CLEANUP_CRON_MINUTE: u32 = 0;

/// 启动材料缓存后台管理任务（仅 Master）
pub fn spawn_material_cache_manager(app_state: &AppState) {
    if app_state.config.deployment.role != DeploymentRole::Master {
        return;
    }

    let upload_db = Arc::clone(&app_state.database);
    let upload_storage = Arc::clone(&app_state.storage);
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(CACHE_UPLOAD_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            if let Err(err) =
                process_pending_uploads(upload_db.clone(), upload_storage.clone()).await
            {
                warn!(error = %err, "材料缓存上传任务失败");
            }
        }
    });

    let cleanup_db = Arc::clone(&app_state.database);
    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(CACHE_CLEANUP_INTERVAL_SECS));
        loop {
            ticker.tick().await;
            let now = Utc::now();
            let is_cron_window = now.hour() == CACHE_CLEANUP_CRON_HOUR_UTC
                && now.minute() == CACHE_CLEANUP_CRON_MINUTE;

            if is_cron_window {
                if let Err(err) = cleanup_stale_cache(cleanup_db.clone()).await {
                    warn!(error = %err, "材料缓存定时清理任务失败");
                }
            } else {
                // 非指定时间不清理，避免误删
                debug!("跳过非清理时段的材料缓存清理任务");
            }
        }
    });
}

async fn process_pending_uploads(
    database: Arc<dyn Database>,
    storage: Arc<dyn Storage>,
) -> Result<()> {
    let mut filter = CachedMaterialFilter::default();
    filter.status = Some(CachedMaterialStatus::Downloaded);
    filter.limit = Some(CACHE_UPLOAD_BATCH);

    let records = database.list_cached_material_records(&filter).await?;
    for record in records {
        if let Err(err) = upload_single(&database, &storage, &record).await {
            warn!(
                preview_id = %record.preview_id,
                material_code = %record.material_code,
                attachment_index = record.attachment_index,
                error = %err,
                "材料缓存上传失败"
            );
        }
    }
    Ok(())
}

async fn upload_single(
    database: &Arc<dyn Database>,
    storage: &Arc<dyn Storage>,
    record: &CachedMaterialRecord,
) -> Result<()> {
    database
        .update_cached_material_status(
            &record.id,
            CachedMaterialStatus::Uploading,
            record.oss_key.as_deref(),
            None,
        )
        .await?;

    let path = Path::new(&record.local_path);
    let data = match tokio::fs::read(path).await {
        Ok(data) => data,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            // 尝试利用已上传的 OSS 记录修复
            if let Some(key) = record.oss_key.as_deref() {
                match storage.get(key).await {
                    Ok(Some(bytes)) => {
                        if let Some(parent) = path.parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }
                        let _ = tokio::fs::write(path, &bytes).await;
                        database
                            .update_cached_material_status(
                                &record.id,
                                CachedMaterialStatus::Uploaded,
                                record.oss_key.as_deref(),
                                None,
                            )
                            .await?;
                        return Ok(());
                    }
                    Ok(None) => {
                        let msg = "OSS对象不存在且本地缓存缺失";
                        database
                            .update_cached_material_status(
                                &record.id,
                                CachedMaterialStatus::Failed,
                                record.oss_key.as_deref(),
                                Some(msg),
                            )
                            .await?;
                        return Err(anyhow!(msg));
                    }
                    Err(err) => {
                        let msg = format!("读取OSS失败: {}", err);
                        database
                            .update_cached_material_status(
                                &record.id,
                                CachedMaterialStatus::Failed,
                                record.oss_key.as_deref(),
                                Some(msg.as_str()),
                            )
                            .await?;
                        return Err(anyhow!(msg));
                    }
                }
            }

            warn!(
                preview_id = %record.preview_id,
                material_code = %record.material_code,
                attachment_index = record.attachment_index,
                path = %record.local_path,
                "材料缓存文件不存在，可能已被清理或任务失败回滚"
            );
            database
                .update_cached_material_status(
                    &record.id,
                    CachedMaterialStatus::Failed,
                    record.oss_key.as_deref(),
                    Some("File not found (cleanup/rollback)"),
                )
                .await?;
            return Ok(());
        }
        Err(e) => {
            return Err(anyhow!(
                "读取材料缓存文件失败: {}:{} (material={}, attachment_index={})",
                record.local_path,
                e,
                record.material_code,
                record.attachment_index
            ))
        }
    };

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}_{}.bin", record.material_code, record.attachment_index));
    let oss_key = format!("cache/{}/{}", record.preview_id, file_name);

    if let Err(err) = storage.put(&oss_key, &data).await {
        let message = truncate_error(&err);
        database
            .update_cached_material_status(
                &record.id,
                CachedMaterialStatus::Failed,
                record.oss_key.as_deref(),
                Some(message.as_str()),
            )
            .await?;
        return Err(anyhow!(err));
    }

    database
        .update_cached_material_status(
            &record.id,
            CachedMaterialStatus::Uploaded,
            Some(&oss_key),
            None,
        )
        .await?;

    info!(
        preview_id = %record.preview_id,
        material_code = %record.material_code,
        attachment_index = record.attachment_index,
        oss_key = %oss_key,
        "材料缓存已上传"
    );

    Ok(())
}

async fn cleanup_stale_cache(database: Arc<dyn Database>) -> Result<()> {
    let mut total_cleaned = 0usize;
    for status in [CachedMaterialStatus::Uploaded, CachedMaterialStatus::Failed] {
        let mut filter = CachedMaterialFilter::default();
        filter.status = Some(status.clone());
        filter.limit = Some(CACHE_UPLOAD_BATCH);

        let records = database.list_cached_material_records(&filter).await?;
        let threshold = Utc::now() - ChronoDuration::hours(CACHE_CLEANUP_MIN_AGE_HOURS);

        for record in records {
            if record.updated_at > threshold {
                continue;
            }

            if let Err(err) = tokio::fs::remove_file(&record.local_path).await {
                if err.kind() != ErrorKind::NotFound {
                    warn!(
                        preview_id = %record.preview_id,
                        material_code = %record.material_code,
                        attachment_index = record.attachment_index,
                        path = %record.local_path,
                        error = %err,
                        "删除本地材料缓存失败"
                    );
                    continue;
                }
            }

            database
                .update_cached_material_status(
                    &record.id,
                    CachedMaterialStatus::Cleaned,
                    record.oss_key.as_deref(),
                    None,
                )
                .await?;

            total_cleaned += 1;
            info!(
                preview_id = %record.preview_id,
                material_code = %record.material_code,
                attachment_index = record.attachment_index,
                status = ?status,
                "材料缓存已清理"
            );
        }
    }

    if total_cleaned > 0 {
        info!(cleaned = total_cleaned, "材料缓存清理完成");
    }

    Ok(())
}

fn truncate_error(err: &dyn std::fmt::Display) -> String {
    let message = err.to_string();
    const MAX_LEN: usize = 512;
    if message.len() > MAX_LEN {
        format!("{}…", &message[..MAX_LEN])
    } else {
        message
    }
}
