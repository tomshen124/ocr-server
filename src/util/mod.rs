use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt::Display;

pub mod config;
pub mod log;
pub mod zen;
pub mod system_info;
pub mod middleware;
pub mod auth;
pub mod report;

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