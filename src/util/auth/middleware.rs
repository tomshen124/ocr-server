//! HTTP中间件模块
//! 处理第三方认证的HTTP中间件逻辑

use super::client::{ThirdPartyAuthService, AuthResult, AuthError};
use super::rate_limit::RateLimiter;
use axum::{
    extract::{Request, Query}, 
    middleware::Next, 
    response::Response,
    http::{StatusCode, HeaderMap}
};
use std::collections::HashMap;
use crate::CONFIG;
use tracing::{info, warn};

/// 第三方系统认证中间件
/// 根据配置决定是否启用验证，无论哪种模式都记录详细访问日志
pub async fn third_party_auth_middleware(
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    
    let api_path = request.uri().path().to_string();
    let remote_addr = extract_remote_addr(&request);
        
    // 检查是否启用第三方访问控制
    if !CONFIG.third_party_access.enabled {
        log_open_access(&api_path, &remote_addr);
        return Ok(next.run(request).await);
    }
    
    log_secured_access_start(&api_path, &remote_addr);
    
    // 从请求中提取认证信息
    let auth_info = match ThirdPartyAuthService::extract_auth_info(&headers, &params) {
        Ok(auth_info) => auth_info,
        Err(error) => {
            log_auth_failure(&api_path, &remote_addr, "missing_auth_params", error.message());
            return Err(StatusCode::UNAUTHORIZED);
        }
    };
    
    // 执行完整的认证流程
    let auth_result = ThirdPartyAuthService::authenticate_client(
        &auth_info,
        &api_path,
        &remote_addr
    );
    
    let authenticated_client = match auth_result {
        AuthResult::Success(client) => client,
        AuthResult::Failed(error) => {
            let status_code = map_auth_error_to_status(&error);
            log_auth_failure(&api_path, &remote_addr, "authentication_failed", error.message());
            return Err(status_code);
        }
    };
    
    // 检查频率限制（如果启用）
    if CONFIG.third_party_access.rate_limiting.enabled {
        let rate_limit_result = RateLimiter::check_rate_limit(
            &authenticated_client.client_id,
            CONFIG.third_party_access.rate_limiting.requests_per_hour
        ).await;
        
        if let Err(error_msg) = rate_limit_result {
            log_rate_limit_exceeded(&api_path, &remote_addr, &authenticated_client, &error_msg);
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }
    
    // 将认证信息添加到请求扩展中
    request.extensions_mut().insert(authenticated_client.clone());
    
    // 记录成功的访问日志
    log_auth_success(&api_path, &remote_addr, &authenticated_client, &auth_info.access_key);
    
    Ok(next.run(request).await)
}

/// 提取远程地址
fn extract_remote_addr(request: &Request) -> String {
    request.headers().get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

/// 记录开放访问日志
fn log_open_access(api_path: &str, remote_addr: &str) {
    info!(
        event = "third_party_access",
        mode = "open",
        api_path = api_path,
        remote_addr = remote_addr,
        result = "allowed",
        "🌐 开放模式访问: {} 来自 {}", api_path, remote_addr
    );
}

/// 记录安全模式访问开始日志
fn log_secured_access_start(api_path: &str, remote_addr: &str) {
    info!(
        event = "third_party_access",
        mode = "secured",
        api_path = api_path,
        remote_addr = remote_addr,
        "🔐 安全模式访问控制开始: {} 来自 {}", api_path, remote_addr
    );
}

/// 记录认证失败日志
fn log_auth_failure(api_path: &str, remote_addr: &str, reason: &str, error: &str) {
    warn!(
        event = "third_party_access",
        mode = "secured",
        api_path = api_path,
        remote_addr = remote_addr,
        result = "failed",
        reason = reason,
        error = error,
        "❌ 认证失败: {} 来自 {} - 原因: {} - 错误: {}", 
        api_path, remote_addr, reason, error
    );
}

/// 记录频率限制超出日志
fn log_rate_limit_exceeded(
    api_path: &str, 
    remote_addr: &str, 
    client: &super::client::AuthenticatedClient, 
    error: &str
) {
    warn!(
        event = "third_party_access",
        mode = "secured",
        api_path = api_path,
        remote_addr = remote_addr,
        client_id = &client.client_id,
        client_name = &client.client_name,
        rate_limit = CONFIG.third_party_access.rate_limiting.requests_per_hour,
        result = "failed",
        reason = "rate_limit_exceeded",
        error = error,
        "❌ 频率限制超出: {} 客户端 {} ({}) 来自 {} 限制 {}/小时 - {}", 
        api_path, client.client_name, client.client_id, remote_addr, 
        CONFIG.third_party_access.rate_limiting.requests_per_hour, error
    );
}

/// 记录认证成功日志
fn log_auth_success(
    api_path: &str, 
    remote_addr: &str, 
    client: &super::client::AuthenticatedClient, 
    access_key: &str
) {
    info!(
        event = "third_party_access",
        mode = "secured",
        api_path = api_path,
        remote_addr = remote_addr,
        client_id = &client.client_id,
        client_name = &client.client_name,
        access_key = access_key,
        result = "success",
        "✅ 访问控制通过: {} 客户端 {} ({}) 来自 {} AK {}", 
        api_path, client.client_name, client.client_id, remote_addr, access_key
    );
}

/// 将认证错误映射到HTTP状态码
fn map_auth_error_to_status(error: &AuthError) -> StatusCode {
    match error {
        AuthError::MissingParameters(_) => StatusCode::BAD_REQUEST,
        AuthError::InvalidClient(_) => StatusCode::UNAUTHORIZED,
        AuthError::InsufficientPermission(_) => StatusCode::FORBIDDEN,
        AuthError::SignatureVerificationFailed(_) => StatusCode::UNAUTHORIZED,
        AuthError::RateLimitExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
        AuthError::TimestampExpired(_) => StatusCode::UNAUTHORIZED,
    }
}

/// 安全审计日志记录器
pub struct SecurityAuditor;

impl SecurityAuditor {
    /// 记录可疑活动
    pub fn log_suspicious_activity(
        api_path: &str,
        remote_addr: &str,
        reason: &str,
        details: &str,
    ) {
        warn!(
            event = "security_audit",
            api_path = api_path,
            remote_addr = remote_addr,
            reason = reason,
            details = details,
            "🚨 可疑活动: {} 来自 {} - 原因: {} - 详情: {}",
            api_path, remote_addr, reason, details
        );
    }
    
    /// 记录访问模式异常
    pub fn log_access_pattern_anomaly(
        client_id: &str,
        pattern_type: &str,
        description: &str,
    ) {
        warn!(
            event = "security_audit",
            client_id = client_id,
            pattern_type = pattern_type,
            description = description,
            "⚠️ 访问模式异常: 客户端 {} - 类型: {} - 描述: {}",
            client_id, pattern_type, description
        );
    }
    
    /// 记录配置变更
    pub fn log_config_change(
        admin_user: Option<&str>,
        change_type: &str,
        details: &str,
    ) {
        info!(
            event = "security_audit",
            admin_user = admin_user,
            change_type = change_type,
            details = details,
            "🔧 配置变更: 管理员 {} - 类型: {} - 详情: {}",
            admin_user.unwrap_or("system"), change_type, details
        );
    }
}

/// 请求上下文扩展
pub trait RequestExt {
    /// 获取认证的客户端信息
    fn authenticated_client(&self) -> Option<&super::client::AuthenticatedClient>;
    
    /// 检查客户端是否有特定权限
    fn has_permission(&self, permission: &str) -> bool;
}

impl RequestExt for Request {
    fn authenticated_client(&self) -> Option<&super::client::AuthenticatedClient> {
        self.extensions().get::<super::client::AuthenticatedClient>()
    }
    
    fn has_permission(&self, permission: &str) -> bool {
        if let Some(client) = self.authenticated_client() {
            client.permissions.contains(&permission.to_string())
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::client::AuthError;
    
    #[test]
    fn test_auth_error_to_status_mapping() {
        assert_eq!(
            map_auth_error_to_status(&AuthError::MissingParameters("test".to_string())),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            map_auth_error_to_status(&AuthError::InvalidClient("test".to_string())),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            map_auth_error_to_status(&AuthError::InsufficientPermission("test".to_string())),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            map_auth_error_to_status(&AuthError::RateLimitExceeded("test".to_string())),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}