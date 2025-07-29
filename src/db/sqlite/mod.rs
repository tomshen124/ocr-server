//! SQLite模块
//! 重构后的模块化SQLite数据库实现

pub mod schemas;
pub mod queries;
pub mod connection;
pub mod monitor_queries;

use async_trait::async_trait;
use anyhow::Result;
use sqlx::SqlitePool;

use super::traits::*;
use schemas::SchemaManager;
use queries::{PreviewQueries, ApiStatsQueries, HealthQueries};
use monitor_queries::MonitorQueries;
use connection::ConnectionManager;

/// SQLite数据库实现
pub struct SqliteDatabase {
    pool: SqlitePool,
}

impl SqliteDatabase {
    /// 创建新的SQLite数据库实例
    pub async fn new(db_path: &str) -> Result<Self> {
        let pool = ConnectionManager::create_pool(db_path).await?;
        Ok(Self { pool })
    }
    
    /// 获取连接池引用
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
    
    /// 获取连接池信息
    pub fn pool_info(&self) -> connection::PoolInfo {
        ConnectionManager::get_pool_info(&self.pool)
    }
    
    // 监控系统数据库方法
    /// 根据用户名查找监控用户
    pub async fn find_monitor_user_by_username(&self, username: &str) -> Result<Option<crate::api::monitor_auth::MonitorUser>> {
        MonitorQueries::find_user_by_username(&self.pool, username).await
    }
    
    /// 获取监控用户密码哈希
    pub async fn get_monitor_user_password_hash(&self, user_id: &str) -> Result<String> {
        MonitorQueries::get_user_password_hash(&self.pool, user_id).await
    }
    
    /// 创建监控会话
    pub async fn create_monitor_session(
        &self,
        session_id: &str,
        user_id: &str,
        ip: &str,
        user_agent: &str,
        created_at: &str,
        expires_at: &str,
    ) -> Result<()> {
        MonitorQueries::create_session(&self.pool, session_id, user_id, ip, user_agent, created_at, expires_at).await
    }
    
    /// 根据会话ID查找监控会话
    pub async fn find_monitor_session_by_id(&self, session_id: &str) -> Result<Option<crate::api::monitor_auth::MonitorSession>> {
        MonitorQueries::find_session_by_id(&self.pool, session_id).await
    }
    
    /// 更新监控用户登录信息
    pub async fn update_monitor_login_info(&self, user_id: &str, now: &str) -> Result<()> {
        MonitorQueries::update_login_info(&self.pool, user_id, now).await
    }
    
    /// 更新监控会话活动时间
    pub async fn update_monitor_session_activity(&self, session_id: &str, now: &str) -> Result<()> {
        MonitorQueries::update_session_activity(&self.pool, session_id, now).await
    }
    
    /// 删除监控会话
    pub async fn delete_monitor_session(&self, session_id: &str) -> Result<()> {
        MonitorQueries::delete_session(&self.pool, session_id).await
    }
    
    /// 清理过期监控会话
    pub async fn cleanup_expired_monitor_sessions(&self, now: &str) -> Result<u64> {
        MonitorQueries::cleanup_expired_sessions(&self.pool, now).await
    }
    
    /// 获取活跃监控会话数量
    pub async fn get_active_monitor_sessions_count(&self, now: &str) -> Result<i64> {
        MonitorQueries::get_active_sessions_count(&self.pool, now).await
    }
}

#[async_trait]
impl Database for SqliteDatabase {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()> {
        PreviewQueries::save_record(&self.pool, record).await
    }
    
    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>> {
        PreviewQueries::get_by_id(&self.pool, id).await
    }
    
    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()> {
        PreviewQueries::update_status(&self.pool, id, status).await
    }
    
    async fn update_preview_evaluation_result(&self, id: &str, evaluation_result: &str) -> Result<()> {
        PreviewQueries::update_evaluation_result(&self.pool, id, evaluation_result).await
    }
    
    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>> {
        PreviewQueries::list_with_filter(&self.pool, filter).await
    }
    
    async fn find_preview_by_third_party_id(&self, third_party_id: &str, user_id: &str) -> Result<Option<PreviewRecord>> {
        PreviewQueries::find_by_third_party_id(&self.pool, third_party_id, user_id).await
    }
    
    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()> {
        ApiStatsQueries::save_stats(&self.pool, stats).await
    }
    
    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>> {
        ApiStatsQueries::get_stats_with_filter(&self.pool, filter).await
    }
    
    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary> {
        ApiStatsQueries::get_summary(&self.pool, filter).await
    }
    
    async fn health_check(&self) -> Result<bool> {
        HealthQueries::check_health(&self.pool).await
    }
    
    async fn initialize(&self) -> Result<()> {
        SchemaManager::create_all_tables(&self.pool).await?;
        Ok(())
    }
}