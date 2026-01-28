use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use once_cell::sync::OnceCell;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio::time::interval;

use crate::db::traits::PreviewCallbackUpdate;
use crate::util::http_client::HttpClient;
use crate::AppState;

#[derive(Clone)]
struct CallbackJob {
    preview_id: String,
}

struct DispatcherRuntime {
    database: Arc<dyn crate::db::Database>,
    http_client: Arc<HttpClient>,
    config: crate::util::config::Config,
    inflight: Arc<Mutex<HashSet<String>>>,
    retry_schedule: Vec<Duration>,
    max_attempts: i32,
    sender: mpsc::Sender<CallbackJob>,
}

impl DispatcherRuntime {
    fn new(
        app_state: &AppState,
        inflight: Arc<Mutex<HashSet<String>>>,
        sender: mpsc::Sender<CallbackJob>,
    ) -> Self {
        Self {
            database: Arc::clone(&app_state.database),
            http_client: Arc::clone(&app_state.http_client),
            config: app_state.config.clone(),
            inflight,
            retry_schedule: vec![
                Duration::from_secs(60),
                Duration::from_secs(5 * 60),
                Duration::from_secs(15 * 60),
                Duration::from_secs(60 * 60),
                Duration::from_secs(3 * 60 * 60),
            ],
            max_attempts: 5,
            sender,
        }
    }

    fn spawn(self: Arc<Self>, mut receiver: mpsc::Receiver<CallbackJob>) {
        tokio::spawn(async move {
            while let Some(job) = receiver.recv().await {
                let preview_id = job.preview_id.clone();
                if let Err(err) = self.process_job(&job.preview_id).await {
                    tracing::error!(
                        preview_id = %job.preview_id,
                        error = %err,
                        "第三方回调处理失败"
                    );
                }
                self.release_inflight(&preview_id);
            }
        });
    }

    fn spawn_scanner(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(60));
            loop {
                ticker.tick().await;
                if let Err(err) = self.scan_due_callbacks().await {
                    tracing::warn!(error = %err, "扫描第三方回调任务失败");
                }
            }
        });
    }

    async fn scan_due_callbacks(&self) -> Result<()> {
        let records = self.database.list_due_callbacks(50).await?;
        for record in records {
            self.enqueue_job(record.id.clone()).await?;
        }
        Ok(())
    }

    fn release_inflight(&self, preview_id: &str) {
        if let Ok(mut guard) = self.inflight.lock() {
            guard.remove(preview_id);
        }
    }

    fn mark_inflight(&self, preview_id: &str) -> bool {
        match self.inflight.lock() {
            Ok(mut guard) => guard.insert(preview_id.to_string()),
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.insert(preview_id.to_string())
            }
        }
    }

    async fn enqueue_job(&self, preview_id: String) -> Result<()> {
        if !self.mark_inflight(&preview_id) {
            return Ok(());
        }
        if let Err(err) = self
            .sender
            .send(CallbackJob {
                preview_id: preview_id.clone(),
            })
            .await
        {
            self.release_inflight(&preview_id);
            return Err(anyhow!("发送第三方回调任务失败: {}", err));
        }
        Ok(())
    }

    async fn process_job(&self, preview_id: &str) -> Result<()> {
        let record = match self.database.get_preview_record(preview_id).await? {
            Some(record) => record,
            None => {
                tracing::warn!(preview_id = %preview_id, "回调任务对应的预审记录不存在");
                return Ok(());
            }
        };

        let callback_url = record
            .callback_url
            .clone()
            .or_else(|| self.config.third_party_callback_url());

        let callback_url = match callback_url {
            Some(url) if !url.is_empty() => url,
            _ => {
                tracing::debug!(preview_id = %preview_id, "未配置第三方回调URL,跳过");
                return Ok(());
            }
        };

        let payload_str = match record.callback_payload.as_ref() {
            Some(payload) if !payload.is_empty() => payload,
            _ => {
                tracing::warn!(preview_id = %preview_id, "第三方回调payload缺失,跳过");
                self.database
                    .update_preview_callback_state(&PreviewCallbackUpdate {
                        preview_id: preview_id.to_string(),
                        callback_status: Some(Some("failed".to_string())),
                        last_callback_error: Some(Some(
                            "缺少回调payload,无法通知第三方".to_string(),
                        )),
                        next_callback_after: Some(None),
                        ..Default::default()
                    })
                    .await?;
                return Ok(());
            }
        };

        let payload_value: Value = serde_json::from_str(payload_str)
            .map_err(|err| anyhow!("解析回调payload失败: {}", err))?;

        #[cfg(not(feature = "reqwest"))]
        {
            let _ = &payload_value;
            tracing::warn!(preview_id = %preview_id, "reqwest 功能未启用,无法执行第三方回调");
            self.database
                .update_preview_callback_state(&PreviewCallbackUpdate {
                    preview_id: preview_id.to_string(),
                    callback_status: Some(Some("failed".to_string())),
                    last_callback_error: Some(Some("HTTP客户端不可用,无法发送回调".to_string())),
                    callback_failures: Some(record.callback_failures + 1),
                    callback_attempts: Some(record.callback_attempts + 1),
                    next_callback_after: Some(None),
                    ..Default::default()
                })
                .await?;
            return Ok(());
        }

        #[cfg(feature = "reqwest")]
        {
            use tokio::time::timeout;

            let client = self
                .http_client
                .reqwest_client()
                .context("HTTP客户端不可用")?;

            let attempt = record.callback_attempts + 1;

            let start = std::time::Instant::now();
            let response_result = timeout(
                Duration::from_secs(30),
                client
                    .post(&callback_url)
                    .header("Content-Type", "application/json")
                    .json(&payload_value)
                    .send(),
            )
            .await;

            let mut update = PreviewCallbackUpdate {
                preview_id: preview_id.to_string(),
                callback_attempts: Some(attempt),
                last_callback_at: Some(Some(Utc::now())),
                callback_url: Some(Some(callback_url.clone())),
                ..Default::default()
            };

            match response_result {
                Err(_) => {
                    let error_msg = "回调请求超时".to_string();
                    update.last_callback_error = Some(Some(error_msg.clone()));
                    update.callback_failures = Some(record.callback_failures + 1);
                    update.callback_status = Some(Some(self.next_status(attempt)));
                    update.last_callback_response = Some(None);
                    update.last_callback_status_code = Some(None);
                    update.next_callback_after = self.next_retry(attempt);
                    self.database.update_preview_callback_state(&update).await?;
                    return Ok(());
                }
                Ok(Err(err)) => {
                    let err_text = truncate_string(&err.to_string(), 4096);
                    update.last_callback_error = Some(Some(err_text.clone()));
                    update.callback_failures = Some(record.callback_failures + 1);
                    update.callback_status = Some(Some(self.next_status(attempt)));
                    update.last_callback_response = Some(None);
                    update.last_callback_status_code = Some(None);
                    update.next_callback_after = self.next_retry(attempt);
                    tracing::warn!(
                        preview_id = %preview_id,
                        attempt = attempt,
                        error = %err_text,
                        "第三方回调请求失败"
                    );
                    self.database.update_preview_callback_state(&update).await?;
                    return Ok(());
                }
                Ok(Ok(response)) => {
                    let status_code = response.status();
                    let text = response
                        .text()
                        .await
                        .unwrap_or_else(|_| "<no-body>".to_string());
                    let truncated_body = truncate_string(&text, 8192);

                    update.last_callback_status_code = Some(Some(status_code.as_u16() as i32));
                    update.last_callback_response = Some(Some(truncated_body.clone()));
                    update.last_callback_error = Some(None);

                    if status_code.is_success() {
                        update.callback_successes = Some(record.callback_successes + 1);
                        update.callback_status = Some(Some("success".to_string()));
                        update.next_callback_after = Some(None);
                        tracing::info!(
                            preview_id = %preview_id,
                            attempt = attempt,
                            elapsed_ms = start.elapsed().as_millis(),
                            status = %status_code,
                            "第三方回调成功"
                        );
                    } else {
                        update.callback_failures = Some(record.callback_failures + 1);
                        update.callback_status = Some(Some(self.next_status(attempt)));
                        update.next_callback_after = self.next_retry(attempt);
                        update.last_callback_error =
                            Some(Some(format!("状态码 {}: {}", status_code, truncated_body)));
                        tracing::warn!(
                            preview_id = %preview_id,
                            attempt = attempt,
                            status = %status_code,
                            "第三方回调返回非成功状态"
                        );
                    }

                    self.database.update_preview_callback_state(&update).await?;
                }
            }
        }

        Ok(())
    }

    fn next_retry(&self, attempt: i32) -> Option<Option<chrono::DateTime<Utc>>> {
        if attempt >= self.max_attempts {
            return Some(None);
        }
        let idx = (attempt - 1).clamp(0, (self.retry_schedule.len() - 1) as i32) as usize;
        Some(Some(
            Utc::now()
                + chrono::Duration::from_std(self.retry_schedule[idx])
                    .unwrap_or_else(|_| chrono::Duration::seconds(60)),
        ))
    }

    fn next_status(&self, attempt: i32) -> String {
        if attempt >= self.max_attempts {
            "failed".to_string()
        } else {
            "retrying".to_string()
        }
    }
}

struct CallbackDispatcherHandle {
    sender: mpsc::Sender<CallbackJob>,
    runtime: Arc<DispatcherRuntime>,
    inflight: Arc<Mutex<HashSet<String>>>,
}

impl CallbackDispatcherHandle {
    fn new(app_state: &AppState) -> Self {
        let (sender, receiver) = mpsc::channel(256);
        let inflight = Arc::new(Mutex::new(HashSet::new()));
        let runtime = Arc::new(DispatcherRuntime::new(
            app_state,
            Arc::clone(&inflight),
            sender.clone(),
        ));
        runtime.clone().spawn(receiver);
        runtime.clone().spawn_scanner();

        Self {
            sender,
            runtime,
            inflight,
        }
    }

    async fn record_and_enqueue(
        &self,
        preview_id: String,
        callback_url: String,
        payload: Value,
        reset_attempts: bool,
    ) -> Result<()> {
        let payload_string = serde_json::to_string(&payload)?;
        let mut update = PreviewCallbackUpdate {
            preview_id: preview_id.clone(),
            callback_url: Some(Some(callback_url)),
            callback_status: if reset_attempts {
                Some(Some("scheduled".to_string()))
            } else {
                None
            },
            callback_payload: Some(Some(payload_string)),
            next_callback_after: Some(Some(Utc::now())),
            last_callback_response: Some(None),
            last_callback_error: Some(None),
            last_callback_status_code: Some(None),
            ..Default::default()
        };

        if reset_attempts {
            update.callback_attempts = Some(0);
            update.callback_successes = Some(0);
            update.callback_failures = Some(0);
        }

        self.runtime
            .database
            .update_preview_callback_state(&update)
            .await?;

        self.runtime.enqueue_job(preview_id).await?;
        Ok(())
    }
}

static DISPATCHER: OnceCell<CallbackDispatcherHandle> = OnceCell::new();

/// 初始化第三方回调调度器
pub fn initialize(app_state: &AppState) {
    DISPATCHER.get_or_init(|| CallbackDispatcherHandle::new(app_state));
}

/// 记录并调度第三方回调任务
pub async fn schedule_callback(
    preview_id: String,
    callback_url: String,
    payload: Value,
    reset_attempts: bool,
) -> Result<()> {
    let dispatcher = DISPATCHER
        .get()
        .ok_or_else(|| anyhow!("第三方回调调度器尚未初始化"))?;
    dispatcher
        .record_and_enqueue(preview_id, callback_url, payload, reset_attempts)
        .await
}

pub fn build_default_download_url(preview_id: &str) -> Option<String> {
    Some(format!(
        "{}/api/preview/download/{}",
        crate::CONFIG.base_url().trim_end_matches('/'),
        preview_id
    ))
}

fn truncate_string(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        value.to_string()
    } else {
        let end = max_len.saturating_sub(3);
        if end == 0 {
            "...".to_string()
        } else {
            format!("{}...", &value[..end])
        }
    }
}
