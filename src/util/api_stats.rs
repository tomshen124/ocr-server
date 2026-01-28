//! API调用统计记录模块
//! 统一记录所有API调用到数据库

use crate::db::traits::{ApiStats, Database};
use chrono::Utc;
use std::sync::Arc;
use std::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

/// 记录API调用统计到数据库
pub async fn record_api_call(
    database: Arc<dyn Database>,
    endpoint: &str,
    method: &str,
    user_id: Option<String>,
    client_id: Option<String>,
    status_code: u16,
    start_time: Instant,
    error_message: Option<String>,
    request_size: u32,
    response_size: u32,
) {
    let response_time_ms = start_time.elapsed().as_millis() as u32;

    let stats = ApiStats {
        id: Uuid::new_v4().to_string(),
        endpoint: endpoint.to_string(),
        method: method.to_string(),
        client_id,
        user_id,
        status_code,
        response_time_ms,
        request_size,
        response_size,
        error_message,
        created_at: Utc::now(),
    };

    // 异步记录，不阻塞主流程
    let db_clone = database.clone();
    tokio::spawn(async move {
        if let Err(e) = db_clone.save_api_stats(&stats).await {
            warn!("记录API统计失败: {}", e);
        } else {
            info!(
                "API统计已记录: {} {} - {}ms - 状态码: {}",
                stats.method, stats.endpoint, stats.response_time_ms, stats.status_code
            );
        }
    });
}

/// API统计记录器 - 可以在中间件中使用
pub struct ApiStatsRecorder {
    pub database: Arc<dyn Database>,
    pub endpoint: String,
    pub method: String,
    pub user_id: Option<String>,
    pub client_id: Option<String>,
    pub start_time: Instant,
    pub request_size: u32,
}

impl ApiStatsRecorder {
    pub fn new(database: Arc<dyn Database>, endpoint: &str, method: &str) -> Self {
        Self {
            database,
            endpoint: endpoint.to_string(),
            method: method.to_string(),
            user_id: None,
            client_id: None,
            start_time: Instant::now(),
            request_size: 0,
        }
    }

    /// 设置用户ID
    pub fn set_user(&mut self, user_id: String) {
        self.user_id = Some(user_id);
    }

    /// 设置客户端ID
    pub fn set_client(&mut self, client_id: String) {
        self.client_id = Some(client_id);
    }

    /// 设置请求大小
    pub fn set_request_size(&mut self, size: u32) {
        self.request_size = size;
    }

    /// 完成记录并保存到数据库
    pub async fn finish(self, status_code: u16, response_size: u32, error_message: Option<String>) {
        record_api_call(
            self.database,
            &self.endpoint,
            &self.method,
            self.user_id,
            self.client_id,
            status_code,
            self.start_time,
            error_message,
            self.request_size,
            response_size,
        )
        .await;
    }
}
