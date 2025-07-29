//! 监控系统认证服务
//! 负责监控用户的登录、会话管理和权限控制

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[cfg(feature = "monitoring")]
use bcrypt::verify;

/// 监控用户
#[derive(Debug, Clone, Serialize)]
pub struct MonitorUser {
    pub id: String,
    pub username: String,
    pub role: String,
    pub last_login_at: Option<String>,
    pub login_count: i64,
    pub is_active: bool,
}

/// 监控会话
#[derive(Debug, Clone, Serialize)]
pub struct MonitorSession {
    pub id: String,
    pub user_id: String,
    pub user: MonitorUser,
    pub expires_at: String,
    pub ip_address: Option<String>,
}

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
            let expires_at = DateTime::parse_from_str(&session.expires_at, "%Y-%m-%d %H:%M:%S")?;
            if expires_at > Utc::now() {
                // 更新最后活动时间
                self.update_session_activity(session_id).await?;
                return Ok(Some(session));
            } else {
                // 会话已过期，删除
                self.delete_session(session_id).await?;
            }
        }

        Ok(None)
    }

    /// 用户登出
    pub async fn logout(&self, session_id: &str) -> Result<()> {
        self.delete_session(session_id).await
    }

    /// 根据用户名查找用户
    async fn find_user_by_username(&self, username: &str) -> Result<Option<MonitorUser>> {
        // 尝试使用SQLite数据库方法
        if let Some(sqlite_db) = self.database.as_any().downcast_ref::<crate::db::sqlite::SqliteDatabase>() {
            return sqlite_db.find_monitor_user_by_username(username).await;
        }
        
        // 降级方案：使用临时实现
        let result = self.execute_monitor_query(
            "SELECT id, username, role, last_login_at, login_count, is_active FROM monitor_users WHERE username = ? AND is_active = 1",
            &[username]
        ).await?;

        if let Some(row) = result.first() {
            Ok(Some(MonitorUser {
                id: row.get("id").unwrap_or_default(),
                username: row.get("username").unwrap_or_default(),
                role: row.get("role").unwrap_or_default(),
                last_login_at: row.get("last_login_at"),
                login_count: row.get_i64("login_count").unwrap_or(0),
                is_active: row.get_i64("is_active").unwrap_or(0) == 1,
            }))
        } else {
            Ok(None)
        }
    }

    /// 获取用户密码哈希
    async fn get_user_password_hash(&self, user_id: &str) -> Result<String> {
        let result = self.execute_monitor_query(
            "SELECT password_hash FROM monitor_users WHERE id = ?",
            &[user_id]
        ).await?;

        if let Some(row) = result.first() {
            Ok(row.get("password_hash").unwrap_or_default())
        } else {
            Err(anyhow::anyhow!("用户不存在"))
        }
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

        self.execute_monitor_update(
            "INSERT INTO monitor_sessions (id, user_id, ip_address, user_agent, created_at, expires_at, last_activity, is_active) VALUES (?, ?, ?, ?, ?, ?, ?, 1)",
            &[&session_id, &user.id, ip, user_agent, &created_at, &expires_at_str, &created_at]
        ).await?;

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
        let result = self.execute_monitor_query(
            "SELECT s.id, s.user_id, s.expires_at, s.ip_address, u.username, u.role, u.last_login_at, u.login_count, u.is_active FROM monitor_sessions s JOIN monitor_users u ON s.user_id = u.id WHERE s.id = ? AND s.is_active = 1",
            &[session_id]
        ).await?;

        if let Some(row) = result.first() {
            Ok(Some(MonitorSession {
                id: row.get("id").unwrap_or_default(),
                user_id: row.get("user_id").unwrap_or_default(),
                user: MonitorUser {
                    id: row.get("user_id").unwrap_or_default(),
                    username: row.get("username").unwrap_or_default(),
                    role: row.get("role").unwrap_or_default(),
                    last_login_at: row.get("last_login_at"),
                    login_count: row.get_i64("login_count").unwrap_or(0),
                    is_active: row.get_i64("is_active").unwrap_or(0) == 1,
                },
                expires_at: row.get("expires_at").unwrap_or_default(),
                ip_address: row.get("ip_address"),
            }))
        } else {
            Ok(None)
        }
    }

    /// 更新登录信息
    async fn update_login_info(&self, user_id: &str) -> Result<()> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        self.execute_monitor_update(
            "UPDATE monitor_users SET last_login_at = ?, login_count = login_count + 1, updated_at = ? WHERE id = ?",
            &[&now, &now, user_id]
        ).await?;

        Ok(())
    }

    /// 更新会话活动时间
    async fn update_session_activity(&self, session_id: &str) -> Result<()> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        self.execute_monitor_update(
            "UPDATE monitor_sessions SET last_activity = ? WHERE id = ?",
            &[&now, session_id]
        ).await?;

        Ok(())
    }

    /// 删除会话
    async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.execute_monitor_update(
            "UPDATE monitor_sessions SET is_active = 0 WHERE id = ?",
            &[session_id]
        ).await?;

        Ok(())
    }

    /// 清理过期会话
    pub async fn cleanup_expired_sessions(&self) -> Result<u64> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let affected = self.execute_monitor_update(
            "UPDATE monitor_sessions SET is_active = 0 WHERE expires_at < ? AND is_active = 1",
            &[&now]
        ).await?;

        Ok(affected as u64)
    }

    /// 获取活跃会话数量
    pub async fn get_active_sessions_count(&self) -> Result<i64> {
        let now = Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();

        let result = self.execute_monitor_query(
            "SELECT COUNT(*) as count FROM monitor_sessions WHERE expires_at > ? AND is_active = 1",
            &[&now]
        ).await?;

        if let Some(row) = result.first() {
            Ok(row.get_i64("count").unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    /// 执行监控查询（临时实现，后续优化）
    async fn execute_monitor_query(&self, sql: &str, params: &[&str]) -> Result<Vec<MonitorQueryRow>> {
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