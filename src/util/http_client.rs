//! HTTP客户端模块 - 支持依赖注入和配置管理
//!
//! 提供可配置的HTTP客户端，支持：
//! - 超时配置
//! - 代理设置
//! - TLS配置
//! - 连接池管理
//! - 重试机制

use anyhow::{Context, Result};
use std::time::Duration;
use tracing::{info, warn};

#[cfg(feature = "reqwest")]
use reqwest::Client;

/// HTTP客户端配置
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// 请求超时时间（秒）
    pub timeout_secs: u64,
    /// 连接超时时间（秒）
    pub connect_timeout_secs: u64,
    /// 是否接受无效证书（仅开发环境）
    pub danger_accept_invalid_certs: bool,
    /// User-Agent
    pub user_agent: String,
    /// TCP keepalive时间（秒）
    pub tcp_keepalive_secs: u64,
    /// 连接池空闲超时（秒）
    pub pool_idle_timeout_secs: u64,
    /// 每个主机的最大空闲连接数
    pub pool_max_idle_per_host: usize,
    /// HTTP代理URL（可选）
    pub http_proxy: Option<String>,
    /// HTTPS代理URL（可选）
    pub https_proxy: Option<String>,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            timeout_secs: 60,
            connect_timeout_secs: 30,
            danger_accept_invalid_certs: true, // 默认接受，由nginx处理SSL
            user_agent: "OCR-Preview-Service/1.0".to_string(),
            tcp_keepalive_secs: 60,
            pool_idle_timeout_secs: 90,
            pool_max_idle_per_host: 10,
            http_proxy: None,
            https_proxy: None,
        }
    }
}

impl HttpClientConfig {
    /// 从环境变量加载代理配置
    pub fn with_env_proxy(mut self) -> Self {
        if let Ok(proxy_url) = std::env::var("HTTP_PROXY") {
            self.http_proxy = Some(proxy_url);
        }
        if let Ok(proxy_url) = std::env::var("HTTPS_PROXY") {
            self.https_proxy = Some(proxy_url);
        }
        self
    }
}

/// HTTP客户端包装器
#[derive(Clone)]
pub struct HttpClient {
    #[cfg(feature = "reqwest")]
    client: Option<Client>,
    config: HttpClientConfig,
}

impl HttpClient {
    /// 创建新的HTTP客户端
    pub fn new(config: HttpClientConfig) -> Result<Self> {
        #[cfg(feature = "reqwest")]
        {
            let client = Self::build_reqwest_client(&config)?;
            Ok(Self {
                client: Some(client),
                config,
            })
        }

        #[cfg(not(feature = "reqwest"))]
        {
            warn!("HTTP客户端功能在当前编译配置下未启用");
            Ok(Self { config })
        }
    }

    /// 创建默认HTTP客户端
    pub fn default_client() -> Result<Self> {
        let config = HttpClientConfig::default().with_env_proxy();
        Self::new(config)
    }

    #[cfg(feature = "reqwest")]
    fn build_reqwest_client(config: &HttpClientConfig) -> Result<Client> {
        let mut client_builder = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .danger_accept_invalid_certs(config.danger_accept_invalid_certs)
            .user_agent(&config.user_agent)
            .tcp_keepalive(Duration::from_secs(config.tcp_keepalive_secs))
            .pool_idle_timeout(Duration::from_secs(config.pool_idle_timeout_secs))
            .pool_max_idle_per_host(config.pool_max_idle_per_host);

        // 配置HTTP代理
        if let Some(proxy_url) = &config.http_proxy {
            if let Ok(proxy) = reqwest::Proxy::http(proxy_url) {
                info!("使用HTTP代理: {}", proxy_url);
                client_builder = client_builder.proxy(proxy);
            } else {
                warn!("HTTP代理配置无效: {}", proxy_url);
            }
        }

        // 配置HTTPS代理
        if let Some(proxy_url) = &config.https_proxy {
            if let Ok(proxy) = reqwest::Proxy::https(proxy_url) {
                info!("使用HTTPS代理: {}", proxy_url);
                client_builder = client_builder.proxy(proxy);
            } else {
                warn!("HTTPS代理配置无效: {}", proxy_url);
            }
        }

        client_builder.build().context("构建HTTP客户端失败")
    }

    /// 获取底层reqwest客户端（如果可用）
    #[cfg(feature = "reqwest")]
    pub fn reqwest_client(&self) -> Result<&Client> {
        self.client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("HTTP客户端不可用"))
    }

    /// 检查HTTP客户端是否可用
    pub fn is_available(&self) -> bool {
        #[cfg(feature = "reqwest")]
        {
            self.client.is_some()
        }

        #[cfg(not(feature = "reqwest"))]
        {
            false
        }
    }

    /// 获取配置
    pub fn config(&self) -> &HttpClientConfig {
        &self.config
    }

    /// 使用新配置重建客户端
    pub fn rebuild_with_config(&mut self, config: HttpClientConfig) -> Result<()> {
        #[cfg(feature = "reqwest")]
        {
            let new_client = Self::build_reqwest_client(&config)?;
            self.client = Some(new_client);
            self.config = config;
            info!("HTTP客户端已使用新配置重建");
            Ok(())
        }

        #[cfg(not(feature = "reqwest"))]
        {
            self.config = config;
            warn!("HTTP客户端功能未启用，仅更新配置");
            Ok(())
        }
    }
}

impl std::fmt::Debug for HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpClient")
            .field("available", &self.is_available())
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HttpClientConfig::default();
        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.connect_timeout_secs, 30);
        assert_eq!(config.pool_max_idle_per_host, 10);
    }

    #[test]
    fn test_env_proxy() {
        std::env::set_var("HTTP_PROXY", "http://proxy.example.com:8080");
        let config = HttpClientConfig::default().with_env_proxy();
        assert_eq!(
            config.http_proxy,
            Some("http://proxy.example.com:8080".to_string())
        );
        std::env::remove_var("HTTP_PROXY");
    }

    #[cfg(feature = "reqwest")]
    #[test]
    fn test_client_creation() {
        let config = HttpClientConfig::default();
        let client = HttpClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_availability() {
        let client = HttpClient::default_client().unwrap();
        #[cfg(feature = "reqwest")]
        assert!(client.is_available());
        #[cfg(not(feature = "reqwest"))]
        assert!(!client.is_available());
    }
}
