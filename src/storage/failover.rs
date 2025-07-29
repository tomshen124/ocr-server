use async_trait::async_trait;
use anyhow::Result;
use chrono::{DateTime, Utc};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

use super::traits::{Storage, FileMetadata};
use super::factory;
use crate::util::config::StorageFailoverConfig;

/// 存储故障转移状态
#[derive(Debug, Clone, PartialEq)]
enum FailoverState {
    /// 使用主存储（OSS）
    Primary,
    /// 使用本地降级存储
    Fallback,
    /// 正在尝试恢复到主存储
    Recovering,
}

/// 待同步文件记录
#[derive(Debug, Clone)]
struct PendingSyncFile {
    key: String,
    local_path: String,
    created_at: DateTime<Utc>,
}

/// 带故障转移功能的存储包装器
pub struct FailoverStorage {
    /// 主存储（通常是OSS）
    primary: Arc<dyn Storage>,
    /// 降级存储（本地文件系统）
    fallback: Arc<dyn Storage>,
    /// 当前状态
    state: Arc<RwLock<FailoverState>>,
    /// 配置
    config: StorageFailoverConfig,
    /// 最后一次健康检查时间
    last_health_check: Arc<RwLock<DateTime<Utc>>>,
    /// 待同步文件列表
    pending_sync: Arc<RwLock<Vec<PendingSyncFile>>>,
}

impl FailoverStorage {
    pub async fn new(
        primary: Arc<dyn Storage>,
        config: StorageFailoverConfig,
        base_url: String,
    ) -> Result<Self> {
        // 创建本地降级存储
        let fallback_config = factory::StorageConfig {
            storage_type: factory::StorageType::Local,
            local: Some(factory::LocalConfig {
                base_path: config.local_fallback_dir.clone(),
                base_url,
            }),
            oss: None,
        };
        
        let fallback = Arc::from(factory::create_storage(&fallback_config).await?);
        
        Ok(Self {
            primary,
            fallback,
            state: Arc::new(RwLock::new(FailoverState::Primary)),
            config,
            last_health_check: Arc::new(RwLock::new(Utc::now())),
            pending_sync: Arc::new(RwLock::new(Vec::new())),
        })
    }
    
    /// 获取当前活动的存储
    async fn get_active_storage(&self) -> Arc<dyn Storage> {
        let state = self.state.read().await;
        match *state {
            FailoverState::Primary => self.primary.clone(),
            FailoverState::Fallback | FailoverState::Recovering => self.fallback.clone(),
        }
    }
    
    /// 执行带重试的存储操作
    async fn execute_with_failover<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn(Arc<dyn Storage>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
    {
        // 检查是否需要健康检查
        self.check_health_if_needed().await;
        
        let mut retries = 0;
        let max_retries = self.config.max_retries;
        
        loop {
            let storage = self.get_active_storage().await;
            let state = self.state.read().await.clone();
            
            match operation(storage.clone()).await {
                Ok(result) => {
                    // 如果当前在恢复状态且操作成功，切换回主存储
                    if state == FailoverState::Recovering {
                        info!("Primary storage recovered, switching back");
                        *self.state.write().await = FailoverState::Primary;
                        
                        // 触发待同步文件的上传
                        if self.config.sync_when_recovered {
                            self.sync_pending_files().await;
                        }
                    }
                    return Ok(result);
                }
                Err(e) => {
                    retries += 1;
                    
                    // 如果是主存储失败，尝试切换到降级存储
                    if state == FailoverState::Primary && self.config.auto_switch_to_local {
                        warn!("Primary storage failed: {}, switching to fallback", e);
                        *self.state.write().await = FailoverState::Fallback;
                        
                        // 重试使用降级存储
                        if retries <= max_retries {
                            tokio::time::sleep(Duration::from_millis(self.config.retry_delay)).await;
                            continue;
                        }
                    }
                    
                    // 如果已经在使用降级存储或重试次数超限，返回错误
                    error!("Storage operation failed after {} retries: {}", retries, e);
                    return Err(e);
                }
            }
        }
    }
    
    /// 检查是否需要进行健康检查
    async fn check_health_if_needed(&self) {
        if !self.config.enabled {
            return;
        }
        
        let now = Utc::now();
        let last_check = *self.last_health_check.read().await;
        let interval = Duration::from_secs(self.config.health_check_interval);
        
        if now.signed_duration_since(last_check).to_std().unwrap_or(Duration::ZERO) < interval {
            return;
        }
        
        // 更新最后检查时间
        *self.last_health_check.write().await = now;
        
        // 如果当前在降级状态，尝试恢复主存储
        let state = self.state.read().await.clone();
        if state == FailoverState::Fallback {
            info!("Attempting to recover primary storage connection");
            *self.state.write().await = FailoverState::Recovering;
            
            // 在后台尝试健康检查
            let primary = self.primary.clone();
            let state_clone = self.state.clone();
            tokio::spawn(async move {
                // 尝试一个简单的exists操作作为健康检查
                match primary.exists(".health_check").await {
                    Ok(_) => {
                        info!("Primary storage health check passed");
                        // 健康检查通过，但不立即切换，等下次操作成功后再切换
                    }
                    Err(e) => {
                        warn!("Primary storage still unhealthy: {}, staying in fallback mode", e);
                        *state_clone.write().await = FailoverState::Fallback;
                    }
                }
            });
        }
    }
    
    /// 同步待上传文件到主存储
    async fn sync_pending_files(&self) {
        let pending = self.pending_sync.write().await.drain(..).collect::<Vec<_>>();
        
        if pending.is_empty() {
            return;
        }
        
        info!("Starting sync of {} pending files to primary storage", pending.len());
        
        let primary = self.primary.clone();
        let fallback = self.fallback.clone();
        
        tokio::spawn(async move {
            let mut success_count = 0;
            let mut failed_files = Vec::new();
            
            for file in pending {
                // 从本地读取文件
                match fallback.get(&file.key).await {
                    Ok(Some(data)) => {
                        // 上传到主存储
                        match primary.put(&file.key, &data).await {
                            Ok(_) => {
                                success_count += 1;
                                // 可选：删除本地文件
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
            
            info!("Sync completed: {} successful, {} failed", success_count, failed_files.len());
            
            // 将失败的文件重新加入待同步列表
            if !failed_files.is_empty() {
                // 这里需要重新获取锁，因为是在异步任务中
                // 实际实现中可能需要更复杂的处理
            }
        });
    }
    
    /// 记录需要同步的文件
    async fn record_pending_sync(&self, key: &str) {
        if !self.config.sync_when_recovered {
            return;
        }
        
        let state = self.state.read().await;
        if *state == FailoverState::Fallback {
            let mut pending = self.pending_sync.write().await;
            pending.push(PendingSyncFile {
                key: key.to_string(),
                local_path: format!("{}/{}", self.config.local_fallback_dir, key),
                created_at: Utc::now(),
            });
        }
    }
}

#[async_trait]
impl Storage for FailoverStorage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let key_clone = key.to_string();
        let data_clone = data.to_vec();
        
        let result = self.execute_with_failover(|storage| {
            let key = key_clone.clone();
            let data = data_clone.clone();
            Box::pin(async move { storage.put(&key, &data).await })
        }).await;
        
        // 如果成功写入降级存储，记录待同步
        if result.is_ok() {
            self.record_pending_sync(key).await;
        }
        
        result
    }
    
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.get(&key).await })
        }).await
    }
    
    async fn delete(&self, key: &str) -> Result<()> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.delete(&key).await })
        }).await
    }
    
    async fn exists(&self, key: &str) -> Result<bool> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.exists(&key).await })
        }).await
    }
    
    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        self.execute_with_failover(|storage| {
            let prefix = prefix.to_string();
            Box::pin(async move { storage.list(&prefix).await })
        }).await
    }
    
    async fn get_public_url(&self, key: &str) -> Result<String> {
        // 总是使用当前活动的存储的URL
        let storage = self.get_active_storage().await;
        storage.get_public_url(key).await
    }
    
    async fn get_presigned_url(&self, key: &str, expires: Duration) -> Result<String> {
        // 总是使用当前活动的存储的URL
        let storage = self.get_active_storage().await;
        storage.get_presigned_url(key, expires).await
    }
    
    async fn get_metadata(&self, key: &str) -> Result<FileMetadata> {
        self.execute_with_failover(|storage| {
            let key = key.to_string();
            Box::pin(async move { storage.get_metadata(&key).await })
        }).await
    }
    
    async fn health_check(&self) -> Result<bool> {
        // 检查主存储的健康状态
        let storage = self.get_active_storage().await;
        storage.health_check().await
    }
}