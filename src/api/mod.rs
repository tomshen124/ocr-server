use crate::model::preview::PreviewBody;
use crate::model::user::User;
use crate::model::{
    ComponentStatus, ComponentsHealth, DetailedHealthStatus, Goto, HealthStatus, Ticket, TicketId, Token, SessionUser,
};
use crate::util::{middleware, system_info, third_party_auth, IntoJson, ServerError};
use crate::{CONFIG, AppState};
use ocr_conn::CURRENT_DIR;
use chrono::Utc;
use axum::extract::{Multipart, Query, State};
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use axum::http::StatusCode;
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
        .route("/", get(root_redirect))  // 根路由重定向到登录页
        .route("/api/verify_user", post(verify_user))
        .route("/api/sso/login", get(sso_login_redirect))  // SSO登录跳转端点
        .route("/api/sso/callback", get(sso_callback))
        .route("/api/third-party/callback", post(third_party_callback))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/logout", post(auth_logout))
        .route("/api/user_save", post(user_save))
        .route("/api/get_token", post(get_token))
        .route("/api/user_info", post(user_info))
        .route("/api/health", get(basic_health_check))
        .route("/api/health/details", get(detailed_health_check))
        .route("/api/health/components", get(components_health_check))
        // 前端配置API - 公开访问
        .route("/api/config/frontend", get(get_frontend_config))
        .route("/api/config/debug", get(get_debug_config))
        // 监控和日志管理API - 不需要业务用户认证
        .route("/api/logs/stats", get(get_log_stats))
        .route("/api/logs/cleanup", post(cleanup_logs))
        .route("/api/logs/health", get(check_log_health))
        // 预审统计API - 简化版本
        .route("/api/stats/previews", get(get_preview_stats))
        // 监控统计API - 新增
        .route("/api/preview/statistics", get(get_preview_statistics))
        .route("/api/preview/records", get(get_preview_records_list))
        // 系统队列状态API - 新增并发控制监控
        .route("/api/queue/status", get(get_queue_status))
        // 安全的预审页面访问接口（不需要API认证，有自己的认证逻辑）
        .route("/api/preview/view/:request_id", get(preview_view_page))
        // 测试模式模拟登录接口 - 仅在配置启用时可用
        .route("/api/test/mock_login", post(mock_login_for_test))
        // 测试数据接口
        .route("/api/test/mock/data", post(get_mock_test_data));

    // 受保护路由 - 需要认证
    let protected_routes = Router::new()
        .route("/api/upload", post(upload))
        .route("/api/download", get(download))
        .route("/api/update_rule", post(update_rule))
        .route("/api/themes", get(get_themes))
        .route("/api/themes/:theme_id/reload", post(reload_theme))
        // 预审接口 - 需要用户认证
        .route("/api/preview", post(preview))
        .route("/api/preview/submit", post(preview_submit))
        // 新增：预审数据获取接口（需要认证）
        .route("/api/preview/data/:request_id", get(get_preview_data))
        // 新增：基于第三方requestId查找预审访问URL的接口
        .route("/api/preview/lookup/:third_party_request_id", get(lookup_preview_url))
        // 新增：预审状态查询接口
        .route("/api/preview/status/:preview_id", get(query_preview_status))
        // 新增：预审结果展示接口
        .route("/api/preview/result/:preview_id", get(get_preview_result))
        .route("/api/preview/download/:preview_id", get(download_preview_report))

        .layer(from_fn(middleware::auth_required));

    // 第三方API路由 - 需要AK/SK认证
    let third_party_api_routes = Router::new()
        .route("/api/third-party/preview", post(preview))
        .route("/api/third-party/preview/status/:preview_id", get(query_preview_status))
        .route("/api/third-party/preview/lookup/:third_party_request_id", get(lookup_preview_url))
        .layer(from_fn(third_party_auth::third_party_auth_middleware));

    Router::new()
        .nest_service("/static", ServeDir::new(static_path))
        .nest_service("/images", ServeDir::new(images_path))
        .merge(public_routes)
        .merge(protected_routes)
        .merge(third_party_api_routes)
        .with_state(app_state)
        // 全局中间件
        .layer(from_fn(middleware::log_request))
        .layer(session_layer)
        .layer(CorsLayer::permissive())
}

async fn user_save(session: Session, Json(ticket_id): Json<TicketId>) -> impl IntoResponse {
    let result = User::user_save(session, ticket_id).await;
    result.into_json()
}

async fn get_token(Json(ticket): Json<Ticket>) -> impl IntoResponse {
    let result = User::get_token_by_ticket(ticket).await;
    result.into_json()
}

async fn user_info(session: Session, Json(token): Json<Token>) -> impl IntoResponse {
    let result = User::get_user_by_token(session, token).await;
    result.into_json()
}

async fn preview(State(app_state): State<AppState>, req: axum::extract::Request) -> impl IntoResponse {
    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();
    
    // 提取请求体
    let (_parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("读取请求体失败: {}", e);
            return crate::util::WebResult::err_custom("无效的请求体").into_json().into_response();
        }
    };
    
    // 尝试解析为标准格式，如果失败则尝试生产环境格式
    let mut preview_body: PreviewBody = match serde_json::from_slice::<PreviewBody>(&bytes) {
        Ok(body) => {
            tracing::info!("✅ 解析为标准格式成功");
            body
        },
        Err(_) => {
            // 尝试解析为生产环境格式
            match serde_json::from_slice::<crate::model::preview::ProductionPreviewRequest>(&bytes) {
                Ok(prod_request) => {
                    tracing::info!("✅ 解析为生产环境格式成功，正在转换...");
                    prod_request.to_preview_body()
                },
                Err(e) => {
                    tracing::error!("解析请求体失败（尝试了标准格式和生产环境格式）: {}", e);
                    return crate::util::WebResult::err_custom("无效的JSON格式").into_json().into_response();
                }
            }
        }
    };
    
    // 简化的用户ID验证
    if let Some(session_user) = session_user {
        tracing::info!("=== 用户身份验证 ===");
        tracing::info!("会话用户ID: {}", session_user.user_id);
        tracing::info!("请求用户ID: {}", preview_body.user_id);
        
        // 只验证用户ID匹配即可
        if preview_body.user_id != session_user.user_id {
            tracing::warn!("❌ 用户ID不匹配: 请求用户={}, 会话用户={}", 
                          preview_body.user_id, session_user.user_id);
            return crate::util::WebResult::err_custom("用户身份验证失败：用户ID不匹配").into_json().into_response();
        }
        
        tracing::info!("✅ 用户身份验证通过: {}", session_user.user_id);
    } else {
        tracing::error!("❌ 认证中间件未提供用户信息");
        return crate::util::WebResult::err_custom("认证信息缺失").into_json().into_response();
    }
    
    // 保留第三方系统的原始requestId
    let third_party_request_id = preview_body.preview.request_id.clone();
    
    // 服务端生成我们自己的安全previewId
    let our_preview_id = generate_secure_preview_id();
    tracing::info!("第三方请求ID: {}", third_party_request_id);
    tracing::info!("我们的预审ID: {}", our_preview_id);
    
    // 使用我们的previewId作为文件名（确保安全）
    preview_body.preview.request_id = our_preview_id.clone();
    
    // 🔒 安全改进：验证第三方提供的用户ID格式
    if preview_body.user_id.is_empty() || preview_body.user_id.len() > 50 {
        tracing::warn!("❌ 无效的用户ID格式: {}", preview_body.user_id);
        return crate::util::WebResult::err_custom("无效的用户ID").into_json().into_response();
    }

    // 建立ID映射关系（使用数据库替代文件操作）
    if let Err(e) = save_id_mapping_to_database(&app_state.database, &our_preview_id, &third_party_request_id, &preview_body.user_id).await {
        tracing::error!("保存ID映射失败: {}", e);
        return crate::util::WebResult::err_custom("系统错误").into_json().into_response();
    }
    
    // 立即返回预审访问URL，不等待预审完成
    let view_url = format!("{}/api/preview/view/{}", CONFIG.host, our_preview_id);

    tracing::info!("立即返回预审访问URL: {}", view_url);

    // � 回滚到原始设计：立即异步处理，提供最佳用户体验
    // 原因：政务服务场景下，用户期望即时反馈，延迟处理严重影响体验
    // 安全性通过严格的身份验证和权限控制来保障
    let mut preview_clone = preview_body.clone();
    let preview_id_clone = our_preview_id.clone();
    let third_party_id_clone = third_party_request_id.clone();
    let database_clone = app_state.database.clone();
    let storage_clone = app_state.storage.clone();
    
    tokio::spawn(async move {
        // 🔥 立即获取OCR并发控制许可
        // 如果系统繁忙，这里会等待，避免系统过载
        let permit = match crate::OCR_SEMAPHORE.try_acquire() {
            Ok(permit) => {
                tracing::info!("✅ 获取OCR处理许可成功，当前可用许可: {}", 
                             crate::OCR_SEMAPHORE.available_permits());
                Some(permit)
            },
            Err(_) => {
                tracing::warn!("⏳ 系统繁忙，OCR任务排队等待...");
                // 如果try_acquire失败，使用acquire等待
                match crate::OCR_SEMAPHORE.acquire().await {
                    Ok(permit) => {
                        tracing::info!("✅ 等待后获取OCR处理许可成功");
                        Some(permit)
                    },
                    Err(e) => {
                        tracing::error!("❌ 获取OCR处理许可失败: {}", e);
                        // 更新数据库状态为失败
                        if let Err(db_err) = database_clone.update_preview_status(&preview_id_clone, crate::db::PreviewStatus::Failed).await {
                            tracing::error!("更新预审状态失败: {}", db_err);
                        }
                        return;
                    }
                }
            }
        };
        
        tracing::info!("=== 开始自动预审任务（并发控制） ===");
        tracing::info!("预审ID: {}", preview_id_clone);
        tracing::info!("第三方请求ID: {}", third_party_id_clone);
        tracing::info!("当前系统可用OCR处理槽位: {}", crate::OCR_SEMAPHORE.available_permits());
        
        // 智能主题匹配
        let theme_id = if let Some(manual_theme) = &preview_clone.preview.theme_id {
            tracing::info!("✅ 使用手动指定的主题ID: {}", manual_theme);
            manual_theme.clone()
        } else {
            // 自动匹配主题
            let auto_theme = crate::util::zen::find_theme_by_matter(
                Some(&preview_clone.preview.matter_id),
                Some(&preview_clone.preview.matter_name)
            );
            tracing::info!("✅ 自动匹配主题ID: {}", auto_theme);
            auto_theme
        };
        
        // 设置主题ID
        preview_clone.preview.theme_id = Some(theme_id);
        
        // 更新预审状态为"处理中"
        if let Err(e) = database_clone.update_preview_status(&preview_id_clone, crate::db::PreviewStatus::Processing).await {
            tracing::error!("更新预审状态失败: {}", e);
        }
        
        // 执行预审逻辑（使用存储抽象层）
        let preview_result = preview_clone.preview_with_storage(&storage_clone).await;
        
        // 根据结果更新数据库状态
        let status = if preview_result.is_ok() {
            crate::db::PreviewStatus::Completed
        } else {
            crate::db::PreviewStatus::Failed
        };
        
        if let Err(e) = database_clone.update_preview_status(&preview_id_clone, status).await {
            tracing::error!("更新最终预审状态失败: {}", e);
        }
        
        match preview_result {
            Ok(result) => {
                tracing::info!("✅ 预审任务完成成功");
                tracing::info!("预审结果: {:?}", result);
                
                // 可选：通知第三方系统（如果配置了回调）
                if let Err(e) = notify_third_party_system(&third_party_id_clone, "completed", Some(&result)).await {
                    tracing::warn!("通知第三方系统失败: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("❌ 预审任务失败: {}", e);
                
                // 通知第三方系统失败
                if let Err(notify_err) = notify_third_party_system(&third_party_id_clone, "failed", None).await {
                    tracing::warn!("通知第三方系统失败: {}", notify_err);
                }
            }
        }
        
        // 🔥 释放OCR处理许可 - 确保许可被正确释放
        if let Some(_permit) = permit {
            tracing::info!("🔓 释放OCR处理许可，当前可用许可: {}", 
                         crate::OCR_SEMAPHORE.available_permits() + 1);
        }
        
        tracing::info!("=== 自动预审任务结束（并发控制） ===");
    });
    
    // 构建响应数据 - 第三方系统只需要知道提交成功
    let response_data = serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "previewId": our_preview_id,
            "thirdPartyRequestId": third_party_request_id,
            "status": "submitted",
            "message": "预审任务已提交，正在后台处理"
        }
    });

    // 预审访问URL是给用户的，不是给第三方系统的
    // 用户会从政务系统跳转到: /api/preview/view/{previewId}
    tracing::info!("用户预审访问URL: {}", view_url);
    
    Json(response_data).into_response()
}

// 生成安全的预审ID
fn generate_secure_preview_id() -> String {
    use chrono::Utc;
    use uuid::Uuid;

    // 组合方案：时间戳 + UUID，确保唯一性和安全性
    let timestamp = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let uuid = Uuid::new_v4().to_string().replace("-", "");

    // 使用UUID的另一部分作为随机后缀，避免额外依赖
    let random_suffix = &uuid[12..18].to_uppercase();

    // 格式：PV{时间戳}{UUID前12位}{UUID的12-18位大写}
    // 总长度：2 + 14 + 12 + 6 = 34位
    // PV = Preview（预审）
    format!("PV{}{}{}", timestamp, &uuid[..12].to_uppercase(), random_suffix)
}



// 使用数据库保存ID映射关系（替代文件操作）
async fn save_id_mapping_to_database(database: &Arc<dyn crate::db::Database>, preview_id: &str, third_party_request_id: &str, user_id: &str) -> anyhow::Result<()> {
    use crate::db::{PreviewRecord, PreviewStatus};
    
    tracing::info!("保存ID映射到数据库: {} -> {}", preview_id, third_party_request_id);
    
    let record = PreviewRecord {
        id: preview_id.to_string(),
        user_id: user_id.to_string(),
        file_name: format!("{}.html", preview_id),
        ocr_text: "".to_string(), // 将在后续处理中填充
        theme_id: None,
        evaluation_result: None,
        preview_url: format!("{}/api/preview/view/{}", CONFIG.host, preview_id),
        status: PreviewStatus::Pending,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        third_party_request_id: Some(third_party_request_id.to_string()),
    };
    
    database.save_preview_record(&record).await?;
    
    tracing::info!("✅ ID映射已保存到数据库");
    Ok(())
}

// 从数据库获取ID映射信息（替代文件操作）
async fn get_id_mapping_from_database(database: &Arc<dyn crate::db::Database>, preview_id: &str) -> anyhow::Result<Option<crate::db::PreviewRecord>> {
    database.get_preview_record(preview_id).await
}

async fn upload(multipart: Multipart) -> impl IntoResponse {
    let result = crate::model::ocr::upload(multipart).await;
    result.into_json()
}

async fn download(Query(goto): Query<Goto>) -> impl IntoResponse {
    let result = PreviewBody::download(goto).await;
    result.map_err(|err| ServerError::Custom(err.to_string()))
}

// 第三方系统回调处理 (POST方式，用于预审完成通知)
async fn third_party_callback(Json(callback_data): Json<serde_json::Value>) -> impl IntoResponse {
    tracing::info!("=== 第三方系统回调接收 ===");
    tracing::info!("回调数据: {}", serde_json::to_string_pretty(&callback_data).unwrap_or_default());
    
    // 在模拟环境中，我们只需要记录回调，不需要实际处理
    if let Some(preview_id) = callback_data.get("previewId").and_then(|v| v.as_str()) {
        tracing::info!("✅ 模拟第三方系统收到预审完成通知: {}", preview_id);
        
        if let Some(status) = callback_data.get("status").and_then(|v| v.as_str()) {
            tracing::info!("预审状态: {}", status);
        }
        
        if let Some(third_party_id) = callback_data.get("thirdPartyRequestId").and_then(|v| v.as_str()) {
            tracing::info!("第三方请求ID: {}", third_party_id);
        }
    }
    
    tracing::info!("=== 第三方系统回调处理完成 ===");
    
    // 返回成功响应（模拟第三方系统接收成功）
    Json(serde_json::json!({
        "success": true,
        "message": "回调接收成功",
        "timestamp": Utc::now().to_rfc3339()
    }))
}

// SSO回调处理 (GET方式)
async fn sso_callback(session: Session, Query(params): Query<std::collections::HashMap<String, String>>) -> impl IntoResponse {
    tracing::info!("=== SSO回调开始 ===");
    tracing::info!("回调URL参数: {:?}", params);
    tracing::info!("参数数量: {}", params.len());

    // 记录所有可能的票据参数名
    let possible_ticket_params = ["ticketId", "ticket", "code", "token", "st", "service_ticket"];
    for param_name in &possible_ticket_params {
        if let Some(value) = params.get(*param_name) {
            tracing::info!("发现票据参数 '{}': {}", param_name, value);
        }
    }

    // 尝试从不同的参数名中获取票据ID
    let ticket_id = params.get("ticketId")
        .or_else(|| params.get("ticket"))
        .or_else(|| params.get("code"))
        .or_else(|| params.get("token"))
        .or_else(|| params.get("st"))
        .or_else(|| params.get("service_ticket"));

    if let Some(ticket_id) = ticket_id {
        tracing::info!("✅ 成功提取票据ID: {}", ticket_id);
        tracing::info!("票据长度: {} 字符", ticket_id.len());

        // 创建SessionUser对象
        let session_user = create_session_user_from_ticket(ticket_id).await;
        
        // 保存完整的用户信息到会话中
        tracing::info!("正在保存用户信息到会话...");
        if let Err(e) = session.insert("session_user", &session_user).await {
            tracing::error!("❌ 保存用户信息到会话失败: {}", e);
            // 会话保存失败，重新尝试SSO登录
            let sso_url = build_sso_login_url(None);
            return Redirect::to(&sso_url);
        }
        tracing::info!("✅ 用户信息已保存到会话");

        tracing::info!("🎉 SSO回调处理完全成功！");
        tracing::info!("用户ID: {}", session_user.user_id);
        tracing::info!("用户姓名: {}", session_user.user_name.as_deref().unwrap_or("未知"));
        
        // 确定重定向URL的优先级：待访问预审记录 > 保存的返回URL > 默认主页
        let redirect_url = if let Ok(Some(pending_request_id)) = session.get::<String>("pending_request_id").await {
            tracing::info!("发现待访问预审记录: {}", pending_request_id);
            // 清除待访问记录
            if let Err(e) = session.remove::<String>("pending_request_id").await {
                tracing::warn!("清除待访问预审记录失败: {}", e);
            }
            // 重定向到静态页面而不是API接口
            format!("/static/index.html?previewId={}&verified=true", pending_request_id)
        } else if let Ok(Some(return_url)) = session.get::<String>("return_url").await {
            tracing::info!("发现保存的返回URL: {}", return_url);
            // 清除返回URL
            if let Err(e) = session.remove::<String>("return_url").await {
                tracing::warn!("清除返回URL失败: {}", e);
            }
            return_url
        } else {
            tracing::info!("无特定跳转目标，重定向到主页");
            format!("/static/index.html?from=sso&user={}", session_user.user_id)
        };
        
        tracing::info!("重定向URL: {}", redirect_url);
        tracing::info!("=== SSO回调结束 ===");

        Redirect::to(&redirect_url)
    } else {
        tracing::warn!("❌ SSO回调中未找到有效的票据参数");
        tracing::warn!("检查的参数名: {:?}", possible_ticket_params);
        tracing::warn!("实际收到的参数: {:?}", params.keys().collect::<Vec<_>>());
        tracing::warn!("可能的原因:");
        tracing::warn!("1. 第三方系统使用了不同的参数名");
        tracing::warn!("2. 第三方系统配置错误");
        tracing::warn!("3. 回调URL配置不正确");
        tracing::info!("=== SSO回调结束（失败）===");

        // SSO回调失败，重新尝试SSO登录
        let sso_url = build_sso_login_url(None);
        Redirect::to(&sso_url)
    }
}

// 验证用户票据
async fn verify_user(session: Session, Json(ticket_id): Json<TicketId>) -> impl IntoResponse {
    tracing::info!("=== 用户票据验证开始 ===");
    tracing::info!("收到票据ID: {}", ticket_id.ticket_id);
    tracing::info!("票据长度: {} 字符", ticket_id.ticket_id.len());

    // 检查是否配置了第三方SSO（增强版本）
    let has_sso_config = !CONFIG.login.access_token_url.is_empty() &&
                        !CONFIG.login.get_user_info_url.is_empty() &&
                        !CONFIG.login.access_key.is_empty() &&
                        !CONFIG.login.secret_key.is_empty();
    tracing::info!("SSO配置检查结果: {}", if has_sso_config { "✅ 完整配置" } else { "⚠️ 配置不完整" });
    tracing::info!("  access_token_url: {}", if CONFIG.login.access_token_url.is_empty() { "❌ 未配置" } else { "✅ 已配置" });
    tracing::info!("  get_user_info_url: {}", if CONFIG.login.get_user_info_url.is_empty() { "❌ 未配置" } else { "✅ 已配置" });
    tracing::info!("  access_key: {}", if CONFIG.login.access_key.is_empty() { "❌ 未配置" } else { "✅ 已配置" });
    tracing::info!("  secret_key: {}", if CONFIG.login.secret_key.is_empty() { "❌ 未配置" } else { "✅ 已配置" });
    tracing::info!("  use_callback: {}", CONFIG.login.use_callback);

    let session_user = if !has_sso_config {
        tracing::warn!("⚠️  SSO配置未完成，使用简化验证模式");
        tracing::info!("简化模式说明: 直接将票据作为用户标识，不调用第三方API验证");
        create_session_user_from_ticket(&ticket_id.ticket_id).await
    } else {
        tracing::info!("🔄 使用完整SSO验证模式");
        tracing::info!("配置信息:");
        tracing::info!("  access_token_url: {}", CONFIG.login.access_token_url);
        tracing::info!("  get_user_info_url: {}", CONFIG.login.get_user_info_url);

        // 调用第三方API获取完整用户信息（带重试机制）
        match get_user_info_from_sso_with_retry(&ticket_id.ticket_id).await {
            Ok(user) => {
                tracing::info!("✅ 完整SSO模式认证成功");
                user
            },
            Err(e) => {
                tracing::error!("❌ 从SSO获取用户信息失败: {}", e);
                tracing::warn!("🔄 自动降级为简化验证模式");
                create_session_user_from_ticket(&ticket_id.ticket_id).await
            }
        }
    };

    // 保存用户信息到会话
    tracing::info!("正在保存用户信息到会话...");
    if let Err(e) = session.insert("session_user", &session_user).await {
        tracing::error!("❌ 保存用户信息到会话失败: {}", e);
        // 会话保存失败，重新尝试SSO登录
        let sso_url = build_sso_login_url(None);
        return Redirect::to(&sso_url);
    }
    tracing::info!("✅ 用户信息已保存到会话");

    tracing::info!("🎉 用户票据验证成功！");
    tracing::info!("用户ID: {}", session_user.user_id);
    tracing::info!("用户姓名: {}", session_user.user_name.as_deref().unwrap_or("未知"));
    
    // 确定重定向URL的优先级：待访问预审记录 > 保存的返回URL > 默认主页
    let redirect_url = if let Ok(Some(pending_request_id)) = session.get::<String>("pending_request_id").await {
        tracing::info!("发现待访问预审记录: {}", pending_request_id);
        // 清除待访问记录
        if let Err(e) = session.remove::<String>("pending_request_id").await {
            tracing::warn!("清除待访问预审记录失败: {}", e);
        }
        // 重定向到静态页面而不是API接口
        format!("/static/index.html?previewId={}&verified=true", pending_request_id)
    } else if let Ok(Some(return_url)) = session.get::<String>("return_url").await {
        tracing::info!("发现保存的返回URL: {}", return_url);
        // 清除返回URL
        if let Err(e) = session.remove::<String>("return_url").await {
            tracing::warn!("清除返回URL失败: {}", e);
        }
        return_url
    } else {
        tracing::info!("无特定跳转目标，重定向到主页");
        "/static/index.html?from=verify".to_string()
    };
    
    tracing::info!("重定向URL: {}", redirect_url);
    tracing::info!("=== 用户票据验证结束 ===");
    
    Redirect::to(&redirect_url)
}

// 创建简化的SessionUser（从票据ID）
async fn create_session_user_from_ticket(ticket_id: &str) -> SessionUser {
    let now = Utc::now().to_rfc3339();
    SessionUser {
        user_id: ticket_id.to_string(),
        user_name: None,  // 简化模式下为空，等待后续请求提供
        certificate_type: "01".to_string(),  // 默认身份证
        certificate_number: None,
        phone_number: None,
        email: None,
        organization_name: None,
        organization_code: None,
        login_time: now.clone(),
        last_active: now,
    }
}

// 带重试机制的SSO用户信息获取
async fn get_user_info_from_sso_with_retry(ticket_id: &str) -> anyhow::Result<SessionUser> {
    const MAX_RETRIES: u32 = 2;
    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        tracing::info!("SSO认证尝试 {}/{}", attempt, MAX_RETRIES);

        match get_user_info_from_sso(ticket_id).await {
            Ok(user) => {
                if attempt > 1 {
                    tracing::info!("✅ SSO认证在第{}次尝试后成功", attempt);
                }
                return Ok(user);
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES {
                    tracing::warn!("⚠️ SSO认证第{}次尝试失败，等待重试...", attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                } else {
                    tracing::error!("❌ SSO认证在{}次尝试后全部失败", MAX_RETRIES);
                }
            }
        }
    }

    Err(last_error.unwrap())
}

// 从SSO获取完整用户信息（如果配置了的话）
async fn get_user_info_from_sso(ticket_id: &str) -> anyhow::Result<SessionUser> {
    use crate::CLIENT;
    
    tracing::info!("=== 开始SSO用户信息获取 ===");
    tracing::info!("票据ID: {}", ticket_id);
    tracing::info!("access_token_url: {}", CONFIG.login.access_token_url);
    tracing::info!("get_user_info_url: {}", CONFIG.login.get_user_info_url);
    
    // 第一步：使用 ticket 获取 access_token
    let token_params = serde_json::json!({
        "ticketId": ticket_id,
        "accessKey": CONFIG.login.access_key,
        "secretKey": CONFIG.login.secret_key
    });
    
    tracing::info!("正在获取 access_token...");
    let token_response = CLIENT
        .post(&CONFIG.login.access_token_url)
        .json(&token_params)
        .timeout(std::time::Duration::from_secs(30)) // 30秒超时
        .send()
        .await?;
    
    let token_status = token_response.status();
    let token_text = token_response.text().await?;
    
    tracing::info!("Token API 响应状态: {}", token_status);
    tracing::info!("Token API 响应内容: {}", token_text);
    
    if !token_status.is_success() {
        anyhow::bail!("获取access_token失败: {} - {}", token_status, token_text);
    }
    
    let token_result: serde_json::Value = serde_json::from_str(&token_text)?;
    
    // 从响应中提取 access_token
    let access_token = token_result
        .get("data")
        .and_then(|d| d.get("accessToken"))
        .or_else(|| token_result.get("accessToken"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("响应中未找到 accessToken"))?;
    
    tracing::info!("✅ 成功获取 access_token: {}...", &access_token[..std::cmp::min(10, access_token.len())]);
    
    // 第二步：使用 access_token 获取用户信息
    let user_params = serde_json::json!({
        "accessToken": access_token,
        "accessKey": CONFIG.login.access_key,
        "secretKey": CONFIG.login.secret_key
    });
    
    tracing::info!("正在获取用户信息...");
    let user_response = CLIENT
        .post(&CONFIG.login.get_user_info_url)
        .json(&user_params)
        .timeout(std::time::Duration::from_secs(30)) // 30秒超时
        .send()
        .await?;
    
    let user_status = user_response.status();
    let user_text = user_response.text().await?;
    
    tracing::info!("UserInfo API 响应状态: {}", user_status);
    tracing::info!("UserInfo API 响应内容: {}", user_text);
    
    if !user_status.is_success() {
        anyhow::bail!("获取用户信息失败: {} - {}", user_status, user_text);
    }
    
    let user_result: serde_json::Value = serde_json::from_str(&user_text)?;
    
    // 从响应中提取用户信息
    let user_data = user_result
        .get("data")
        .ok_or_else(|| anyhow::anyhow!("响应中未找到 data 字段"))?;
    
    // 构建 SessionUser 对象
    let now = Utc::now().to_rfc3339();
    let session_user = SessionUser {
        user_id: user_data.get("userId")
            .or_else(|| user_data.get("userCode"))
            .or_else(|| user_data.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or(ticket_id)
            .to_string(),
        user_name: user_data.get("userName")
            .or_else(|| user_data.get("name"))
            .or_else(|| user_data.get("realName"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        certificate_type: user_data.get("certificateType")
            .or_else(|| user_data.get("idType"))
            .and_then(|v| v.as_str())
            .unwrap_or("01")  // 默认身份证
            .to_string(),
        certificate_number: user_data.get("certificateNumber")
            .or_else(|| user_data.get("idNumber"))
            .or_else(|| user_data.get("cardNo"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        phone_number: user_data.get("phoneNumber")
            .or_else(|| user_data.get("mobile"))
            .or_else(|| user_data.get("phone"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        email: user_data.get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        organization_name: user_data.get("organizationName")
            .or_else(|| user_data.get("orgName"))
            .or_else(|| user_data.get("company"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        organization_code: user_data.get("organizationCode")
            .or_else(|| user_data.get("orgCode"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        login_time: now.clone(),
        last_active: now,
    };
    
    tracing::info!("✅ SSO用户信息获取成功！");
    tracing::info!("用户ID: {}", session_user.user_id);
    tracing::info!("用户姓名: {}", session_user.user_name.as_deref().unwrap_or("未提供"));
    tracing::info!("证件类型: {}", session_user.certificate_type);
    tracing::info!("=== SSO用户信息获取完成 ===");
    
    Ok(session_user)
}

// 认证状态检查
async fn auth_status(session: Session) -> impl IntoResponse {
    // 检查会话中是否有用户信息
    match session.get::<SessionUser>("session_user").await {
        Ok(Some(session_user)) => {
            tracing::info!("用户认证状态检查: {} ({})", 
                          session_user.user_id, 
                          session_user.user_name.as_deref().unwrap_or("未知用户"));
            
            // 用户已登录，返回完整信息
            Json(serde_json::json!({
                "authenticated": true,
                "user": {
                    "userId": session_user.user_id,
                    "userName": session_user.user_name,
                    "certificateType": session_user.certificate_type,
                    "certificateNumber": session_user.certificate_number,
                    "phoneNumber": session_user.phone_number,
                    "email": session_user.email,
                    "organizationName": session_user.organization_name,
                    "organizationCode": session_user.organization_code,
                    "loginTime": session_user.login_time,
                    "lastActive": session_user.last_active
                }
            }))
        }
        _ => {
            tracing::info!("用户未认证或会话已过期");
            // 用户未登录
            Json(serde_json::json!({
                "authenticated": false,
                "error": "用户未登录或会话已过期",
                "redirect": "/static/login.html"
            }))
        }
    }
}

// 用户登出
async fn auth_logout(session: Session) -> impl IntoResponse {
    tracing::info!("用户登出请求");

    // 获取当前用户信息（用于日志记录）
    if let Ok(Some(session_user)) = session.get::<SessionUser>("session_user").await {
        tracing::info!("用户 {} ({}) 正在登出",
                      session_user.user_id,
                      session_user.user_name.as_deref().unwrap_or("未知用户"));
    }

    // 清除会话 - session.clear() 返回 ()，不是 Result
    session.clear().await;
    tracing::info!("✅ 用户会话已清除");

    // 返回成功响应
    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "message": "登出成功"
        }
    }))
}

async fn update_rule(multipart: Multipart) -> impl IntoResponse {
    let result = crate::util::zen::update_rule(multipart).await;
    result.into_json()
}

// 获取所有可用主题
async fn get_themes() -> impl IntoResponse {
    tracing::info!("获取所有可用主题");
    let themes = crate::util::zen::get_available_themes();
    tracing::info!("可用主题数量: {}", themes.len());
    for theme in &themes {
        tracing::info!("  - {}: {} ({})", theme.id, theme.name, theme.description);
    }

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "themes": themes,
            "total": themes.len()
        }
    }))
}

// 重新加载指定主题的规则
async fn reload_theme(axum::extract::Path(theme_id): axum::extract::Path<String>) -> impl IntoResponse {
    tracing::info!("重新加载主题规则: {}", theme_id);
    let result = crate::util::zen::reload_theme_rule(&theme_id).await;
    result.into_json()
}

// 获取前端配置
async fn get_frontend_config() -> impl IntoResponse {
    tracing::info!("获取前端配置");

    // 获取主题配置
    let themes = crate::util::zen::get_available_themes();

    // 测试模式已移除，确保生产环境安全

    // 构建前端需要的配置数据
    let frontend_config = serde_json::json!({
        "themes": themes,
        "features": {
            "theme_selection": true,
            "ocr_preview": true,
            "pdf_download": true,
            "elder_mode": true
        },
        "ui": {
            "title": "材料智能预审",
            "upload_hint": "支持PDF、JPG、PNG、BMP等格式",
            "max_file_size": "10MB"
        },
        "api": {
            "base_url": "/api",
            "timeout": 30000
        },
        "test_mode": if let Some(test_config) = &CONFIG.test_mode {
            serde_json::json!({
                "enabled": test_config.enabled,
                "auto_login": test_config.auto_login,
                "mock_ocr": test_config.mock_ocr,
                "test_user": {
                    "id": test_config.test_user.id,
                    "username": test_config.test_user.username,
                    "email": test_config.test_user.email,
                    "role": test_config.test_user.role
                }
            })
        } else {
            serde_json::json!({
                "enabled": false,
                "auto_login": false,
                "mock_ocr": false,
                "test_user": null
            })
        },
        "debug": {
            "enabled": CONFIG.debug.enabled,
            "mock_login": CONFIG.debug.enable_mock_login
        }
    });

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": frontend_config
    }))
}

// 基本健康检查
async fn basic_health_check() -> impl IntoResponse {
    let status = HealthStatus {
        status: "healthy".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime: system_info::get_uptime_seconds(),
        timestamp: Utc::now().to_rfc3339(),
    };

    Json(status)
}

// 详细健康检查
async fn detailed_health_check() -> impl IntoResponse {
    // 使用 tokio::time::timeout 添加超时
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        collect_detailed_health_info()
    ).await {
        Ok(status) => Json(status),
        Err(_) => {
            let error_status = DetailedHealthStatus {
                status: "error".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                uptime: system_info::get_uptime_seconds(),
                timestamp: Utc::now().to_rfc3339(),
                memory: system_info::get_memory_usage(),
                cpu: system_info::get_cpu_usage(),
                disk: system_info::get_disk_usage(),
                queue: crate::model::QueueStatus {
                    pending: 0,
                    processing: 0,
                    completed_last_hour: 0,
                    failed_last_hour: 0,
                },
                last_error: Some(crate::model::ErrorInfo {
                    timestamp: Utc::now().to_rfc3339(),
                    message: "Health check timed out".to_string(),
                }),
            };
            Json(error_status)
        }
    }
}

// 收集详细健康信息
async fn collect_detailed_health_info() -> DetailedHealthStatus {
    let memory = system_info::get_memory_usage();
    let cpu = system_info::get_cpu_usage();
    let disk = system_info::get_disk_usage();
    let queue = system_info::get_queue_status().await;

    // 确定服务状态
    let status = if cpu.usage_percent > 90.0 || memory.usage_percent > 90.0 {
        "degraded"
    } else {
        "healthy"
    };

    DetailedHealthStatus {
        status: status.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime: system_info::get_uptime_seconds(),
        timestamp: Utc::now().to_rfc3339(),
        memory,
        cpu,
        disk,
        queue,
        last_error: None,
    }
}

// 组件健康检查
async fn components_health_check() -> impl IntoResponse {
    let db_connection = system_info::check_database_connection().await;

    let components = vec![
        ComponentStatus {
            name: "database".to_string(),
            status: if db_connection { "healthy" } else { "unhealthy" }.to_string(),
            details: Some(if db_connection { "Connection successful" } else { "Connection failed" }.to_string()),
            response_time_ms: None,
        },
        ComponentStatus {
            name: "file_system".to_string(),
            status: "healthy".to_string(),
            details: Some("Read/Write operations normal".to_string()),
            response_time_ms: None,
        },
        // 可以添加更多组件状态
    ];

    Json(ComponentsHealth { components })
}

// 获取日志统计信息
async fn get_log_stats() -> impl IntoResponse {
    use crate::util::log::get_log_stats;
    
    let log_dir = std::path::Path::new(&CONFIG.logging.file.directory);
    match get_log_stats(log_dir) {
        Ok(stats) => {
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": stats
            }))
        }
        Err(e) => {
            tracing::error!("获取日志统计信息失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("获取日志统计信息失败: {}", e),
                "data": null
            }))
        }
    }
}

// 手动清理日志
async fn cleanup_logs() -> impl IntoResponse {
    use crate::util::log::cleanup_old_logs;
    
    let retention_days = CONFIG.logging.file.retention_days.unwrap_or(7);
    let log_dir = std::path::Path::new(&CONFIG.logging.file.directory);
    
    match cleanup_old_logs(log_dir, retention_days) {
        Ok(_) => {
            tracing::info!("手动清理日志完成");
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "message": "日志清理完成",
                    "retention_days": retention_days
                }
            }))
        }
        Err(e) => {
            tracing::error!("手动清理日志失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("日志清理失败: {}", e),
                "data": null
            }))
        }
    }
}

// 检查日志系统健康状态
async fn check_log_health() -> impl IntoResponse {
    use crate::util::log::check_log_health;
    
    let log_dir = std::path::Path::new(&CONFIG.logging.file.directory);
    match check_log_health(log_dir) {
        Ok(health) => {
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": health
            }))
        }
        Err(e) => {
            tracing::error!("检查日志健康状态失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("检查日志健康状态失败: {}", e),
                "data": null
            }))
        }
    }
}

// 构建第三方SSO登录URL的辅助函数
fn build_sso_login_url(pending_request_id: Option<&str>) -> String {
    let base_sso_url = &CONFIG.login.sso_login_url;
    let app_id = &CONFIG.app_id;
    
    if CONFIG.login.use_callback {
        // 回调模式：添加回调参数
        let callback_url = &CONFIG.callback_url;
        if let Some(request_id) = pending_request_id {
            // 将待访问的预审ID通过状态参数传递
            format!("{}?appId={}&redirectUri={}&state={}", base_sso_url, app_id, 
                   urlencoding::encode(callback_url), request_id)
        } else {
            format!("{}?appId={}&redirectUri={}", base_sso_url, app_id, 
                   urlencoding::encode(callback_url))
        }
    } else {
        // 直跳模式：不添加任何参数，直接使用基础URL
        // appId只在回调模式下需要，用于回调验证
        base_sso_url.to_string()
    }
}

// 安全的预审页面访问接口
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
        let sso_url = build_sso_login_url(None);
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
                    if let Ok(Some(mapping)) = get_id_mapping_from_database(&app_state.database, &request_id).await {
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
                    let sso_url = build_sso_login_url(Some(&request_id));
                    tracing::info!("身份不匹配，直接跳转到第三方SSO登录: {}", sso_url);
                    Redirect::to(&sso_url)
                }
            } else {
                tracing::error!("❌ 验证预审访问权限时出错");
                let sso_url = build_sso_login_url(Some(&request_id));
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
            let sso_url = build_sso_login_url(Some(&request_id));
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
    match get_id_mapping_from_database(database, preview_id).await? {
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
    let mapping = match get_id_mapping_from_database(database, preview_id).await? {
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
    let mapping = match get_id_mapping_from_database(database, preview_id).await? {
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

// 通知第三方系统预审结果
async fn notify_third_party_system(
    third_party_request_id: &str, 
    status: &str, 
    result: Option<&crate::util::WebResult>
) -> anyhow::Result<()> {
    tracing::info!("=== 准备通知第三方系统 ===");
    tracing::info!("第三方请求ID: {}", third_party_request_id);
    tracing::info!("预审状态: {}", status);
    
    // 检查是否配置了回调URL
    let callback_url = &CONFIG.callback_url;
    if callback_url.is_empty() {
        tracing::info!("⚠️  未配置第三方回调URL，跳过结果推送");
        return Ok(());
    }
    
    tracing::info!("回调URL: {}", callback_url);
    
    // 构建回调数据
    let mut callback_data = serde_json::json!({
        "previewId": third_party_request_id, // 使用第三方requestId作为previewId
        "thirdPartyRequestId": third_party_request_id,
        "status": status,
        "timestamp": Utc::now().to_rfc3339(),
        "callbackType": "preview_result"
    });
    
    // 根据状态添加不同的数据
    match status {
        "completed" => {
            if let Some(web_result) = result {
                callback_data["result"] = serde_json::json!({
                    "success": web_result.success,
                    "data": web_result.data,
                    "message": "预审完成"
                });
                
                // 添加文件下载URL（如果需要）
                let view_url = format!("{}/api/preview/view/{}", CONFIG.host, third_party_request_id);
                callback_data["viewUrl"] = serde_json::json!(view_url);
            }
        }
        "failed" => {
            callback_data["result"] = serde_json::json!({
                "success": false,
                "message": "预审处理失败"
            });
        }
        _ => {}
    }
    
    // 发送回调请求
    match send_callback_request(callback_url, &callback_data).await {
        Ok(_) => {
            tracing::info!("✅ 第三方系统回调成功");
        }
        Err(e) => {
            tracing::error!("❌ 第三方系统回调失败: {}", e);
            // 这里可以考虑重试机制
        }
    }
    
    tracing::info!("=== 第三方系统通知结束 ===");
    Ok(())
}

// 发送回调请求
async fn send_callback_request(callback_url: &str, data: &serde_json::Value) -> anyhow::Result<()> {
    tracing::info!("发送回调请求到: {}", callback_url);
    
    // 创建HTTP客户端
    let client = reqwest::Client::new();
    
    // 发送POST请求
    let response = client
        .post(callback_url)
        .header("Content-Type", "application/json")
        .header("User-Agent", "OCR-Preview-Service/1.0")
        .json(data)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await?;
    
    let status_code = response.status();
    let response_text = response.text().await?;
    
    if status_code.is_success() {
        tracing::info!("回调请求成功: {} - {}", status_code, response_text);
    } else {
        tracing::warn!("回调请求失败: {} - {}", status_code, response_text);
        return Err(anyhow::anyhow!("回调请求失败: {}", status_code));
    }
    
    Ok(())
}


// SSO登录跳转端点
async fn sso_login_redirect(
    session: Session,
    Query(params): Query<std::collections::HashMap<String, String>>
) -> impl IntoResponse {
    tracing::info!("=== SSO登录跳转请求 ===");

    // 获取可选的返回URL参数
    let return_url = params.get("return_url");
    let pending_request_id = params.get("request_id");

    tracing::info!("返回URL: {:?}", return_url);
    tracing::info!("待访问预审ID: {:?}", pending_request_id);

    // 如果有待访问的预审ID，保存到会话中
    if let Some(request_id) = pending_request_id {
        if let Err(e) = session.insert("pending_request_id", request_id).await {
            tracing::warn!("保存待访问预审记录ID失败: {}", e);
        } else {
            tracing::info!("已保存待访问预审记录ID: {}", request_id);
        }
    }

    // 如果有返回URL，保存到会话中
    if let Some(url) = return_url {
        if let Err(e) = session.insert("return_url", url).await {
            tracing::warn!("保存返回URL失败: {}", e);
        } else {
            tracing::info!("已保存返回URL: {}", url);
        }
    }

    // 构建SSO登录URL
    let sso_url = build_sso_login_url(pending_request_id.map(|s| s.as_str()));

    tracing::info!("构建的SSO登录URL: {}", sso_url);
    tracing::info!("=== SSO登录跳转执行 ===");

    // 重定向到第三方SSO登录
    Redirect::to(&sso_url)
}

// 根路由重定向到登录页面
async fn root_redirect() -> impl IntoResponse {
    tracing::info!("根路由访问，重定向到登录页面");
    Redirect::to("/static/login.html")
}

// 获取Debug配置
async fn get_debug_config() -> impl IntoResponse {
    tracing::info!("获取Debug配置");

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "Debug配置获取成功",
        "data": {
            "enabled": CONFIG.debug.enabled,
            "enable_mock_login": CONFIG.debug.enable_mock_login,
            "mock_login_warning": CONFIG.debug.mock_login_warning,
            "tools": {
                "api_test": CONFIG.debug.tools_enabled.api_test,
                "mock_login": CONFIG.debug.tools_enabled.mock_login,
                "preview_demo": CONFIG.debug.tools_enabled.preview_demo,
                "flow_test": CONFIG.debug.tools_enabled.flow_test,
                "system_monitor": CONFIG.debug.tools_enabled.system_monitor,
                "data_manager": CONFIG.debug.tools_enabled.data_manager
            }
        }
    }))
}

// 预审提交接口 - 简化版本
async fn preview_submit(Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    tracing::info!("收到预审提交请求: {:?}", payload);

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "预审任务已提交",
        "data": {
            "message": "预审任务已提交，后台正在处理中",
            "previewId": format!("PREVIEW_{}", chrono::Utc::now().timestamp()),
            "status": "submitted"
        }
    }))
}





// 根据材料名称和状态获取对应的图片路径
fn get_material_image_path(material_name: &str, status: &str) -> String {
    let base_path = "/static/images/";

    // 根据状态优先选择
    match status {
        "approved" | "passed" => {
            if material_name.contains("章程") {
                format!("{}智能预审_已通过材料1.3.png", base_path)
            } else {
                format!("{}预审通过1.3.png", base_path)
            }
        }
        "rejected" | "failed" => {
            format!("{}智能预审异常提示1.3.png", base_path)
        }
        "pending" | "reviewing" => {
            if material_name.contains("合同") || material_name.contains("协议") {
                format!("{}智能预审_有审查点1.3.png", base_path)
            } else if material_name.contains("申请") || material_name.contains("登记") {
                format!("{}智能预审_审核依据材料1.3.png", base_path)
            } else {
                format!("{}智能预审_审核依据材料1.3.png", base_path)
            }
        }
        "no_reference" => {
            format!("{}智能预审_无审核依据材料1.3.png", base_path)
        }
        _ => {
            format!("{}智能预审_审核依据材料1.3.png", base_path)
        }
    }
}

// 获取预审结果详情（用于政务风格展示页面）
async fn get_preview_result(
    axum::extract::Path(preview_id): axum::extract::Path<String>,
    State(state): State<AppState>
) -> impl IntoResponse {
    tracing::info!("获取预审结果详情: {}", preview_id);

    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            // 解析评估结果（如果存在）
            let evaluation_data = if let Some(eval_result) = &preview.evaluation_result {
                serde_json::from_str::<serde_json::Value>(eval_result).unwrap_or_default()
            } else {
                serde_json::json!({})
            };

            // 构建政务风格的预审结果数据
            let result_data = serde_json::json!({
                "preview_id": preview_id,
                "applicant": evaluation_data.get("applicant").and_then(|v| v.as_str()).unwrap_or("申请人"),
                "applicant_name": evaluation_data.get("applicant").and_then(|v| v.as_str()).unwrap_or("申请人"),
                "matter_name": evaluation_data.get("matter_name").and_then(|v| v.as_str()).unwrap_or(&preview.file_name),
                "theme_name": crate::util::zen::get_theme_name(preview.theme_id.as_deref()).unwrap_or_else(|| "未知主题".to_string()),
                "status": preview.status,
                "created_at": preview.created_at,
                "materials": evaluation_data.get("materials").and_then(|v| v.as_array()).map(|materials| {
                    materials.iter().map(|material| {
                        let material_name = material.get("name").and_then(|v| v.as_str()).unwrap_or("未知材料");
                        let material_status = material.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
                        let image_path = get_material_image_path(material_name, material_status);

                        serde_json::json!({
                            "id": material.get("id").and_then(|v| v.as_u64()).unwrap_or(1),
                            "name": material_name,
                            "status": material_status,
                            "pages": material.get("pages").and_then(|v| v.as_u64()).unwrap_or(1),
                            "count": material.get("pages").and_then(|v| v.as_u64()).unwrap_or(1),
                            "image": image_path,
                            "preview_url": image_path,
                            "review_points": material.get("review_points").cloned().unwrap_or_default(),
                            "review_notes": material.get("review_notes").and_then(|v| v.as_str())
                        })
                    }).collect::<Vec<_>>()
                }).unwrap_or_else(|| {
                    // 如果没有材料数据，创建一个默认的材料项
                    vec![serde_json::json!({
                        "id": 1,
                        "name": preview.file_name,
                        "status": "pending",
                        "pages": 1,
                        "count": 1,
                        "image": get_material_image_path(&preview.file_name, "pending"),
                        "preview_url": get_material_image_path(&preview.file_name, "pending"),
                        "review_points": [],
                        "review_notes": null
                    })]
                })
            });

            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": result_data
            }))
        }
        Ok(None) => {
            Json(serde_json::json!({
                "success": false,
                "errorCode": 404,
                "errorMsg": "预审记录不存在",
                "data": null
            }))
        }
        Err(e) => {
            tracing::error!("获取预审结果失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取预审结果失败",
                "data": null
            }))
        }
    }
}

// 下载预审报告
async fn download_preview_report(
    axum::extract::Path(preview_id): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    State(state): State<AppState>
) -> impl IntoResponse {
    tracing::info!("下载预审报告: {}, 格式: {:?}", preview_id, params.get("format"));

    let format = params.get("format").unwrap_or(&"pdf".to_string()).clone();

    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            match format.as_str() {
                "pdf" => {
                    // PDF生成暂时不支持，返回HTML格式
                    tracing::warn!("PDF生成功能暂未实现，返回HTML格式");

                    // 从评估结果中提取材料名称，如果没有则使用文件名
                    let material_names = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("materials")
                                .and_then(|v| v.as_array())
                                .map(|materials| {
                                    materials.iter()
                                        .filter_map(|m| m.get("name").and_then(|v| v.as_str()))
                                        .map(|s| s.to_string())
                                        .collect()
                                })
                                .unwrap_or_else(|| vec![preview.file_name.clone()])
                        } else {
                            vec![preview.file_name.clone()]
                        }
                    } else {
                        vec![preview.file_name.clone()]
                    };

                    let matter_name = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("matter_name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| preview.file_name.clone())
                        } else {
                            preview.file_name.clone()
                        }
                    } else {
                        preview.file_name.clone()
                    };

                    let html_content = crate::util::report_generator::PreviewReportGenerator::generate_simple_html(
                        &matter_name,
                        &preview_id,
                        &material_names
                    );

                    let headers = [
                        ("Content-Type", "text/html; charset=utf-8"),
                        ("Content-Disposition", &format!("attachment; filename=\"预审报告_{}.html\"", preview_id)),
                    ];
                    (headers, html_content).into_response()
                }
                "html" => {
                    // 生成简化HTML报告
                    let material_names = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("materials")
                                .and_then(|v| v.as_array())
                                .map(|materials| {
                                    materials.iter()
                                        .filter_map(|m| m.get("name").and_then(|v| v.as_str()))
                                        .map(|s| s.to_string())
                                        .collect()
                                })
                                .unwrap_or_else(|| vec![preview.file_name.clone()])
                        } else {
                            vec![preview.file_name.clone()]
                        }
                    } else {
                        vec![preview.file_name.clone()]
                    };

                    let matter_name = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("matter_name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| preview.file_name.clone())
                        } else {
                            preview.file_name.clone()
                        }
                    } else {
                        preview.file_name.clone()
                    };

                    let html_content = crate::util::report_generator::PreviewReportGenerator::generate_simple_html(
                        &matter_name,
                        &preview_id,
                        &material_names
                    );

                    let headers = [
                        ("Content-Type", "text/html; charset=utf-8"),
                        ("Content-Disposition", &format!("attachment; filename=\"预审报告_{}.html\"", preview_id)),
                    ];
                    (headers, html_content).into_response()
                }
                _ => {
                    (StatusCode::BAD_REQUEST, "不支持的格式").into_response()
                }
            }
        }
        Ok(None) => {
            (StatusCode::NOT_FOUND, "预审记录不存在").into_response()
        }
        Err(e) => {
            tracing::error!("获取预审记录失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "获取预审记录失败").into_response()
        }
    }
}

// 获取预审统计数据 - 简化版本
async fn get_preview_stats(
    State(app_state): State<AppState>,
    Query(params): Query<std::collections::HashMap<String, String>>
) -> impl IntoResponse {
    tracing::info!("获取预审统计数据");
    
    // 解析查询参数
    let limit = params.get("limit")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(100); // 默认返回最近100条记录
    
    let offset = params.get("offset")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    
    // 构建查询过滤条件
    let filter = crate::db::PreviewFilter {
        user_id: None, // 不过滤用户，显示所有记录
        status: None,  // 不过滤状态
        theme_id: None, // 不过滤主题
        start_date: None, // 不过滤开始时间
        end_date: None,   // 不过滤结束时间
        limit: Some(limit),
        offset: Some(offset),
    };
    
    // 从数据库获取预审记录
    match app_state.database.list_preview_records(&filter).await {
        Ok(records) => {
            // 构建简化的统计数据：只包含ID、事项名称、时间
            let stats_data: Vec<serde_json::Value> = records.iter().map(|record| {
                // 尝试从评估结果中提取事项名称
                let matter_name = if let Some(eval_result) = &record.evaluation_result {
                    if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                        eval_data.get("matter_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or(&record.file_name)
                            .to_string()
                    } else {
                        record.file_name.clone()
                    }
                } else {
                    // 如果没有评估结果，使用文件名作为事项名称
                    record.file_name.clone()
                };
                
                serde_json::json!({
                    "id": record.id,
                    "matter_name": matter_name,
                    "created_at": record.created_at.to_rfc3339(),
                    "status": record.status.to_string(),
                    "user_id": record.user_id
                })
            }).collect();
            
            tracing::info!("✅ 成功获取 {} 条预审统计记录", stats_data.len());
            
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "records": stats_data,
                    "total": stats_data.len(),
                    "limit": limit,
                    "offset": offset
                }
            }))
        }
        Err(e) => {
            tracing::error!("❌ 获取预审统计数据失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取预审统计数据失败",
                "data": null
            }))
        }
    }
}

// 测试模式模拟登录 - 仅在配置启用时可用
async fn mock_login_for_test(
    session: Session,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let config = &CONFIG;

    // 检查是否启用测试模式
    let test_mode_enabled = config.test_mode.as_ref()
        .map(|tm| tm.enabled && tm.auto_login)
        .unwrap_or(false);

    if !test_mode_enabled {
        return Ok(Json(serde_json::json!({
            "success": false,
            "msg": "测试模式未启用",
            "error": "TEST_MODE_DISABLED"
        })));
    }

    // 从配置或请求中获取用户信息
    let (user_id, user_name) = if let Some(test_config) = &config.test_mode {
        (
            test_config.test_user.id.clone(),
            test_config.test_user.username.clone()
        )
    } else {
        // 从请求中获取
        let user_id = payload.get("userId")
            .and_then(|v| v.as_str())
            .unwrap_or("test_user_001")
            .to_string();
        let user_name = payload.get("userName")
            .and_then(|v| v.as_str())
            .unwrap_or("测试用户")
            .to_string();
        (user_id, user_name)
    };

    // 创建会话用户
    let session_user = crate::model::SessionUser {
        user_id: user_id.clone(),
        user_name: Some(user_name.clone()),
        certificate_type: "ID_CARD".to_string(),
        certificate_number: Some("test_cert_001".to_string()),
        email: Some("test@example.com".to_string()),
        phone_number: Some("13800000000".to_string()),
        organization_name: None,
        organization_code: None,
        login_time: chrono::Utc::now().to_string(),
        last_active: chrono::Utc::now().to_string(),
    };

    // 保存到会话
    if let Err(e) = session.insert("session_user", &session_user).await {
        tracing::error!("保存测试用户会话失败: {}", e);
        return Ok(Json(serde_json::json!({
            "success": false,
            "msg": "保存会话失败",
            "error": "SESSION_SAVE_FAILED"
        })));
    }

    tracing::info!("🧪 测试模式模拟登录成功: user_id={}", user_id);

    Ok(Json(serde_json::json!({
        "success": true,
        "msg": "模拟登录成功",
        "data": {
            "userId": user_id,
            "userName": user_name,
            "loginTime": chrono::Utc::now().to_string()
        }
    })))
}

// 获取测试模拟数据
async fn get_mock_test_data(
    Json(request): Json<serde_json::Value>
) -> Result<Json<serde_json::Value>, StatusCode> {
    tracing::info!("获取测试模拟数据请求: {:?}", request);
    
    let data_type = request.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("preview");
        
    match data_type {
        "preview" => {
            // 返回预审测试数据
            Ok(Json(serde_json::json!({
                "success": true,
                "data": {
                    "userId": "test_user_001",
                    "preview": {
                        "matterId": "MATTER_001",
                        "matterName": "测试事项",
                        "matterType": "test",
                        "requestId": format!("TEST_{}", chrono::Utc::now().timestamp()),
                        "sequenceNo": format!("SEQ_{}", chrono::Utc::now().timestamp()),
                        "copy": false,
                        "channel": "web",
                        "formData": [],
                        "materialData": [],
                        "agentInfo": {
                            "userId": "test_user_001",
                            "userName": "测试用户"
                        },
                        "subjectInfo": {
                            "name": "测试主体",
                            "type": "individual"
                        }
                    }
                }
            })))
        }
        "qingqiu" => {
            // 返回qingqiu.json格式的测试数据
            Ok(Json(serde_json::json!({
                "success": true,
                "data": {
                    "requestId": format!("QINGQIU_{}", chrono::Utc::now().timestamp()),
                    "sequenceNo": format!("SEQ_{}", chrono::Utc::now().timestamp()),
                    "userId": "test_user_001",
                    "materials": []
                }
            })))
        }
        _ => {
            Ok(Json(serde_json::json!({
                "success": false,
                "error": "未知的数据类型"
            })))
        }
    }
}

// 获取预审统计数据
async fn get_preview_statistics(
    State(app_state): State<AppState>
) -> impl IntoResponse {
    tracing::info!("=== 获取预审统计数据 ===");
    
    // 获取各状态的统计数据
    let statistics = match calculate_preview_statistics(&app_state.database).await {
        Ok(stats) => stats,
        Err(e) => {
            tracing::error!("获取预审统计失败: {}", e);
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取统计数据失败",
                "data": null
            }));
        }
    };
    
    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": statistics
    }))
}

// 获取预审记录列表
async fn get_preview_records_list(
    State(app_state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>
) -> impl IntoResponse {
    tracing::info!("=== 获取预审记录列表 ===");
    tracing::info!("查询参数: {:?}", params);
    
    // 解析查询参数
    let page = params.get("page")
        .and_then(|p| p.parse::<u32>().ok())
        .unwrap_or(1);
    let size = params.get("size")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(20)
        .min(100); // 限制最大页面大小
    
    // 构建过滤条件
    let mut filter = crate::db::PreviewFilter {
        user_id: None,
        status: None,
        theme_id: None,
        start_date: None,
        end_date: None,
        limit: None,
        offset: None,
    };
    
    // 状态过滤
    if let Some(status_str) = params.get("status") {
        if !status_str.is_empty() {
            filter.status = match status_str.as_str() {
                "pending" => Some(crate::db::PreviewStatus::Pending),
                "processing" => Some(crate::db::PreviewStatus::Processing),
                "completed" => Some(crate::db::PreviewStatus::Completed),
                "failed" => Some(crate::db::PreviewStatus::Failed),
                _ => None,
            };
        }
    }
    
    // 日期过滤
    if let Some(date_from) = params.get("date_from") {
        if !date_from.is_empty() {
            if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_from, "%Y-%m-%d") {
                filter.start_date = Some(dt.and_hms_opt(0, 0, 0).unwrap().and_utc());
            }
        }
    }
    
    if let Some(date_to) = params.get("date_to") {
        if !date_to.is_empty() {
            if let Ok(dt) = chrono::NaiveDate::parse_from_str(date_to, "%Y-%m-%d") {
                filter.end_date = Some(dt.and_hms_opt(23, 59, 59).unwrap().and_utc());
            }
        }
    }
    
    // 设置分页参数
    filter.limit = Some(size);
    filter.offset = Some((page - 1) * size);
    
    // 首先获取总数（不带分页的查询）
    let total_filter = crate::db::PreviewFilter {
        user_id: filter.user_id.clone(),
        status: filter.status.clone(),
        theme_id: filter.theme_id.clone(),
        start_date: filter.start_date,
        end_date: filter.end_date,
        limit: None,
        offset: None,
    };
    
    // 查询数据
    match app_state.database.list_preview_records(&filter).await {
        Ok(records) => {
            // 获取总数用于分页计算
            let total = match app_state.database.list_preview_records(&total_filter).await {
                Ok(all_records) => all_records.len() as u32,
                Err(_) => records.len() as u32, // 降级处理
            };
            
            let total_pages = (total + size - 1) / size;
            
            // 增强记录信息
            let enhanced_records: Vec<_> = records
                .into_iter()
                .map(|record| enhance_preview_record(record))
                .collect();
            
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "records": enhanced_records,
                    "pagination": {
                        "current_page": page,
                        "page_size": size,
                        "total_records": total,
                        "total_pages": total_pages
                    }
                }
            }))
        }
        Err(e) => {
            tracing::error!("查询预审记录失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "查询记录失败",
                "data": null
            }))
        }
    }
}

// 计算预审统计数据
async fn calculate_preview_statistics(database: &Arc<dyn crate::db::Database>) -> anyhow::Result<serde_json::Value> {
    use crate::db::{PreviewFilter, PreviewStatus};
    
    // 查询所有记录
    let all_records = database.list_preview_records(&PreviewFilter {
        user_id: None,
        status: None,
        theme_id: None,
        start_date: None,
        end_date: None,
        limit: None,
        offset: None,
    }).await?;
    
    let total = all_records.len();
    let completed = all_records.iter().filter(|r| r.status == PreviewStatus::Completed).count();
    let processing = all_records.iter().filter(|r| r.status == PreviewStatus::Processing).count();
    let failed = all_records.iter().filter(|r| r.status == PreviewStatus::Failed).count();
    let pending = all_records.iter().filter(|r| r.status == PreviewStatus::Pending).count();
    
    Ok(serde_json::json!({
        "total": total,
        "completed": completed,
        "processing": processing,
        "failed": failed,
        "pending": pending,
        "success_rate": if total > 0 { (completed as f64 / total as f64 * 100.0).round() } else { 0.0 }
    }))
}

// 增强预审记录信息（从evaluation_result中提取matter_name和matter_id）
fn enhance_preview_record(record: crate::db::PreviewRecord) -> serde_json::Value {
    let mut result = serde_json::json!({
        "id": record.id,
        "user_id": record.user_id,
        "file_name": record.file_name,
        "third_party_request_id": record.third_party_request_id,
        "status": format!("{:?}", record.status).to_lowercase(),
        "created_at": record.created_at.to_rfc3339(),
        "updated_at": record.updated_at.to_rfc3339(),
        "preview_url": record.preview_url,
        "matter_name": None::<String>,
        "matter_id": None::<String>
    });
    
    // 尝试从evaluation_result中提取matter信息
    if let Some(eval_result) = &record.evaluation_result {
        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
            if let Some(matter_name) = eval_data.get("matter_name").and_then(|v| v.as_str()) {
                result["matter_name"] = serde_json::Value::String(matter_name.to_string());
            }
            if let Some(matter_id) = eval_data.get("matter_id").and_then(|v| v.as_str()) {
                result["matter_id"] = serde_json::Value::String(matter_id.to_string());
            }
        }
    }
    
    result
}

/// 获取系统队列状态 - 并发控制监控
/// 提供OCR处理队列的实时状态信息
async fn get_queue_status() -> impl IntoResponse {
    tracing::info!("=== 获取系统队列状态 ===");
    
    // 获取当前信号量状态
    let available_permits = crate::OCR_SEMAPHORE.available_permits();
    let max_concurrent = 12; // 与main.rs中的设置保持一致
    let processing_tasks = max_concurrent - available_permits;
    
    // 计算系统负载百分比
    let system_load_percent = if max_concurrent > 0 {
        (processing_tasks as f64 / max_concurrent as f64 * 100.0).round()
    } else {
        0.0
    };
    
    let queue_status = serde_json::json!({
        "success": true,
        "data": {
            "queue": {
                "max_concurrent_tasks": max_concurrent,
                "available_slots": available_permits,
                "processing_tasks": processing_tasks,
                "system_load_percent": system_load_percent
            },
            "system_info": {
                "cpu_cores": 32,
                "memory_gb": 64,
                "optimized_for": "32核64G服务器",
                "concurrency_strategy": "信号量控制"
            },
            "performance": {
                "current_capacity": format!("{}/{} 处理槽位", processing_tasks, max_concurrent),
                "status": if system_load_percent < 70.0 { "正常" } 
                         else if system_load_percent < 90.0 { "繁忙" } 
                         else { "过载" },
                "recommended_action": if system_load_percent < 70.0 { "系统运行正常" }
                                    else if system_load_percent < 90.0 { "建议稍后提交任务" }
                                    else { "建议等待当前任务完成后再提交" }
            }
        }
    });
    
    tracing::info!("队列状态: 处理中任务={}, 可用槽位={}, 负载={}%", 
                  processing_tasks, available_permits, system_load_percent);
    
    Json(queue_status)
}

