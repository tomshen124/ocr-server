//! 监控系统数据库查询方法
//! 提供监控用户和会话的数据库操作

use anyhow::Result;
use sqlx::{Row, SqlitePool};

use crate::db::models::{MonitorSession, MonitorUser};

/// 监控查询管理器
pub struct MonitorQueries;

impl MonitorQueries {
    /// 根据用户名查找用户
    pub async fn find_user_by_username(
        pool: &SqlitePool,
        username: &str,
    ) -> Result<Option<MonitorUser>> {
        let row = sqlx::query(
            "SELECT id, username, role, last_login_at, login_count, is_active FROM monitor_users WHERE username = ? AND is_active = 1"
        )
        .bind(username)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(MonitorUser {
                id: row.get("id"),
                username: row.get("username"),
                role: row.get("role"),
                last_login_at: row.get("last_login_at"),
                login_count: row.get("login_count"),
                is_active: row.get::<i64, _>("is_active") == 1,
            }))
        } else {
            Ok(None)
        }
    }

    /// 获取用户密码哈希
    pub async fn get_user_password_hash(pool: &SqlitePool, user_id: &str) -> Result<String> {
        let row = sqlx::query("SELECT password_hash FROM monitor_users WHERE id = ?")
            .bind(user_id)
            .fetch_one(pool)
            .await?;

        Ok(row.get("password_hash"))
    }

    /// 根据ID查找监控用户
    pub async fn find_user_by_id(pool: &SqlitePool, user_id: &str) -> Result<Option<MonitorUser>> {
        let row = sqlx::query(
            "SELECT id, username, role, last_login_at, login_count, is_active \
             FROM monitor_users WHERE id = ?",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(MonitorUser {
                id: row.get("id"),
                username: row.get("username"),
                role: row.get("role"),
                last_login_at: row.get("last_login_at"),
                login_count: row.get("login_count"),
                is_active: row.get::<i64, _>("is_active") == 1,
            }))
        } else {
            Ok(None)
        }
    }

    /// 列出所有监控用户
    pub async fn list_users(pool: &SqlitePool) -> Result<Vec<MonitorUser>> {
        let rows = sqlx::query(
            "SELECT id, username, role, last_login_at, login_count, is_active \
             FROM monitor_users ORDER BY created_at ASC",
        )
        .fetch_all(pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|row| MonitorUser {
                id: row.get("id"),
                username: row.get("username"),
                role: row.get("role"),
                last_login_at: row.get("last_login_at"),
                login_count: row.get("login_count"),
                is_active: row.get::<i64, _>("is_active") == 1,
            })
            .collect())
    }

    /// 创建会话
    pub async fn create_session(
        pool: &SqlitePool,
        session_id: &str,
        user_id: &str,
        ip: &str,
        user_agent: &str,
        created_at: &str,
        expires_at: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO monitor_sessions (id, user_id, ip_address, user_agent, created_at, expires_at, last_activity, is_active) 
             VALUES (?, ?, ?, ?, ?, ?, ?, 1)"
        )
        .bind(session_id)
        .bind(user_id)
        .bind(ip)
        .bind(user_agent)
        .bind(created_at)
        .bind(expires_at)
        .bind(created_at)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 根据会话ID查找会话
    pub async fn find_session_by_id(
        pool: &SqlitePool,
        session_id: &str,
    ) -> Result<Option<MonitorSession>> {
        let row = sqlx::query(
            "SELECT s.id, s.user_id, s.expires_at, s.ip_address, u.username, u.role, u.last_login_at, u.login_count, u.is_active 
             FROM monitor_sessions s 
             JOIN monitor_users u ON s.user_id = u.id 
             WHERE s.id = ? AND s.is_active = 1"
        )
        .bind(session_id)
        .fetch_optional(pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(MonitorSession {
                id: row.get("id"),
                user_id: row.get("user_id"),
                user: MonitorUser {
                    id: row.get("user_id"),
                    username: row.get("username"),
                    role: row.get("role"),
                    last_login_at: row.get("last_login_at"),
                    login_count: row.get("login_count"),
                    is_active: row.get::<i64, _>("is_active") == 1,
                },
                expires_at: row.get("expires_at"),
                ip_address: row.get("ip_address"),
            }))
        } else {
            Ok(None)
        }
    }

    /// 更新登录信息
    pub async fn update_login_info(pool: &SqlitePool, user_id: &str, now: &str) -> Result<()> {
        sqlx::query(
            "UPDATE monitor_users SET last_login_at = ?, login_count = login_count + 1, updated_at = ? WHERE id = ?"
        )
        .bind(now)
        .bind(now)
        .bind(user_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 更新会话活动时间
    pub async fn update_session_activity(
        pool: &SqlitePool,
        session_id: &str,
        now: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE monitor_sessions SET last_activity = ? WHERE id = ?")
            .bind(now)
            .bind(session_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// 删除会话（软删除）
    pub async fn delete_session(pool: &SqlitePool, session_id: &str) -> Result<()> {
        sqlx::query("UPDATE monitor_sessions SET is_active = 0 WHERE id = ?")
            .bind(session_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// 清理过期会话
    pub async fn cleanup_expired_sessions(pool: &SqlitePool, now: &str) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE monitor_sessions SET is_active = 0 WHERE expires_at < ? AND is_active = 1",
        )
        .bind(now)
        .execute(pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// 获取活跃会话数量
    pub async fn get_active_sessions_count(pool: &SqlitePool, now: &str) -> Result<i64> {
        let row = sqlx::query(
            "SELECT COUNT(*) as count FROM monitor_sessions WHERE expires_at > ? AND is_active = 1",
        )
        .bind(now)
        .fetch_one(pool)
        .await?;

        Ok(row.get("count"))
    }

    /// 创建监控用户
    pub async fn create_user(
        pool: &SqlitePool,
        id: &str,
        username: &str,
        password_hash: &str,
        role: &str,
        now: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO monitor_users (id, username, password_hash, role, login_count, \
             created_at, updated_at, is_active) VALUES (?, ?, ?, ?, 0, ?, ?, 1)",
        )
        .bind(id)
        .bind(username)
        .bind(password_hash)
        .bind(role)
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;

        Ok(())
    }

    /// 更新监控用户角色
    pub async fn update_user_role(
        pool: &SqlitePool,
        user_id: &str,
        role: &str,
        now: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE monitor_users SET role = ?, updated_at = ? WHERE id = ?")
            .bind(role)
            .bind(now)
            .bind(user_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// 更新监控用户密码
    pub async fn update_user_password(
        pool: &SqlitePool,
        user_id: &str,
        password_hash: &str,
        now: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE monitor_users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(password_hash)
            .bind(now)
            .bind(user_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// 设置监控用户是否启用
    pub async fn set_user_active(
        pool: &SqlitePool,
        user_id: &str,
        is_active: bool,
        now: &str,
    ) -> Result<()> {
        sqlx::query("UPDATE monitor_users SET is_active = ?, updated_at = ? WHERE id = ?")
            .bind(if is_active { 1 } else { 0 })
            .bind(now)
            .bind(user_id)
            .execute(pool)
            .await?;

        Ok(())
    }

    /// 统计活跃管理员数量
    pub async fn count_active_admins(pool: &SqlitePool) -> Result<i64> {
        let row = sqlx::query(
            "SELECT COUNT(*) as count FROM monitor_users \
             WHERE role IN ('super_admin', 'admin') AND is_active = 1",
        )
        .fetch_one(pool)
        .await?;

        Ok(row.get("count"))
    }
}
