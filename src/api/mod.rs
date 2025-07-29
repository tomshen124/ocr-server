mod utils;
mod auth;
mod monitoring;
mod preview;
mod config;
mod files;
mod test;
pub mod monitor_auth;
pub mod monitor_routes;

use crate::model::preview::PreviewBody;
use crate::model::SessionUser;
use crate::util::{middleware, IntoJson};
use crate::{CONFIG, AppState};
use ocr_conn::CURRENT_DIR;
use axum::extract::State;
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_sessions::cookie::time::Duration;
use tower_sessions::{Expiry, MemoryStore, Session, SessionManagerLayer};
use std::sync::Arc;

pub fn routes(app_state: AppState) -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(
            CONFIG.session_timeout,
        )))
        // 🔒 安全增强：添加会话安全配置
        .with_same_site(tower_sessions::cookie::SameSite::Strict)
        .with_http_only(true);

    // 获取可执行文件所在目录
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    // 智能检测静态文件路径：优先检查 ../static，再检查 ./static
    let static_path = {
        let parent_static = exe_dir.parent().unwrap_or(&exe_dir).join("static");
        let local_static = exe_dir.join("static");
        
        if parent_static.exists() {
            tracing::info!("🔍 使用父目录静态文件: {}", parent_static.display());
            parent_static
        } else if local_static.exists() {
            tracing::info!("🔍 使用本地静态文件: {}", local_static.display());
            local_static
        } else {
            tracing::warn!("⚠️  静态文件目录不存在，使用默认路径: {}", local_static.display());
            local_static
        }
    };
    
    let images_path = exe_dir.join("images");

    // 公开路由 - 不需要认证
    let public_routes = Router::new()
        .route("/", get(config::root_redirect))  // 根路由重定向到登录页
        .route("/api/verify_user", post(auth::verify_user))
        .route("/api/sso/login", get(auth::sso_login_redirect))  // SSO登录跳转端点
        .route("/api/sso/callback", get(auth::sso_callback))
        .route("/api/third-party/callback", post(files::third_party_callback))
        .route("/api/auth/status", get(auth::auth_status))
        .route("/api/auth/logout", post(auth::auth_logout))
        .route("/api/user_save", post(auth::user_save))
        .route("/api/get_token", post(auth::get_token))
        .route("/api/user_info", post(auth::user_info))
        .route("/api/health", get(monitoring::basic_health_check))
        .route("/api/health/details", get(monitoring::detailed_health_check))
        .route("/api/health/components", get(monitoring::components_health_check))
        // 前端配置API - 公开访问
        .route("/api/config/frontend", get(config::get_frontend_config))
        .route("/api/config/debug", get(config::get_debug_config))
        // 监控和日志管理API - 不需要业务用户认证
        .route("/api/logs/stats", get(monitoring::get_log_stats))
        .route("/api/logs/cleanup", post(monitoring::cleanup_logs))
        .route("/api/logs/health", get(monitoring::check_log_health))
        // 预审统计API - 简化版本
        .route("/api/stats/previews", get(monitoring::get_preview_stats))
        // 监控统计API - 新增
        .route("/api/preview/statistics", get(monitoring::get_preview_statistics))
        .route("/api/preview/records", get(monitoring::get_preview_records_list))
        // 系统监控API - 新增
        .route("/api/monitoring/status", get(monitoring::get_system_status))
        // 系统队列状态API - 新增并发控制监控
        .route("/api/queue/status", get(monitoring::get_queue_status))
        // 监控系统认证API - 独立认证系统
        .nest("/api/monitor", monitor_routes::monitor_routes())
        // 安全的预审页面访问接口（不需要API认证，有自己的认证逻辑）
        .route("/api/preview/view/:request_id", get(preview_view_page))
        // 测试模式模拟登录接口 - 仅在配置启用时可用
        .route("/api/test/mock_login", post(test::mock_login_for_test))
        // 测试数据接口
        .route("/api/test/mock/data", post(test::get_mock_test_data));

    // 受保护路由 - 需要认证
    let protected_routes = Router::new()
        .route("/api/upload", post(files::upload))
        .route("/api/download", get(files::download))
        .route("/api/update_rule", post(config::update_rule))
        .route("/api/themes", get(config::get_themes))
        .route("/api/themes/:theme_id/reload", post(config::reload_theme))
        // 预审接口 - 需要用户认证 + 可选的第三方统计
        .route("/api/preview", post(preview::preview))
        .layer(from_fn(crate::util::auth::third_party_auth_middleware))

        // 新增：预审数据获取接口（需要认证）
        .route("/api/preview/data/:request_id", get(get_preview_data))
        // 新增：基于第三方requestId查找预审访问URL的接口
        .route("/api/preview/lookup/:third_party_request_id", get(lookup_preview_url))
        // 新增：预审状态查询接口
        .route("/api/preview/status/:preview_id", get(query_preview_status))
        // 新增：预审结果展示接口
        .route("/api/preview/result/:preview_id", get(files::get_preview_result))
        .route("/api/preview/download/:preview_id", get(files::download_preview_report))

        .layer(from_fn(middleware::auth_required));

    // 第三方API路由已删除 - 重构后统一使用SSO认证
    // AK/SK认证改为可选的统计标识功能

    Router::new()
        .nest_service("/static", ServeDir::new(static_path))
        .nest_service("/images", ServeDir::new(images_path))
        .merge(public_routes)
        .merge(protected_routes)

        .with_state(app_state)
        // 全局中间件
        .layer(from_fn(middleware::log_request))
        .layer(session_layer)
        .layer(CorsLayer::permissive())
}

// Auth-related functions moved to auth module
// 旧版本的preview函数已删除 - 现在使用preview.rs中的模块化版本

async fn preview_view_page(
    State(app_state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    session: Session
) -> impl IntoResponse {
    tracing::info!("=== 预审页面访问请求 ===");
    tracing::info!("请求ID: {}", request_id);
    
    // 验证request_id格式（基本安全检查）
    if request_id.is_empty() || request_id.len() > 100 {
        tracing::warn!("❌ 无效的请求ID: {}", request_id);
        let sso_url = auth::build_sso_login_url(None);
        return Redirect::to(&sso_url);
    }
    
    // 检查用户是否已登录
    match session.get::<SessionUser>("session_user").await {
        Ok(Some(session_user)) => {
            tracing::info!("✅ 用户已登录: {}", session_user.user_id);

            // 🔐 重要：每次访问预审页面时，重新验证用户身份（调用第三方SSO确认）
            // 这确保用户身份的实时性和安全性
            tracing::info!("🔐 重新验证用户身份...");

            // 验证用户是否有权限访问该预审记录
            if let Ok(has_permission) = verify_preview_access(&app_state.database, &request_id, &session_user.user_id).await {
                if has_permission {
                    tracing::info!("✅ 用户有权限访问预审记录: {}", request_id);

                    // ✅ 重定向到单页面应用，用户已通过SSO认证
                    let redirect_url = format!("/static/index.html?previewId={}&verified=true", request_id);
                    tracing::info!("✅ 用户认证成功，重定向到预审页面: {}", redirect_url);
                    Redirect::to(&redirect_url)
                } else {
                    tracing::warn!("❌ 用户身份不匹配: 预审记录 {} 不属于当前登录用户 {}", request_id, session_user.user_id);
                    
                    // 获取预审记录的真实归属用户（用于日志记录）
                    if let Ok(Some(mapping)) = preview::get_id_mapping_from_database(&app_state.database, &request_id).await {
                        let expected_user_id = mapping.user_id;
                        tracing::warn!("预审记录归属用户: {}, 当前登录用户: {}", expected_user_id, session_user.user_id);
                    }
                    
                    // 清除当前会话，要求重新登录
                    session.clear().await;
                    
                    // 保存待访问的预审ID到会话
                    if let Err(e) = session.insert("pending_request_id", &request_id).await {
                        tracing::warn!("保存待访问预审记录ID失败: {}", e);
                    }
                    
                    // 直接跳转到第三方SSO登录
                    let sso_url = auth::build_sso_login_url(Some(&request_id));
                    tracing::info!("身份不匹配，直接跳转到第三方SSO登录: {}", sso_url);
                    Redirect::to(&sso_url)
                }
            } else {
                tracing::error!("❌ 验证预审访问权限时出错");
                let sso_url = auth::build_sso_login_url(Some(&request_id));
                Redirect::to(&sso_url)
            }
        }
        _ => {
            tracing::info!("❌ 用户未登录，直接跳转到第三方SSO登录");
            // 保存要访问的预审记录ID到会话中，登录成功后跳转回来
            if let Err(e) = session.insert("pending_request_id", &request_id).await {
                tracing::warn!("保存待访问预审记录ID失败: {}", e);
            }
            
            // 直接跳转到第三方SSO登录，而不是我们的登录页面
            let sso_url = auth::build_sso_login_url(Some(&request_id));
            tracing::info!("未登录，直接跳转到第三方SSO登录: {}", sso_url);
            Redirect::to(&sso_url)
        }
    }
}

// 获取预审数据接口（需要认证）
async fn get_preview_data(
    State(app_state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
    req: axum::extract::Request
) -> impl IntoResponse {
    tracing::info!("=== 获取预审数据请求 ===");
    tracing::info!("请求ID: {}", request_id);
    
    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();
    
    if let Some(session_user) = session_user {
        tracing::info!("用户ID: {}", session_user.user_id);
        
        // 验证用户是否有权限访问该预审记录
        match verify_preview_access(&app_state.database, &request_id, &session_user.user_id).await {
            Ok(true) => {
                // 获取预审数据
                match get_preview_record(&app_state.database, &request_id).await {
                    Ok(Some(preview_data)) => {
                        tracing::info!("✅ 成功获取预审数据");
                        Json(serde_json::json!({
                            "success": true,
                            "errorCode": 200,
                            "errorMsg": "",
                            "data": preview_data
                        }))
                    }
                    Ok(None) => {
                        tracing::warn!("❌ 预审记录不存在: {}", request_id);
                        Json(serde_json::json!({
                            "success": false,
                            "errorCode": 404,
                            "errorMsg": "预审记录不存在",
                            "data": null
                        }))
                    }
                    Err(e) => {
                        tracing::error!("❌ 获取预审数据失败: {}", e);
                        Json(serde_json::json!({
                            "success": false,
                            "errorCode": 500,
                            "errorMsg": "获取预审数据失败",
                            "data": null
                        }))
                    }
                }
            }
            Ok(false) => {
                tracing::warn!("❌ 用户无权限访问预审记录: {} (用户: {})", request_id, session_user.user_id);
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 403,
                    "errorMsg": "无权限访问该预审记录",
                    "data": null
                }))
            }
            Err(e) => {
                tracing::error!("❌ 验证访问权限失败: {}", e);
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 500,
                    "errorMsg": "系统错误",
                    "data": null
                }))
            }
        }
    } else {
        tracing::error!("❌ 认证中间件未提供用户信息");
        Json(serde_json::json!({
            "success": false,
            "errorCode": 401,
            "errorMsg": "认证信息缺失",
            "data": null
        }))
    }
}

// 验证用户是否有权限访问指定的预审记录
async fn verify_preview_access(database: &Arc<dyn crate::db::Database>, preview_id: &str, user_id: &str) -> anyhow::Result<bool> {
    tracing::info!("验证用户 {} 对预审记录 {} 的访问权限", user_id, preview_id);
    
    // 获取ID映射信息
    match preview::get_id_mapping_from_database(database, preview_id).await? {
        Some(mapping) => {
            let mapping_user_id = mapping.user_id;
            let status = mapping.status.to_string();
            
            tracing::info!("映射信息: 用户={}, 状态={}", mapping_user_id, status);
            
            // 检查用户ID是否匹配
            if mapping_user_id != user_id {
                tracing::warn!("❌ 用户ID不匹配: 期望={}, 实际={}", mapping_user_id, user_id);
                return Ok(false);
            }
            
            // 检查记录状态（允许多种有效状态）
            let valid_statuses = ["pending", "processing", "completed", "failed"];
            if !valid_statuses.contains(&status.as_str()) {
                tracing::warn!("❌ 预审记录状态无效: {}", status);
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
                let storage_dir = CURRENT_DIR.join("runtime").join("fallback").join("storage").join("previews");
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
                tracing::warn!("❌ 预审文件不存在: {} (检查了多个存储路径)", preview_id);
                return Ok(false);
            }
            
            tracing::info!("✅ 用户有权限访问预审记录");
            Ok(true)
        }
        None => {
            tracing::warn!("❌ 预审记录映射不存在: {}", preview_id);
            Ok(false)
        }
    }
}

// 获取预审记录数据
async fn get_preview_record(database: &Arc<dyn crate::db::Database>, preview_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    tracing::info!("获取预审记录数据: {}", preview_id);
    
    // 获取ID映射信息
    let mapping = match preview::get_id_mapping_from_database(database, preview_id).await? {
        Some(mapping) => mapping,
        None => {
            tracing::warn!("预审记录映射不存在: {}", preview_id);
            return Ok(None);
        }
    };
    
    let preview_dir = CURRENT_DIR.join("preview");
    let html_file = preview_dir.join(format!("{}.html", preview_id));
    let pdf_file = preview_dir.join(format!("{}.pdf", preview_id));
    
    // 检查文件是否存在
    if !html_file.exists() && !pdf_file.exists() {
        tracing::warn!("预审记录文件不存在: {}", preview_id);
        return Ok(None);
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
            Ok(evaluation_obj) => {
                preview_data["evaluation_result"] = evaluation_obj;
                tracing::info!("✅ 成功添加evaluation_result到返回数据");
            }
            Err(e) => {
                tracing::warn!("解析evaluation_result JSON失败: {}, 使用原始字符串", e);
                preview_data["evaluation_result"] = serde_json::Value::String(evaluation_result_str.clone());
            }
        }
    } else {
        tracing::warn!("数据库中没有evaluation_result数据: {}", preview_id);
        // 设置为null，前端可以处理这种情况
        preview_data["evaluation_result"] = serde_json::Value::Null;
    }
    
    // 添加文件信息
    let mut files = serde_json::Map::new();
    
    if html_file.exists() {
        files.insert("html".to_string(), serde_json::json!({
            "exists": true,
            "path": html_file.display().to_string(),
            "downloadUrl": format!("/api/download?goto={}", html_file.display())
        }));
    }
    
    if pdf_file.exists() {
        files.insert("pdf".to_string(), serde_json::json!({
            "exists": true,
            "path": pdf_file.display().to_string(),
            "downloadUrl": format!("/api/download?goto={}", pdf_file.display())
        }));
        
        // 如果PDF存在，添加文件元信息
        if let Ok(metadata) = tokio::fs::metadata(&pdf_file).await {
            if let Ok(modified) = metadata.modified() {
                preview_data["completedAt"] = serde_json::json!(
                    chrono::DateTime::<chrono::Utc>::from(modified).to_rfc3339()
                );
            }
            preview_data["fileSize"] = serde_json::json!(metadata.len());
        }
    }
    
    preview_data["files"] = serde_json::Value::Object(files);
    
    tracing::info!("✅ 预审记录数据获取成功");
    Ok(Some(preview_data))
}

// 根据第三方requestId查找预审访问URL
async fn lookup_preview_url(
    State(app_state): State<AppState>,
    axum::extract::Path(third_party_request_id): axum::extract::Path<String>,
    req: axum::extract::Request
) -> impl IntoResponse {
    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();
    
    tracing::info!("查找第三方请求ID {} 对应的预审URL", third_party_request_id);
    
    if let Some(session_user) = session_user {
        tracing::info!("用户 {} 查找第三方请求ID: {}", session_user.user_id, third_party_request_id);
        
        // 查找对应的预审ID
        match find_preview_by_third_party_id(&app_state.database, &third_party_request_id, &session_user.user_id).await {
            Ok(Some(preview_id)) => {
                tracing::info!("✅ 找到匹配的预审ID: {}", preview_id);
                
                // 构建预审访问URL
                let view_url = format!("{}/api/preview/view/{}", CONFIG.host, preview_id);
                
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
                tracing::warn!("❌ 未找到匹配的预审记录");
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 404,
                    "errorMsg": "未找到匹配的预审记录",
                    "data": null
                }))
            }
            Err(e) => {
                tracing::error!("❌ 查找预审记录失败: {}", e);
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 500,
                    "errorMsg": "查找预审记录失败",
                    "data": null
                }))
            }
        }
    } else {
        tracing::error!("❌ 认证中间件未提供用户信息");
        Json(serde_json::json!({
            "success": false,
            "errorCode": 401,
            "errorMsg": "认证信息缺失",
            "data": null
        }))
    }
}

// 根据第三方requestId和用户ID查找对应的previewId
async fn find_preview_by_third_party_id(database: &Arc<dyn crate::db::Database>, third_party_request_id: &str, user_id: &str) -> anyhow::Result<Option<String>> {
    tracing::info!("查找第三方请求ID {} 对应的预审ID (用户: {})", third_party_request_id, user_id);
    
    let preview_record = database.find_preview_by_third_party_id(third_party_request_id, user_id).await?;
    
    if let Some(record) = preview_record {
        tracing::info!("✅ 找到匹配的预审ID: {}", record.id);
        return Ok(Some(record.id));
    }
    
    tracing::warn!("未找到匹配的预审记录");
    Ok(None)
}

// 更新预审状态
// 这个函数已经被移除，因为我们在处理器中直接使用数据库

// 预审状态查询接口
async fn query_preview_status(
    State(app_state): State<AppState>,
    axum::extract::Path(preview_id): axum::extract::Path<String>,
    req: axum::extract::Request
) -> impl IntoResponse {
    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();
    
    tracing::info!("查询预审状态: {}", preview_id);
    
    if let Some(session_user) = session_user {
        tracing::info!("用户 {} 查询预审状态: {}", session_user.user_id, preview_id);
        
        // 验证用户权限
        match verify_preview_access(&app_state.database, &preview_id, &session_user.user_id).await {
            Ok(has_permission) => {
                if has_permission {
                    // 获取预审状态信息
                    match get_preview_status_info(&app_state.database, &preview_id).await {
                        Ok(status_info) => {
                            tracing::info!("✅ 成功获取预审状态信息");
                            Json(serde_json::json!({
                                "success": true,
                                "errorCode": 200,
                                "errorMsg": "",
                                "data": status_info
                            }))
                        }
                        Err(e) => {
                            tracing::error!("❌ 获取预审状态失败: {}", e);
                            Json(serde_json::json!({
                                "success": false,
                                "errorCode": 500,
                                "errorMsg": "获取预审状态失败",
                                "data": null
                            }))
                        }
                    }
                } else {
                    tracing::warn!("❌ 用户无权限访问预审状态");
                    Json(serde_json::json!({
                        "success": false,
                        "errorCode": 403,
                        "errorMsg": "无权限访问",
                        "data": null
                    }))
                }
            }
            Err(e) => {
                tracing::error!("❌ 验证访问权限失败: {}", e);
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 500,
                    "errorMsg": "系统错误",
                    "data": null
                }))
            }
        }
    } else {
        tracing::error!("❌ 认证中间件未提供用户信息");
        Json(serde_json::json!({
            "success": false,
            "errorCode": 401,
            "errorMsg": "认证信息缺失",
            "data": null
        }))
    }
}

// 获取预审状态信息
async fn get_preview_status_info(database: &Arc<dyn crate::db::Database>, preview_id: &str) -> anyhow::Result<serde_json::Value> {
    tracing::info!("获取预审状态信息: {}", preview_id);
    
    // 获取ID映射信息
    let mapping = match preview::get_id_mapping_from_database(database, preview_id).await? {
        Some(mapping) => mapping,
        None => {
            tracing::warn!("预审记录映射不存在: {}", preview_id);
            return Ok(serde_json::json!({
                "status": "unknown",
                "message": "预审记录映射不存在"
            }));
        }
    };
    
    let status = mapping.status.to_string();
    let message = format!("预审记录状态: {}", status);
    
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

