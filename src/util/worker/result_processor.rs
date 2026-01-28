use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, StreamExt};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::api::worker_proxy::{process_worker_result_logic, WorkerResultRequest};
use crate::AppState;

#[cfg(unix)]
use tokio::signal::unix::{signal, SignalKind};

pub struct ResultProcessor {
    app_state: AppState,
    batch_size: usize,
    max_concurrency: usize,
    idle_backoff: Duration,
    max_backoff: Duration,
}

impl ResultProcessor {
    pub fn new(app_state: AppState) -> Self {
        let cfg = app_state
            .config
            .master
            .background_processing
            .result_processor
            .clone();

        Self {
            app_state,
            batch_size: cfg.batch_size.max(1) as usize,
            max_concurrency: cfg.max_concurrency.max(1) as usize,
            idle_backoff: Duration::from_millis(cfg.idle_backoff_ms.max(10)),
            max_backoff: Duration::from_millis(cfg.max_backoff_ms.max(cfg.idle_backoff_ms)),
        }
    }

    pub async fn run(&self) {
        info!("Worker Result Processor started");
        let mut backoff = self.idle_backoff;
        loop {
            tokio::select! {
                _ = wait_for_shutdown_signal() => {
                    info!("Worker Result Processor received shutdown signal, exiting");
                    break;
                }
                res = self.process_batch() => {
                    match res {
                        Ok(0) => {
                            sleep(backoff).await;
                            backoff = (backoff.saturating_mul(2)).min(self.max_backoff);
                        }
                        Ok(_) => {
                            backoff = self.idle_backoff;
                        }
                        Err(e) => {
                            error!("Error processing worker results batch: {}", e);
                            sleep(backoff).await;
                            backoff = (backoff.saturating_mul(2)).min(self.max_backoff);
                        }
                    }
                }
            }
        }
        info!("Worker Result Processor stopped");
    }

    async fn process_batch(&self) -> anyhow::Result<usize> {
        let results = self
            .app_state
            .database
            .fetch_pending_worker_results(self.batch_size as u32)
            .await?;

        if results.is_empty() {
            return Ok(0);
        }

        let db = self.app_state.database.clone();
        let app_state = self.app_state.clone();
        let concurrency = self.max_concurrency;

        stream::iter(results.into_iter())
            .map(|record| {
                let db = db.clone();
                let app_state = app_state.clone();
                async move {
                    let payload: WorkerResultRequest = match serde_json::from_str(&record.payload) {
                        Ok(p) => p,
                        Err(e) => {
                            error!(
                                id = %record.id,
                                error = %e,
                                "Failed to deserialize worker result payload"
                            );
                            let _ = db
                                .update_worker_result_status(
                                    &record.id,
                                    "failed",
                                    Some(&format!("Deserialize error: {}", e)),
                                )
                                .await;
                            return;
                        }
                    };

                    info!(
                        id = %record.id,
                        preview_id = %record.preview_id,
                        "Processing queued worker result"
                    );

                    match process_worker_result_logic(
                        &app_state,
                        &record.preview_id,
                        payload,
                        "async-result-processor",
                    )
                    .await
                    {
                        Ok(_) => {
                            let _ = db
                                .update_worker_result_status(&record.id, "completed", None)
                                .await;
                        }
                        Err(e) => {
                            error!(
                                id = %record.id,
                                preview_id = %record.preview_id,
                                error = %e,
                                "Failed to process worker result"
                            );
                            let _ = db
                                .update_worker_result_status(
                                    &record.id,
                                    "failed",
                                    Some(&e.to_string()),
                                )
                                .await;
                        }
                    }
                }
            })
            .buffer_unordered(concurrency)
            .collect::<Vec<_>>()
            .await;

        Ok(self.batch_size)
    }
}

async fn wait_for_shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = signal(SignalKind::terminate()).ok();
        let mut sighup = signal(SignalKind::hangup()).ok();

        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = async {
                if let Some(ref mut s) = sigterm {
                    let _ = s.recv().await;
                }
            } => {},
            _ = async {
                if let Some(ref mut s) = sighup {
                    let _ = s.recv().await;
                }
            } => {},
        }
        return;
    }

    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
