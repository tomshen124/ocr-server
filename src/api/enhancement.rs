//! API增强中间件
//! 基于配置开关，智能启用新功能，保持接口URL不变

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

/// API增强中间件
pub async fn api_enhancement_middleware(mut req: Request, next: Next) -> Response {
    let config = &CONFIG.api_enhancement;

    // 根据配置决定是否启用功能
    let trace_id = if config.trace_id_enabled {
        Uuid::new_v4().to_string()
    } else {
        "disabled".to_string()
    };

    // 插入请求上下文
    let ctx = RequestContext {
        trace_id: trace_id.clone(),
        enhanced_features: config.enhanced_error_handling,
    };
    req.extensions_mut().insert(ctx);

    // 条件性日志记录
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

    // 增强响应处理
    enhance_response(response, trace_id, config).await
}

async fn enhance_response(
    mut response: Response,
    trace_id: String,
    config: &crate::util::config::ApiEnhancementConfig,
) -> Response {
    // 条件性添加trace_id到响应头
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

/// 错误包装工具 - 根据配置决定是否增强
pub fn maybe_enhance_error(
    error: impl std::fmt::Display,
    request_ctx: &RequestContext,
) -> axum::response::Response {
    if request_ctx.enhanced_features {
        // 使用增强错误处理
        enhanced_error_response(error, &request_ctx.trace_id)
    } else {
        // 保持原有错误处理
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

    // 智能错误分类和用户友好消息
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
    // 保持原有的错误处理方式
    use crate::util::WebResult;
    WebResult::err_custom(&error.to_string())
        .into_json()
        .into_response()
}
