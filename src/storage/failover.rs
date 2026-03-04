use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use super::factory;
use super::traits::{FileMetadata, Storage};
use crate::util::config::StorageFailoverConfig;

#[derive(Debug, Clone, PartialEq)]
enum FailoverState {
    Primary,
    Fallback,
    Recovering,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingSyncFile {
    key: String,
    local_path: String,
    created_at: DateTime<Utc>,
}

pub struct FailoverStorage {
    primary: Arc<dyn Storage>,
    fallback: Arc<dyn Storage>,
    state: Arc<RwLock<FailoverState>>,
    config: StorageFailoverConfig,
    last_health_check: Arc<RwLock<DateTime<Utc>>>,
    pending_sync: Arc<RwLock<Vec<PendingSyncFile>>>,
    pending_sync_path: PathBuf,
}

impl FailoverStorage {
    pub async fn new(
        primary: Arc<dyn Storage>,
        config: StorageFailoverConfig,
        base_url: String,
    ) -> Result<Self> {
        let fallback_config = factory::StorageConfig {
            storage_type: factory::StorageType::Local,
            local: Some(factory::LocalConfig {
                base_path: config.local_fallback_dir.clone(),
                base_url,
            }),
            oss: None,
        };

        let fallback = Arc::from(factory::create_storage(&fallback_config).await?);

        let pending_sync_path = PathBuf::from(&config.local_fallback_dir).join("pending_sync.json");
        let pending_records = load_pending_sync(&pending_sync_path)
            .await
            .unwrap_or_else(|err| {
                warn!(
                    error = %err,
                    path = %pending_sync_path.display(),
                    "加载 pending_sync 文件失败，将使用空列表"
                );
                Vec::new()
            });

        Ok(Self {
            primary,
            fallback,
            state: Arc::new(RwLock::new(FailoverState::Primary)),
            config,
            last_health_check: Arc::new(RwLock::new(Utc::now())),
            pending_sync: Arc::new(RwLock::new(pending_records)),
            pending_sync_path,
        })
    }

    async fn get_active_storage(&self) -> Arc<dyn Storage> {
        let state = self.state.read().await;
        match *state {
            FailoverState::Primary => self.primary.clone(),
            FailoverState::Fallback | FailoverState::Recovering => self.fallback.clone(),
        }
    }

    async fn execute_with_failover<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn(
            Arc<dyn Storage>,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
    {
        self.check_health_if_needed().await;

        let mut retries = 0;
        let max_retries = self.config.max_retries;

        loop {
            let storage = self.get_active_storage().await;
            let state = self.state.read().await.clone();

            match operation(storage.clone()).await {
                Ok(result) => {
                    if state == FailoverState::Recovering {
                        info!("Primary storage recovered, switching back");
                        *self.state.write().await = FailoverState::Primary;

                        if self.config.sync_when_recovered {
                            self.sync_pending_files().await;
                        }
                    }
                    return Ok(result);
                }
                Err(e) => {
                    retries += 1;

                    if state == FailoverState::Primary && self.config.auto_switch_to_local {
                        warn!("Primary storage failed: {}, switching to fallback", e);
                        *self.state.write().await = FailoverState::Fallback;

                        if retries <= max_retries {
                            tokio::time::sleep(Duration::from_millis(self.config.retry_delay))
                                .await;
                            continue;
                        }
                    }

                    error!("Storage operation failed after {} retries: {}", retries, e);
                    return Err(e);
                }
            }
        }
    }

    async fn check_health_if_needed(&self) {
        if !self.config.enabled {
            return;
        }

        let now = Utc::now();
        let last_check = *self.last_health_check.read().await;
        let interval = Duration::from_secs(self.config.health_check_interval);

        if now
            .signed_duration_since(last_check)
            .to_std()
            .unwrap_or(Duration::ZERO)
            < interval
        {
            return;
        }

        *self.last_health_check.write().await = now;

        let state = self.state.read().await.clone();
        if state == FailoverState::Fallback {
            info!("Attempting to recover primary storage connection");
            *self.state.write().await = FailoverState::Recovering;

            let primary = self.primary.clone();
            let state_clone = self.state.clone();
            let config = self.config.clone();
            tokio::spawn(async move {
                match primary.health_check().await {
                    Ok(true) => {
                        info!(
                            "Primary storage health check passed (interval={}s)",
                            config.health_check_interval
                        );
                    }
                    Ok(false) => {
                        warn!("Primary storage still unhealthy, staying in fallback mode");
                        *state_clone.write().await = FailoverState::Fallback;
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            "Primary storage health check errored, staying in fallback mode"
                        );
                        *state_clone.write().await = FailoverState::Fallback;
                    }
                }
            });
        }
    }

    async fn sync_pending_files(&self) {
        let pending = {
            let mut guard = self.pending_sync.write().await;
            let drained = guard.drain(..).collect::<Vec<_>>();
            drop(guard);
            if let Err(err) = self.persist_pending_sync().await {
                warn!(
                    error = %err,
                    path = %self.pending_sync_path.display(),
                    "持久化 pending_sync 文件失败（已清空）"
                );
            }
            drained
        };

        if pending.is_empty() {
            return;
        }

        info!(
            "Starting sync of {} pending files to primary storage",
            pending.len()
        );

        let primary = self.primary.clone();
        let fallback = self.fallback.clone();
        let pending_sync = self.pending_sync.clone();
        let persist_path = self.pending_sync_path.clone();

        tokio::spawn(async move {
            let mut success_count = 0;
            let mut failed_files = Vec::new();

            for file in pending {
                match fallback.get(&file.key).await {
                    Ok(Some(data)) => {
                        match primary.put(&file.key, &data).await {
                            Ok(_) => {
                                success_count += 1;
                                let _ = fallback.delete(&file.key).await;
                            }
                            Err(e) => {
                                warn!("Failed to sync file {} to primary storage: {}", file.key, e);
                                failed_files.push(file);
                            }
                        }
                    }
                    Ok(None) => {
                        warn!("Pending sync file {} not found in local storage", file.key);
                    }
                    Err(e) => {
                        warn!("Failed to read pending sync file {}: {}", file.key, e);
                        failed_files.push(file);
                    }
                }
            }

            info!(
                "Sync completed: {} successful, {} failed",
                success_count,
                failed_files.len()
            );

            if !failed_files.is_empty() {
                let mut guard = pending_sync.write().await;
                guard.extend(failed_files);
                let snapshot = guard.clone();
                drop(guard);
                if let Err(err) = persist_pending_sync_to_path(&persist_path, &snapshot).await {
                    warn!(
                        error = %err,
                        path = %persist_path.display(),
                        "持久化 pending_sync 文件失败（重试阶段）"
                    );
                }
            } else {
                let snapshot = pending_sync.read().await.clone();
                if let Err(err) = persist_pending_sync_to_path(&persist_path, &snapshot).await {
                    warn!(
                        error = %err,
                        path = %persist_path.display(),
                        "持久化 pending_sync 文件失败（同步完成）"
                    );
                }
            }
        });
    }

    async fn record_pending_sync(&self, key: &str) {
        if !self.config.sync_when_recovered {
            return;
        }

        let state = self.state.read().await;
        if *state == FailoverState::Fallback {
            let mut pending = self.pending_sync.write().await;
            let already_recorded = pending.iter().any(|entry| entry.key == key);
            if !already_recorded {
                pending.push(PendingSyncFile {
                    key: key.to_string(),
                    local_path: format!("{}/{}", self.config.local_fallback_dir, key),
                    created_at: Utc::now(),
                });
            }
            drop(pending);
            if let Err(err) = self.persist_pending_sync().await {
                warn!(
                    error = %err,
                    path = %self.pending_sync_path.display(),
                    key = %key,
                    "持久化 pending_sync 文件失败（记录阶段）"
                );
            }
        }
    }

    async fn persist_pending_sync(&self) -> Result<()> {
        let snapshot = self.pending_sync.read().await.clone();
        persist_pending_sync_to_path(&self.pending_sync_path, &snapshot).await
    }
}

#[async_trait]
impl Storage for FailoverStorage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let key_clone = key.to_string();
        let data_clone = data.to_vec();

        let result = self
            .execute_with_failover(|storage| {
                let key = key_clone.clone();
                let data = data_clone.clone();
                Box::pin(async move { storage.put(&key, &data).await })
            })
            .await;

        if result.is_ok() {
            self.record_pending_sync(key).await;
        }

        result
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.get(&key).await })
        })
        .await
    }

    async fn delete(&self, key: &str) -> Result<()> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.delete(&key).await })
        })
        .await
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.exists(&key).await })
        })
        .await
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        self.execute_with_failover(|storage| {
            let prefix = prefix.to_string();
            Box::pin(async move { storage.list(&prefix).await })
        })
        .await
    }

    async fn get_public_url(&self, key: &str) -> Result<String> {
        let storage = self.get_active_storage().await;
        storage.get_public_url(key).await
    }

    async fn get_presigned_url(&self, key: &str, expires: Duration) -> Result<String> {
        let storage = self.get_active_storage().await;
        storage.get_presigned_url(key, expires).await
    }

    async fn get_metadata(&self, key: &str) -> Result<FileMetadata> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.get_metadata(&key).await })
        })
        .await
    }

    async fn health_check(&self) -> Result<bool> {
        let storage = self.get_active_storage().await;
        storage.health_check().await
    }
}

async fn load_pending_sync(path: &Path) -> Result<Vec<PendingSyncFile>> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            if bytes.is_empty() {
                return Ok(Vec::new());
            }
            let parsed = serde_json::from_slice(&bytes).context("解析 pending_sync JSON 失败")?;
            Ok(parsed)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(err) => Err(err).context("读取 pending_sync 文件失败"),
    }
}

async fn persist_pending_sync_to_path(path: &Path, entries: &[PendingSyncFile]) -> Result<()> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("创建 pending_sync 目录失败: {}", parent.display()))?;
    }
    let json = serde_json::to_vec_pretty(entries).context("序列化 pending_sync JSON 失败")?;
    tokio::fs::write(path, json)
        .await
        .with_context(|| format!("写入 pending_sync 文件失败: {}", path.display()))?;
    Ok(())
}
