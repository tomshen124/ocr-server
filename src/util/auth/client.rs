//! 第三方客户端认证模块
//! 负责客户端身份验证、权限检查和签名验证

use crate::util::config::ThirdPartyClient;
use crate::CONFIG;
use anyhow::Result;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use std::collections::HashMap;
use tracing::{error, info, warn};

/// 认证后的客户端信息
#[derive(Debug, Clone)]
pub struct AuthenticatedClient {
    pub client_id: String,
    pub client_name: String,
    pub source_type: String,      // "platform_gateway" | "direct_api"
    pub permissions: Vec<String>, // 保留向后兼容
}

/// API请求认证信息
#[derive(Debug, Deserialize, Clone)]
pub struct ApiAuthRequest {
    pub access_key: String,
    pub timestamp: String,
    pub signature: String,
    pub nonce: Option<String>,
}

/// 认证结果
#[derive(Debug, Clone)]
pub enum AuthResult {
    Success(AuthenticatedClient),
    Failed(AuthError),
}

/// 认证错误类型
#[derive(Debug, Clone)]
pub enum AuthError {
    MissingParameters(String),
    InvalidClient(String),
    InsufficientPermission(String),
    SignatureVerificationFailed(String),
    RateLimitExceeded(String),
    TimestampExpired(String),
}

impl AuthError {
    pub fn message(&self) -> &str {
        match self {
            AuthError::MissingParameters(msg) => msg,
            AuthError::InvalidClient(msg) => msg,
            AuthError::InsufficientPermission(msg) => msg,
            AuthError::SignatureVerificationFailed(msg) => msg,
            AuthError::RateLimitExceeded(msg) => msg,
            AuthError::TimestampExpired(msg) => msg,
        }
    }
}

/// 第三方认证服务
pub struct ThirdPartyAuthService;

impl ThirdPartyAuthService {
    /// 验证第三方客户端完整认证流程
    pub fn authenticate_client(
        auth_info: &ApiAuthRequest,
        api_path: &str,
        remote_addr: &str,
    ) -> AuthResult {
        info!(
            event = "third_party_auth",
            api_path = api_path,
            remote_addr = remote_addr,
            access_key = auth_info.access_key,
            "[lock] 开始第三方认证验证"
        );

        // 1. 验证客户端身份
        let client = match Self::verify_client_identity(auth_info) {
            Ok(client) => client,
            Err(error) => {
                warn!(
                    event = "third_party_auth",
                    api_path = api_path,
                    remote_addr = remote_addr,
                    access_key = auth_info.access_key,
                    error = error.message(),
                    "[fail] 客户端身份验证失败"
                );
                return AuthResult::Failed(error);
            }
        };

        // 2. 简化权限验证 - 只要AK/SK验证通过就可以调用预审接口
        // 不再需要复杂的权限检查，配了AK/SK就能调用
        info!(
            event = "third_party_auth",
            api_path = api_path,
            client_name = client.name,
            source_type = client.source_type,
            "[stats] 客户端来源类型: {} - {}",
            client.source_type,
            match client.source_type.as_str() {
                "platform_gateway" => "通过上架平台网关路由",
                "direct_api" => "直接API调用",
                _ => "未知来源类型",
            }
        );

        // 3. 验证签名（如果启用）
        if CONFIG.third_party_access.signature.required {
            if let Err(error) = Self::verify_signature(auth_info, &client.secret_key) {
                warn!(
                    event = "third_party_auth",
                    api_path = api_path,
                    remote_addr = remote_addr,
                    client_id = client.client_id,
                    client_name = client.name,
                    error = error.message(),
                    "[fail] 签名验证失败"
                );
                return AuthResult::Failed(error);
            }
        }

        info!(
            event = "third_party_auth",
            api_path = api_path,
            remote_addr = remote_addr,
            client_id = client.client_id,
            client_name = client.name,
            "[ok] 第三方认证成功"
        );

        AuthResult::Success(AuthenticatedClient {
            client_id: client.client_id.clone(),
            client_name: client.name.clone(),
            source_type: client.source_type.clone(),
            permissions: client.permissions.clone(),
        })
    }

    /// 验证客户端身份
    fn verify_client_identity(auth_info: &ApiAuthRequest) -> Result<ThirdPartyClient, AuthError> {
        let clients = &CONFIG.third_party_access.clients;

        for client in clients {
            if client.client_id == auth_info.access_key {
                if !client.enabled {
                    return Err(AuthError::InvalidClient("客户端已被禁用".to_string()));
                }
                return Ok(client.clone());
            }
        }

        Err(AuthError::InvalidClient("无效的访问密钥".to_string()))
    }

    /// 验证HMAC-SHA256签名
    fn verify_signature(auth_info: &ApiAuthRequest, secret_key: &str) -> Result<(), AuthError> {
        // 检查时间戳有效性
        let request_time = DateTime::parse_from_rfc3339(&auth_info.timestamp)
            .map_err(|_| AuthError::TimestampExpired("无效的时间戳格式".to_string()))?;

        let now = Utc::now();
        let time_diff = (now.timestamp() - request_time.timestamp()).abs();

        if time_diff > CONFIG.third_party_access.signature.timestamp_tolerance as i64 {
            return Err(AuthError::TimestampExpired("请求时间戳已过期".to_string()));
        }

        // 构建签名字符串
        let sign_string = format!(
            "access_key={}&timestamp={}&nonce={}",
            auth_info.access_key,
            auth_info.timestamp,
            auth_info.nonce.as_deref().unwrap_or("")
        );

        // 计算HMAC-SHA256签名
        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
            .map_err(|_| AuthError::SignatureVerificationFailed("无效的密钥".to_string()))?;

        mac.update(sign_string.as_bytes());
        let expected_signature = hex::encode(mac.finalize().into_bytes());

        if expected_signature.to_lowercase() != auth_info.signature.to_lowercase() {
            return Err(AuthError::SignatureVerificationFailed(
                "签名验证失败".to_string(),
            ));
        }

        Ok(())
    }

    /// 从请求中提取认证信息
    pub fn extract_auth_info(
        headers: &axum::http::HeaderMap,
        params: &HashMap<String, String>,
    ) -> Result<ApiAuthRequest, AuthError> {
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
                nonce: headers
                    .get("X-Nonce")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string()),
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

        Err(AuthError::MissingParameters(
            "缺少必要的认证参数".to_string(),
        ))
    }

    /// 生成API签名示例（供第三方系统参考）
    pub fn generate_signature_example(
        access_key: &str,
        secret_key: &str,
        nonce: Option<&str>,
    ) -> String {
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

    /// 验证客户端配置有效性 - 简化版本
    pub fn validate_client_config(client: &ThirdPartyClient) -> Result<(), String> {
        if client.client_id.is_empty() {
            return Err("客户端ID不能为空".to_string());
        }

        if client.secret_key.is_empty() {
            return Err("客户端密钥不能为空".to_string());
        }

        if client.name.is_empty() {
            return Err("客户端名称不能为空".to_string());
        }

        // 验证来源类型
        if !["platform_gateway", "direct_api"].contains(&client.source_type.as_str()) {
            return Err(format!(
                "无效的来源类型: {}，应为 'platform_gateway' 或 'direct_api'",
                client.source_type
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_generation() {
        let access_key = "test_key";
        let secret_key = "test_secret";
        let nonce = Some("test_nonce");

        let signature =
            ThirdPartyAuthService::generate_signature_example(access_key, secret_key, nonce);

        assert!(!signature.is_empty());
        assert_eq!(signature.len(), 64); // HMAC-SHA256 hex长度
    }

    #[test]
    fn test_client_config_validation() {
        let mut client = ThirdPartyClient {
            client_id: "test_client".to_string(),
            secret_key: "test_secret".to_string(),
            name: "Test Client".to_string(),
            source_type: "direct_api".to_string(),
            enabled: true,
            permissions: vec![], // 权限现在是可选的
        };

        // 有效配置
        assert!(ThirdPartyAuthService::validate_client_config(&client).is_ok());

        // 测试平台网关类型
        client.source_type = "platform_gateway".to_string();
        assert!(ThirdPartyAuthService::validate_client_config(&client).is_ok());

        // 无效来源类型
        client.source_type = "invalid_type".to_string();
        assert!(ThirdPartyAuthService::validate_client_config(&client).is_err());

        // 空客户端ID
        client.source_type = "direct_api".to_string();
        client.client_id = "".to_string();
        assert!(ThirdPartyAuthService::validate_client_config(&client).is_err());
    }
}
