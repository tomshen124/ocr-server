
use crate::model::user::User;
use crate::model::{SessionUser, Ticket, TicketId, Token};
use crate::util::IntoJson;
use crate::{AppState, CONFIG};
use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Redirect};
use axum::Json;
use base64::Engine;
use chrono::Utc;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::process::Command;
use tokio::task;
use tower_sessions::Session;
use url::Url;

fn generate_signature_headers(
    url_str: &str,
    method: &str,
) -> anyhow::Result<std::collections::HashMap<String, String>> {
    type HmacSha256 = Hmac<Sha256>;

    let access_key = &CONFIG.login.access_key;
    let secret_key = &CONFIG.login.secret_key;

    let now = chrono::Utc::now();
    let date = now.format("%a, %d %b %Y %H:%M:%S GMT").to_string();

    let url = Url::parse(url_str)?;
    let path = url.path();
    let query = url.query().unwrap_or("");

    let signing_string = format!(
        "{}\n{}\n{}\n{}\n{}\n",
        method.to_uppercase(),
        path,
        query,
        access_key,
        date
    );

    tracing::debug!("签名字符串: {}", signing_string);

    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())?;
    mac.update(signing_string.as_bytes());
    let result = mac.finalize();
    let signature = base64::engine::general_purpose::STANDARD.encode(result.into_bytes());

    let mut headers = std::collections::HashMap::new();
    headers.insert("X-BG-HMAC-SIGNATURE".to_string(), signature);
    headers.insert("X-BG-HMAC-ALGORITHM".to_string(), "hmac-sha256".to_string());
    headers.insert("X-BG-HMAC-ACCESS-KEY".to_string(), access_key.clone());
    headers.insert("X-BG-DATE-TIME".to_string(), date);

    tracing::info!("生成签名头部: {:?}", headers);
    Ok(headers)
}

async fn call_sso_api_with_curl(url: &str, json_data: &str) -> anyhow::Result<String> {
    tracing::info!("使用curl调用SSO API: {}", url);
    tracing::debug!("请求数据: {}", json_data);

    let signature_headers = generate_signature_headers(url, "POST")?;

    let curl_cmd = if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    };

    let mut command = Command::new(curl_cmd);
    command
        .arg("-X")
        .arg("POST")
        .arg("-k")
        .arg("--connect-timeout")
        .arg("30")
        .arg("--max-time")
        .arg("60")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(json_data)
        .arg(url);

    for (key, value) in signature_headers {
        command.arg("-H").arg(format!("{}: {}", key, value));
    }

    tracing::debug!("执行curl命令: {:?}", command);

    let output = task::spawn_blocking(move || command.output()).await??;

    tracing::debug!("curl命令执行完成，状态码: {:?}", output.status);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("curl命令执行失败: {}", stderr);
        anyhow::bail!("curl命令执行失败: {}", stderr);
    }

    let response_text = String::from_utf8_lossy(&output.stdout).to_string();
    tracing::debug!("curl响应: {}", response_text);

    Ok(response_text)
}

pub fn build_sso_login_url(return_url: Option<&str>, user_type: Option<&str>) -> String {
    let base_sso_url = &CONFIG.login.sso_login_url;
    let app_id = &CONFIG.app_id;

    if CONFIG.login.use_callback {
        let callback_url = CONFIG.callback_url();

        let sp_param = if let Some(return_url) = return_url {
            format!(
                "{}?return_url={}",
                callback_url,
                urlencoding::encode(return_url)
            )
        } else {
            callback_url.clone()
        };

        let mut url = format!(
            "{}?appId={}&sp={}",
            base_sso_url,
            app_id,
            urlencoding::encode(&sp_param)
        );

        let user_type = user_type.unwrap_or("person");
        url.push_str(&format!("&userType={}", user_type));

        url
    } else {
        base_sso_url.to_string()
    }
}

pub async fn create_session_user_from_ticket(ticket_id: &str) -> SessionUser {
    let now = Utc::now().to_rfc3339();
    SessionUser {
        user_id: ticket_id.to_string(),
        user_name: None,
        certificate_type: "01".to_string(),
        certificate_number: None,
        phone_number: None,
        email: None,
        organization_name: None,
        organization_code: None,
        login_time: now.clone(),
        last_active: now,
    }
}

pub async fn get_user_info_from_sso_with_retry(ticket_id: &str) -> anyhow::Result<SessionUser> {
    const MAX_RETRIES: u32 = 2;
    let mut last_error = None;

    for attempt in 1..=MAX_RETRIES {
        tracing::info!("SSO认证尝试 {}/{}", attempt, MAX_RETRIES);

        match get_user_info_from_sso(ticket_id).await {
            Ok(user) => {
                if attempt > 1 {
                    tracing::info!("[ok] SSO认证在第{}次尝试后成功", attempt);
                }
                return Ok(user);
            }
            Err(e) => {
                last_error = Some(e);
                if attempt < MAX_RETRIES {
                    tracing::warn!("[warn] SSO认证第{}次尝试失败，等待重试...", attempt);
                    tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                } else {
                    tracing::error!("[fail] SSO认证在{}次尝试后全部失败", MAX_RETRIES);
                }
            }
        }
    }

    match last_error {
        Some(error) => Err(error),
        None => Err(anyhow::anyhow!("SSO认证失败：未知错误")),
    }
}

pub async fn get_user_info_from_sso(ticket_id: &str) -> anyhow::Result<SessionUser> {
    tracing::info!("=== 开始SSO用户信息获取（使用curl命令） ===");
    tracing::info!("票据ID: {}", ticket_id);
    tracing::info!("access_token_url: {}", CONFIG.login.access_token_url);
    tracing::info!("get_user_info_url: {}", CONFIG.login.get_user_info_url);

    let token_params = serde_json::json!({
        "ticketId": ticket_id,
        "appId": CONFIG.app_id
    });

    tracing::info!("正在获取 access_token...");

    let token_text =
        call_sso_api_with_curl(&CONFIG.login.access_token_url, &token_params.to_string()).await?;

    tracing::info!("Token API 响应内容: {}", token_text);

    let token_result: serde_json::Value = serde_json::from_str(&token_text).map_err(|e| {
        anyhow::anyhow!("解析access_token响应失败: {} - 响应内容: {}", e, token_text)
    })?;

    if let Some(success) = token_result.get("success").and_then(|s| s.as_bool()) {
        if !success {
            let error_msg = token_result
                .get("errorMsg")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            anyhow::bail!("获取access_token失败: {}", error_msg);
        }
    }

    let access_token = token_result
        .get("data")
        .and_then(|d| d.get("accessToken"))
        .or_else(|| token_result.get("accessToken"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow::anyhow!("响应中未找到 accessToken"))?;

    tracing::info!(
        "[ok] 成功获取 access_token: {}...",
        &access_token[..std::cmp::min(10, access_token.len())]
    );

    let user_params = serde_json::json!({
        "token": access_token
    });

    tracing::info!("正在获取用户信息...");

    let user_text =
        call_sso_api_with_curl(&CONFIG.login.get_user_info_url, &user_params.to_string()).await?;

    tracing::info!("UserInfo API 响应内容: {}", user_text);

    let user_result: serde_json::Value = serde_json::from_str(&user_text)
        .map_err(|e| anyhow::anyhow!("解析用户信息响应失败: {} - 响应内容: {}", e, user_text))?;

    if let Some(success) = user_result.get("success").and_then(|s| s.as_bool()) {
        if !success {
            let error_msg = user_result
                .get("errorMsg")
                .and_then(|m| m.as_str())
                .unwrap_or("未知错误");
            anyhow::bail!("获取用户信息失败: {}", error_msg);
        }
    }

    let user_data = user_result
        .get("data")
        .ok_or_else(|| anyhow::anyhow!("响应中未找到 data 字段"))?;

    let now = Utc::now().to_rfc3339();
    let session_user = SessionUser {
        user_id: user_data
            .get("userId")
            .or_else(|| user_data.get("userCode"))
            .or_else(|| user_data.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or(ticket_id)
            .to_string(),
        user_name: user_data
            .get("userName")
            .or_else(|| user_data.get("name"))
            .or_else(|| user_data.get("realName"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        certificate_type: user_data
            .get("certificateType")
            .or_else(|| user_data.get("idType"))
            .and_then(|v| v.as_str())
            .unwrap_or("01")
            .to_string(),
        certificate_number: user_data
            .get("certificateNumber")
            .or_else(|| user_data.get("idNumber"))
            .or_else(|| user_data.get("cardNo"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        phone_number: user_data
            .get("phoneNumber")
            .or_else(|| user_data.get("mobile"))
            .or_else(|| user_data.get("phone"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        email: user_data
            .get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        organization_name: user_data
            .get("organizationName")
            .or_else(|| user_data.get("orgName"))
            .or_else(|| user_data.get("company"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        organization_code: user_data
            .get("organizationCode")
            .or_else(|| user_data.get("orgCode"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        login_time: now.clone(),
        last_active: now,
    };

    tracing::info!("[ok] SSO用户信息获取成功！");
    tracing::info!("用户ID: {}", session_user.user_id);
    tracing::info!(
        "用户姓名: {}",
        session_user.user_name.as_deref().unwrap_or("未提供")
    );
    tracing::info!("证件类型: {}", session_user.certificate_type);
    tracing::info!("=== SSO用户信息获取完成 ===");

    Ok(session_user)
}

pub async fn user_save(session: Session, Json(ticket_id): Json<TicketId>) -> impl IntoResponse {
    let result = User::user_save(session, ticket_id).await;
    result.into_json()
}

pub async fn get_token(Json(ticket): Json<Ticket>) -> impl IntoResponse {
    let result = User::get_token_by_ticket(ticket).await;
    result.into_json()
}

pub async fn user_info(session: Session, Json(token): Json<Token>) -> impl IntoResponse {
    let result = User::get_user_by_token(session, token).await;
    result.into_json()
}

pub async fn sso_callback(
    State(app_state): State<AppState>,
    headers: HeaderMap,
    session: Session,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    tracing::info!("=== SSO回调开始 ===");
    tracing::info!("回调URL参数: {:?}", params);
    tracing::info!("参数数量: {}", params.len());

    let possible_ticket_params = [
        "ticketId",
        "ticket",
        "code",
        "token",
        "st",
        "service_ticket",
    ];
    for param_name in &possible_ticket_params {
        if let Some(value) = params.get(*param_name) {
            tracing::info!("发现票据参数 '{}': {}", param_name, value);
        }
    }

    let ticket_id = params
        .get("ticketId")
        .or_else(|| params.get("ticket"))
        .or_else(|| params.get("code"))
        .or_else(|| params.get("token"))
        .or_else(|| params.get("st"))
        .or_else(|| params.get("service_ticket"));

    if let Some(ticket_id) = ticket_id {
        tracing::info!("[ok] 成功提取票据ID: {}", ticket_id);
        tracing::info!("票据长度: {} 字符", ticket_id.len());

        let session_user = create_session_user_from_ticket(ticket_id).await;

        tracing::info!("正在保存用户信息到会话...");
        if let Err(e) = session.insert("session_user", &session_user).await {
            tracing::error!("[fail] 保存用户信息到会话失败: {}", e);
            let sso_url = build_sso_login_url(None, Some("person"));
            return Redirect::to(&sso_url);
        }
        tracing::info!("[ok] 用户信息已保存到会话");

        if let Err(err) = crate::api::preview::save_user_login_record(
            &app_state.database,
            &session_user,
            "sso_login",
            &headers,
        )
        .await
        {
            tracing::warn!(
                "[warn] SSO登录审计记录写入失败: {}，用户={}",
                err,
                session_user.user_id
            );
        }

        tracing::info!("[celebrate] SSO回调处理完全成功！");
        tracing::info!("用户ID: {}", session_user.user_id);
        tracing::info!(
            "用户姓名: {}",
            session_user.user_name.as_deref().unwrap_or("未知")
        );

        let return_url_from_params = params
            .get("redirectURL")
            .or_else(|| params.get("return_url"))
            .or_else(|| params.get("state"));

        let redirect_url = if let Some(url) = return_url_from_params {
            tracing::info!("从回调参数中获取返回URL: {}", url);
            url.to_string()
        } else if let Ok(Some(pending_request_id)) =
            session.get::<String>("pending_request_id").await
        {
            tracing::info!("发现待访问预审记录: {}", pending_request_id);
            if let Err(e) = session.remove::<String>("pending_request_id").await {
                tracing::warn!("清除待访问预审记录失败: {}", e);
            }
            format!(
                "/static/index.html?previewId={}&verified=true",
                pending_request_id
            )
        } else if let Ok(Some(return_url)) = session.get::<String>("return_url").await {
            tracing::info!("发现保存的返回URL: {}", return_url);
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
        tracing::warn!("[fail] SSO回调中未找到有效的票据参数");
        tracing::warn!("检查的参数名: {:?}", possible_ticket_params);
        tracing::warn!("实际收到的参数: {:?}", params.keys().collect::<Vec<_>>());
        tracing::warn!("可能的原因:");
        tracing::warn!("1. 第三方系统使用了不同的参数名");
        tracing::warn!("2. 第三方系统配置错误");
        tracing::warn!("3. 回调URL配置不正确");
        tracing::info!("=== SSO回调结束（失败）===");

        let sso_url = build_sso_login_url(None, Some("person"));
        Redirect::to(&sso_url)
    }
}

pub async fn verify_user(session: Session, Json(ticket_id): Json<TicketId>) -> impl IntoResponse {
    tracing::info!("=== 用户票据验证开始 ===");
    tracing::info!("收到票据ID: {}", ticket_id.ticket_id);
    tracing::info!("票据长度: {} 字符", ticket_id.ticket_id.len());

    let is_debug_ticket = ticket_id.ticket_id.starts_with("debug_tk_")
        || ticket_id.ticket_id.starts_with("test_tk_")
        || ticket_id.ticket_id == "debug_test_ticket";

    if is_debug_ticket {
        tracing::info!("[lab] 检测到调试票据，使用开发模式认证");

        let debug_enabled = CONFIG.debug.enabled || CONFIG.runtime_mode.mode == "development";

        if debug_enabled {
            tracing::info!("[ok] 调试模式已启用，创建调试用户会话");

            let (base_id, user_data) = if let Some(hash_pos) = ticket_id.ticket_id.find('#') {
                let base_part = &ticket_id.ticket_id[..hash_pos];
                let data_part = &ticket_id.ticket_id[hash_pos + 1..];

                match serde_json::from_str::<serde_json::Value>(&String::from_utf8_lossy(
                    &base64::engine::general_purpose::STANDARD
                        .decode(data_part)
                        .map_err(|e| tracing::warn!("Base64解码失败: {}", e))
                        .unwrap_or_default(),
                )) {
                    Ok(data) => (base_part, Some(data)),
                    Err(_) => (ticket_id.ticket_id.as_str(), None),
                }
            } else {
                (ticket_id.ticket_id.as_str(), None)
            };

            let debug_user = SessionUser {
                user_id: format!("debug_user_{}", &base_id[9..19.min(base_id.len())]),
                user_name: user_data
                    .as_ref()
                    .and_then(|d| d["user_name"].as_str())
                    .map(|s| s.to_string()),
                certificate_type: user_data
                    .as_ref()
                    .and_then(|d| d["certificate_type"].as_str())
                    .unwrap_or("ID_CARD")
                    .to_string(),
                certificate_number: user_data
                    .as_ref()
                    .and_then(|d| d["certificate_number"].as_str())
                    .map(|s| s.to_string()),
                phone_number: user_data
                    .as_ref()
                    .and_then(|d| d["phone_number"].as_str())
                    .map(|s| s.to_string()),
                email: user_data
                    .as_ref()
                    .and_then(|d| d["email"].as_str())
                    .map(|s| s.to_string()),
                organization_name: user_data
                    .as_ref()
                    .and_then(|d| d["organization_name"].as_str())
                    .map(|s| s.to_string()),
                organization_code: user_data
                    .as_ref()
                    .and_then(|d| d["organization_code"].as_str())
                    .map(|s| s.to_string()),
                login_time: Utc::now().to_rfc3339(),
                last_active: Utc::now().to_rfc3339(),
            };

            if let Err(e) = session.insert("session_user", &debug_user).await {
                tracing::error!("[fail] 保存调试用户信息到会话失败: {}", e);
                return Json(serde_json::json!({
                    "success": false,
                    "errorCode": 500,
                    "errorMsg": "会话保存失败",
                    "data": null
                }))
                .into_response();
            }

            tracing::info!("[celebrate] 调试票据验证成功！");
            tracing::info!("调试用户ID: {}", debug_user.user_id);
            tracing::info!(
                "调试用户姓名: {}",
                debug_user.user_name.as_deref().unwrap_or("未知")
            );

            return Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": {
                    "userId": debug_user.user_id,
                    "userName": debug_user.user_name,
                    "debugMode": true,
                    "message": "调试票据验证成功"
                }
            }))
            .into_response();
        } else {
            tracing::warn!("[warn] 调试票据但调试模式未启用");
            return Json(serde_json::json!({
                "success": false,
                "errorCode": 403,
                "errorMsg": "调试模式未启用，无法使用调试票据",
                "data": null
            }))
            .into_response();
        }
    }

    let has_sso_config = !CONFIG.login.access_token_url.is_empty()
        && !CONFIG.login.get_user_info_url.is_empty()
        && !CONFIG.login.access_key.is_empty()
        && !CONFIG.login.secret_key.is_empty();
    tracing::info!(
        "SSO配置检查结果: {}",
        if has_sso_config {
            "[ok] 完整配置"
        } else {
            "[warn] 配置不完整"
        }
    );
    tracing::info!(
        "  access_token_url: {}",
        if CONFIG.login.access_token_url.is_empty() {
            "[fail] 未配置"
        } else {
            "[ok] 已配置"
        }
    );
    tracing::info!(
        "  get_user_info_url: {}",
        if CONFIG.login.get_user_info_url.is_empty() {
            "[fail] 未配置"
        } else {
            "[ok] 已配置"
        }
    );
    tracing::info!(
        "  access_key: {}",
        if CONFIG.login.access_key.is_empty() {
            "[fail] 未配置"
        } else {
            "[ok] 已配置"
        }
    );
    tracing::info!(
        "  secret_key: {}",
        if CONFIG.login.secret_key.is_empty() {
            "[fail] 未配置"
        } else {
            "[ok] 已配置"
        }
    );
    tracing::info!("  use_callback: {}", CONFIG.login.use_callback);

    let session_user = if !has_sso_config {
        tracing::warn!("[warn]  SSO配置未完成，使用简化验证模式");
        tracing::info!("简化模式说明: 直接将票据作为用户标识，不调用第三方API验证");
        create_session_user_from_ticket(&ticket_id.ticket_id).await
    } else {
        tracing::info!("[loop] 使用完整SSO验证模式");
        tracing::info!("配置信息:");
        tracing::info!("  access_token_url: {}", CONFIG.login.access_token_url);
        tracing::info!("  get_user_info_url: {}", CONFIG.login.get_user_info_url);

        match get_user_info_from_sso_with_retry(&ticket_id.ticket_id).await {
            Ok(user) => {
                tracing::info!("[ok] 完整SSO模式认证成功");
                user
            }
            Err(e) => {
                tracing::error!("[fail] 从SSO获取用户信息失败: {}", e);
                tracing::warn!("[loop] 自动降级为简化验证模式");
                create_session_user_from_ticket(&ticket_id.ticket_id).await
            }
        }
    };

    tracing::info!("正在保存用户信息到会话...");
    if let Err(e) = session.insert("session_user", &session_user).await {
        tracing::error!("[fail] 保存用户信息到会话失败: {}", e);
        return Json(serde_json::json!({
            "success": false,
            "errorCode": 500,
            "errorMsg": "会话保存失败",
            "data": null
        }))
        .into_response();
    }
    tracing::info!("[ok] 用户信息已保存到会话");

    tracing::info!("[celebrate] 用户票据验证成功！");
    tracing::info!("用户ID: {}", session_user.user_id);
    tracing::info!(
        "用户姓名: {}",
        session_user.user_name.as_deref().unwrap_or("未知")
    );

    let redirect_url =
        if let Ok(Some(pending_request_id)) = session.get::<String>("pending_request_id").await {
            tracing::info!("发现待访问预审记录: {}", pending_request_id);
            if let Err(e) = session.remove::<String>("pending_request_id").await {
                tracing::warn!("清除待访问预审记录失败: {}", e);
            }
            format!(
                "/static/index.html?previewId={}&verified=true",
                pending_request_id
            )
        } else if let Ok(Some(return_url)) = session.get::<String>("return_url").await {
            tracing::info!("发现保存的返回URL: {}", return_url);
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

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "userId": session_user.user_id,
            "userName": session_user.user_name,
            "certificateType": session_user.certificate_type,
            "certificateNumber": session_user.certificate_number,
            "phoneNumber": session_user.phone_number,
            "email": session_user.email,
            "organizationName": session_user.organization_name,
            "organizationCode": session_user.organization_code,
            "loginTime": session_user.login_time,
            "lastActive": session_user.last_active,
            "redirectUrl": redirect_url,
            "message": "用户验证成功"
        }
    }))
    .into_response()
}

pub async fn auth_status(session: Session) -> impl IntoResponse {
    match session.get::<SessionUser>("session_user").await {
        Ok(Some(session_user)) => {
            tracing::info!(
                "用户认证状态检查: {} ({})",
                session_user.user_id,
                session_user.user_name.as_deref().unwrap_or("未知用户")
            );

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
            Json(serde_json::json!({
                "authenticated": false,
                "error": "用户未登录或会话已过期",
                "redirect": "/api/sso/login"
            }))
        }
    }
}

pub async fn auth_logout(session: Session) -> impl IntoResponse {
    tracing::info!("用户登出请求");

    if let Ok(Some(session_user)) = session.get::<SessionUser>("session_user").await {
        tracing::info!(
            "用户 {} ({}) 正在登出",
            session_user.user_id,
            session_user.user_name.as_deref().unwrap_or("未知用户")
        );
    }

    session.clear().await;
    tracing::info!("[ok] 用户会话已清除");

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "message": "登出成功"
        }
    }))
}

pub async fn sso_login_redirect(
    session: Session,
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    tracing::info!("=== SSO登录跳转请求 ===");

    let return_url = params.get("return_url");
    let pending_request_id = params.get("request_id");

    tracing::info!("返回URL: {:?}", return_url);
    tracing::info!("待访问预审ID: {:?}", pending_request_id);

    if let Some(request_id) = pending_request_id {
        if let Err(e) = session.insert("pending_request_id", request_id).await {
            tracing::warn!("保存待访问预审记录ID失败: {}", e);
        } else {
            tracing::info!("已保存待访问预审记录ID: {}", request_id);
        }
    }

    if let Some(url) = return_url {
        if let Err(e) = session.insert("return_url", url).await {
            tracing::warn!("保存返回URL失败: {}", e);
        } else {
            tracing::info!("已保存返回URL: {}", url);
        }
    }

    let sso_url = build_sso_login_url(return_url.map(|s| s.as_str()), Some("person"));

    tracing::info!("构建的SSO登录URL: {}", sso_url);
    tracing::info!("=== SSO登录跳转执行 ===");

    Redirect::to(&sso_url)
}
