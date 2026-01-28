//! HTTP中间件模块
//! 处理第三方认证的HTTP中间件逻辑

use super::client::{AuthError, AuthResult, ThirdPartyAuthService};
use super::global_throttle::{GlobalThrottleGuard, ThrottleCheckResult};
use super::rate_limit::RateLimiter;
use crate::CONFIG;
use axum::{
    extract::{Query, Request},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{info, warn};

static OPEN_WARNED: AtomicBool = AtomicBool::new(false);

/// 第三方系统认证中间件 - 最大兼容模式
/// 支持多种调用方式：
/// 1. 开放模式：无任何认证信息，直接放行（直接API调用）
/// 2. 标识模式：有AK标识但无签名，仅做来源识别（平台路由但平台不支持签名）
/// 3. 认证模式：有完整AK/SK签名信息，进行完整验证（支持签名的平台路由）
pub async fn third_party_auth_middleware(
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let api_path = request.uri().path().to_string();
    let remote_addr = extract_remote_addr(&request);

    // 全局限流检查 (在所有其他检查之前)
    if let ThrottleCheckResult::Blocked { current, max } = GlobalThrottleGuard::global().check() {
        warn!(
            event = "global_throttle",
            api_path = api_path,
            remote_addr = remote_addr,
            current = current,
            max = max,
            "[blocked] 全局限流拦截: {} 来自 {} (已处理 {}/{})",
            api_path,
            remote_addr,
            current,
            max
        );
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    let is_dev_mode = CONFIG.runtime_mode.mode.eq_ignore_ascii_case("development");
    let strict_auth = std::env::var("OCR_FORCE_THIRD_PARTY_AUTH")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // 检查是否启用第三方访问控制
    if !CONFIG.third_party_access.enabled {
        if strict_auth && !is_dev_mode {
            warn!(
                event = "third_party_access",
                mode = "closed",
                api_path = api_path,
                remote_addr = remote_addr,
                "[warn] 已启用 OCR_FORCE_THIRD_PARTY_AUTH，拒绝未配置的第三方调用"
            );
            return Err(StatusCode::FORBIDDEN);
        }

        if !OPEN_WARNED.swap(true, Ordering::Relaxed) && !is_dev_mode {
            warn!(
                event = "third_party_access",
                mode = "open_without_auth",
                "[warn] third_party_access.disabled 且未强制认证，生产环境将允许开放调用"
            );
        }

        log_open_access(&api_path, &remote_addr);
        return Ok(next.run(request).await);
    }

    // 检查是否有AK标识（不管是否有完整签名信息）
    let access_key = extract_access_key(&headers, &params);

    match access_key {
        Some(ak) => {
            // 有AK标识 - 尝试识别为第三方平台调用
            log_platform_access_start(&api_path, &remote_addr, &ak);

            // 查找对应的客户端配置
            let client = find_client_by_access_key(&ak);

            match client {
                Some(client) => {
                    // 找到客户端配置，尝试完整认证（如果有签名信息）
                    let auth_info_result =
                        ThirdPartyAuthService::extract_auth_info(&headers, &params);

                    match auth_info_result {
                        Ok(auth_info) => {
                            // 有完整认证信息，进行签名验证（如果启用了签名验证）
                            if CONFIG.third_party_access.signature.required {
                                let auth_result = ThirdPartyAuthService::authenticate_client(
                                    &auth_info,
                                    &api_path,
                                    &remote_addr,
                                );

                                match auth_result {
                                    AuthResult::Success(authenticated_client) => {
                                        request
                                            .extensions_mut()
                                            .insert(authenticated_client.clone());
                                        log_full_auth_success(
                                            &api_path,
                                            &remote_addr,
                                            &authenticated_client,
                                            &ak,
                                        );
                                        return Ok(next.run(request).await);
                                    }
                                    AuthResult::Failed(error) => {
                                        log_auth_failure(
                                            &api_path,
                                            &remote_addr,
                                            "signature_verification_failed",
                                            error.message(),
                                        );
                                        return Err(map_auth_error_to_status(&error));
                                    }
                                }
                            } else {
                                // 签名验证未启用，仅做来源识别
                                let identified_client = super::client::AuthenticatedClient {
                                    client_id: client.client_id.clone(),
                                    client_name: client.name.clone(),
                                    source_type: client.source_type.clone(),
                                    permissions: client.permissions.clone(),
                                };

                                request.extensions_mut().insert(identified_client.clone());
                                log_identified_access_success(
                                    &api_path,
                                    &remote_addr,
                                    &identified_client,
                                    &ak,
                                );
                                return Ok(next.run(request).await);
                            }
                        }
                        Err(error) => {
                            // 强校验模式：要求签名时，缺少认证参数直接拒绝
                            if strict_auth && !is_dev_mode && CONFIG.third_party_access.signature.required
                            {
                                warn!(
                                    event = "third_party_access",
                                    mode = "secured",
                                    api_path = api_path,
                                    remote_addr = remote_addr,
                                    access_key = ak,
                                    reason = "missing_signature_parameters",
                                    error = error.message(),
                                    "[fail] 缺少签名参数，已启用 OCR_FORCE_THIRD_PARTY_AUTH，将拒绝访问"
                                );
                                return Err(StatusCode::UNAUTHORIZED);
                            }
                            // 仅有AK但没有完整签名信息，作为平台标识处理
                            let identified_client = super::client::AuthenticatedClient {
                                client_id: client.client_id.clone(),
                                client_name: client.name.clone(),
                                source_type: client.source_type.clone(),
                                permissions: client.permissions.clone(),
                            };

                            request.extensions_mut().insert(identified_client.clone());
                            log_identified_access_success(
                                &api_path,
                                &remote_addr,
                                &identified_client,
                                &ak,
                            );
                            return Ok(next.run(request).await);
                        }
                    }
                }
                None => {
                    // 有AK但不在配置中，记录为未知第三方
                    log_unknown_third_party(&api_path, &remote_addr, &ak);
                    // 强校验模式：未知AK直接拒绝
                    if strict_auth && !is_dev_mode {
                        warn!(
                            event = "third_party_access",
                            mode = "secured",
                            api_path = api_path,
                            remote_addr = remote_addr,
                            access_key = ak,
                            reason = "unknown_access_key",
                            "[fail] 未配置的AK，已启用 OCR_FORCE_THIRD_PARTY_AUTH，将拒绝访问"
                        );
                        return Err(StatusCode::UNAUTHORIZED);
                    }
                    return Ok(next.run(request).await);
                }
            }
        }
        None => {
            // 无AK标识 - 开放模式（直接API调用）
            if strict_auth && !is_dev_mode {
                warn!(
                    event = "third_party_access",
                    mode = "secured",
                    api_path = api_path,
                    remote_addr = remote_addr,
                    reason = "missing_access_key",
                    "[fail] 缺少 X-Access-Key/access_key，已启用 OCR_FORCE_THIRD_PARTY_AUTH，将拒绝访问"
                );
                return Err(StatusCode::UNAUTHORIZED);
            }
            log_open_access(&api_path, &remote_addr);
            Ok(next.run(request).await)
        }
    }
}

/// 提取远程地址
fn extract_remote_addr(request: &Request) -> String {
    request
        .headers()
        .get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string()
}

/// 提取访问密钥（仅AK，不要求完整签名信息）
fn extract_access_key(headers: &HeaderMap, params: &HashMap<String, String>) -> Option<String> {
    // 优先从请求头获取
    if let Some(ak) = headers.get("X-Access-Key").and_then(|v| v.to_str().ok()) {
        return Some(ak.to_string());
    }

    // 否则从查询参数获取
    if let Some(ak) = params.get("access_key") {
        return Some(ak.clone());
    }

    None
}

/// 根据AK查找客户端配置
fn find_client_by_access_key(
    access_key: &str,
) -> Option<&crate::util::config::types::ThirdPartyClient> {
    CONFIG
        .third_party_access
        .clients
        .iter()
        .find(|client| client.client_id == access_key && client.enabled)
}

/// 记录平台访问开始日志
fn log_platform_access_start(api_path: &str, remote_addr: &str, access_key: &str) {
    info!(
        event = "third_party_access",
        mode = "platform_identification",
        api_path = api_path,
        remote_addr = remote_addr,
        access_key = access_key,
        "[search] 检测到平台标识: {} AK {} 来自 {}",
        api_path,
        access_key,
        remote_addr
    );
}

/// 记录完整认证成功日志
fn log_full_auth_success(
    api_path: &str,
    remote_addr: &str,
    client: &super::client::AuthenticatedClient,
    access_key: &str,
) {
    info!(
        event = "third_party_access",
        mode = "full_authentication",
        api_path = api_path,
        remote_addr = remote_addr,
        client_id = &client.client_id,
        client_name = &client.client_name,
        source_type = &client.source_type,
        access_key = access_key,
        result = "success",
        "[ok] 完整认证通过: {} [lock] 签名验证 客户端 {} ({}) 来自 {} AK {}",
        api_path,
        client.client_name,
        client.client_id,
        remote_addr,
        access_key
    );
}

/// 记录标识访问成功日志（无签名验证）
fn log_identified_access_success(
    api_path: &str,
    remote_addr: &str,
    client: &super::client::AuthenticatedClient,
    access_key: &str,
) {
    let source_description = match client.source_type.as_str() {
        "platform_gateway" => "[global] 平台网关路由",
        _ => "[question] 第三方调用",
    };

    info!(
        event = "third_party_access",
        mode = "identification_only",
        api_path = api_path,
        remote_addr = remote_addr,
        client_id = &client.client_id,
        client_name = &client.client_name,
        source_type = &client.source_type,
        access_key = access_key,
        result = "success",
        "[ok] 标识识别成功: {} {} 客户端 {} ({}) 来自 {} AK {} [无签名验证]",
        api_path,
        source_description,
        client.client_name,
        client.client_id,
        remote_addr,
        access_key
    );
}

/// 记录未知第三方访问日志
fn log_unknown_third_party(api_path: &str, remote_addr: &str, access_key: &str) {
    warn!(
        event = "third_party_access",
        mode = "unknown_third_party",
        api_path = api_path,
        remote_addr = remote_addr,
        access_key = access_key,
        result = "allowed_but_unknown",
        "[warn] 未知第三方: {} 未配置的AK {} 来自 {} [允许访问但未识别]",
        api_path,
        access_key,
        remote_addr
    );
}

/// 记录开放访问日志
fn log_open_access(api_path: &str, remote_addr: &str) {
    info!(
        event = "third_party_access",
        mode = "open",
        api_path = api_path,
        remote_addr = remote_addr,
        result = "allowed",
        "[global] 开放模式访问: {} 来自 {}",
        api_path,
        remote_addr
    );
}

/// 记录安全模式访问开始日志
fn log_secured_access_start(api_path: &str, remote_addr: &str) {
    info!(
        event = "third_party_access",
        mode = "secured",
        api_path = api_path,
        remote_addr = remote_addr,
        "[lock] 安全模式访问控制开始: {} 来自 {}",
        api_path,
        remote_addr
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
        "[fail] 认证失败: {} 来自 {} - 原因: {} - 错误: {}",
        api_path,
        remote_addr,
        reason,
        error
    );
}

/// 记录频率限制超出日志
fn log_rate_limit_exceeded(
    api_path: &str,
    remote_addr: &str,
    client: &super::client::AuthenticatedClient,
    error: &str,
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
        "[fail] 频率限制超出: {} 客户端 {} ({}) 来自 {} 限制 {}/小时 - {}",
        api_path,
        client.client_name,
        client.client_id,
        remote_addr,
        CONFIG.third_party_access.rate_limiting.requests_per_hour,
        error
    );
}

/// 记录认证成功日志
fn log_auth_success(
    api_path: &str,
    remote_addr: &str,
    client: &super::client::AuthenticatedClient,
    access_key: &str,
) {
    let source_description = match client.source_type.as_str() {
        "platform_gateway" => "[global] 平台网关路由",
        "direct_api" => "[link] 直接API调用",
        _ => "[question] 未知来源",
    };

    info!(
        event = "third_party_access",
        mode = "secured",
        api_path = api_path,
        remote_addr = remote_addr,
        client_id = &client.client_id,
        client_name = &client.client_name,
        source_type = &client.source_type,
        access_key = access_key,
        result = "success",
        "[ok] 访问控制通过: {} {} 客户端 {} ({}) 来自 {} AK {}",
        api_path,
        source_description,
        client.client_name,
        client.client_id,
        remote_addr,
        access_key
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
    pub fn log_suspicious_activity(api_path: &str, remote_addr: &str, reason: &str, details: &str) {
        warn!(
            event = "security_audit",
            api_path = api_path,
            remote_addr = remote_addr,
            reason = reason,
            details = details,
            "[alert] 可疑活动: {} 来自 {} - 原因: {} - 详情: {}",
            api_path,
            remote_addr,
            reason,
            details
        );
    }

    /// 记录访问模式异常
    pub fn log_access_pattern_anomaly(client_id: &str, pattern_type: &str, description: &str) {
        warn!(
            event = "security_audit",
            client_id = client_id,
            pattern_type = pattern_type,
            description = description,
            "[warn] 访问模式异常: 客户端 {} - 类型: {} - 描述: {}",
            client_id,
            pattern_type,
            description
        );
    }

    /// 记录配置变更
    pub fn log_config_change(admin_user: Option<&str>, change_type: &str, details: &str) {
        info!(
            event = "security_audit",
            admin_user = admin_user,
            change_type = change_type,
            details = details,
            "[tool] 配置变更: 管理员 {} - 类型: {} - 详情: {}",
            admin_user.unwrap_or("system"),
            change_type,
            details
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
        self.extensions()
            .get::<super::client::AuthenticatedClient>()
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
    use super::super::client::AuthError;
    use super::*;

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
