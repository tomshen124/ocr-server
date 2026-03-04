
use crate::util::IntoJson;
use crate::CONFIG;
use axum::{
    extract::Request,
    http::HeaderValue,
    middleware::Next,
    response::{IntoResponse, Response},
};
use uuid::Uuid;

#[derive(Clone)]
pub struct RequestContext {
    pub trace_id: String,
    pub enhanced_features: bool,
}

pub async fn api_enhancement_middleware(mut req: Request, next: Next) -> Response {
    let config = &CONFIG.api_enhancement;

    let trace_id = if config.trace_id_enabled {
        Uuid::new_v4().to_string()
    } else {
        "disabled".to_string()
    };

    let ctx = RequestContext {
        trace_id: trace_id.clone(),
        enhanced_features: config.enhanced_error_handling,
    };
    req.extensions_mut().insert(ctx);

    if config.trace_id_enabled {
        tracing::debug!(
            target: "api.enhancement",
            event = "api.request_start",
            trace_id = %trace_id,
            method = %req.method(),
            uri = %req.uri(),
            enhanced = %config.enhanced_error_handling
        );
    }

    let response = next.run(req).await;

    enhance_response(response, trace_id, config).await
}

async fn enhance_response(
    mut response: Response,
    trace_id: String,
    config: &crate::util::config::ApiEnhancementConfig,
) -> Response {
    if config.trace_id_enabled {
        if let Ok(header_value) = HeaderValue::from_str(&trace_id) {
            response.headers_mut().insert("X-Trace-ID", header_value);
        }

        tracing::debug!(
            target: "api.enhancement",
            event = "api.request_complete",
            trace_id = %trace_id,
            status = response.status().as_u16()
        );
    }

    response
}

pub fn maybe_enhance_error(
    error: impl std::fmt::Display,
    request_ctx: &RequestContext,
) -> axum::response::Response {
    if request_ctx.enhanced_features {
        enhanced_error_response(error, &request_ctx.trace_id)
    } else {
        legacy_error_response(error)
    }
}

fn enhanced_error_response(
    error: impl std::fmt::Display,
    trace_id: &str,
) -> axum::response::Response {
    use axum::{http::StatusCode, response::IntoResponse, Json};
    use serde_json::json;

    let error_msg = error.to_string();

    let (user_msg, status_code) = if error_msg.contains("认证") || error_msg.contains("登录") {
        ("请重新登录后再试", StatusCode::UNAUTHORIZED)
    } else if error_msg.contains("预审") || error_msg.contains("处理") {
        ("预审处理失败，请稍后重试", StatusCode::UNPROCESSABLE_ENTITY)
    } else if error_msg.contains("参数") || error_msg.contains("格式") {
        ("请检查输入参数格式", StatusCode::BAD_REQUEST)
    } else {
        ("系统繁忙，请稍后重试", StatusCode::INTERNAL_SERVER_ERROR)
    };

    tracing::error!(
        target: "api.enhancement",
        event = "api.enhancement_error",
        trace_id = %trace_id,
        error_msg = %error_msg,
        user_msg = %user_msg
    );

    let response_body = json!({
        "success": false,
        "error_msg": error_msg,
        "user_msg": user_msg,
        "trace_id": trace_id,
        "timestamp": chrono::Utc::now()
    });

    (status_code, Json(response_body)).into_response()
}

fn legacy_error_response(error: impl std::fmt::Display) -> axum::response::Response {
    use crate::util::WebResult;
    WebResult::err_custom(&error.to_string())
        .into_json()
        .into_response()
}
