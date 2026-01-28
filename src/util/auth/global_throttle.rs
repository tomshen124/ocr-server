//! 全局限流控制模块
//! 提供系统级请求限流功能，仅限管理员操作

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::RwLock;
use tracing::{info, warn};

/// 全局限流守卫
pub struct GlobalThrottleGuard {
    enabled: AtomicBool,
    max_requests: AtomicU32,
    current_count: AtomicU32,
    blocked_count: AtomicU32,
    state: RwLock<ThrottleState>,
}

#[derive(Debug, Clone, Default)]
struct ThrottleState {
    activated_at: Option<DateTime<Utc>>,
    activated_by: Option<String>,
    reason: Option<String>,
}

/// 限流状态响应
#[derive(Debug, Clone, Serialize)]
pub struct ThrottleStatus {
    pub enabled: bool,
    pub max_requests: u32,
    pub current_count: u32,
    pub blocked_count: u32,
    pub activated_at: Option<String>,
    pub activated_by: Option<String>,
    pub reason: Option<String>,
}

/// 启用限流请求
#[derive(Debug, Deserialize)]
pub struct EnableThrottleRequest {
    pub max_requests: u32,
    #[serde(default)]
    pub reason: Option<String>,
}

/// 限流检查结果
pub enum ThrottleCheckResult {
    Allowed,
    Blocked { current: u32, max: u32 },
}

impl GlobalThrottleGuard {
    pub fn new() -> Self {
        Self {
            enabled: AtomicBool::new(false),
            max_requests: AtomicU32::new(0),
            current_count: AtomicU32::new(0),
            blocked_count: AtomicU32::new(0),
            state: RwLock::new(ThrottleState::default()),
        }
    }

    /// 检查请求是否被限流
    pub fn check(&self) -> ThrottleCheckResult {
        if !self.enabled.load(Ordering::Relaxed) {
            return ThrottleCheckResult::Allowed;
        }

        let max = self.max_requests.load(Ordering::Relaxed);
        let current = self.current_count.fetch_add(1, Ordering::Relaxed);

        if current >= max {
            self.current_count.fetch_sub(1, Ordering::Relaxed);
            self.blocked_count.fetch_add(1, Ordering::Relaxed);
            ThrottleCheckResult::Blocked { current, max }
        } else {
            ThrottleCheckResult::Allowed
        }
    }

    /// 启用限流
    pub fn enable(&self, max_requests: u32, operator: &str, reason: Option<String>) {
        self.max_requests.store(max_requests, Ordering::Relaxed);
        self.current_count.store(0, Ordering::Relaxed);
        self.blocked_count.store(0, Ordering::Relaxed);

        if let Ok(mut state) = self.state.write() {
            state.activated_at = Some(Utc::now());
            state.activated_by = Some(operator.to_string());
            state.reason = reason.clone();
        }

        self.enabled.store(true, Ordering::Relaxed);

        warn!(
            event = "global_throttle",
            action = "enable",
            max_requests = max_requests,
            operator = operator,
            reason = reason.as_deref().unwrap_or("未指定"),
            "[alert] 全局限流已启用: 最大请求数 {}, 操作者 {}",
            max_requests,
            operator
        );
    }

    /// 解除限流
    pub fn disable(&self, operator: &str) -> (u32, u32, i64) {
        let blocked = self.blocked_count.load(Ordering::Relaxed);
        let processed = self.current_count.load(Ordering::Relaxed);

        let duration = if let Ok(state) = self.state.read() {
            state
                .activated_at
                .map(|t| (Utc::now() - t).num_seconds())
                .unwrap_or(0)
        } else {
            0
        };

        self.enabled.store(false, Ordering::Relaxed);

        if let Ok(mut state) = self.state.write() {
            *state = ThrottleState::default();
        }

        info!(
            event = "global_throttle",
            action = "disable",
            blocked_count = blocked,
            processed_count = processed,
            duration_secs = duration,
            operator = operator,
            "[ok] 全局限流已解除: 拦截 {} 请求, 处理 {} 请求, 持续 {} 秒",
            blocked,
            processed,
            duration
        );

        (blocked, processed, duration)
    }

    /// 获取当前状态
    pub fn status(&self) -> ThrottleStatus {
        let state = self.state.read().ok();

        ThrottleStatus {
            enabled: self.enabled.load(Ordering::Relaxed),
            max_requests: self.max_requests.load(Ordering::Relaxed),
            current_count: self.current_count.load(Ordering::Relaxed),
            blocked_count: self.blocked_count.load(Ordering::Relaxed),
            activated_at: state
                .as_ref()
                .and_then(|s| s.activated_at.map(|t| t.to_rfc3339())),
            activated_by: state.as_ref().and_then(|s| s.activated_by.clone()),
            reason: state.as_ref().and_then(|s| s.reason.clone()),
        }
    }

    /// 获取全局实例
    pub fn global() -> &'static Self {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<GlobalThrottleGuard> = OnceLock::new();
        INSTANCE.get_or_init(GlobalThrottleGuard::new)
    }
}

impl Default for GlobalThrottleGuard {
    fn default() -> Self {
        Self::new()
    }
}
