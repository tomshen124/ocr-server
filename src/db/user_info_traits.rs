
use anyhow::Result;
use async_trait::async_trait;
use crate::model::user_info::{UserInfo, UserInfoFilter, UserInfoStats};

#[async_trait]
pub trait UserInfoDatabase: Send + Sync {
    async fn upsert_user_info(&self, user_info: &UserInfo) -> Result<()>;
    
    async fn get_user_info(&self, user_id: &str) -> Result<Option<UserInfo>>;
    
    async fn list_user_info(&self, filter: &UserInfoFilter) -> Result<Vec<UserInfo>>;
    
    async fn update_user_login(&self, user_id: &str) -> Result<()>;
    
    async fn deactivate_user(&self, user_id: &str) -> Result<()>;
    
    async fn get_user_stats(&self) -> Result<UserInfoStats>;
    
    async fn find_users_by_organization(&self, org_code: &str) -> Result<Vec<UserInfo>>;
    
    async fn cleanup_inactive_users(&self, days_inactive: u32) -> Result<u64>;
}

use super::traits::Database;

#[async_trait]
pub trait ExtendedDatabase: Database + UserInfoDatabase {
}