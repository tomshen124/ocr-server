use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::api::worker_proxy;
use crate::db::{PreviewFilter, PreviewStatus};
use crate::util::config::types::ProcessingWatchdogConfig;
use crate::util::task_queue::{PreviewTask, TaskQueue, PREVIEW_QUEUE_NAME};
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;
use crate::AppState;

/// 启动处理超时任务的看门狗
pub fn spawn_processing_watchdog(app_state: &AppState, cfg: &ProcessingWatchdogConfig) {
    if !cfg.enabled {
        info!("Processing watchdog 已禁用");
        return;
    }

    let interval_duration = Duration::from_secs(cfg.interval_secs.max(10));
    let timeout = chrono::Duration::seconds(cfg.timeout_secs.max(60) as i64);
    let max_retries = cfg.max_retries as i32;
    let worker_grace = chrono::Duration::seconds(cfg.worker_grace_secs.min(i64::MAX as u64) as i64);

    let database = Arc::clone(&app_state.database);
    let task_queue = Arc::clone(&app_state.task_queue);

    tokio::spawn(async move {
        let mut ticker = interval(interval_duration);
        ticker.tick().await; // 跳过首次延迟
        loop {
            ticker.tick().await;
            if let Err(err) = check_processing_timeouts(
                &database,
                &task_queue,
                timeout,
                max_retries,
                worker_grace,
            )
            .await
            {
                warn!(error = %err, "Processing watchdog 执行失败");
            }
        }
    });
}

async fn check_processing_timeouts(
    database: &Arc<dyn crate::db::Database>,
    task_queue: &Arc<dyn TaskQueue>,
    timeout: chrono::Duration,
    max_retries: i32,
    worker_grace: chrono::Duration,
) -> Result<()> {
    let mut filter = PreviewFilter::default();
    filter.status = Some(PreviewStatus::Processing);
    filter.limit = Some(500);

    let now = Utc::now();
    let records = database.list_preview_records(&filter).await?;

    for record in records {
        let request_key = derive_request_key(&record);
        let started = record.processing_started_at.unwrap_or(record.updated_at);
        if now.signed_duration_since(started) <= timeout {
            continue;
        }

        if let Some(worker_id) = record
            .last_worker_id
            .as_deref()
            .filter(|_| worker_grace > chrono::Duration::zero())
            .filter(|id| !id.eq_ignore_ascii_case("master"))
        {
            if let Some(info) = worker_proxy::get_worker_heartbeat_info(worker_id).await {
                if info.seconds_since <= worker_grace.num_seconds() {
                    debug!(
                        preview_id = %record.id,
                        worker_id = %worker_id,
                        elapsed_secs = %now.signed_duration_since(started).num_seconds(),
                        seconds_since_heartbeat = info.seconds_since,
                        grace_secs = worker_grace.num_seconds(),
                        "Worker 心跳仍活跃，暂不触发 Processing watchdog 回收"
                    );
                    continue;
                } else {
                    info!(
                        preview_id = %record.id,
                        worker_id = %worker_id,
                        seconds_since_heartbeat = info.seconds_since,
                        grace_secs = worker_grace.num_seconds(),
                        "Worker 心跳超过宽限期，触发回收"
                    );
                }
            } else {
                info!(
                    preview_id = %record.id,
                    worker_id = %worker_id,
                    grace_secs = worker_grace.num_seconds(),
                    "未找到 Worker 心跳信息，视为离线，触发回收"
                );
            }
        }

        let age = now.signed_duration_since(started);
        if record.retry_count >= max_retries {
            warn!(
                preview_id = %record.id,
                retry_count = record.retry_count,
                started_at = %format_datetime(record.processing_started_at),
                elapsed_secs = age.num_seconds(),
                "Processing 任务超时并达到最大重试次数，标记为 failed"
            );
            let status = PreviewStatus::Failed;
            match database
                .update_preview_status(&record.id, status.clone())
                .await
            {
                Ok(_) => {
                    if let Err(err) = database
                        .update_preview_request_latest(&request_key, Some(&record.id), Some(status))
                        .await
                    {
                        warn!(preview_id = %record.id, error = %err, "同步预审请求状态失败");
                    }
                    METRICS_COLLECTOR
                        .record_preview_persistence_failure("processing_watchdog_failed");
                }
                Err(err) => {
                    warn!(preview_id = %record.id, error = %err, "标记任务失败时出错");
                }
            }
            let _ = database.delete_task_payload(&record.id).await;
            continue;
        }

        match database.load_task_payload(&record.id).await? {
            Some(payload_str) => match serde_json::from_str::<PreviewTask>(&payload_str) {
                Ok(task) => {
                    info!(
                        preview_id = %record.id,
                        retry_count = record.retry_count,
                        elapsed_secs = age.num_seconds(),
                        "Processing 任务超时，将重新入队"
                    );
                    if let Err(err) = task_queue.enqueue(task).await {
                        error!(
                            preview_id = %record.id,
                            error = %err,
                            "超时任务重新入队失败"
                        );
                        let status = PreviewStatus::Failed;
                        match database
                            .update_preview_status(&record.id, status.clone())
                            .await
                        {
                            Ok(_) => {
                                if let Err(update_err) = database
                                    .update_preview_request_latest(
                                        &request_key,
                                        Some(&record.id),
                                        Some(status),
                                    )
                                    .await
                                {
                                    warn!(preview_id = %record.id, error = %update_err, "同步预审请求状态失败");
                                }
                            }
                            Err(update_err) => {
                                warn!(preview_id = %record.id, error = %update_err, "更新任务状态失败");
                            }
                        }
                        METRICS_COLLECTOR
                            .record_preview_persistence_failure("processing_watchdog_requeue_fail");
                        let _ = database.delete_task_payload(&record.id).await;
                    } else {
                        METRICS_COLLECTOR.record_queue_retry(PREVIEW_QUEUE_NAME);
                        let status = PreviewStatus::Queued;
                        match database
                            .update_preview_status(&record.id, status.clone())
                            .await
                        {
                            Ok(_) => {
                                if let Err(update_err) = database
                                    .update_preview_request_latest(
                                        &request_key,
                                        Some(&record.id),
                                        Some(status),
                                    )
                                    .await
                                {
                                    warn!(
                                        preview_id = %record.id,
                                        error = %update_err,
                                        "同步预审请求状态失败"
                                    );
                                }
                            }
                            Err(update_err) => {
                                warn!(
                                    preview_id = %record.id,
                                    error = %update_err,
                                    "重新入队后更新状态失败"
                                );
                            }
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        preview_id = %record.id,
                        error = %err,
                        "任务payload解析失败，标记为 failed"
                    );
                    let status = PreviewStatus::Failed;
                    match database
                        .update_preview_status(&record.id, status.clone())
                        .await
                    {
                        Ok(_) => {
                            if let Err(update_err) = database
                                .update_preview_request_latest(
                                    &request_key,
                                    Some(&record.id),
                                    Some(status),
                                )
                                .await
                            {
                                warn!(preview_id = %record.id, error = %update_err, "同步预审请求状态失败");
                            }
                        }
                        Err(update_err) => {
                            warn!(preview_id = %record.id, error = %update_err, "更新任务状态失败");
                        }
                    }
                    METRICS_COLLECTOR
                        .record_preview_persistence_failure("processing_watchdog_payload_parse");
                    let _ = database.delete_task_payload(&record.id).await;
                }
            },
            None => {
                warn!(
                    preview_id = %record.id,
                    elapsed_secs = age.num_seconds(),
                    "任务缺少payload，无法重新入队，标记为 failed"
                );
                let status = PreviewStatus::Failed;
                match database
                    .update_preview_status(&record.id, status.clone())
                    .await
                {
                    Ok(_) => {
                        if let Err(update_err) = database
                            .update_preview_request_latest(
                                &request_key,
                                Some(&record.id),
                                Some(status),
                            )
                            .await
                        {
                            warn!(preview_id = %record.id, error = %update_err, "同步预审请求状态失败");
                        }
                    }
                    Err(update_err) => {
                        warn!(preview_id = %record.id, error = %update_err, "更新任务状态失败");
                    }
                }
                METRICS_COLLECTOR
                    .record_preview_persistence_failure("processing_watchdog_missing_payload");
                let _ = database.delete_task_payload(&record.id).await;
            }
        }
    }

    Ok(())
}

fn format_datetime(dt: Option<DateTime<Utc>>) -> String {
    dt.map(|d| d.to_rfc3339())
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn derive_request_key(record: &crate::db::PreviewRecord) -> String {
    record
        .third_party_request_id
        .as_deref()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .unwrap_or(&record.id)
        .to_string()
}
