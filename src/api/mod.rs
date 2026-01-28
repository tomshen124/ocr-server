mod auth;
mod config;
mod files;
mod meta;
mod monitoring;
pub mod preview;
mod rules;
pub use preview::{LocalPreviewTaskHandler, RemotePreviewTaskHandler};
mod utils;
// Mock登录相关测试接口已移除，使用debug ticket进行开发环境测试
mod dynamic_worker_status; // NEW 动态Worker状态API
mod enhancement; // 新增：API增强功能
mod failover_status; // [loop] 新增：故障转移状态查询API
pub mod monitor_auth;
pub mod monitor_routes;
mod resource_monitoring; // [stats] 新增：资源监控API
mod tracing_api; // [search] 新增：分布式链路追踪API (重命名避免冲突)
pub mod worker_proxy; // [handshake] Worker 内部代理接口

use crate::model::evaluation::PreviewEvaluationResult;
use crate::model::SessionUser;
use crate::api::worker_proxy::WorkerResultRequest;
use crate::util::logging::standards::events;
use crate::util::middleware;
use crate::{AppState, CONFIG};
use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::{from_fn, from_fn_with_state};
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use ocr_conn::CURRENT_DIR;
use std::sync::Arc;
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_sessions::cookie::time::Duration;
use tower_sessions::{Expiry, MemoryStore, SessionManagerLayer};
use tracing::{debug, error, info, warn}; // [search] 添加tracing宏导入

/// 创建安全的CORS配置
fn create_cors_layer() -> CorsLayer {
    // 从环境变量读取允许的域名，默认为本地开发
    let allowed_origins = std::env::var("CORS_ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:8964,http://127.0.0.1:8964".to_string());

    info!("[global] CORS配置 - 允许的源: {}", allowed_origins);

    let origins: Vec<&str> = allowed_origins.split(',').collect();

    // 构建CORS层
    CorsLayer::new()
        .allow_origin(
            origins
                .into_iter()
                .filter_map(|s| match s.trim().parse() {
                    Ok(origin) => Some(origin),
                    Err(e) => {
                        warn!("无效的CORS源: {} - {}", s, e);
                        None
                    }
                })
                .collect::<Vec<_>>(),
        )
        .allow_methods([
            axum::http::Method::GET,
            axum::http::Method::POST,
            axum::http::Method::PUT,
            axum::http::Method::DELETE,
            axum::http::Method::OPTIONS,
        ])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
            axum::http::header::ACCEPT,
            axum::http::HeaderName::from_static("x-monitor-session-id"),
            axum::http::HeaderName::from_static("x-access-key"),
        ])
        .allow_credentials(true)
}

pub fn routes(app_state: AppState) -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(
            CONFIG.session_timeout,
        )))
        // [locked] 安全增强：添加会话安全配置
        .with_same_site(tower_sessions::cookie::SameSite::Lax)
        .with_http_only(true);

    worker_proxy::spawn_heartbeat_watchdog(&app_state);

    // 获取可执行文件所在目录
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // 智能检测静态文件路径：优先使用构建后的static-dist，降级到static
    let static_path = {
        let parent_static_dist = exe_dir.parent().unwrap_or(&exe_dir).join("static-dist");
        let local_static_dist = exe_dir.join("static-dist");
        let parent_static = exe_dir.parent().unwrap_or(&exe_dir).join("static");
        let local_static = exe_dir.join("static");

        if parent_static_dist.exists() {
            info!(
                "[search] 使用父目录构建静态文件: {}",
                parent_static_dist.display()
            );
            parent_static_dist
        } else if local_static_dist.exists() {
            info!(
                "[search] 使用本地构建静态文件: {}",
                local_static_dist.display()
            );
            local_static_dist
        } else if parent_static.exists() {
            info!(
                "[search] 使用父目录源码静态文件: {}",
                parent_static.display()
            );
            parent_static
        } else if local_static.exists() {
            info!("[search] 使用本地源码静态文件: {}", local_static.display());
            local_static
        } else {
            warn!(
                "[warn]  静态文件目录不存在，使用默认路径: {}",
                local_static.display()
            );
            local_static
        }
    };

    let images_path = exe_dir.join("images");

    // 公开路由 - 不需要认证
    let public_routes = Router::new()
        .route("/", get(config::root_redirect)) // 根路由重定向到登录页
        .route("/api/verify_user", post(auth::verify_user))
        .route("/api/sso/login", get(auth::sso_login_redirect)) // SSO登录跳转端点
        .route("/api/sso/callback", get(auth::sso_callback))
        .route(
            "/api/third-party/callback",
            post(files::third_party_callback),
        )
        // 外部分享一次性链接（免登录，仅token校验）
        .route("/api/share/:token", get(files::download_shared_preview_report))
        .route("/api/auth/status", get(auth::auth_status))
        .route("/api/auth/logout", post(auth::auth_logout))
        .route("/api/user_save", post(auth::user_save))
        .route("/api/get_token", post(auth::get_token))
        .route("/api/user_info", post(auth::user_info))
        .route("/api/health", get(monitoring::basic_health_check))
        .route(
            "/api/health/details",
            get(monitoring::detailed_health_check),
        )
        .route(
            "/api/health/components",
            get(monitoring::components_health_check),
        )
        // 前端配置API - 公开访问
        .route("/api/config/frontend", get(config::get_frontend_config))
        .route("/api/config/debug", get(config::get_debug_config))
        // 元数据接口（需要 X-API-Key）
        .route("/meta/tables", get(meta::list_tables))
        // 监控系统认证API - 独立认证系统
        .nest("/api/monitor", monitor_routes::monitor_routes())
        .route(
            "/api/dynamic-worker/status",
            get(dynamic_worker_status::get_dynamic_worker_status),
        )
        // [stats] API调用统计接口 (开放访问，用于查看调用情况)
        .route(
            "/api/stats/calls",
            get(crate::util::auth::get_api_call_stats),
        )
        .route(
            "/api/stats/calls/recent",
            get(crate::util::auth::get_recent_api_calls),
        )
        // [search] 分布式链路追踪接口 (开放访问，用于调试和监控)
        .merge(tracing_api::tracing_routes())
        // [loop] 故障转移状态查询接口 (开放访问，用于监控系统状态)
        .merge(failover_status::configure_failover_status_routes())
        // [stats] 资源监控接口 (开放访问，用于系统监控)
        .merge(resource_monitoring::create_resource_monitoring_routes());

    // [loop] 预审接口 - 使用第三方认证中间件（支持最大兼容模式）
    let preview_routes = Router::new()
        .route("/api/preview", post(preview::preview)) // 混合认证模式：开放+识别+完整认证
        .layer(from_fn(crate::util::auth::third_party_auth_middleware))
        // 应用API增强中间件到核心接口
        .layer(axum::middleware::from_fn(
            enhancement::api_enhancement_middleware,
        ));

    // 受保护路由 - 需要SSO认证或monitor_session_id认证
    let protected_routes = Router::new()
        .route("/api/upload", post(files::upload))
        .route("/api/download", get(files::download))
        .route("/api/rules/matters", get(rules::list_matter_rules))
        .route("/api/rules/matters/:matter_id", get(rules::get_matter_rule))
        .route(
            "/api/rules/matters/:matter_id/reload",
            post(rules::reload_matter_rule),
        )
        // 预审页面访问接口（支持SSO和monitor_session_id双重认证）
        .route("/api/preview/view/:request_id", get(preview_view_page))
        // 新增：预审数据获取接口（需要认证）
        .route("/api/preview/data/:request_id", get(get_preview_data))
        // 新增：基于第三方requestId查找预审访问URL的接口
        .route(
            "/api/preview/lookup/:third_party_request_id",
            get(lookup_preview_url),
        )
        // 新增：预审状态查询接口
        .route("/api/preview/status/:preview_id", get(query_preview_status))
        // 新增：预审结果展示接口
        .route(
            "/api/preview/result/:preview_id",
            get(files::get_preview_result),
        )
        .route(
            "/api/preview/download/:preview_id",
            get(files::download_preview_report),
        )
        // 生成外部分享一次性链接（需要监控后台登录）
        .route(
            "/api/preview/share/:preview_id",
            post(files::create_preview_share_url),
        )
        // 图片服务API - 新增OCR图片支持
        .route(
            "/api/files/ocr-image/:pdf_name/:page_index",
            get(files::get_ocr_image),
        )
        .route(
            "/api/files/preview-thumbnail/:preview_id/:page_index",
            get(files::get_preview_thumbnail),
        )
        .route(
            "/api/files/material-preview/:preview_id/:material_name",
            get(files::get_material_preview),
        )
        .route("/api/storage/files/*key", get(files::proxy_storage_file))
        // 监控与日志接口（需要认证）
        .route("/api/logs/stats", get(monitoring::get_log_stats))
        .route("/api/logs/cleanup", post(monitoring::cleanup_logs))
        .route("/api/logs/health", get(monitoring::check_log_health))
        .route("/api/stats/previews", get(monitoring::get_preview_stats))
        .route(
            "/api/preview/statistics",
            get(monitoring::get_preview_statistics),
        )
        .route(
            "/api/preview/records",
            get(monitoring::get_preview_records_list),
        )
        .route(
            "/api/preview/requests",
            get(monitoring::get_preview_requests_list),
        )
        .route(
            "/api/preview/requests/:request_id",
            get(monitoring::get_preview_request_detail),
        )
        .route(
            "/api/preview/failures",
            get(monitoring::get_recent_failed_previews),
        )
        .route("/api/monitoring/status", get(monitoring::get_system_status))
        .route("/api/queue/status", get(monitoring::get_queue_status))
        .route(
            "/api/permits/tracker",
            get(monitoring::get_permit_tracker_status),
        )
        .route(
            "/api/monitoring/attachment-logging",
            post(monitoring::update_attachment_logging_settings),
        )
        // 预审页面访问接口需要认证
        .route("/:preview_id.pdf", get(preview::download_latest_pdf))
        .layer(from_fn_with_state(
            app_state.clone(),
            middleware::auth_required,
        ));

    // 第三方API路由已删除 - 重构后统一使用SSO认证
    // AK/SK认证改为可选的统计标识功能

    // NEW 集成用户管理和第三方平台集成路由
    // let admin_routes = user_admin_routes::create_user_admin_routes();

    Router::new()
        .nest_service("/static", ServeDir::new(static_path))
        .nest_service("/images", ServeDir::new(images_path))
        .merge(public_routes)
        .merge(preview_routes) // [loop] 预审路由（第三方认证）
        .merge(protected_routes)
        .merge(worker_proxy::routes())
        // .merge(admin_routes) // NEW 集成管理路由
        .with_state(app_state)
        // 全局中间件
        // .layer(from_fn(crate::util::tracing::middleware::tracing_middleware)) // [search] 分布式链路追踪 - 暂时禁用
        .layer(from_fn(middleware::request_logging_middleware))
        .layer(session_layer)
        .layer(create_cors_layer())
}

// Auth-related functions moved to auth module
// 旧版本的preview函数已删除 - 现在使用preview.rs中的模块化版本

async fn preview_view_page(
    State(app_state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let view_span =
        tracing::info_span!(target: "preview.view", "preview_view", request_id = %request_id);
    let _guard = view_span.enter();

    info!(
        target: "preview.view",
        event = events::PREVIEW_RECEIVED,
        request_id = %request_id
    );

    // 验证request_id格式（基本安全检查）
    if request_id.is_empty() || request_id.len() > 100 {
        warn!(
            target: "preview.view",
            event = events::PREVIEW_VALIDATE_FAILED,
            request_id = %request_id,
            reason = "invalid_request_id"
        );
        return (StatusCode::BAD_REQUEST, "无效的请求ID").into_response();
    }

    // 从认证中间件获取SessionUser（支持SSO和monitor_session_id）
    let session_user = match req.extensions().get::<SessionUser>() {
        Some(user) => user.clone(),
        None => {
            error!(
                target: "preview.view",
                event = events::AUTH_ERROR,
                request_id = %request_id,
                reason = "missing_session"
            );
            return (StatusCode::UNAUTHORIZED, "未授权访问").into_response();
        }
    };

    debug!(
        target: "preview.view",
        event = events::AUTH_SUCCESS,
        request_id = %request_id,
        user_id = %session_user.user_id
    );

    // 验证用户是否有权限访问该预审记录
    match verify_preview_access(&app_state.database, &request_id, &session_user).await {
        Ok(true) => {
            // 从查询参数提取 monitor_session_id（如果有）
            let monitor_session_id = req.uri().query().and_then(|q| {
                url::form_urlencoded::parse(q.as_bytes())
                    .find(|(key, _)| key == "monitor_session_id")
                    .map(|(_, value)| value.into_owned())
            });

            // 重定向到单页面应用，用户已通过认证
            let redirect_url = if let Some(session_id) = monitor_session_id {
                format!(
                    "/static/index.html?previewId={}&verified=true&monitor_session_id={}",
                    request_id, session_id
                )
            } else {
                format!("/static/index.html?previewId={}&verified=true", request_id)
            };

            info!(
                target: "preview.view",
                event = events::PREVIEW_COMPLETE,
                request_id = %request_id,
                user_id = %session_user.user_id,
                redirect = %redirect_url
            );
            Redirect::to(&redirect_url).into_response()
        }
        Ok(false) => {
            warn!(
                target: "preview.view",
                event = events::AUTH_FAILURE,
                request_id = %request_id,
                user_id = %session_user.user_id,
                reason = "preview_owner_mismatch"
            );

            // 获取预审记录的真实归属用户（用于日志记录）
            if let Ok(Some(mapping)) =
                preview::get_id_mapping_from_database(&app_state.database, &request_id).await
            {
                let expected_user_id = mapping.user_id;
                warn!(
                    target: "preview.view",
                    event = events::PREVIEW_VALIDATE_FAILED,
                    request_id = %request_id,
                    expected_user_id = %expected_user_id,
                    user_id = %session_user.user_id,
                    reason = "preview_owner_mismatch"
                );
            }

            (StatusCode::FORBIDDEN, "无权访问此预审记录").into_response()
        }
        Err(e) => {
            error!(
                target: "preview.view",
                event = events::PREVIEW_ERROR,
                request_id = %request_id,
                error = %e
            );
            (StatusCode::INTERNAL_SERVER_ERROR, "验证权限失败").into_response()
        }
    }
}

// 获取预审数据接口（需要认证）
async fn get_preview_data(
    State(app_state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let data_span =
        tracing::info_span!(target: "preview.data", "preview_data", request_id = %request_id);
    let _guard = data_span.enter();

    info!(
        target: "preview.data",
        event = events::PREVIEW_RECEIVED,
        request_id = %request_id
    );

    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();

    let Some(session_user) = session_user else {
        warn!(
            target: "preview.data",
            event = events::AUTH_FAILURE,
            request_id = %request_id,
            reason = "missing_session"
        );
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 401,
            "errorMsg": "认证信息缺失",
            "data": null
        }));
    };

    debug!(
        target: "preview.data",
        event = events::AUTH_SUCCESS,
        request_id = %request_id,
        user_id = %session_user.user_id
    );

    match verify_preview_access(&app_state.database, &request_id, &session_user).await {
        Ok(true) => match get_preview_record(&app_state, &request_id).await {
            Ok(Some(preview_data)) => {
                let status = preview_data
                    .get("status")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_lowercase();
                let eval_missing = preview_data
                    .get("evaluation_result")
                    .map(|v| v.is_null())
                    .unwrap_or(true);

                info!(
                    target: "preview.data",
                    event = events::PREVIEW_COMPLETE,
                    request_id = %request_id,
                    user_id = %session_user.user_id
                );
                let mut response = serde_json::json!({
                    "success": true,
                    "errorCode": 200,
                    "errorMsg": "",
                    "data": preview_data
                });
                if status == "completed" && eval_missing {
                    warn!(
                        target: "preview.data",
                        event = events::PREVIEW_VALIDATE_FAILED,
                        request_id = %request_id,
                        user_id = %session_user.user_id,
                        reason = "evaluation_result_missing"
                    );
                    response["warning"] = serde_json::json!(
                        "预审已完成但报告数据缺失，已返回现有文件，请检查评估写入链路"
                    );
                }
                Json(response)
            }
            Ok(None) => {
                warn!(
                    target: "preview.data",
                    event = events::PREVIEW_VALIDATE_FAILED,
                    request_id = %request_id,
                    user_id = %session_user.user_id,
                    reason = "not_found"
                );
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 404,
                    "errorMsg": "预审记录不存在",
                    "data": null
                }))
            }
            Err(e) => {
                error!(
                    target: "preview.data",
                    event = events::PREVIEW_ERROR,
                    request_id = %request_id,
                    user_id = %session_user.user_id,
                    error = %e
                );

                let msg = e.to_string();
                let (code, human_msg) = if msg.contains("worker-cache") {
                    (
                        503,
                        "预览附件已过期或尚未持久化，请重试任务或联系运维".to_string(),
                    )
                } else {
                    (500, "获取预审数据失败".to_string())
                };

                Json(serde_json::json!({
                    "success": false,
                    "errorCode": code,
                    "errorMsg": human_msg,
                    "data": null
                }))
            }
        },
        Ok(false) => {
            warn!(
                target: "preview.data",
                event = events::AUTH_FAILURE,
                request_id = %request_id,
                user_id = %session_user.user_id,
                reason = "preview_owner_mismatch"
            );
            Json(serde_json::json!({
                "success": false,
                "errorCode": 403,
                "errorMsg": "无权限访问该预审记录",
                "data": null
            }))
        }
        Err(e) => {
            error!(
                target: "preview.data",
                event = events::PREVIEW_ERROR,
                request_id = %request_id,
                user_id = %session_user.user_id,
                error = %e
            );
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "系统错误",
                "data": null
            }))
        }
    }
}

// 验证用户是否有权限访问指定的预审记录
async fn verify_preview_access(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
    session_user: &SessionUser,
) -> anyhow::Result<bool> {
    let access_span = tracing::info_span!(
        target: "preview.access",
        "verify_access",
        user_id = %session_user.user_id,
        preview_id = %preview_id
    );
    let _guard = access_span.enter();

    debug!(
        target: "preview.access",
        event = events::AUTH_CHECK,
        user_id = %session_user.user_id,
        preview_id = %preview_id
    );

    // 监控系统账号具备全局查看能力
    if session_user
        .certificate_type
        .eq_ignore_ascii_case("monitor")
    {
        debug!(
            target: "preview.access",
            event = events::AUTH_SUCCESS,
            user_id = %session_user.user_id,
            preview_id = %preview_id,
            reason = "monitor_certificate"
        );
        return Ok(true);
    }

    // 获取ID映射信息
    match preview::get_id_mapping_from_database(database, preview_id).await? {
        Some(mapping) => {
            let mapping_user_id = mapping.user_id;
            let status = mapping.status.to_string();

            debug!(
                target: "preview.access",
                event = events::AUTH_CHECK,
                user_id = %mapping_user_id,
                preview_id = %preview_id,
                status = %status
            );

            // 检查用户ID是否匹配
            if mapping_user_id != session_user.user_id {
                warn!(
                    target: "preview.access",
                    event = events::AUTH_FAILURE,
                    user_id = %session_user.user_id,
                    preview_id = %preview_id,
                    expected_user_id = %mapping_user_id,
                    reason = "user_mismatch"
                );
                return Ok(false);
            }

            // 检查记录状态（允许多种有效状态）
            let valid_statuses = ["pending", "processing", "completed", "failed"];
            if !valid_statuses.contains(&status.as_str()) {
                warn!(
                    target: "preview.access",
                    event = events::PREVIEW_VALIDATE_FAILED,
                    preview_id = %preview_id,
                    status = %status,
                    reason = "invalid_status"
                );
                return Ok(false);
            }

            // 检查预审文件是否存在（支持多种存储路径）
            let mut file_exists = false;

            // 检查旧的preview目录
            let preview_dir = CURRENT_DIR.join("preview");
            let html_file = preview_dir.join(format!("{}.html", preview_id));
            let pdf_file = preview_dir.join(format!("{}.pdf", preview_id));

            if html_file.exists() || pdf_file.exists() {
                file_exists = true;
            }

            // 检查新的存储系统路径
            if !file_exists {
                let storage_dir = CURRENT_DIR
                    .join("runtime")
                    .join("fallback")
                    .join("storage")
                    .join("previews");
                let storage_html = storage_dir.join(format!("{}.html", preview_id));
                let storage_pdf = storage_dir.join(format!("{}.pdf", preview_id));

                if storage_html.exists() || storage_pdf.exists() {
                    file_exists = true;
                }
            }

            // 检查主存储路径（如果配置了OSS等）
            if !file_exists {
                let main_storage_dir = CURRENT_DIR.join("storage").join("previews");
                let main_html = main_storage_dir.join(format!("{}.html", preview_id));
                let main_pdf = main_storage_dir.join(format!("{}.pdf", preview_id));

                if main_html.exists() || main_pdf.exists() {
                    file_exists = true;
                }
            }

            if !file_exists {
                warn!(
                    target: "preview.access",
                    event = events::PREVIEW_VALIDATE_FAILED,
                    preview_id = %preview_id,
                    reason = "preview_files_missing"
                );
                return Ok(false);
            }

            debug!(
                target: "preview.access",
                event = events::AUTH_SUCCESS,
                user_id = %session_user.user_id,
                preview_id = %preview_id,
                reason = "ownership_confirmed"
            );
            Ok(true)
        }
        None => {
            warn!(
                target: "preview.access",
                event = events::PREVIEW_VALIDATE_FAILED,
                preview_id = %preview_id,
                reason = "mapping_missing"
            );
            Ok(false)
        }
    }
}

// 获取预审记录数据
async fn get_preview_record(
    app_state: &AppState,
    preview_id: &str,
) -> anyhow::Result<Option<serde_json::Value>> {
    let record_span = tracing::info_span!(
        target: "preview.data",
        "load_preview_record",
        preview_id = %preview_id
    );
    let _guard = record_span.enter();

    debug!(
        target: "preview.data",
        event = events::PIPELINE_STAGE,
        stage = "load_mapping",
        preview_id = %preview_id
    );

    // 获取ID映射信息
    let mut mapping =
        match preview::get_id_mapping_from_database(&app_state.database, preview_id).await? {
            Some(mapping) => mapping,
            None => {
                warn!(
                    target: "preview.data",
                    event = events::PREVIEW_VALIDATE_FAILED,
                    preview_id = %preview_id,
                    reason = "mapping_missing"
                );
                return Ok(None);
            }
        };

    // 已完成但 evaluation_result 为空时，尝试从 Worker 结果队列回补（防止“已完成但无预览”）
    if mapping.status == crate::db::PreviewStatus::Completed && mapping.evaluation_result.is_none()
    {
        match app_state
            .database
            .get_worker_result_by_preview_id(preview_id)
            .await
        {
            Ok(Some(record)) => match serde_json::from_str::<WorkerResultRequest>(&record.payload) {
                Ok(payload) => {
                    if let Some(eval) = payload.evaluation_result {
                        if let Ok(serialized) = serde_json::to_string(&eval) {
                            mapping.evaluation_result = Some(serialized.clone());
                            if let Err(err) = app_state
                                .database
                                .update_preview_evaluation_result(preview_id, &serialized)
                                .await
                            {
                                warn!(
                                    target: "preview.data",
                                    event = events::PREVIEW_ERROR,
                                    preview_id = %preview_id,
                                    error = %err,
                                    reason = "evaluation_backfill_write_failed"
                                );
                            }
                            info!(
                                target: "preview.data",
                                event = events::PIPELINE_STAGE,
                                stage = "evaluation_result_backfilled",
                                preview_id = %preview_id
                            );
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        target: "preview.data",
                        event = events::PREVIEW_ERROR,
                        preview_id = %preview_id,
                        error = %err,
                        reason = "worker_result_payload_parse_failed"
                    );
                }
            },
            Ok(None) => {}
            Err(err) => {
                debug!(
                    target: "preview.data",
                    event = events::PIPELINE_STAGE,
                    stage = "evaluation_backfill_unavailable",
                    preview_id = %preview_id,
                    error = %err
                );
            }
        }
    }

    let preview_dir = CURRENT_DIR.join("preview");
    let html_file = preview_dir.join(format!("{}.html", preview_id));
    let pdf_file = preview_dir.join(format!("{}.pdf", preview_id));

    // 如果文件尚未生成，也返回占位信息，避免前端收到404
    if !html_file.exists() && !pdf_file.exists() {
        warn!(
            target: "preview.data",
            event = events::PREVIEW_VALIDATE_FAILED,
            preview_id = %preview_id,
            reason = "preview_files_missing"
        );
    }

    // 构建预审数据响应
    let mut preview_data = serde_json::json!({
        "previewId": preview_id,
        "thirdPartyRequestId": mapping.third_party_request_id.unwrap_or_else(|| format!("third_party_{}", preview_id)),
        "userId": mapping.user_id,
        "status": mapping.status.to_string(),
        "createdAt": mapping.created_at.to_rfc3339(),
        "files": {}
    });

    // 添加evaluation_result字段到返回数据中
    if let Some(ref evaluation_result_str) = mapping.evaluation_result {
        // 尝试解析JSON字符串为对象
        match serde_json::from_str::<serde_json::Value>(evaluation_result_str) {
            Ok(mut evaluation_obj) => {
                if let Ok(mut evaluation_struct) =
                    serde_json::from_value::<PreviewEvaluationResult>(evaluation_obj.clone())
                {
                    if let Err(err) = crate::api::worker_proxy::enrich_preview_attachments(
                        &app_state.database,
                        &app_state.storage,
                        preview_id,
                        &mut evaluation_struct,
                    )
                    .await
                    {
                        warn!(
                            target: "preview.data",
                            event = events::PREVIEW_ERROR,
                            preview_id = %preview_id,
                            error = %err,
                            reason = "evaluation_enrich_failed"
                        );
                    }

                    // 安全加固：清洗系统内部信息，避免前端直接展示路径/调试字段
                    crate::api::utils::sanitize_evaluation_result(&mut evaluation_struct);
                    evaluation_obj =
                        serde_json::to_value(&evaluation_struct).unwrap_or_else(|_| evaluation_obj);
                }

                preview_data["evaluation_result"] = evaluation_obj;
                debug!(
                    target: "preview.data",
                    event = events::PIPELINE_STAGE,
                    stage = "evaluation_result_embedded",
                    preview_id = %preview_id
                );
            }
            Err(e) => {
                warn!(
                    target: "preview.data",
                    event = events::PREVIEW_ERROR,
                    preview_id = %preview_id,
                    error = %e,
                    reason = "evaluation_parse_failed"
                );
                preview_data["evaluation_result"] =
                    serde_json::Value::String(evaluation_result_str.clone());
            }
        }
    } else {
        debug!(
            target: "preview.data",
            event = events::PIPELINE_STAGE,
            stage = "evaluation_result_missing",
            preview_id = %preview_id
        );
        // 设置为null，前端可以处理这种情况
        preview_data["evaluation_result"] = serde_json::Value::Null;
    }

    // 添加文件信息
    let mut files = serde_json::Map::new();

    if html_file.exists() {
        let html_metadata = tokio::fs::metadata(&html_file).await.ok();
        let mut html_entry = serde_json::json!({
            "exists": true,
            "path": html_file.display().to_string(),
            "downloadUrl": format!("/api/preview/download/{}?format=html", preview_id),
            "legacyDownloadUrl": format!("/api/download?goto={}", html_file.display()),
            "contentType": "text/html"
        });

        if let Some(meta) = html_metadata.as_ref() {
            html_entry["size"] = serde_json::json!(meta.len());
            if let Ok(modified) = meta.modified() {
                html_entry["lastModified"] =
                    serde_json::json!(chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339());
            }
        }

        files.insert("html".to_string(), html_entry);
    }

    if pdf_file.exists() {
        let pdf_metadata = tokio::fs::metadata(&pdf_file).await.ok();

        if let Some(meta) = pdf_metadata.as_ref() {
            if let Ok(modified) = meta.modified() {
                preview_data["completedAt"] =
                    serde_json::json!(chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339());
            }
            preview_data["fileSize"] = serde_json::json!(meta.len());
        }

        let mut pdf_entry = serde_json::json!({
            "exists": true,
            "path": pdf_file.display().to_string(),
            "downloadUrl": format!("/api/preview/download/{}?format=pdf", preview_id),
            "legacyDownloadUrl": format!("/api/download?goto={}", pdf_file.display()),
            "contentType": "application/pdf"
        });

        if let Some(meta) = pdf_metadata.as_ref() {
            pdf_entry["size"] = serde_json::json!(meta.len());
            if let Ok(modified) = meta.modified() {
                pdf_entry["lastModified"] =
                    serde_json::json!(chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339());
            }
        }

        files.insert("pdf".to_string(), pdf_entry);
    }

    if let Some(remote_download) = mapping.preview_download_url.as_deref() {
        let source =
            if remote_download.starts_with("http://") || remote_download.starts_with("https://") {
                "remote"
            } else {
                "local"
            };

        match files.get_mut("pdf") {
            Some(entry) => {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert(
                        "downloadUrl".to_string(),
                        serde_json::json!(remote_download),
                    );
                    obj.insert("source".to_string(), serde_json::json!(source));
                }
            }
            None => {
                let mut obj = serde_json::Map::new();
                obj.insert("exists".to_string(), serde_json::json!(true));
                obj.insert(
                    "downloadUrl".to_string(),
                    serde_json::json!(remote_download),
                );
                obj.insert("source".to_string(), serde_json::json!(source));
                files.insert("pdf".to_string(), serde_json::Value::Object(obj));
            }
        }
    }

    if !mapping.preview_url.is_empty() {
        let source = if mapping.preview_url.starts_with("http://")
            || mapping.preview_url.starts_with("https://")
        {
            "remote"
        } else {
            "local"
        };

        match files.get_mut("html") {
            Some(entry) => {
                if let Some(obj) = entry.as_object_mut() {
                    obj.insert(
                        "downloadUrl".to_string(),
                        serde_json::json!(mapping.preview_url.clone()),
                    );
                    obj.insert("source".to_string(), serde_json::json!(source));
                }
            }
            None => {
                let mut obj = serde_json::Map::new();
                obj.insert("exists".to_string(), serde_json::json!(true));
                obj.insert(
                    "downloadUrl".to_string(),
                    serde_json::json!(mapping.preview_url.clone()),
                );
                obj.insert("source".to_string(), serde_json::json!(source));
                files.insert("html".to_string(), serde_json::Value::Object(obj));
            }
        }
    }

    preview_data["files"] = serde_json::Value::Object(files);

    info!(
        target: "preview.data",
        event = events::PIPELINE_COMPLETE,
        stage = "load_preview_record",
        preview_id = %preview_id
    );
    Ok(Some(preview_data))
}

// 根据第三方requestId查找预审访问URL
async fn lookup_preview_url(
    State(app_state): State<AppState>,
    axum::extract::Path(third_party_request_id): axum::extract::Path<String>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let lookup_span = tracing::info_span!(
        target: "preview.lookup",
        "preview_lookup",
        third_party_request_id = %third_party_request_id
    );
    let _guard = lookup_span.enter();

    info!(
        target: "preview.lookup",
        event = events::PREVIEW_RECEIVED,
        third_party_request_id = %third_party_request_id
    );

    let session_user = req.extensions().get::<SessionUser>().cloned();
    let Some(session_user) = session_user else {
        warn!(
            target: "preview.lookup",
            event = events::AUTH_FAILURE,
            third_party_request_id = %third_party_request_id,
            reason = "missing_session"
        );
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 401,
            "errorMsg": "认证信息缺失",
            "data": null
        }));
    };

    debug!(
        target: "preview.lookup",
        event = events::AUTH_SUCCESS,
        third_party_request_id = %third_party_request_id,
        user_id = %session_user.user_id
    );

    match find_preview_by_third_party_id(
        &app_state.database,
        &third_party_request_id,
        &session_user.user_id,
    )
    .await
    {
        Ok(Some(preview_id)) => {
            let view_url = CONFIG.preview_view_url(&preview_id);
            info!(
                target: "preview.lookup",
                event = events::PREVIEW_COMPLETE,
                third_party_request_id = %third_party_request_id,
                user_id = %session_user.user_id,
                preview_id = %preview_id,
                preview_url = %view_url
            );

            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "previewId": preview_id,
                    "previewUrl": view_url,
                    "thirdPartyRequestId": third_party_request_id
                }
            }))
        }
        Ok(None) => {
            warn!(
                target: "preview.lookup",
                event = events::PREVIEW_VALIDATE_FAILED,
                third_party_request_id = %third_party_request_id,
                user_id = %session_user.user_id,
                reason = "not_found"
            );
            Json(serde_json::json!({
                "success": false,
                "errorCode": 404,
                "errorMsg": "未找到匹配的预审记录",
                "data": null
            }))
        }
        Err(e) => {
            error!(
                target: "preview.lookup",
                event = events::PREVIEW_ERROR,
                third_party_request_id = %third_party_request_id,
                user_id = %session_user.user_id,
                error = %e
            );
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "查找预审记录失败",
                "data": null
            }))
        }
    }
}

// 根据第三方requestId和用户ID查找对应的previewId
async fn find_preview_by_third_party_id(
    database: &Arc<dyn crate::db::Database>,
    third_party_request_id: &str,
    user_id: &str,
) -> anyhow::Result<Option<String>> {
    debug!(
        target: "preview.lookup",
        event = events::PIPELINE_STAGE,
        stage = "lookup_db",
        third_party_request_id = %third_party_request_id,
        user_id = %user_id
    );

    let preview_record = database
        .find_preview_by_third_party_id(third_party_request_id, user_id)
        .await?;

    if let Some(record) = preview_record {
        debug!(
            target: "preview.lookup",
            event = events::PREVIEW_COMPLETE,
            third_party_request_id = %third_party_request_id,
            user_id = %user_id,
            preview_id = %record.id,
            stage = "lookup_db"
        );
        return Ok(Some(record.id));
    }

    warn!(
        target: "preview.lookup",
        event = events::PREVIEW_VALIDATE_FAILED,
        third_party_request_id = %third_party_request_id,
        user_id = %user_id,
        reason = "not_found"
    );
    Ok(None)
}

// 更新预审状态
// 这个函数已经被移除，因为我们在处理器中直接使用数据库

// 预审状态查询接口
async fn query_preview_status(
    State(app_state): State<AppState>,
    axum::extract::Path(preview_id): axum::extract::Path<String>,
    req: axum::extract::Request,
) -> impl IntoResponse {
    let status_span = tracing::info_span!(
        target: "preview.status",
        "preview_status",
        preview_id = %preview_id
    );
    let _guard = status_span.enter();

    info!(
        target: "preview.status",
        event = events::PREVIEW_RECEIVED,
        preview_id = %preview_id
    );

    let session_user = req.extensions().get::<SessionUser>().cloned();
    let Some(session_user) = session_user else {
        warn!(
            target: "preview.status",
            event = events::AUTH_FAILURE,
            preview_id = %preview_id,
            reason = "missing_session"
        );
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 401,
            "errorMsg": "认证信息缺失",
            "data": null
        }));
    };

    debug!(
        target: "preview.status",
        event = events::AUTH_SUCCESS,
        preview_id = %preview_id,
        user_id = %session_user.user_id
    );

    match verify_preview_access(&app_state.database, &preview_id, &session_user).await {
        Ok(true) => match get_preview_status_info(&app_state.database, &preview_id).await {
            Ok(status_info) => {
                info!(
                    target: "preview.status",
                    event = events::PREVIEW_COMPLETE,
                    preview_id = %preview_id,
                    user_id = %session_user.user_id
                );
                Json(serde_json::json!({
                    "success": true,
                    "errorCode": 200,
                    "errorMsg": "",
                    "data": status_info
                }))
            }
            Err(e) => {
                error!(
                    target: "preview.status",
                    event = events::PREVIEW_ERROR,
                    preview_id = %preview_id,
                    user_id = %session_user.user_id,
                    error = %e
                );
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 500,
                    "errorMsg": "获取预审状态失败",
                    "data": null
                }))
            }
        },
        Ok(false) => {
            warn!(
                target: "preview.status",
                event = events::AUTH_FAILURE,
                preview_id = %preview_id,
                user_id = %session_user.user_id,
                reason = "preview_owner_mismatch"
            );
            Json(serde_json::json!({
                "success": false,
                "errorCode": 403,
                "errorMsg": "无权限访问",
                "data": null
            }))
        }
        Err(e) => {
            error!(
                target: "preview.status",
                event = events::PREVIEW_ERROR,
                preview_id = %preview_id,
                user_id = %session_user.user_id,
                error = %e
            );
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "系统错误",
                "data": null
            }))
        }
    }
}

// 获取预审状态信息
async fn get_preview_status_info(
    database: &Arc<dyn crate::db::Database>,
    preview_id: &str,
) -> anyhow::Result<serde_json::Value> {
    debug!(
        target: "preview.status",
        event = events::PIPELINE_STAGE,
        stage = "load_status",
        preview_id = %preview_id
    );

    // 获取ID映射信息
    let mapping = match preview::get_id_mapping_from_database(database, preview_id).await? {
        Some(mapping) => mapping,
        None => {
            warn!(
                target: "preview.status",
                event = events::PREVIEW_VALIDATE_FAILED,
                preview_id = %preview_id,
                reason = "mapping_missing"
            );
            return Ok(serde_json::json!({
                "status": "unknown",
                "message": "预审记录映射不存在"
            }));
        }
    };

    let status = mapping.status.to_string();
    let message = format!("预审记录状态: {}", status);

    debug!(
        target: "preview.status",
        event = events::PIPELINE_STAGE,
        stage = "load_status_complete",
        preview_id = %preview_id,
        status = %status
    );

    Ok(serde_json::json!({
        "status": status,
        "message": message
    }))
}

// Notify third party system function moved to preview module

// SSO login redirect function moved to auth module

// Root redirect and debug config functions moved to config module

// Preview submit function moved to preview module

// File management functions moved to files module

// Test functions moved to test module
