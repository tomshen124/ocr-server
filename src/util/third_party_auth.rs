use axum::{
    extract::{Request, Query}, 
    middleware::Next, 
    response::Response,
    http::{StatusCode, HeaderMap}
};
use serde::Deserialize;
use std::collections::HashMap;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use chrono::{Utc, DateTime};
use crate::CONFIG;
use crate::util::config::ThirdPartyClient;

/// 认证后的客户端信息
#[derive(Debug, Clone)]
pub struct AuthenticatedClient {
    pub client_id: String,
    pub client_name: String,
    pub permissions: Vec<String>,
}

/// API请求认证信息
#[derive(Debug, Deserialize)]
pub struct ApiAuthRequest {
    pub access_key: String,
    pub timestamp: String,
    pub signature: String,
    pub nonce: Option<String>,
}

/// 第三方系统认证中间件
/// 根据配置决定是否启用验证，无论哪种模式都记录详细访问日志
pub async fn third_party_auth_middleware(
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    
    let api_path = request.uri().path().to_string();
    let remote_addr = request.headers().get("x-forwarded-for")
        .or_else(|| request.headers().get("x-real-ip"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
        
    // 检查是否启用第三方访问控制
    if !CONFIG.third_party_access.enabled {
        // 未启用访问控制，记录开放访问日志
        tracing::info!(
            event = "third_party_access",
            mode = "open",
            api_path = &api_path,
            remote_addr = remote_addr,
            result = "allowed",
            "🌐 开放模式访问: {} 来自 {}", api_path, remote_addr
        );
        return Ok(next.run(request).await);
    }
    
    tracing::info!(
        event = "third_party_access",
        mode = "secured",
        api_path = &api_path,
        remote_addr = remote_addr,
        "🔐 安全模式访问控制开始: {} 来自 {}", api_path, remote_addr
    );
    
    // 从请求中提取认证信息
    let auth_info = extract_auth_info(&headers, &params)
        .map_err(|e| {
            tracing::warn!(
                event = "third_party_access",
                mode = "secured",
                api_path = api_path,
                remote_addr = remote_addr,
                result = "failed",
                reason = "missing_auth_params",
                error = e,
                "❌ 认证参数缺失: {} 来自 {} - {}", api_path, remote_addr, e
            );
            StatusCode::UNAUTHORIZED
        })?;
    
    // 验证第三方客户端身份
    let client = verify_third_party_client(&auth_info)
        .map_err(|e| {
            tracing::warn!(
                event = "third_party_access",
                mode = "secured",
                api_path = api_path,
                remote_addr = remote_addr,
                access_key = auth_info.access_key,
                result = "failed",
                reason = "invalid_client",
                error = e,
                "❌ 客户端验证失败: {} 使用AK {} 来自 {} - {}", api_path, auth_info.access_key, remote_addr, e
            );
            StatusCode::UNAUTHORIZED
        })?;
    
    // 验证API访问权限
    if !verify_api_permission(&client, &api_path) {
        tracing::warn!(
            event = "third_party_access",
            mode = "secured",
            api_path = api_path,
            remote_addr = remote_addr,
            client_id = client.client_id,
            client_name = client.name,
            permissions = ?client.permissions,
            result = "failed",
            reason = "insufficient_permission",
            "❌ API权限不足: {} 客户端 {} ({}) 来自 {} 无权访问", api_path, client.name, client.client_id, remote_addr
        );
        return Err(StatusCode::FORBIDDEN);
    }
    
    // 验证签名（如果启用）
    if CONFIG.third_party_access.signature.required {
        verify_signature(&auth_info, &client.secret_key)
            .map_err(|e| {
                tracing::warn!(
                    event = "third_party_access",
                    mode = "secured",
                    api_path = api_path,
                    remote_addr = remote_addr,
                    client_id = client.client_id,
                    client_name = client.name,
                    result = "failed",
                    reason = "signature_verification_failed",
                    error = e,
                    "❌ 签名验证失败: {} 客户端 {} ({}) 来自 {} - {}", api_path, client.name, client.client_id, remote_addr, e
                );
                StatusCode::UNAUTHORIZED
            })?;
    }
    
    // 检查频率限制（如果启用）
    if CONFIG.third_party_access.rate_limiting.enabled {
        check_rate_limit(&client.client_id, CONFIG.third_party_access.rate_limiting.requests_per_hour).await
            .map_err(|e| {
                tracing::warn!(
                    event = "third_party_access",
                    mode = "secured",
                    api_path = &api_path,
                    remote_addr = remote_addr,
                    client_id = &client.client_id,
                    client_name = &client.name,
                    rate_limit = CONFIG.third_party_access.rate_limiting.requests_per_hour,
                    result = "failed",
                    reason = "rate_limit_exceeded",
                    error = e,
                    "❌ 频率限制超出: {} 客户端 {} ({}) 来自 {} 限制 {}/小时 - {}", api_path, client.name, client.client_id, remote_addr, CONFIG.third_party_access.rate_limiting.requests_per_hour, e
                );
                StatusCode::TOO_MANY_REQUESTS
            })?;
    }
    
    // 将认证信息添加到请求扩展中
    request.extensions_mut().insert(AuthenticatedClient {
        client_id: client.client_id.clone(),
        client_name: client.name.clone(),
        permissions: client.permissions.clone(),
    });
    
    // 记录成功的访问日志
    tracing::info!(
        event = "third_party_access",
        mode = "secured",
        api_path = api_path,
        remote_addr = remote_addr,
        client_id = client.client_id,
        client_name = &client.name,
        access_key = auth_info.access_key,
        result = "success",
        "✅ 访问控制通过: {} 客户端 {} ({}) 来自 {} AK {}", api_path, client.name, client.client_id, remote_addr, auth_info.access_key
    );
    
    Ok(next.run(request).await)
}

/// 从请求中提取认证信息
fn extract_auth_info(
    headers: &HeaderMap,
    params: &HashMap<String, String>
) -> Result<ApiAuthRequest, &'static str> {
    
    // 优先从请求头获取（推荐方式）
    if let (Some(access_key), Some(timestamp), Some(signature)) = (
        headers.get("X-Access-Key").and_then(|v| v.to_str().ok()),
        headers.get("X-Timestamp").and_then(|v| v.to_str().ok()),
        headers.get("X-Signature").and_then(|v| v.to_str().ok()),
    ) {
        return Ok(ApiAuthRequest {
            access_key: access_key.to_string(),
            timestamp: timestamp.to_string(),
            signature: signature.to_string(),
            nonce: headers.get("X-Nonce").and_then(|v| v.to_str().ok()).map(|s| s.to_string()),
        });
    }
    
    // 否则从查询参数获取（兼容方式）
    if let (Some(access_key), Some(timestamp), Some(signature)) = (
        params.get("access_key"),
        params.get("timestamp"),
        params.get("signature"),
    ) {
        return Ok(ApiAuthRequest {
            access_key: access_key.clone(),
            timestamp: timestamp.clone(),
            signature: signature.clone(),
            nonce: params.get("nonce").cloned(),
        });
    }
    
    Err("缺少必要的认证参数")
}

/// 验证第三方客户端身份
fn verify_third_party_client(auth_info: &ApiAuthRequest) -> Result<ThirdPartyClient, &'static str> {
    
    // 从配置中查找客户端
    let clients = &CONFIG.third_party_access.clients;
    
    for client in clients {
        if client.client_id == auth_info.access_key {
            if !client.enabled {
                return Err("客户端已被禁用");
            }
            return Ok(client.clone());
        }
    }
    
    Err("无效的访问密钥")
}

/// 验证API访问权限
fn verify_api_permission(client: &ThirdPartyClient, api_path: &str) -> bool {
    
    // 从路径中提取API类型
    let api_type = if api_path.starts_with("/api/preview/status") {
        "status"
    } else if api_path.starts_with("/api/preview/view") {
        "view"
    } else if api_path.starts_with("/api/preview") {
        "preview"
    } else {
        return false;  // 未知或不受控制的API
    };
    
    client.permissions.contains(&api_type.to_string())
}

/// 验证HMAC-SHA256签名
fn verify_signature(auth_info: &ApiAuthRequest, secret_key: &str) -> Result<(), &'static str> {
    
    // 检查时间戳有效性
    let request_time = DateTime::parse_from_rfc3339(&auth_info.timestamp)
        .map_err(|_| "无效的时间戳格式")?;
    
    let now = Utc::now();
    let time_diff = (now.timestamp() - request_time.timestamp()).abs();
    
    if time_diff > CONFIG.third_party_access.signature.timestamp_tolerance as i64 {
        return Err("请求时间戳已过期");
    }
    
    // 构建签名字符串（标准格式）
    let sign_string = format!(
        "access_key={}&timestamp={}&nonce={}",
        auth_info.access_key,
        auth_info.timestamp,
        auth_info.nonce.as_deref().unwrap_or("")
    );
    
    // 计算HMAC-SHA256签名
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .map_err(|_| "无效的密钥")?;
    
    mac.update(sign_string.as_bytes());
    let expected_signature = hex::encode(mac.finalize().into_bytes());
    
    if expected_signature.to_lowercase() != auth_info.signature.to_lowercase() {
        return Err("签名验证失败");
    }
    
    Ok(())
}

/// 检查频率限制
async fn check_rate_limit(client_id: &str, limit: u32) -> Result<(), &'static str> {
    // TODO: 实现基于Redis或内存的频率限制逻辑
    // 这里可以使用sliding window或token bucket算法
    
    tracing::info!("频率限制检查: {} (限制: {}/小时)", client_id, limit);
    
    // 暂时简化为总是通过
    Ok(())
}

/// 生成API签名的示例函数（供第三方系统参考）
pub fn generate_signature_example(access_key: &str, secret_key: &str, nonce: Option<&str>) -> String {
    let timestamp = Utc::now().to_rfc3339();
    let sign_string = format!(
        "access_key={}&timestamp={}&nonce={}",
        access_key,
        timestamp,
        nonce.unwrap_or("")
    );
    
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes()).unwrap();
    mac.update(sign_string.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_signature_generation() {
        let access_key = "test_key";
        let secret_key = "test_secret";
        let nonce = Some("test_nonce");
        
        let signature = generate_signature_example(access_key, secret_key, nonce);
        assert!(!signature.is_empty());
        assert_eq!(signature.len(), 64); // HMAC-SHA256 hex长度
    }
} 