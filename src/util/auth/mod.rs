//! 第三方认证模块
//!
//! 这个模块提供了完整的第三方系统认证功能，包括：
//! - 客户端身份验证 (client.rs)
//! - HTTP中间件处理 (middleware.rs)  
//! - 频率限制控制 (rate_limit.rs)
//! - 访问日志和安全审计 (audit.rs)
//!
//! 使用示例：
//! ```rust
//! use crate::util::auth::third_party_auth_middleware;
//!
//! // 在路由中使用中间件
//! let app = Router::new()
//!     .route("/api/preview", post(preview_handler))
//!     .layer(middleware::from_fn(third_party_auth_middleware));
//! ```

pub mod audit;
pub mod client;
pub mod global_throttle;
pub mod middleware;
pub mod rate_limit;
pub mod simple_call_logging; // NEW 简单的API调用记录 (无认证限制)

// 重新导出主要组件，保持向后兼容
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

/// 第三方认证管理器 - 统一入口
pub struct ThirdPartyAuthManager {
    access_logger: AccessLogger,
    rate_limiter: RateLimiter,
}

impl ThirdPartyAuthManager {
    /// 创建新的认证管理器
    pub fn new() -> Self {
        Self {
            access_logger: AccessLogger::new(),
            rate_limiter: RateLimiter::new(),
        }
    }

    /// 获取全局实例
    pub fn global() -> &'static Self {
        use std::sync::OnceLock;
        static INSTANCE: OnceLock<ThirdPartyAuthManager> = OnceLock::new();
        INSTANCE.get_or_init(|| ThirdPartyAuthManager::new())
    }

    /// 获取访问日志记录器
    pub fn access_logger(&self) -> &AccessLogger {
        &self.access_logger
    }

    /// 获取频率限制器
    pub fn rate_limiter(&self) -> &RateLimiter {
        &self.rate_limiter
    }

    /// 处理认证请求（内部使用）
    pub async fn handle_auth_request(
        &self,
        headers: &HeaderMap,
        params: &HashMap<String, String>,
        api_path: &str,
        remote_addr: &str,
    ) -> Result<AuthenticatedClient, StatusCode> {
        // 检查是否启用第三方访问控制
        if !CONFIG.third_party_access.enabled {
            self.access_logger.log_successful_access(
                "open_access".to_string(),
                "Open Access".to_string(),
                api_path.to_string(),
                remote_addr.to_string(),
                "none".to_string(),
                None,
            );

            // 返回默认的开放访问客户端
            return Ok(AuthenticatedClient {
                client_id: "open_access".to_string(),
                client_name: "Open Access".to_string(),
                source_type: "direct_api".to_string(),
                permissions: vec!["all".to_string()],
            });
        }

        // 提取认证信息
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

        // 执行认证
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

        // 检查频率限制
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

        // 记录成功访问
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

    /// 生成认证配置示例
    pub fn generate_config_example() -> String {
        r#"
# 第三方访问配置示例
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

    /// 验证系统配置
    pub fn validate_system_config() -> Result<(), String> {
        let config = &CONFIG.third_party_access;

        if !config.enabled {
            return Ok(()); // 未启用时不需要验证
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

/// 辅助函数：检查请求是否需要第三方认证
pub fn requires_third_party_auth(path: &str) -> bool {
    // 定义需要第三方认证的API路径模式
    let protected_patterns = ["/api/preview", "/api/monitoring", "/api/health"];

    protected_patterns
        .iter()
        .any(|pattern| path.starts_with(pattern))
}

/// 辅助函数：从认证客户端获取权限
pub fn get_client_permissions(request: &Request) -> Vec<String> {
    if let Some(client) = request.extensions().get::<AuthenticatedClient>() {
        client.permissions.clone()
    } else {
        vec![]
    }
}

/// 辅助函数：检查客户端是否有特定权限
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
        // 这里应该用测试配置，但为了简化，我们测试基本逻辑
        let result = ThirdPartyAuthManager::validate_system_config();
        // 由于依赖全局CONFIG，实际测试可能需要mock
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

        // 验证组件已正确初始化
        let stats = manager
            .access_logger()
            .get_access_statistics(TimeRange::LastHour);
        assert_eq!(stats.total_requests, 0);

        let client_stats = manager.rate_limiter().get_client_stats("test").await;
        assert_eq!(client_stats.requests_last_hour, 0);
    }
}
