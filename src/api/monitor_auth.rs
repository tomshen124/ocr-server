//! 监控系统认证服务
//! 负责监控用户的登录、会话管理和权限控制

use anyhow::{anyhow, ensure, Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[cfg(feature = "monitoring")]
use bcrypt::{hash, verify};

/// 默认监控管理员初始密码（部署后请及时重置）
/// 生产环境务必通过环境变量 MONITOR_ADMIN_PASSWORD 覆盖
pub const DEFAULT_MONITOR_ADMIN_PASSWORD: &str = "CHANGE_ME_ADMIN_PASSWORD";

#[cfg(feature = "dm_go")]
use crate::db::dm::DmDatabase;
use crate::db::sqlite::SqliteDatabase;

use crate::db::models::{MonitorSession, MonitorUser};

/// 登录请求
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// 登录响应
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub message: String,
    pub session: Option<MonitorSession>,
}

/// 监控认证服务
pub struct MonitorAuthService {
    database: Arc<dyn crate::db::Database>,
}

impl MonitorAuthService {
    pub fn new(database: Arc<dyn crate::db::Database>) -> Self {
        Self { database }
    }

    fn sqlite_db(&self) -> Option<&SqliteDatabase> {
        self.database.as_any().downcast_ref::<SqliteDatabase>()
    }

    #[cfg(feature = "dm_go")]
    fn dm_db(&self) -> Option<&DmDatabase> {
        self.database.as_any().downcast_ref::<DmDatabase>()
    }

    #[cfg(not(feature = "dm_go"))]
    fn dm_db(&self) -> Option<()> {
        None
    }

    fn normalize_role(role: &str) -> String {
        role.trim().to_ascii_lowercase()
    }

    fn canonical_role(role: &str) -> String {
        let normalized = Self::normalize_role(role);
        match normalized.as_str() {
            "admin" => "super_admin".to_string(), // 兼容旧值
            other => other.to_string(),
        }
    }

    fn is_super_admin(role: &str) -> bool {
        matches!(Self::canonical_role(role).as_str(), "super_admin")
    }

    fn is_sys_admin(role: &str) -> bool {
        matches!(
            Self::canonical_role(role).as_str(),
            "sys_admin" | "super_admin"
        )
    }

    fn is_ops_admin(role: &str) -> bool {
        matches!(
            Self::canonical_role(role).as_str(),
            "ops_admin" | "sys_admin" | "super_admin"
        )
    }

    fn validate_role(role: &str) -> Result<String> {
        let normalized = Self::canonical_role(role);
        let allowed = ["super_admin", "sys_admin", "ops_admin"];
        ensure!(
            allowed.contains(&normalized.as_str()),
            "无效的角色类型: {}",
            role
        );
        Ok(normalized)
    }

    fn validate_username(username: &str) -> Result<()> {
        let trimmed = username.trim();
        ensure!(!trimmed.is_empty(), "用户名不能为空");
        ensure!(
            trimmed.len() >= 3 && trimmed.len() <= 64,
            "用户名长度须在3~64字符之间"
        );
        ensure!(
            trimmed
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')),
            "用户名仅支持字母、数字、.-_"
        );
        ensure!(trimmed != "admin", "admin 用户为系统保留，请使用其他用户名");
        Ok(())
    }

    fn validate_password(password: &str) -> Result<()> {
        ensure!(password.len() >= 8, "密码长度至少需要8位");
        Ok(())
    }

    fn hash_password(password: &str) -> Result<String> {
        #[cfg(feature = "monitoring")]
        {
            Ok(hash(password, bcrypt::DEFAULT_COST)?)
        }
        #[cfg(not(feature = "monitoring"))]
        {
            Ok(password.to_string())
        }
    }

    fn now_string() -> String {
        Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }

    /// 用户登录
    pub async fn login(
        &self,
        username: &str,
        password: &str,
        ip: &str,
        user_agent: &str,
    ) -> Result<LoginResponse> {
        // 查找用户
        let user = match self.find_user_by_username(username).await? {
            Some(user) => user,
            None => {
                return Ok(LoginResponse {
                    success: false,
                    message: "用户名或密码错误".to_string(),
                    session: None,
                });
            }
        };

        if !user.is_active {
            return Ok(LoginResponse {
                success: false,
                message: "账户已被禁用".to_string(),
                session: None,
            });
        }

        // 验证密码
        let password_hash = self.get_user_password_hash(&user.id).await?;
        let password_valid = self.verify_password(password, &password_hash)?;

        if !password_valid {
            return Ok(LoginResponse {
                success: false,
                message: "用户名或密码错误".to_string(),
                session: None,
            });
        }

        // 创建会话 (12小时有效期)
        let session = self.create_session(&user, ip, user_agent).await?;

        // 更新登录信息
        self.update_login_info(&user.id).await?;

        Ok(LoginResponse {
            success: true,
            message: "登录成功".to_string(),
            session: Some(session),
        })
    }

    /// 验证会话
    pub async fn verify_session(&self, session_id: &str) -> Result<Option<MonitorSession>> {
        let session = self.find_session_by_id(session_id).await?;

        if let Some(session) = session {
            // 检查是否过期
            let expires_at = parse_db_datetime(&session.expires_at)
                .with_context(|| format!("解析会话过期时间失败: {}", session.expires_at))?;

            if expires_at > Utc::now() {
                // 更新最后活动时间
                self.update_session_activity(session_id).await?;
                return Ok(Some(session));
            } else {
                tracing::info!(
                    target: "auth",
                    session_id = %session_id,
                    expires_at = %session.expires_at,
                    "Monitor session expired"
                );
                // 会话已过期，删除
                self.delete_session(session_id).await?;
            }
        } else {
            tracing::info!(
                target: "auth",
                session_id = %session_id,
                "Monitor session not found in database"
            );
        }

        Ok(None)
    }

    /// 用户登出
    pub async fn logout(&self, session_id: &str) -> Result<()> {
        self.delete_session(session_id).await
    }

    /// 根据用户名查找用户
    async fn find_user_by_username(&self, username: &str) -> Result<Option<MonitorUser>> {
        self.database.find_monitor_user_by_username(username).await
    }

    /// 获取用户密码哈希
    async fn get_user_password_hash(&self, user_id: &str) -> Result<String> {
        self.database.get_monitor_user_password_hash(user_id).await
    }

    /// 验证密码
    fn verify_password(&self, password: &str, hash: &str) -> Result<bool> {
        #[cfg(feature = "monitoring")]
        {
            Ok(verify(password, hash)?)
        }
        #[cfg(not(feature = "monitoring"))]
        {
            // 简单字符串比较 (仅用于非监控模式)
            Ok(password == hash)
        }
    }

    /// 创建会话
    async fn create_session(
        &self,
        user: &MonitorUser,
        ip: &str,
        user_agent: &str,
    ) -> Result<MonitorSession> {
        let session_id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires_at = now + Duration::hours(12); // 12小时有效期

        let created_at = now.format("%Y-%m-%d %H:%M:%S").to_string();
        let expires_at_str = expires_at.format("%Y-%m-%d %H:%M:%S").to_string();

        self.database
            .create_monitor_session(
                &session_id,
                &user.id,
                ip,
                user_agent,
                &created_at,
                &expires_at_str,
            )
            .await?;

        Ok(MonitorSession {
            id: session_id,
            user_id: user.id.clone(),
            user: user.clone(),
            expires_at: expires_at_str,
            ip_address: Some(ip.to_string()),
        })
    }

    /// 根据会话ID查找会话
    async fn find_session_by_id(&self, session_id: &str) -> Result<Option<MonitorSession>> {
        self.database.find_monitor_session_by_id(session_id).await
    }

    /// 更新登录信息
    async fn update_login_info(&self, user_id: &str) -> Result<()> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.database.update_monitor_login_info(user_id, &now).await
    }

    /// 更新会话活动时间
    async fn update_session_activity(&self, session_id: &str) -> Result<()> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.database
            .update_monitor_session_activity(session_id, &now)
            .await
    }

    /// 删除会话
    async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.database.delete_monitor_session(session_id).await
    }

    /// 清理过期会话
    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.database.cleanup_expired_monitor_sessions(&now).await
    }

    /// 获取活跃会话数量
    pub async fn get_active_sessions_count(&self) -> Result<i64> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
        self.database.get_active_monitor_sessions_count(&now).await
    }

    /// 列出所有监控用户
    pub async fn list_users(&self) -> Result<Vec<MonitorUser>> {
        let mut users = self.database.list_monitor_users().await?;
        users.sort_by(|a, b| a.username.cmp(&b.username));
        Ok(users)
    }

    /// 创建监控用户
    pub async fn create_user(
        &self,
        username: &str,
        password: &str,
        role: &str,
    ) -> Result<MonitorUser> {
        Self::validate_username(username)?;
        Self::validate_password(password)?;
        let normalized_role = Self::validate_role(role)?;

        if self.find_user_by_username(username).await?.is_some() {
            return Err(anyhow!("用户名 {} 已存在", username));
        }

        let id = Uuid::new_v4().to_string();
        let password_hash = Self::hash_password(password)?;

        let id = Uuid::new_v4().to_string();
        let password_hash = Self::hash_password(password)?;
        let now = Self::now_string();

        self.database
            .create_monitor_user(&id, username, &password_hash, &normalized_role, &now)
            .await?;

        self.database
            .find_monitor_user_by_id(&id)
            .await?
            .ok_or_else(|| anyhow!("创建用户失败"))
    }

    /// 更新监控用户角色
    pub async fn update_user_role(&self, user_id: &str, role: &str) -> Result<()> {
        let normalized_role = Self::validate_role(role)?;
        let user = self
            .database
            .find_monitor_user_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow!("用户不存在"))?;

        let current_role = Self::canonical_role(&user.role);

        if current_role == normalized_role {
            return Ok(());
        }

        if Self::is_super_admin(&current_role) && !Self::is_super_admin(&normalized_role) {
            let admin_count = self.database.count_active_monitor_admins().await?;
            if admin_count <= 1 {
                return Err(anyhow!("无法修改：系统至少需要保留一个超级管理员"));
            }
        }

        let now = Self::now_string();
        self.database
            .update_monitor_user_role(user_id, &normalized_role, &now)
            .await?;
        Ok(())
    }

    /// 重置监控用户密码
    pub async fn reset_user_password(&self, user_id: &str, password: &str) -> Result<()> {
        Self::validate_password(password)?;
        let password_hash = Self::hash_password(password)?;

        if self
            .database
            .find_monitor_user_by_id(user_id)
            .await?
            .is_none()
        {
            return Err(anyhow!("用户不存在"));
        }

        let now = Self::now_string();
        self.database
            .update_monitor_user_password(user_id, &password_hash, &now)
            .await?;
        Ok(())
    }

    /// 禁用监控用户
    pub async fn deactivate_user(&self, user_id: &str) -> Result<()> {
        let user = self
            .database
            .find_monitor_user_by_id(user_id)
            .await?
            .ok_or_else(|| anyhow!("用户不存在"))?;

        if user.username == "admin" {
            return Err(anyhow!("默认管理员账户无法禁用"));
        }

        if Self::is_super_admin(&user.role) {
            let admin_count = self.database.count_active_monitor_admins().await?;
            if admin_count <= 1 {
                return Err(anyhow!("无法禁用：系统至少需要保留一个超级管理员"));
            }
        }

        let now = Self::now_string();
        self.database
            .set_monitor_user_active(user_id, false, &now)
            .await?;
        Ok(())
    }

    /// 重新启用监控用户
    pub async fn activate_user(&self, user_id: &str) -> Result<()> {
        if self
            .database
            .find_monitor_user_by_id(user_id)
            .await?
            .is_none()
        {
            return Err(anyhow!("用户不存在"));
        }

        let now = Self::now_string();
        self.database
            .set_monitor_user_active(user_id, true, &now)
            .await?;
        Ok(())
    }

    /// 执行监控查询（临时实现，后续优化）
    async fn execute_monitor_query(
        &self,
        sql: &str,
        params: &[&str],
    ) -> Result<Vec<MonitorQueryRow>> {
        // 这是一个临时实现，用于避免sqlx宏的编译问题
        // 实际部署时会直接使用数据库连接
        tracing::warn!("监控查询执行: {} 参数: {:?}", sql, params);

        // 返回空结果（临时）
        Ok(vec![])
    }

    /// 执行监控更新（临时实现，后续优化）
    async fn execute_monitor_update(&self, sql: &str, params: &[&str]) -> Result<usize> {
        // 这是一个临时实现，用于避免sqlx宏的编译问题
        // 实际部署时会直接使用数据库连接
        tracing::warn!("监控更新执行: {} 参数: {:?}", sql, params);

        // 返回影响行数（临时）
        Ok(1)
    }
}

/// 临时查询结果行结构
#[derive(Debug, Clone)]
pub struct MonitorQueryRow {
    data: std::collections::HashMap<String, String>,
    int_data: std::collections::HashMap<String, i64>,
}

impl MonitorQueryRow {
    pub fn get(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        if let Some(value) = self.int_data.get(key) {
            Some(*value)
        } else if let Some(value) = self.data.get(key) {
            value.parse::<i64>().ok()
        } else {
            None
        }
    }
}

fn parse_db_datetime(value: &str) -> Result<DateTime<Utc>> {
    if let Ok(dt) = DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(value) {
        return Ok(dt.with_timezone(&Utc));
    }
    Err(anyhow!("unsupported datetime format: {}", value))
}
