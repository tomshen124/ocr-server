//! 信号量追踪辅助工具 - 轻量级追踪方案

use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::{RwLock, SemaphorePermit};

/// 全局permit计数器
static PERMIT_COUNTER: AtomicU64 = AtomicU64::new(0);

/// 全局permit追踪器
pub static PERMIT_TRACKER: once_cell::sync::Lazy<Arc<RwLock<HashMap<String, PermitRecord>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(HashMap::new())));

/// Permit记录
#[derive(Debug, Clone, serde::Serialize)]
pub struct PermitRecord {
    pub id: String,
    pub task: String,
    pub location: String,
    pub acquired_at: DateTime<Utc>,
    pub released_at: Option<DateTime<Utc>>,
    pub status: String,
}

/// 生成新的permit ID
pub fn generate_permit_id() -> String {
    let seq = PERMIT_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("P{:04}", seq)
}

/// 记录permit获取
pub async fn track_permit_acquired(id: &str, task: &str, location: &str) {
    let record = PermitRecord {
        id: id.to_string(),
        task: task.to_string(),
        location: location.to_string(),
        acquired_at: Utc::now(),
        released_at: None,
        status: "active".to_string(),
    };

    PERMIT_TRACKER.write().await.insert(id.to_string(), record);

    tracing::info!(
        "[ticket] [{}] 信号量已获取 - 任务: {}, 位置: {}",
        id,
        task,
        location
    );
}

/// 记录permit释放
pub async fn track_permit_released(id: &str) {
    if let Some(record) = PERMIT_TRACKER.write().await.get_mut(id) {
        record.released_at = Some(Utc::now());
        record.status = "released".to_string();

        let duration = record
            .released_at
            .unwrap()
            .signed_duration_since(record.acquired_at);

        tracing::info!(
            "[unlocked] [{}] 信号量已释放 - 持有时间: {}秒",
            id,
            duration.num_seconds()
        );
    }
}

/// 包装的permit，自动追踪释放
pub struct TrackedPermit<'a> {
    permit: SemaphorePermit<'a>,
    id: String,
}

impl<'a> TrackedPermit<'a> {
    pub fn new(permit: SemaphorePermit<'a>, id: String) -> Self {
        Self { permit, id }
    }
}

impl<'a> Drop for TrackedPermit<'a> {
    fn drop(&mut self) {
        let id = self.id.clone();
        tokio::spawn(async move {
            track_permit_released(&id).await;
        });
    }
}

/// 获取当前活跃的permits
pub async fn get_active_permits() -> Vec<PermitRecord> {
    PERMIT_TRACKER
        .read()
        .await
        .values()
        .filter(|r| r.status == "active")
        .cloned()
        .collect()
}

/// 检查可能泄漏的permits（超过指定分钟数）
pub async fn check_leaked_permits(minutes: i64) -> Vec<PermitRecord> {
    PERMIT_TRACKER
        .read()
        .await
        .values()
        .filter(|r| {
            r.status == "active"
                && Utc::now()
                    .signed_duration_since(r.acquired_at)
                    .num_minutes()
                    > minutes
        })
        .cloned()
        .collect()
}

/// 清理旧记录
pub async fn cleanup_old_records(keep_hours: i64) {
    let cutoff = Utc::now() - chrono::Duration::hours(keep_hours);
    PERMIT_TRACKER
        .write()
        .await
        .retain(|_, record| record.acquired_at > cutoff || record.status == "active");
}
