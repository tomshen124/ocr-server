use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Display;

pub mod adaptive_limiter;
pub mod api_stats; // [stats] 新增API统计记录模块
pub mod auth;
pub mod callbacks;
pub mod config;
pub mod converter;
pub mod crypto;
pub mod dynamic_worker; // [target] 新增动态Worker管理模块
pub mod extract;
pub mod http_client; // [global] HTTP客户端模块（支持依赖注入）
pub mod log;
pub mod logging;
pub mod material;
pub mod material_cache;
pub mod material_cache_manager;
pub mod middleware;
pub mod outbox;
pub mod permit_tracker;
pub mod processing; // [launch] 新增处理流水线模块
pub mod report;
pub mod rules;
pub mod service_watchdog;
pub mod system_info;
pub mod task_queue;
pub mod task_recovery;
pub mod tracing; // [search] 新增分布式链路追踪模块
pub mod worker; // [handshake] Worker proxy 客户端与运行时
pub mod zen; // [ticket] 新增信号量追踪模块

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WebResult {
    pub success: bool,
    #[serde(rename = "errorCode")]
    pub code: u32,
    #[serde(rename = "errorMsg")]
    pub msg: String,
    pub data: Value,
}

#[derive(Debug, Clone)]
pub enum ServerError {
    Server,
    Custom(String),
}

impl Display for ServerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            ServerError::Server => "Server internal error",
            ServerError::Custom(err) => err,
        };
        write!(f, "{}", str)
    }
}

impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        (StatusCode::BAD_REQUEST, WebResult::err(self).into_json()).into_response()
    }
}

impl WebResult {
    pub fn ok(data: impl Serialize) -> Self {
        Self {
            success: true,
            code: 200,
            msg: "".to_string(),
            data: json!(data),
        }
    }

    pub fn ok_with_data(data: impl Serialize) -> Self {
        Self::ok(data)
    }

    pub fn err(err: ServerError) -> Self {
        Self {
            success: false,
            code: 500,
            msg: err.to_string(),
            data: Default::default(),
        }
    }

    pub fn err_custom(msg: impl ToString) -> Self {
        Self {
            success: false,
            code: 500,
            msg: msg.to_string(),
            data: Default::default(),
        }
    }

    pub fn err_with_code(code: u32, msg: impl ToString) -> Self {
        Self {
            success: false,
            code,
            msg: msg.to_string(),
            data: Default::default(),
        }
    }
}

pub trait IntoJson {
    fn into_json(self) -> Json<WebResult>;
}

impl IntoJson for anyhow::Result<WebResult> {
    fn into_json(self) -> Json<WebResult> {
        self.unwrap_or_else(WebResult::err_custom).into_json()
    }
}

impl IntoJson for WebResult {
    fn into_json(self) -> Json<WebResult> {
        Json(self)
    }
}
