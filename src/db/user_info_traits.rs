//! 用户信息数据库操作模块

use anyhow::Result;
use async_trait::async_trait;
use crate::model::user_info::{UserInfo, UserInfoFilter, UserInfoStats};

/// 用户信息数据库操作trait
#[async_trait]
pub trait UserInfoDatabase: Send + Sync {
    /// 保存或更新用户信息
    async fn upsert_user_info(&self, user_info: &UserInfo) -> Result<()>;
    
    /// 根据用户ID获取用户信息
    async fn get_user_info(&self, user_id: &str) -> Result<Option<UserInfo>>;
    
    /// 查询用户信息列表
    async fn list_user_info(&self, filter: &UserInfoFilter) -> Result<Vec<UserInfo>>;
    
    /// 更新用户最后登录时间和登录次数
    async fn update_user_login(&self, user_id: &str) -> Result<()>;
    
    /// 软删除用户信息 (设置为非活跃状态)
    async fn deactivate_user(&self, user_id: &str) -> Result<()>;
    
    /// 获取用户信息统计
    async fn get_user_stats(&self) -> Result<UserInfoStats>;
    
    /// 根据组织代码查询用户
    async fn find_users_by_organization(&self, org_code: &str) -> Result<Vec<UserInfo>>;
    
    /// 清理过期的非活跃用户数据 (GDPR合规)
    async fn cleanup_inactive_users(&self, days_inactive: u32) -> Result<u64>;
}

/// 向现有Database trait添加用户信息操作
use super::traits::Database;

#[async_trait]
pub trait ExtendedDatabase: Database + UserInfoDatabase {
    // 组合trait，继承Database和UserInfoDatabase的所有功能
}