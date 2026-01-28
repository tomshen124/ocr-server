use std::env;
use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::Utc;
use tracing::{error, info, warn};

use ocr_server::db::{PreviewFilter, PreviewStatus};
use ocr_server::server::{ConfigManager, DatabaseInitializer};
use ocr_server::util::task_queue::{initialize_task_queue, PreviewTask, PreviewTaskHandler};

struct NoopHandler;

#[async_trait::async_trait]
impl PreviewTaskHandler for NoopHandler {
    async fn handle_preview_task(&self, _task: PreviewTask) -> Result<()> {
        Ok(())
    }
}

fn parse_env(name: &str, default: u32) -> u32 {
    env::var(name)
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|&v| v > 0)
        .unwrap_or(default)
}

fn derive_request_key(record: &ocr_server::db::PreviewRecord) -> String {
    record
        .third_party_request_id
        .as_deref()
        .map(|id| id.trim())
        .filter(|id| !id.is_empty())
        .unwrap_or(&record.id)
        .to_string()
}

#[tokio::main]
async fn main() -> Result<()> {
    let batch_size = parse_env("REQUEUE_BATCH", 10);

    let (config, validation) = ConfigManager::load_and_validate().context("加载配置文件失败")?;

    if validation.has_errors() {
        return Err(anyhow::anyhow!(
            "配置校验失败，共 {} 个错误，请先修复配置",
            validation.error_count()
        ));
    }

    let _guard = ConfigManager::initialize_logging(&config).context("初始化日志系统失败")?;

    ocr_server::initialize_globals();

    let database = DatabaseInitializer::create_from_config(&config)
        .await
        .context("初始化数据库失败")?;

    let handler: Arc<dyn PreviewTaskHandler> = Arc::new(NoopHandler);

    let task_queue = initialize_task_queue(config.distributed.enabled, &config.task_queue, handler)
        .await
        .context("初始化任务队列失败")?;

    let mut filter = PreviewFilter::default();
    filter.status = Some(PreviewStatus::Queued);
    filter.limit = Some(batch_size);

    let mut records = database
        .list_preview_records(&filter)
        .await
        .context("查询 queued 任务失败")?;

    if records.is_empty() {
        info!("没有需要重新入队的 queued 任务");
        return Ok(());
    }

    records.sort_by_key(|r| r.queued_at.unwrap_or(r.updated_at));
    records.truncate(batch_size as usize);

    info!(
        "准备重新入队 {} 个任务 (当前批次上限 {})",
        records.len(),
        batch_size
    );

    let mut success = 0_u32;

    for record in records {
        let request_key = derive_request_key(&record);
        let preview_id = record.id.clone();
        match database.load_task_payload(&preview_id).await {
            Ok(Some(payload_str)) => match serde_json::from_str::<PreviewTask>(&payload_str) {
                Ok(task) => {
                    info!(preview_id = %preview_id, "开始重新入队");
                    if let Err(err) = task_queue.enqueue(task).await {
                        error!(
                            preview_id = %preview_id,
                            error = %err,
                            "重新入队失败"
                        );
                        continue;
                    }

                    let status = PreviewStatus::Queued;
                    match database
                        .update_preview_status(&preview_id, status.clone())
                        .await
                    {
                        Ok(_) => {
                            if let Err(err) = database
                                .update_preview_request_latest(
                                    &request_key,
                                    Some(&preview_id),
                                    Some(status),
                                )
                                .await
                            {
                                warn!(
                                    preview_id = %preview_id,
                                    error = %err,
                                    "同步预审请求状态失败"
                                );
                            }
                            success += 1;
                            info!(
                                preview_id = %preview_id,
                                queued_at = %Utc::now().to_rfc3339(),
                                "任务已重新入队"
                            );
                        }
                        Err(err) => {
                            warn!(
                                preview_id = %preview_id,
                                error = %err,
                                "入队成功但刷新 queued_at 失败"
                            );
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        preview_id = %preview_id,
                        error = %err,
                        "payload 解析失败，将任务标记为 failed"
                    );
                    let status = PreviewStatus::Failed;
                    if let Err(update_err) = database
                        .update_preview_status(&preview_id, status.clone())
                        .await
                    {
                        warn!(
                            preview_id = %preview_id,
                            error = %update_err,
                            "标记任务失败时出错"
                        );
                    } else if let Err(err) = database
                        .update_preview_request_latest(
                            &request_key,
                            Some(&preview_id),
                            Some(status),
                        )
                        .await
                    {
                        warn!(
                            preview_id = %preview_id,
                            error = %err,
                            "同步预审请求状态失败"
                        );
                    }
                }
            },
            Ok(None) => {
                warn!(
                    preview_id = %preview_id,
                    "找不到 payload，标记任务为 failed"
                );
                let status = PreviewStatus::Failed;
                if let Err(update_err) = database
                    .update_preview_status(&preview_id, status.clone())
                    .await
                {
                    warn!(
                        preview_id = %preview_id,
                        error = %update_err,
                        "标记任务失败时出错"
                    );
                } else if let Err(err) = database
                    .update_preview_request_latest(&request_key, Some(&preview_id), Some(status))
                    .await
                {
                    warn!(
                        preview_id = %preview_id,
                        error = %err,
                        "同步预审请求状态失败"
                    );
                }
            }
            Err(err) => {
                error!(
                    preview_id = %preview_id,
                    error = %err,
                    "读取 payload 失败"
                );
            }
        }
    }

    info!("本次批量处理完成，成功重新入队 {} 个任务", success);

    Ok(())
}
