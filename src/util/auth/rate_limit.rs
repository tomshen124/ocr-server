//! 频率限制模块
//! 实现API调用频率限制和配额管理

use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tracing::{debug, info, warn};

/// 安全地获取Mutex锁，处理poison错误
/// 如果Mutex被污染（因为panic），会恢复并继续使用
fn lock_mutex_safe<T>(mutex: &Mutex<T>) -> Result<MutexGuard<T>, String> {
    match mutex.lock() {
        Ok(guard) => Ok(guard),
        Err(poison_error) => {
            // Mutex被污染，但我们可以恢复数据继续使用
            warn!("[warn] Mutex被污染，正在恢复...");
            Ok(poison_error.into_inner())
        }
    }
}

/// 频率限制器
pub struct RateLimiter {
    /// 客户端访问记录
    client_records: Arc<Mutex<HashMap<String, ClientAccessRecord>>>,
}

/// 客户端访问记录
#[derive(Debug, Clone)]
struct ClientAccessRecord {
    /// 每分钟请求计数
    minute_requests: Vec<RequestRecord>,
    /// 每小时请求计数
    hour_requests: Vec<RequestRecord>,
    /// 最后清理时间
    last_cleanup: DateTime<Utc>,
}

/// 请求记录
#[derive(Debug, Clone)]
struct RequestRecord {
    timestamp: DateTime<Utc>,
    api_path: String,
}

/// 限流结果
#[derive(Debug)]
pub enum RateLimitResult {
    Allowed,
    Exceeded {
        limit_type: String,
        current_count: u32,
        limit: u32,
        reset_time: DateTime<Utc>,
    },
}

impl RateLimiter {
    /// 创建新的频率限制器
    pub fn new() -> Self {
        Self {
            client_records: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 检查客户端频率限制
    pub async fn check_rate_limit(client_id: &str, hourly_limit: u32) -> Result<(), String> {
        // 这里使用简化的实现，实际项目中应该使用Redis或更高级的限流算法
        let instance = Self::get_global_instance();
        instance
            .check_client_rate_limit(client_id, hourly_limit)
            .await
    }

    /// 检查具体客户端的频率限制
    async fn check_client_rate_limit(
        &self,
        client_id: &str,
        hourly_limit: u32,
    ) -> Result<(), String> {
        let mut records = self.client_records.lock().unwrap();
        let now = Utc::now();

        // 获取或创建客户端记录
        let client_record =
            records
                .entry(client_id.to_string())
                .or_insert_with(|| ClientAccessRecord {
                    minute_requests: Vec::new(),
                    hour_requests: Vec::new(),
                    last_cleanup: now,
                });

        // 清理过期记录
        self.cleanup_expired_records(client_record, now);

        // 检查小时限制
        if client_record.hour_requests.len() >= hourly_limit as usize {
            return Err(format!(
                "超出小时限制: {}/{} 请求",
                client_record.hour_requests.len(),
                hourly_limit
            ));
        }

        // 添加当前请求记录
        let request_record = RequestRecord {
            timestamp: now,
            api_path: "api_request".to_string(), // 可以从上下文获取具体API路径
        };

        client_record.hour_requests.push(request_record.clone());
        client_record.minute_requests.push(request_record);

        debug!(
            client_id = client_id,
            current_hour_requests = client_record.hour_requests.len(),
            hourly_limit = hourly_limit,
            "频率限制检查通过"
        );

        Ok(())
    }

    /// 清理过期的请求记录
    fn cleanup_expired_records(&self, record: &mut ClientAccessRecord, now: DateTime<Utc>) {
        let one_hour_ago = now - Duration::hours(1);
        let one_minute_ago = now - Duration::minutes(1);

        // 清理超过1小时的记录
        record
            .hour_requests
            .retain(|req| req.timestamp > one_hour_ago);

        // 清理超过1分钟的记录
        record
            .minute_requests
            .retain(|req| req.timestamp > one_minute_ago);

        record.last_cleanup = now;
    }

    /// 获取全局实例（简化实现）
    fn get_global_instance() -> &'static Self {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<RateLimiter> = OnceLock::new();
        INSTANCE.get_or_init(|| RateLimiter::new())
    }

    /// 获取客户端当前使用统计
    pub async fn get_client_stats(&self, client_id: &str) -> ClientUsageStats {
        let records = self.client_records.lock().unwrap();
        let now = Utc::now();

        if let Some(record) = records.get(client_id) {
            ClientUsageStats {
                client_id: client_id.to_string(),
                requests_last_minute: record.minute_requests.len() as u32,
                requests_last_hour: record.hour_requests.len() as u32,
                last_request_time: record.hour_requests.last().map(|req| req.timestamp),
                total_requests_today: self.count_requests_today(record, now),
            }
        } else {
            ClientUsageStats::empty(client_id.to_string())
        }
    }

    /// 统计今天的请求数量
    fn count_requests_today(&self, record: &ClientAccessRecord, now: DateTime<Utc>) -> u32 {
        let today_start = now
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .map(|dt| dt.and_utc())
            .unwrap_or(now);

        record
            .hour_requests
            .iter()
            .filter(|req| req.timestamp >= today_start)
            .count() as u32
    }

    /// 重置客户端限制
    pub async fn reset_client_limit(&self, client_id: &str) {
        let mut records = self.client_records.lock().unwrap();
        if let Some(record) = records.get_mut(client_id) {
            record.minute_requests.clear();
            record.hour_requests.clear();
            record.last_cleanup = Utc::now();

            info!(client_id = client_id, "重置客户端频率限制");
        }
    }

    /// 获取所有客户端统计
    pub async fn get_all_client_stats(&self) -> Vec<ClientUsageStats> {
        let records = self.client_records.lock().unwrap();
        let mut stats = Vec::new();

        for (client_id, _) in records.iter() {
            stats.push(self.get_client_stats(client_id).await);
        }

        stats
    }
}

/// 客户端使用统计
#[derive(Debug, Clone)]
pub struct ClientUsageStats {
    pub client_id: String,
    pub requests_last_minute: u32,
    pub requests_last_hour: u32,
    pub last_request_time: Option<DateTime<Utc>>,
    pub total_requests_today: u32,
}

impl ClientUsageStats {
    fn empty(client_id: String) -> Self {
        Self {
            client_id,
            requests_last_minute: 0,
            requests_last_hour: 0,
            last_request_time: None,
            total_requests_today: 0,
        }
    }
}

/// 高级频率限制器（基于滑动窗口）
pub struct SlidingWindowRateLimiter {
    window_size: Duration,
    max_requests: u32,
    client_windows: Arc<Mutex<HashMap<String, SlidingWindow>>>,
}

/// 滑动窗口
#[derive(Debug)]
struct SlidingWindow {
    requests: Vec<DateTime<Utc>>,
    last_cleanup: DateTime<Utc>,
}

impl SlidingWindowRateLimiter {
    /// 创建滑动窗口限流器
    pub fn new(window_size: Duration, max_requests: u32) -> Self {
        Self {
            window_size,
            max_requests,
            client_windows: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 检查滑动窗口限制
    pub fn check_limit(&self, client_id: &str) -> RateLimitResult {
        let mut windows = self.client_windows.lock().unwrap();
        let now = Utc::now();
        let window_start = now - self.window_size;

        let window = windows
            .entry(client_id.to_string())
            .or_insert_with(|| SlidingWindow {
                requests: Vec::new(),
                last_cleanup: now,
            });

        // 清理窗口外的请求
        window
            .requests
            .retain(|&timestamp| timestamp > window_start);
        window.last_cleanup = now;

        // 检查是否超过限制
        if window.requests.len() >= self.max_requests as usize {
            let oldest_request = window.requests.first().cloned().unwrap_or(now);
            let reset_time = oldest_request + self.window_size;

            return RateLimitResult::Exceeded {
                limit_type: "sliding_window".to_string(),
                current_count: window.requests.len() as u32,
                limit: self.max_requests,
                reset_time,
            };
        }

        // 添加当前请求
        window.requests.push(now);

        RateLimitResult::Allowed
    }
}

/// 令牌桶限流器
pub struct TokenBucketRateLimiter {
    capacity: u32,
    refill_rate: u32, // tokens per second
    client_buckets: Arc<Mutex<HashMap<String, TokenBucket>>>,
}

/// 令牌桶
#[derive(Debug)]
struct TokenBucket {
    tokens: f64,
    last_refill: DateTime<Utc>,
}

impl TokenBucketRateLimiter {
    /// 创建令牌桶限流器
    pub fn new(capacity: u32, refill_rate: u32) -> Self {
        Self {
            capacity,
            refill_rate,
            client_buckets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// 尝试消费令牌
    pub fn try_consume(&self, client_id: &str, tokens: u32) -> RateLimitResult {
        let mut buckets = self.client_buckets.lock().unwrap();
        let now = Utc::now();

        let bucket = buckets
            .entry(client_id.to_string())
            .or_insert_with(|| TokenBucket {
                tokens: self.capacity as f64,
                last_refill: now,
            });

        // 补充令牌
        let time_passed = now.signed_duration_since(bucket.last_refill).num_seconds() as f64;
        let tokens_to_add = time_passed * self.refill_rate as f64;
        bucket.tokens = (bucket.tokens + tokens_to_add).min(self.capacity as f64);
        bucket.last_refill = now;

        // 检查是否有足够令牌
        if bucket.tokens >= tokens as f64 {
            bucket.tokens -= tokens as f64;
            RateLimitResult::Allowed
        } else {
            let wait_time = (tokens as f64 - bucket.tokens) / self.refill_rate as f64;
            let reset_time = now + Duration::seconds(wait_time.ceil() as i64);

            RateLimitResult::Exceeded {
                limit_type: "token_bucket".to_string(),
                current_count: bucket.tokens as u32,
                limit: self.capacity,
                reset_time,
            }
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, Duration as TokioDuration};

    #[tokio::test]
    async fn test_rate_limiter_basic() {
        let limiter = RateLimiter::new();
        let client_id = "test_client";
        let hourly_limit = 10;

        // 应该允许前10个请求
        for i in 0..hourly_limit {
            let result = limiter
                .check_client_rate_limit(client_id, hourly_limit)
                .await;
            assert!(result.is_ok(), "请求 {} 应该被允许", i + 1);
        }

        // 第11个请求应该被拒绝
        let result = limiter
            .check_client_rate_limit(client_id, hourly_limit)
            .await;
        assert!(result.is_err(), "第11个请求应该被拒绝");
    }

    #[tokio::test]
    async fn test_client_stats() {
        let limiter = RateLimiter::new();
        let client_id = "stats_test_client";

        // 发送几个请求
        for _ in 0..5 {
            let _ = limiter.check_client_rate_limit(client_id, 100).await;
        }

        let stats = limiter.get_client_stats(client_id).await;
        assert_eq!(stats.requests_last_hour, 5);
        assert_eq!(stats.requests_last_minute, 5);
    }

    #[test]
    fn test_sliding_window_limiter() {
        let limiter = SlidingWindowRateLimiter::new(Duration::seconds(60), 5);

        let client_id = "sliding_test_client";

        // 应该允许前5个请求
        for i in 0..5 {
            match limiter.check_limit(client_id) {
                RateLimitResult::Allowed => {}
                _ => panic!("请求 {} 应该被允许", i + 1),
            }
        }

        // 第6个请求应该被拒绝
        match limiter.check_limit(client_id) {
            RateLimitResult::Exceeded { .. } => {}
            _ => panic!("第6个请求应该被拒绝"),
        }
    }

    #[test]
    fn test_token_bucket_limiter() {
        let limiter = TokenBucketRateLimiter::new(10, 1); // 10个令牌容量，每秒补充1个
        let client_id = "token_test_client";

        // 应该允许消费10个令牌
        match limiter.try_consume(client_id, 10) {
            RateLimitResult::Allowed => {}
            _ => panic!("应该允许消费10个令牌"),
        }

        // 桶已空，应该拒绝额外请求
        match limiter.try_consume(client_id, 1) {
            RateLimitResult::Exceeded { .. } => {}
            _ => panic!("桶已空，应该拒绝额外请求"),
        }
    }
}
