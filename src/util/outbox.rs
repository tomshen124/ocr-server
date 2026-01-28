use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::time::interval;

use crate::db::traits::{NewOutboxEvent, OutboxEvent};
use crate::util::callbacks;
use crate::util::config::types::OutboxConfig;
use crate::AppState;

static OUTBOX_RUNTIME: OnceCell<Arc<OutboxManager>> = OnceCell::new();

/// 初始化 Outbox 运行时
pub fn initialize(app_state: &AppState) {
    if !app_state.config.outbox.enabled {
        tracing::info!("[outbox] Outbox 已禁用，跳过初始化");
        return;
    }

    let runtime = Arc::new(OutboxManager::new(app_state));
    runtime.clone().spawn_worker();
    if OUTBOX_RUNTIME.set(runtime).is_err() {
        tracing::warn!("[outbox] Outbox 运行时已存在，跳过重复初始化");
    }
}

/// 将第三方回调写入 Outbox
pub async fn enqueue_third_party_callback_event(
    preview_id: &str,
    callback_url: &str,
    payload: Value,
    reset_attempts: bool,
) -> Result<()> {
    let runtime = match OUTBOX_RUNTIME.get() {
        Some(runtime) => runtime,
        None => {
            tracing::warn!(
                target: "outbox",
                "Outbox未初始化，降级为直接调度回调"
            );
            return callbacks::schedule_callback(
                preview_id.to_string(),
                callback_url.to_string(),
                payload,
                reset_attempts,
            )
            .await;
        }
    };

    let envelope = OutboxEnvelope::ThirdPartyCallback(ThirdPartyCallbackPayload {
        preview_id: preview_id.to_string(),
        callback_url: callback_url.to_string(),
        payload,
        reset_attempts,
    });

    runtime
        .enqueue_event("preview_records", "callback_dispatch", preview_id, envelope)
        .await
}

struct OutboxManager {
    database: Arc<dyn crate::db::Database>,
    config: OutboxConfig,
}

impl OutboxManager {
    fn new(app_state: &AppState) -> Self {
        Self {
            database: Arc::clone(&app_state.database),
            config: app_state.config.outbox.clone(),
        }
    }

    fn poll_interval(&self) -> Duration {
        let secs = self.config.poll_interval_secs.max(1);
        Duration::from_secs(secs)
    }

    fn batch_size(&self) -> u32 {
        self.config.batch_size.max(1)
    }

    fn max_retries(&self) -> u32 {
        self.config.max_retries
    }

    fn error_limit(&self) -> usize {
        self.config.max_error_len.max(32)
    }

    fn spawn_worker(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut ticker = interval(self.poll_interval());
            loop {
                ticker.tick().await;
                if let Err(err) = self.process_batch().await {
                    tracing::warn!(target: "outbox", error = %err, "Outbox 批处理失败");
                }
            }
        });
    }

    async fn process_batch(&self) -> Result<()> {
        let events = self
            .database
            .fetch_pending_outbox_events(self.batch_size())
            .await?;
        for event in events {
            if event.retries as u32 >= self.max_retries() {
                tracing::error!(
                    target: "outbox",
                    event_id = %event.id,
                    retries = event.retries,
                    "Outbox事件超过最大重试次数，标记为完成"
                );
                self.database.mark_outbox_event_applied(&event.id).await?;
                continue;
            }

            match self.handle_event(&event).await {
                Ok(_) => {
                    self.database.mark_outbox_event_applied(&event.id).await?;
                }
                Err(err) => {
                    let mut err_msg = err.to_string();
                    if err_msg.len() > self.error_limit() {
                        err_msg.truncate(self.error_limit());
                    }
                    tracing::warn!(
                        target: "outbox",
                        event_id = %event.id,
                        error = %err_msg,
                        "Outbox事件处理失败"
                    );
                    self.database
                        .mark_outbox_event_failed(&event.id, &err_msg)
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn handle_event(&self, event: &OutboxEvent) -> Result<()> {
        let envelope: OutboxEnvelope = serde_json::from_str(&event.payload)?;
        match envelope {
            OutboxEnvelope::ThirdPartyCallback(payload) => {
                callbacks::schedule_callback(
                    payload.preview_id,
                    payload.callback_url,
                    payload.payload,
                    payload.reset_attempts,
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn enqueue_event(
        &self,
        table_name: &str,
        op_type: &str,
        pk_value: &str,
        envelope: OutboxEnvelope,
    ) -> Result<()> {
        let payload_str = serde_json::to_string(&envelope)?;
        let idempotency_key = compute_idempotency_key(op_type, pk_value, &payload_str);
        let event = NewOutboxEvent {
            table_name: table_name.to_string(),
            op_type: op_type.to_string(),
            pk_value: pk_value.to_string(),
            idempotency_key,
            payload: payload_str,
        };
        self.database.enqueue_outbox_event(&event).await
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
enum OutboxEnvelope {
    ThirdPartyCallback(ThirdPartyCallbackPayload),
}

#[derive(Debug, Serialize, Deserialize)]
struct ThirdPartyCallbackPayload {
    preview_id: String,
    callback_url: String,
    payload: Value,
    #[serde(default)]
    reset_attempts: bool,
}

fn compute_idempotency_key(op_type: &str, pk_value: &str, payload: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(payload.as_bytes());
    let digest = hasher.finalize();
    format!("{}:{}:{:x}", op_type, pk_value, digest)
}
