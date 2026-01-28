//! 元数据接口：表列表与行数估算（通过达梦 Go 网关）

use crate::util::config::types::GoGatewayConfig;
use crate::AppState;
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
struct GoGatewayMetaResponse {
    success: bool,
    #[serde(default)]
    data: Option<Vec<TableRow>>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct TableRow {
    #[serde(rename = "TABLE_NAME")]
    table_name: Option<String>,
    #[serde(rename = "NUM_ROWS")]
    num_rows: Option<u64>,
}

struct CacheEntry {
    expires_at: Instant,
    payload: serde_json::Value,
}

static TABLES_CACHE: OnceLock<Mutex<Option<CacheEntry>>> = OnceLock::new();

/// GET /meta/tables
pub async fn list_tables(
    State(app_state): State<AppState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let gateway_cfg = match resolve_go_gateway_config() {
        Some(cfg) if cfg.enabled => cfg,
        _ => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "success": false,
                    "message": "Go网关未配置或未启用"
                })),
            )
                .into_response()
        }
    };

    let provided_key = headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if provided_key != gateway_cfg.api_key {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "success": false,
                "message": "invalid api key"
            })),
        )
            .into_response();
    }

    // 尝试缓存
    let cache_ttl = cache_ttl_secs();
    if let Some((payload, cache_header)) = {
        let cache = TABLES_CACHE.get_or_init(|| Mutex::new(None));
        let mut guard = cache.lock().unwrap();
        guard
            .as_ref()
            .filter(|entry| entry.expires_at > Instant::now())
            .map(|entry| (entry.payload.clone(), true))
    } {
        let mut resp = Json::<serde_json::Value>(payload).into_response();
        resp.headers_mut()
            .insert("X-Cache-Hit", HeaderValue::from_static("true"));
        return resp;
    }

    #[cfg(not(feature = "reqwest"))]
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({
                "success": false,
                "message": "HTTP客户端不可用"
            })),
        )
            .into_response();
    }

    #[cfg(feature = "reqwest")]
    {
        let client = match app_state.http_client.reqwest_client() {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "success": false,
                        "message": format!("HTTP客户端不可用: {}", e)
                    })),
                )
                    .into_response()
            }
        };

        let url = format!("{}/db", gateway_cfg.url.trim_end_matches('/'));
        let sql = "SELECT TABLE_NAME, NUM_ROWS FROM USER_TABLES ORDER BY TABLE_NAME";
        let payload = serde_json::json!({
            "sql": sql,
            "options": { "timeout_ms": gateway_cfg.timeout.max(1) * 1000 }
        });

        let resp = match client
            .post(url)
            .header("X-API-Key", &gateway_cfg.api_key)
            .json(&payload)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "success": false,
                        "message": format!("网关请求失败: {}", e)
                    })),
                )
                    .into_response()
            }
        };

        let status = resp.status();
        let body = match resp.json::<GoGatewayMetaResponse>().await {
            Ok(b) => b,
            Err(e) => {
                return (
                    StatusCode::BAD_GATEWAY,
                    Json(serde_json::json!({
                        "success": false,
                        "message": format!("网关响应解析失败: {}", e)
                    })),
                )
                    .into_response()
            }
        };

        if !status.is_success() || !body.success {
            let msg = body
                .message
                .unwrap_or_else(|| format!("网关返回错误: HTTP {}", status.as_u16()));
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "success": false,
                    "message": msg
                })),
            )
                .into_response();
        }

        let tables: Vec<serde_json::Value> = body
            .data
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| {
                row.table_name.map(|name| {
                    let count = row.num_rows.unwrap_or(0);
                    serde_json::json!({
                        "table_name": name,
                        "row_count": count
                    })
                })
            })
            .collect();

        let payload = serde_json::json!({
            "success": true,
            "tables": tables
        });

        {
            let cache = TABLES_CACHE.get_or_init(|| Mutex::new(None));
            let mut guard = cache.lock().unwrap();
            guard.replace(CacheEntry {
                expires_at: Instant::now() + Duration::from_secs(cache_ttl),
                payload: payload.clone(),
            });
        }

        let mut resp = Json(payload).into_response();
        resp.headers_mut()
            .insert("X-Cache-Hit", HeaderValue::from_static("false"));
        return resp;
    }
}

fn resolve_go_gateway_config() -> Option<GoGatewayConfig> {
    // 优先新配置 database.go_gateway，再回退 database.dm.go_gateway
    if let Some(db_cfg) = crate::CONFIG.database.as_ref() {
        if let Some(gw) = db_cfg.go_gateway.as_ref() {
            return Some(gw.clone());
        }
        if let Some(dm) = db_cfg.dm.as_ref() {
            if let Some(gw) = dm.go_gateway.as_ref() {
                return Some(gw.clone());
            }
        }
    }
    None
}

fn cache_ttl_secs() -> u64 {
    std::env::var("META_TABLES_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30)
}
