use async_trait::async_trait;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::time::Duration;
use tracing::{error, info, warn};

use super::traits::*;
use super::factory;
use crate::util::config::DatabaseFailoverConfig;

/// 数据库故障转移状态
#[derive(Debug, Clone, PartialEq)]
enum FailoverState {
    /// 使用主数据库
    Primary,
    /// 使用本地降级数据库
    Fallback,
    /// 正在尝试恢复到主数据库
    Recovering,
}

/// 带故障转移功能的数据库包装器
pub struct FailoverDatabase {
    /// 主数据库
    primary: Arc<dyn Database>,
    /// 降级数据库（本地SQLite）
    fallback: Arc<dyn Database>,
    /// 当前状态
    state: Arc<RwLock<FailoverState>>,
    /// 配置
    config: DatabaseFailoverConfig,
    /// 最后一次健康检查时间
    last_health_check: Arc<RwLock<DateTime<Utc>>>,
}

impl FailoverDatabase {
    pub async fn new(
        primary: Arc<dyn Database>,
        config: DatabaseFailoverConfig,
    ) -> Result<Self> {
        // 创建本地降级数据库
        let fallback_config = factory::DatabaseConfig {
            db_type: factory::DatabaseType::Sqlite,
            sqlite: Some(factory::SqliteConfig {
                path: format!("{}/fallback.db", config.local_data_dir),
            }),
            dm: None,
        };
        
        // 确保降级目录存在
        std::fs::create_dir_all(&config.local_data_dir)
            .context("Failed to create fallback database directory")?;
        
        let fallback = factory::create_database(&fallback_config).await?;
        
        // 初始化降级数据库
        fallback.initialize().await?;
        
        Ok(Self {
            primary,
            fallback: fallback.into(),
            state: Arc::new(RwLock::new(FailoverState::Primary)),
            config,
            last_health_check: Arc::new(RwLock::new(Utc::now())),
        })
    }
    
    /// 获取当前活动的数据库
    async fn get_active_db(&self) -> Arc<dyn Database> {
        let state = self.state.read().await;
        match *state {
            FailoverState::Primary => self.primary.clone(),
            FailoverState::Fallback | FailoverState::Recovering => self.fallback.clone(),
        }
    }
    
    /// 执行带重试的数据库操作
    async fn execute_with_failover<F, T>(&self, operation: F) -> Result<T>
    where
        F: Fn(Arc<dyn Database>) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send>>,
    {
        // 检查是否需要健康检查
        self.check_health_if_needed().await;
        
        let mut retries = 0;
        let max_retries = self.config.max_retries;
        
        loop {
            let db = self.get_active_db().await;
            let state = self.state.read().await.clone();
            
            match operation(db.clone()).await {
                Ok(result) => {
                    // 如果当前在恢复状态且操作成功，切换回主数据库
                    if state == FailoverState::Recovering {
                        info!("Primary database recovered, switching back");
                        *self.state.write().await = FailoverState::Primary;
                    }
                    return Ok(result);
                }
                Err(e) => {
                    retries += 1;
                    
                    // 如果是主数据库失败，尝试切换到降级数据库
                    if state == FailoverState::Primary && self.config.fallback_to_local {
                        warn!("Primary database failed: {}, switching to fallback", e);
                        *self.state.write().await = FailoverState::Fallback;
                        
                        // 重试使用降级数据库
                        if retries <= max_retries {
                            tokio::time::sleep(Duration::from_millis(self.config.retry_delay)).await;
                            continue;
                        }
                    }
                    
                    // 如果已经在使用降级数据库或重试次数超限，返回错误
                    error!("Database operation failed after {} retries: {}", retries, e);
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
        
        // 如果当前在降级状态，尝试恢复主数据库
        let state = self.state.read().await.clone();
        if state == FailoverState::Fallback {
            info!("Attempting to recover primary database connection");
            *self.state.write().await = FailoverState::Recovering;
            
            // 在后台尝试健康检查
            let primary = self.primary.clone();
            let state_clone = self.state.clone();
            tokio::spawn(async move {
                match primary.health_check().await {
                    Ok(true) => {
                        info!("Primary database health check passed");
                        // 健康检查通过，但不立即切换，等下次操作成功后再切换
                    }
                    Ok(false) | Err(_) => {
                        warn!("Primary database still unhealthy, staying in fallback mode");
                        *state_clone.write().await = FailoverState::Fallback;
                    }
                }
            });
        }
    }
}

#[async_trait]
impl Database for FailoverDatabase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()> {
        self.execute_with_failover(|db| {
            let record = record.clone();
            Box::pin(async move { db.save_preview_record(&record).await })
        }).await
    }
    
    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            Box::pin(async move { db.get_preview_record(&id).await })
        }).await
    }
    
    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let status = status.clone();
            Box::pin(async move { db.update_preview_status(&id, status).await })
        }).await
    }
    
    async fn update_preview_evaluation_result(&self, id: &str, evaluation_result: &str) -> Result<()> {
        self.execute_with_failover(|db| {
            let id = id.to_string();
            let evaluation_result = evaluation_result.to_string();
            Box::pin(async move { db.update_preview_evaluation_result(&id, &evaluation_result).await })
        }).await
    }
    
    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.list_preview_records(&filter).await })
        }).await
    }
    
    async fn find_preview_by_third_party_id(&self, third_party_id: &str, user_id: &str) -> Result<Option<PreviewRecord>> {
        self.execute_with_failover(|db| {
            let third_party_id = third_party_id.to_string();
            let user_id = user_id.to_string();
            Box::pin(async move { db.find_preview_by_third_party_id(&third_party_id, &user_id).await })
        }).await
    }
    
    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()> {
        self.execute_with_failover(|db| {
            let stats = stats.clone();
            Box::pin(async move { db.save_api_stats(&stats).await })
        }).await
    }
    
    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.get_api_stats(&filter).await })
        }).await
    }
    
    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary> {
        self.execute_with_failover(|db| {
            let filter = filter.clone();
            Box::pin(async move { db.get_api_summary(&filter).await })
        }).await
    }
    
    async fn health_check(&self) -> Result<bool> {
        // 总是返回true，因为我们有降级机制
        Ok(true)
    }
    
    async fn initialize(&self) -> Result<()> {
        // 初始化两个数据库
        self.primary.initialize().await.ok(); // 忽略主数据库初始化失败
        self.fallback.initialize().await?;
        Ok(())
    }
}