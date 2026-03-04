//!
//!
//! ```rust
//! use crate::util::auth::third_party_auth_middleware;
//!
//! let app = Router::new()
//!     .route("/api/preview", post(preview_handler))
//!     .layer(middleware::from_fn(third_party_auth_middleware));
//! ```

pub mod audit;
pub mod client;
pub mod global_throttle;
pub mod middleware;
pub mod rate_limit;
pub mod simple_call_logging;

pub use audit::{
    AccessLogger, AccessRecord, SecurityEvent, SecurityEventType, SecuritySeverity, TimeRange,
};
pub use client::{
    ApiAuthRequest, AuthError, AuthResult, AuthenticatedClient, ThirdPartyAuthService,
};
pub use middleware::{third_party_auth_middleware, RequestExt, SecurityAuditor};
pub use rate_limit::{
    ClientUsageStats, RateLimiter, SlidingWindowRateLimiter, TokenBucketRateLimiter,
};
pub use simple_call_logging::{get_api_call_stats, get_recent_api_calls};

use crate::CONFIG;
use axum::{
    extract::{Query, Request},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use std::collections::HashMap;
use tracing::{info, warn};

pub struct ThirdPartyAuthManager {
    access_logger: AccessLogger,
    rate_limiter: RateLimiter,
}

impl ThirdPartyAuthManager {
    pub fn new() -> Self {
        Self {
            access_logger: AccessLogger::new(),
            rate_limiter: RateLimiter::new(),
        }
    }

    pub fn global() -> &'static Self {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<ThirdPartyAuthManager> = OnceLock::new();
        INSTANCE.get_or_init(|| ThirdPartyAuthManager::new())
    }

    pub fn access_logger(&self) -> &AccessLogger {
        &self.access_logger
    }

    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }

    pub async fn handle_auth_request(
        &self,
        headers: &HeaderMap,
        params: &HashMap<String, String>,
        api_path: &str,
        remote_addr: &str,
    ) -> Result<AuthenticatedClient, StatusCode> {
        if !CONFIG.third_party_access.enabled {
            self.access_logger.log_successful_access(
                "open_access".to_string(),
                "Open Access".to_string(),
                api_path.to_string(),
                remote_addr.to_string(),
                "none".to_string(),
                None,
            );

            return Ok(AuthenticatedClient {
                client_id: "open_access".to_string(),
                client_name: "Open Access".to_string(),
                source_type: "direct_api".to_string(),
                permissions: vec!["all".to_string()],
            });
        }

        let auth_info =
            ThirdPartyAuthService::extract_auth_info(headers, params).map_err(|error| {
                self.access_logger.log_failed_access(
                    None,
                    api_path.to_string(),
                    remote_addr.to_string(),
                    None,
                    error.message().to_string(),
                    audit::AccessResult::AuthFailed(error.message().to_string()),
                );
                StatusCode::UNAUTHORIZED
            })?;

        let auth_result =
            ThirdPartyAuthService::authenticate_client(&auth_info, api_path, remote_addr);

        let authenticated_client = match auth_result {
            AuthResult::Success(client) => client,
            AuthResult::Failed(error) => {
                let access_result = match error {
                    AuthError::InvalidClient(_) => {
                        audit::AccessResult::AuthFailed(error.message().to_string())
                    }
                    AuthError::InsufficientPermission(_) => {
                        audit::AccessResult::PermissionDenied(error.message().to_string())
                    }
                    AuthError::RateLimitExceeded(_) => {
                        audit::AccessResult::RateLimited(error.message().to_string())
                    }
                    _ => audit::AccessResult::AuthFailed(error.message().to_string()),
                };

                self.access_logger.log_failed_access(
                    Some(auth_info.access_key.clone()),
                    api_path.to_string(),
                    remote_addr.to_string(),
                    Some(auth_info.access_key),
                    error.message().to_string(),
                    access_result,
                );

                return Err(match error {
                    AuthError::MissingParameters(_) => StatusCode::BAD_REQUEST,
                    AuthError::InvalidClient(_) => StatusCode::UNAUTHORIZED,
                    AuthError::InsufficientPermission(_) => StatusCode::FORBIDDEN,
                    AuthError::SignatureVerificationFailed(_) => StatusCode::UNAUTHORIZED,
                    AuthError::RateLimitExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
                    AuthError::TimestampExpired(_) => StatusCode::UNAUTHORIZED,
                });
            }
        };

        if CONFIG.third_party_access.rate_limiting.enabled {
            if let Err(error) = RateLimiter::check_rate_limit(
                &authenticated_client.client_id,
                CONFIG.third_party_access.rate_limiting.requests_per_hour,
            )
            .await
            {
                self.access_logger.log_failed_access(
                    Some(authenticated_client.client_id.clone()),
                    api_path.to_string(),
                    remote_addr.to_string(),
                    Some(auth_info.access_key),
                    error.clone(),
                    audit::AccessResult::RateLimited(error),
                );
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
        }

        self.access_logger.log_successful_access(
            authenticated_client.client_id.clone(),
            authenticated_client.client_name.clone(),
            api_path.to_string(),
            remote_addr.to_string(),
            auth_info.access_key,
            None,
        );

        Ok(authenticated_client)
    }

    pub fn generate_config_example() -> String {
        r#"
third_party_access:
  enabled: true  # 启用第三方访问控制
  
  clients:
    - client_id: "example_client_001"
      secret_key: "your_secret_key_here_change_in_production"
      name: "示例客户端"
      enabled: true
      permissions:
        - "preview"  # 预审权限
        - "status"   # 状态查询权限
        - "view"     # 查看权限
        
  signature:
    required: true           # 是否要求签名验证
    timestamp_tolerance: 300 # 时间戳容差（秒）
    
  rate_limiting:
    enabled: true
    requests_per_minute: 100  # 每分钟请求限制
    requests_per_hour: 1000   # 每小时请求限制
"#
        .to_string()
    }

    pub fn validate_system_config() -> Result<(), String> {
        let config = &CONFIG.third_party_access;

        if !config.enabled {
            return Ok(());
        }

        if config.clients.is_empty() {
            return Err("启用了第三方访问但未配置任何客户端".to_string());
        }

        for (index, client) in config.clients.iter().enumerate() {
            if let Err(error) = ThirdPartyAuthService::validate_client_config(client) {
                return Err(format!("客户端 {} 配置无效: {}", index, error));
            }
        }

        if config.signature.required && config.signature.timestamp_tolerance == 0 {
            return Err("启用签名验证但时间戳容差为0".to_string());
        }

        if config.rate_limiting.enabled {
            if config.rate_limiting.requests_per_minute == 0
                && config.rate_limiting.requests_per_hour == 0
            {
                return Err("启用频率限制但所有限制值都为0".to_string());
            }
        }

        Ok(())
    }
}

impl Default for ThirdPartyAuthManager {
    fn default() -> Self {
        Self::new()
    }
}

pub fn requires_third_party_auth(path: &str) -> bool {
    let protected_patterns = ["/api/preview", "/api/monitoring", "/api/health"];

    protected_patterns
        .iter()
        .any(|pattern| path.starts_with(pattern))
}

pub fn get_client_permissions(request: &Request) -> Vec<String> {
    if let Some(client) = request.extensions().get::<AuthenticatedClient>() {
        client.permissions.clone()
    } else {
        vec![]
    }
}

pub fn has_permission(request: &Request, permission: &str) -> bool {
    if let Some(client) = request.extensions().get::<AuthenticatedClient>() {
        client.permissions.contains(&permission.to_string())
            || client.permissions.contains(&"all".to_string())
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_config_validation() {
        let result = ThirdPartyAuthManager::validate_system_config();
        println!("Config validation result: {:?}", result);
    }

    #[test]
    fn test_auth_requirements() {
        assert!(requires_third_party_auth("/api/preview/submit"));
        assert!(requires_third_party_auth("/api/monitoring/status"));
        assert!(!requires_third_party_auth("/static/index.html"));
        assert!(!requires_third_party_auth("/health"));
    }

    #[test]
    fn test_config_example_generation() {
        let example = ThirdPartyAuthManager::generate_config_example();
        assert!(example.contains("third_party_access"));
        assert!(example.contains("clients"));
        assert!(example.contains("signature"));
        assert!(example.contains("rate_limiting"));
    }

    #[tokio::test]
    async fn test_auth_manager_creation() {
        let manager = ThirdPartyAuthManager::new();

        let stats = manager
            .access_logger()
            .get_access_statistics(TimeRange::LastHour);
        assert_eq!(stats.total_requests, 0);

        let client_stats = manager.rate_limiter().get_client_stats("test").await;
        assert_eq!(client_stats.requests_last_hour, 0);
    }
}
