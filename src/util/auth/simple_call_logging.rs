//! 简单的API调用记录中间件
//! 记录所有API调用情况，支持配置化管理

use crate::util::logging::standards::events;
use crate::CONFIG;
use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, NaiveDate, Utc};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

/// 简单的调用记录
#[derive(Debug, Clone, Serialize)]
pub struct SimpleCallLog {
    pub timestamp: DateTime<Utc>,
    pub date: NaiveDate,
    pub method: String,
    pub endpoint: String,
    pub user_agent: Option<String>,
    pub client_ip: Option<String>,
    pub access_key: Option<String>,              // 如果有AK标识
    pub request_id: Option<String>,              // 如果有请求ID
    pub authorization: Option<String>,           // 如果有Authorization头
    pub custom_headers: HashMap<String, String>, // 其他自定义头部
    pub response_status: u16,
    pub response_time_ms: u64,
    pub request_size: usize,
    pub response_size: usize,
    pub success: bool,
}

/// 每日统计数据
#[derive(Debug, Serialize)]
pub struct DailyStats {
    pub date: NaiveDate,
    pub total_calls: u32,
    pub success_calls: u32,
    pub error_calls: u32,
    pub success_rate: f64,
    pub avg_response_time_ms: f64,
    pub unique_clients: u32,
    pub endpoints: HashMap<String, u32>, // 各个接口的调用次数
}

/// 调用记录管理器
#[derive(Debug)]
pub struct CallLogManager {
    /// 调用历史记录 (内存中保留最近1000条)
    call_history: Vec<SimpleCallLog>,
    /// 每日统计: date -> stats
    daily_stats: HashMap<NaiveDate, DailyStats>,
    /// 当前日期
    current_date: NaiveDate,
}

impl CallLogManager {
    pub fn new() -> Self {
        let config = CONFIG.api_call_tracking.as_ref();
        let retention = config.map(|c| c.memory_retention).unwrap_or(1000);

        Self {
            call_history: Vec::with_capacity(retention),
            daily_stats: HashMap::new(),
            current_date: Utc::now().date_naive(),
        }
    }

    /// 记录API调用
    pub fn log_call(&mut self, call_log: SimpleCallLog) {
        let date = call_log.date;

        // 检查是否需要新建日统计
        if date != self.current_date || !self.daily_stats.contains_key(&date) {
            if date != self.current_date {
                self.current_date = date;
            }

            self.daily_stats.insert(
                date,
                DailyStats {
                    date,
                    total_calls: 0,
                    success_calls: 0,
                    error_calls: 0,
                    success_rate: 0.0,
                    avg_response_time_ms: 0.0,
                    unique_clients: 0,
                    endpoints: HashMap::new(),
                },
            );
        }

        // 更新日统计
        if let Some(stats) = self.daily_stats.get_mut(&date) {
            stats.total_calls += 1;

            if call_log.success {
                stats.success_calls += 1;
            } else {
                stats.error_calls += 1;
            }

            // 更新成功率
            stats.success_rate = if stats.total_calls > 0 {
                (stats.success_calls as f64 / stats.total_calls as f64) * 100.0
            } else {
                0.0
            };

            // 更新平均响应时间
            stats.avg_response_time_ms = (stats.avg_response_time_ms
                * (stats.total_calls - 1) as f64
                + call_log.response_time_ms as f64)
                / stats.total_calls as f64;

            // 更新接口调用统计
            *stats
                .endpoints
                .entry(call_log.endpoint.clone())
                .or_insert(0) += 1;
        }

        // 添加到历史记录
        self.call_history.push(call_log.clone());

        // 保持历史记录在配置的范围内
        let max_retention = CONFIG
            .api_call_tracking
            .as_ref()
            .map(|c| c.memory_retention)
            .unwrap_or(1000);

        if self.call_history.len() > max_retention {
            self.call_history.remove(0);
        }

        tracing::debug!(
            target: "api_call_tracking",
            event = events::API_CALL_RECORDED,
            method = %call_log.method,
            endpoint = %call_log.endpoint,
            status = call_log.response_status,
            duration_ms = call_log.response_time_ms,
            success = call_log.success
        );
    }

    /// 获取今日统计
    pub fn get_today_stats(&self) -> Option<&DailyStats> {
        self.daily_stats.get(&self.current_date)
    }

    /// 获取所有统计
    pub fn get_all_stats(&self) -> serde_json::Value {
        let total_calls: u32 = self.daily_stats.values().map(|s| s.total_calls).sum();
        let recent_calls = self.call_history.len();

        // 分析最近调用的特征
        let mut access_keys = HashMap::new();
        let mut user_agents = HashMap::new();
        let mut client_ips = HashMap::new();

        for log in &self.call_history {
            if let Some(ak) = &log.access_key {
                *access_keys.entry(ak.clone()).or_insert(0) += 1;
            }
            if let Some(ua) = &log.user_agent {
                *user_agents.entry(ua.clone()).or_insert(0) += 1;
            }
            if let Some(ip) = &log.client_ip {
                *client_ips.entry(ip.clone()).or_insert(0) += 1;
            }
        }

        json!({
            "summary": {
                "total_calls_all_time": total_calls,
                "recent_calls_in_memory": recent_calls,
                "daily_stats_count": self.daily_stats.len(),
                "current_date": self.current_date.to_string()
            },
            "today_stats": self.get_today_stats(),
            "analysis": {
                "access_keys_found": access_keys,
                "user_agents": user_agents,
                "client_ips": client_ips
            },
            "daily_breakdown": self.daily_stats
        })
    }

    /// 获取最近调用记录
    pub fn get_recent_calls(&self, limit: usize) -> &[SimpleCallLog] {
        let start = if self.call_history.len() > limit {
            self.call_history.len() - limit
        } else {
            0
        };
        &self.call_history[start..]
    }
}

/// 全局调用记录管理器
static CALL_LOG_MANAGER: LazyLock<Arc<Mutex<CallLogManager>>> =
    LazyLock::new(|| Arc::new(Mutex::new(CallLogManager::new())));

/// 从请求头提取所有可能的标识信息
fn extract_all_identifiers(
    headers: &HeaderMap,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    HashMap<String, String>,
) {
    let mut custom_headers = HashMap::new();

    // 提取常见的标识头部
    let access_key = headers
        .get("x-api-access-key")
        .or_else(|| headers.get("x-api-key"))
        .or_else(|| headers.get("x-client-id"))
        .and_then(|h| h.to_str().ok())
        .map(String::from);

    let request_id = headers
        .get("x-request-id")
        .or_else(|| headers.get("x-trace-id"))
        .or_else(|| headers.get("request-id"))
        .and_then(|h| h.to_str().ok())
        .map(String::from);

    let authorization = headers
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .map(String::from);

    // 收集所有自定义头部 (X- 开头的)
    for (name, value) in headers.iter() {
        let name_str = name.as_str();
        if name_str.starts_with("x-") && name_str != "x-forwarded-for" {
            if let Ok(value_str) = value.to_str() {
                custom_headers.insert(name_str.to_string(), value_str.to_string());
            }
        }
    }

    (access_key, request_id, authorization, custom_headers)
}

/// 根据配置记录一次 API 调用
pub fn record_api_call(
    method: &str,
    path: &str,
    headers: &HeaderMap,
    response_status: u16,
    response_time_ms: u64,
    request_size: usize,
    response_size: usize,
) {
    // 检查是否启用调用记录
    let tracking_enabled = CONFIG
        .api_call_tracking
        .as_ref()
        .map(|c| c.enabled)
        .unwrap_or(false);

    if !tracking_enabled {
        return;
    }

    // 检查是否需要记录此路径
    let should_track = CONFIG
        .api_call_tracking
        .as_ref()
        .map(|c| {
            c.tracked_paths
                .iter()
                .any(|pattern| path.starts_with(pattern))
        })
        .unwrap_or(false);

    if !should_track {
        return;
    }

    // 提取所有可能的标识信息
    let (access_key, request_id, authorization, custom_headers) = extract_all_identifiers(&headers);

    // 获取用户代理和IP
    let user_agent = headers
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(String::from);

    let client_ip = headers
        .get("x-forwarded-for")
        .or_else(|| headers.get("x-real-ip"))
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string());

    // 创建调用记录
    let call_log = SimpleCallLog {
        timestamp: Utc::now(),
        date: Utc::now().date_naive(),
        method: method.to_string(),
        endpoint: path.to_string(),
        user_agent,
        client_ip,
        access_key,
        request_id,
        authorization,
        custom_headers,
        response_status,
        response_time_ms,
        request_size,
        response_size,
        success: response_status < 400,
    };

    // 记录调用
    let mut manager = CALL_LOG_MANAGER.lock().unwrap();
    manager.log_call(call_log);
}

/// 简单的API调用记录中间件 - 根据配置决定是否记录
pub async fn simple_call_logging_middleware(request: Request, next: Next) -> Response {
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let headers = request.headers().clone();
    let request_size = headers
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    let start_time = std::time::Instant::now();
    let response = next.run(request).await;
    let response_time_ms = start_time.elapsed().as_millis() as u64;
    let response_status = response.status().as_u16();
    let response_size = response
        .headers()
        .get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);

    record_api_call(
        &method,
        &path,
        &headers,
        response_status,
        response_time_ms,
        request_size,
        response_size,
    );

    response
}

/// 获取调用统计API
pub async fn get_api_call_stats() -> axum::Json<serde_json::Value> {
    let manager = CALL_LOG_MANAGER.lock().unwrap();
    axum::Json(manager.get_all_stats())
}

/// 获取最近调用记录API
pub async fn get_recent_api_calls(
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> axum::Json<serde_json::Value> {
    let limit = params
        .get("limit")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50);

    let manager = CALL_LOG_MANAGER.lock().unwrap();
    let recent_calls = manager.get_recent_calls(limit);

    axum::Json(json!({
        "success": true,
        "data": {
            "limit": limit,
            "count": recent_calls.len(),
            "calls": recent_calls
        }
    }))
}
