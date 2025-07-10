use crate::model::preview::PreviewBody;
use crate::model::user::User;
use crate::model::{
    ComponentStatus, ComponentsHealth, DetailedHealthStatus, Goto, HealthStatus, Ticket, TicketId, Token, SessionUser,
};
use crate::util::{middleware, system_info, third_party_auth, IntoJson, ServerError};
use crate::CONFIG;
use ocr_conn::CURRENT_DIR;
use chrono::Utc;
use axum::extract::{Multipart, Query};
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use tower_http::cors::CorsLayer;
use tower_http::services::ServeDir;
use tower_sessions::cookie::time::Duration;
use tower_sessions::{Expiry, MemoryStore, Session, SessionManagerLayer};
use urlencoding::encode;

pub fn routes() -> Router {
    let session_store = MemoryStore::default();
    let session_layer = SessionManagerLayer::new(session_store)
        .with_secure(false)
        .with_expiry(Expiry::OnInactivity(Duration::seconds(
            CONFIG.session_timeout,
        )));

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
        .route("/api/sso/callback", get(sso_callback))
        .route("/api/third-party/callback", post(third_party_callback))
        .route("/api/auth/status", get(auth_status))
        .route("/api/dev/mock_login", post(mock_login))
        .route("/api/user_save", post(user_save))
        .route("/api/get_token", post(get_token))
        .route("/api/user_info", post(user_info))
        .route("/api/health", get(basic_health_check))
        .route("/api/health/details", get(detailed_health_check))
        .route("/api/health/components", get(components_health_check))
        // 前端配置API - 公开访问
        .route("/api/config", get(get_frontend_config))
        .route("/api/config/debug", get(get_debug_config))
        // 监控和日志管理API - 不需要业务用户认证
        .route("/api/logs/stats", get(get_log_stats))
        .route("/api/logs/cleanup", post(cleanup_logs))
        .route("/api/logs/health", get(check_log_health))
        // 安全的预审页面访问接口（不需要API认证，有自己的认证逻辑）
        .route("/api/preview/view/:request_id", get(preview_view_page));

    // 受保护路由 - 需要认证
    let protected_routes = Router::new()
        .route("/api/upload", post(upload))
        .route("/api/download", get(download))
        .route("/api/update_rule", post(update_rule))
        .route("/api/themes", get(get_themes))
        .route("/api/themes/:theme_id/reload", post(reload_theme))
        .route("/api/config/frontend", get(get_frontend_config))
        // 预审接口 - 需要用户认证
        .route("/api/preview", post(preview))
        .route("/api/preview/submit", post(preview_submit))
        // 新增：预审数据获取接口（需要认证）
        .route("/api/preview/data/:request_id", get(get_preview_data))
        // 新增：基于第三方requestId查找预审访问URL的接口
        .route("/api/preview/lookup/:third_party_request_id", get(lookup_preview_url))
        // 新增：预审状态查询接口
        .route("/api/preview/status/:preview_id", get(query_preview_status))

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

async fn preview(req: axum::extract::Request) -> impl IntoResponse {
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
    
    let mut preview_body: PreviewBody = match serde_json::from_slice(&bytes) {
        Ok(body) => body,
        Err(e) => {
            tracing::error!("解析请求体失败: {}", e);
            return crate::util::WebResult::err_custom("无效的JSON格式").into_json().into_response();
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
    
    // 建立ID映射关系（保存到文件或数据库）
    if let Err(e) = save_id_mapping(&our_preview_id, &third_party_request_id, &preview_body.user_id).await {
        tracing::error!("保存ID映射失败: {}", e);
        return crate::util::WebResult::err_custom("系统错误").into_json().into_response();
    }
    
    // 立即返回预审访问URL，不等待预审完成
    let view_url = format!("{}/api/preview/view/{}", CONFIG.host, our_preview_id);
    
    tracing::info!("立即返回预审访问URL: {}", view_url);
    
    // 异步启动预审任务（与用户是否查看无关，自动处理）
    let mut preview_clone = preview_body.clone();
    let preview_id_clone = our_preview_id.clone();
    let third_party_id_clone = third_party_request_id.clone();
    
    tokio::spawn(async move {
        tracing::info!("=== 开始自动预审任务 ===");
        tracing::info!("预审ID: {}", preview_id_clone);
        tracing::info!("第三方请求ID: {}", third_party_id_clone);
        
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
        if let Err(e) = update_preview_status(&preview_id_clone, "processing").await {
            tracing::error!("更新预审状态失败: {}", e);
        }
        
        // 执行预审逻辑
        let preview_result = preview_clone.preview().await;
        
        match preview_result {
            Ok(web_result) => {
                tracing::info!("✅ 自动预审完成: {}", preview_id_clone);
                
                // 更新状态为"已完成"
                if let Err(e) = update_preview_status(&preview_id_clone, "completed").await {
                    tracing::error!("更新预审完成状态失败: {}", e);
                }
                
                // 如果配置了回调URL，推送结果给第三方系统
                if let Err(e) = notify_third_party_system(&preview_id_clone, &third_party_id_clone, "completed", Some(&web_result)).await {
                    tracing::error!("推送预审结果给第三方失败: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("❌ 自动预审失败: {} - {}", preview_id_clone, e);
                
                // 更新状态为"失败"
                if let Err(e) = update_preview_status(&preview_id_clone, "failed").await {
                    tracing::error!("更新预审失败状态失败: {}", e);
                }
                
                // 推送失败结果给第三方系统
                if let Err(e) = notify_third_party_system(&preview_id_clone, &third_party_id_clone, "failed", None).await {
                    tracing::error!("推送预审失败结果给第三方失败: {}", e);
                }
            }
        }
        
        tracing::info!("=== 自动预审任务结束 ===");
    });
    
    // 立即返回成功响应（不包含viewUrl，系统内部知道即可）
    let response_data = serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "预审任务已提交，请稍后查看结果",
        "data": {
            "requestId": third_party_request_id,  // 返回第三方原始请求ID
            "status": "submitted",
            "message": "预审任务已提交，后台正在处理中"
        }
    });
    
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

// 保存ID映射关系
async fn save_id_mapping(preview_id: &str, third_party_request_id: &str, user_id: &str) -> anyhow::Result<()> {
    use serde_json::json;
    
    tracing::info!("保存ID映射: {} -> {}", preview_id, third_party_request_id);
    
    // 创建映射目录
    let mapping_dir = CURRENT_DIR.join("preview_mappings");
    if !mapping_dir.exists() {
        tokio::fs::create_dir_all(&mapping_dir).await?;
    }
    
    // 映射文件名使用我们的previewId
    let mapping_file = mapping_dir.join(format!("{}.json", preview_id));
    
    // 映射信息
    let mapping_info = json!({
        "previewId": preview_id,
        "thirdPartyRequestId": third_party_request_id,
        "userId": user_id,
        "createdAt": Utc::now().to_rfc3339(),
        "status": "submitted",
        "updatedAt": Utc::now().to_rfc3339()
    });
    
    // 保存映射文件
    tokio::fs::write(&mapping_file, mapping_info.to_string()).await?;
    
    tracing::info!("✅ ID映射已保存到: {}", mapping_file.display());
    Ok(())
}

// 根据previewId获取映射信息
async fn get_id_mapping(preview_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    let mapping_dir = CURRENT_DIR.join("preview_mappings");
    let mapping_file = mapping_dir.join(format!("{}.json", preview_id));
    
    if !mapping_file.exists() {
        return Ok(None);
    }
    
    let content = tokio::fs::read_to_string(&mapping_file).await?;
    let mapping: serde_json::Value = serde_json::from_str(&content)?;
    Ok(Some(mapping))
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
        
        // 检查是否有待访问的预审记录
        let redirect_url = if let Ok(Some(pending_request_id)) = session.get::<String>("pending_request_id").await {
            tracing::info!("发现待访问预审记录: {}", pending_request_id);
            // 清除待访问记录
            if let Err(e) = session.remove::<String>("pending_request_id").await {
                tracing::warn!("清除待访问预审记录失败: {}", e);
            }
            format!("/api/preview/view/{}", pending_request_id)
        } else {
            tracing::info!("无待访问预审记录，重定向到主页");
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

    // 检查是否配置了第三方SSO
    let has_sso_config = !CONFIG.login.access_token_url.is_empty() && !CONFIG.login.access_key.is_empty();
    tracing::info!("SSO配置检查:");
    tracing::info!("  access_token_url: {}", if CONFIG.login.access_token_url.is_empty() { "未配置" } else { "已配置" });
    tracing::info!("  access_key: {}", if CONFIG.login.access_key.is_empty() { "未配置" } else { "已配置" });
    tracing::info!("  secret_key: {}", if CONFIG.login.secret_key.is_empty() { "未配置" } else { "已配置" });

    let session_user = if !has_sso_config {
        tracing::warn!("⚠️  SSO配置未完成，使用简化验证模式");
        tracing::info!("简化模式说明: 直接将票据作为用户标识，不调用第三方API验证");
        create_session_user_from_ticket(&ticket_id.ticket_id).await
    } else {
        tracing::info!("🔄 使用完整SSO验证模式");
        tracing::info!("配置信息:");
        tracing::info!("  access_token_url: {}", CONFIG.login.access_token_url);
        tracing::info!("  get_user_info_url: {}", CONFIG.login.get_user_info_url);

        // 调用第三方API获取完整用户信息
        match get_user_info_from_sso(&ticket_id.ticket_id).await {
            Ok(user) => user,
            Err(e) => {
                tracing::error!("❌ 从SSO获取用户信息失败: {}", e);
                tracing::info!("降级为简化模式");
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
    
    // 检查是否有待访问的预审记录
    let redirect_url = if let Ok(Some(pending_request_id)) = session.get::<String>("pending_request_id").await {
        tracing::info!("发现待访问预审记录: {}", pending_request_id);
        // 清除待访问记录
        if let Err(e) = session.remove::<String>("pending_request_id").await {
            tracing::warn!("清除待访问预审记录失败: {}", e);
        }
        format!("/api/preview/view/{}", pending_request_id)
    } else {
        tracing::info!("无待访问预审记录，重定向到主页");
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
    let base_sso_url = "https://ibcdsg.zj.gov.cn:8443/restapi/prod/IC33000020220329000006/uc/sso/login";
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
        // 直跳模式：只添加应用ID
        format!("{}?appId={}", base_sso_url, app_id)
    }
}

// 安全的预审页面访问接口
async fn preview_view_page(
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
            
            // 验证用户是否有权限访问该预审记录
            if let Ok(has_permission) = verify_preview_access(&request_id, &session_user.user_id).await {
                if has_permission {
                    tracing::info!("✅ 用户有权限访问预审记录: {}", request_id);
                    // 重定向到预审页面，并传递安全的参数
                    let redirect_url = format!("/static/index.html?requestId={}&verified=true", request_id);
                    tracing::info!("重定向到预审页面: {}", redirect_url);
                    Redirect::to(&redirect_url)
                } else {
                    tracing::warn!("❌ 用户身份不匹配: 预审记录 {} 不属于当前登录用户 {}", request_id, session_user.user_id);
                    
                    // 获取预审记录的真实归属用户（用于日志记录）
                    if let Ok(Some(mapping)) = get_id_mapping(&request_id).await {
                        let expected_user_id = mapping["userId"].as_str().unwrap_or("未知");
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
        match verify_preview_access(&request_id, &session_user.user_id).await {
            Ok(true) => {
                // 获取预审数据
                match get_preview_record(&request_id).await {
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
async fn verify_preview_access(preview_id: &str, user_id: &str) -> anyhow::Result<bool> {
    tracing::info!("验证用户 {} 对预审记录 {} 的访问权限", user_id, preview_id);
    
    // 获取ID映射信息
    match get_id_mapping(preview_id).await? {
        Some(mapping) => {
            let mapping_user_id = mapping["userId"].as_str().unwrap_or("");
            let status = mapping["status"].as_str().unwrap_or("");
            
            tracing::info!("映射信息: 用户={}, 状态={}", mapping_user_id, status);
            
            // 检查用户ID是否匹配
            if mapping_user_id != user_id {
                tracing::warn!("❌ 用户ID不匹配: 期望={}, 实际={}", mapping_user_id, user_id);
                return Ok(false);
            }
            
            // 检查记录状态（允许多种有效状态）
            let valid_statuses = ["submitted", "processing", "completed", "failed"];
            if !valid_statuses.contains(&status) {
                tracing::warn!("❌ 预审记录状态无效: {}", status);
                return Ok(false);
            }
            
            // 检查预审文件是否存在
            let preview_dir = CURRENT_DIR.join("preview");
            let html_file = preview_dir.join(format!("{}.html", preview_id));
            let pdf_file = preview_dir.join(format!("{}.pdf", preview_id));
            
            if !html_file.exists() && !pdf_file.exists() {
                tracing::warn!("❌ 预审文件不存在: {}", preview_id);
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
async fn get_preview_record(preview_id: &str) -> anyhow::Result<Option<serde_json::Value>> {
    tracing::info!("获取预审记录数据: {}", preview_id);
    
    // 获取ID映射信息
    let mapping = match get_id_mapping(preview_id).await? {
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
        "thirdPartyRequestId": mapping["thirdPartyRequestId"],
        "userId": mapping["userId"],
        "status": "completed",
        "createdAt": mapping["createdAt"],
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
    
    tracing::info!("✅ 成功构建预审记录数据");
    Ok(Some(preview_data))
}

// 根据第三方requestId查找预审访问URL
async fn lookup_preview_url(
    axum::extract::Path(third_party_request_id): axum::extract::Path<String>,
    req: axum::extract::Request
) -> impl IntoResponse {
    tracing::info!("=== 查找预审访问URL ===");
    tracing::info!("第三方请求ID: {}", third_party_request_id);
    
    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();
    
    if let Some(session_user) = session_user {
        tracing::info!("用户ID: {}", session_user.user_id);
        
        // 查找对应的previewId
        match find_preview_by_third_party_id(&third_party_request_id, &session_user.user_id).await {
            Ok(Some(preview_id)) => {
                let view_url = format!("{}/api/preview/view/{}", CONFIG.host, preview_id);
                tracing::info!("✅ 找到预审访问URL: {}", view_url);
                
                Json(serde_json::json!({
                    "success": true,
                    "errorCode": 200,
                    "errorMsg": "",
                    "data": {
                        "viewUrl": view_url,
                        "previewId": preview_id,
                        "thirdPartyRequestId": third_party_request_id
                    }
                }))
            }
            Ok(None) => {
                tracing::warn!("❌ 未找到对应的预审记录: {}", third_party_request_id);
                Json(serde_json::json!({
                    "success": false,
                    "errorCode": 404,
                    "errorMsg": "未找到对应的预审记录",
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
async fn find_preview_by_third_party_id(third_party_request_id: &str, user_id: &str) -> anyhow::Result<Option<String>> {
    tracing::info!("查找第三方请求ID {} 对应的预审ID (用户: {})", third_party_request_id, user_id);
    
    let mapping_dir = CURRENT_DIR.join("preview_mappings");
    if !mapping_dir.exists() {
        return Ok(None);
    }
    
    // 遍历所有映射文件
    let mut entries = tokio::fs::read_dir(&mapping_dir).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().map(|s| s == "json").unwrap_or(false) {
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                if let Ok(mapping) = serde_json::from_str::<serde_json::Value>(&content) {
                    let mapping_third_party_id = mapping["thirdPartyRequestId"].as_str().unwrap_or("");
                    let mapping_user_id = mapping["userId"].as_str().unwrap_or("");
                    let mapping_status = mapping["status"].as_str().unwrap_or("");
                    
                    if mapping_third_party_id == third_party_request_id 
                        && mapping_user_id == user_id 
                        && (mapping_status == "submitted" || mapping_status == "processing" || mapping_status == "completed") {
                        
                        if let Some(preview_id) = mapping["previewId"].as_str() {
                            tracing::info!("✅ 找到匹配的预审ID: {}", preview_id);
                            return Ok(Some(preview_id.to_string()));
                        }
                    }
                }
            }
        }
    }
    
    tracing::warn!("未找到匹配的预审记录");
    Ok(None)
}

// 更新预审状态
async fn update_preview_status(preview_id: &str, status: &str) -> anyhow::Result<()> {
    tracing::info!("更新预审状态: {} -> {}", preview_id, status);
    
    let mapping_dir = CURRENT_DIR.join("preview_mappings");
    let mapping_file = mapping_dir.join(format!("{}.json", preview_id));
    
    if !mapping_file.exists() {
        return Err(anyhow::anyhow!("映射文件不存在: {}", preview_id));
    }
    
    // 读取当前映射
    let content = tokio::fs::read_to_string(&mapping_file).await?;
    let mut mapping: serde_json::Value = serde_json::from_str(&content)?;
    
    // 更新状态和时间戳
    mapping["status"] = serde_json::json!(status);
    mapping["updatedAt"] = serde_json::json!(Utc::now().to_rfc3339());
    
    // 根据状态添加特定字段
    match status {
        "processing" => {
            mapping["processingStartedAt"] = serde_json::json!(Utc::now().to_rfc3339());
        }
        "completed" => {
            mapping["completedAt"] = serde_json::json!(Utc::now().to_rfc3339());
        }
        "failed" => {
            mapping["failedAt"] = serde_json::json!(Utc::now().to_rfc3339());
        }
        _ => {}
    }
    
    // 保存更新后的映射
    tokio::fs::write(&mapping_file, mapping.to_string()).await?;
    
    tracing::info!("✅ 预审状态已更新: {} -> {}", preview_id, status);
    Ok(())
}

// 预审状态查询接口
async fn query_preview_status(
    axum::extract::Path(preview_id): axum::extract::Path<String>,
    req: axum::extract::Request
) -> impl IntoResponse {
    tracing::info!("=== 获取预审状态请求 ===");
    tracing::info!("请求ID: {}", preview_id);
    
    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();
    
    if let Some(session_user) = session_user {
        tracing::info!("用户ID: {}", session_user.user_id);
        
        // 验证用户是否有权限访问该预审记录
        match verify_preview_access(&preview_id, &session_user.user_id).await {
            Ok(true) => {
                // 获取预审状态
                match get_preview_status_info(&preview_id).await {
                    Ok(status_info) => {
                        tracing::info!("✅ 成功获取预审状态");
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
            }
            Ok(false) => {
                tracing::warn!("❌ 用户无权限访问预审记录: {} (用户: {})", preview_id, session_user.user_id);
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

// 获取预审状态信息
async fn get_preview_status_info(preview_id: &str) -> anyhow::Result<serde_json::Value> {
    tracing::info!("获取预审状态信息: {}", preview_id);
    
    // 获取ID映射信息
    let mapping = match get_id_mapping(preview_id).await? {
        Some(mapping) => mapping,
        None => {
            tracing::warn!("预审记录映射不存在: {}", preview_id);
            return Ok(serde_json::json!({
                "status": "unknown",
                "message": "预审记录映射不存在"
            }));
        }
    };
    
    let status = mapping["status"].as_str().unwrap_or("unknown");
    let message = format!("预审记录状态: {}", status);
    
    Ok(serde_json::json!({
        "status": status,
        "message": message
    }))
}

// 通知第三方系统预审结果
async fn notify_third_party_system(
    preview_id: &str, 
    third_party_request_id: &str, 
    status: &str, 
    result: Option<&crate::util::WebResult>
) -> anyhow::Result<()> {
    tracing::info!("=== 准备通知第三方系统 ===");
    tracing::info!("预审ID: {}", preview_id);
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
        "previewId": preview_id,
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
                let download_url = format!("{}/api/preview/view/{}", CONFIG.host, preview_id);
                callback_data["viewUrl"] = serde_json::json!(download_url);
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

// 开发测试用模拟登录接口
async fn mock_login(session: Session, Json(mock_user): Json<serde_json::Value>) -> impl IntoResponse {
    // 首先检查配置是否允许模拟登录
    if !CONFIG.debug.enable_mock_login {
        tracing::error!("❌ 模拟登录功能已禁用");
        tracing::error!("提示：请在 config.yaml 中设置 debug.enable_mock_login: true");
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 403,
            "errorMsg": "模拟登录功能已禁用，请检查配置文件"
        }));
    }

    // 安全警告
    if CONFIG.debug.mock_login_warning {
        tracing::warn!("⚠️ =====================================================");
        tracing::warn!("⚠️  警告：模拟登录功能已启用！");
        tracing::warn!("⚠️  这是开发测试功能，生产环境中必须禁用！");
        tracing::warn!("⚠️  请确保 config.yaml 中 debug.enable_mock_login: false");
        tracing::warn!("⚠️ =====================================================");
    }
    
    tracing::warn!("🧪 开发测试模式：模拟用户登录");
    
    // 额外的安全检查：如果配置了SSO且在生产环境，拒绝模拟登录
    if !CONFIG.login.access_token_url.is_empty() && 
       std::env::var("RUST_ENV").unwrap_or_default() == "production" {
        tracing::error!("❌ 生产环境中禁止使用模拟登录");
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 403,
            "errorMsg": "生产环境中禁止使用模拟登录"
        }));
    }

    let user_id = mock_user.get("userId")
        .and_then(|v| v.as_str())
        .unwrap_or("test_user_001")
        .to_string();
    
    let user_name = mock_user.get("userName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let now = Utc::now().to_rfc3339();
    let session_user = SessionUser {
        user_id: user_id.clone(),
        user_name,
        certificate_type: "01".to_string(),
        certificate_number: mock_user.get("certificateNumber")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        phone_number: mock_user.get("phoneNumber")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        email: mock_user.get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        organization_name: mock_user.get("organizationName")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        organization_code: mock_user.get("organizationCode")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        login_time: now.clone(),
        last_active: now,
    };

    // 保存用户信息到会话
    if let Err(e) = session.insert("session_user", &session_user).await {
        tracing::error!("❌ 保存模拟用户信息到会话失败: {}", e);
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 500,
            "errorMsg": "保存会话失败"
        }));
    }

    tracing::info!("✅ 模拟用户登录成功: {} ({})", 
                   user_id, 
                   session_user.user_name.as_deref().unwrap_or("未知用户"));

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "userId": session_user.user_id,
            "userName": session_user.user_name,
            "message": "模拟登录成功"
        }
    }))
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

