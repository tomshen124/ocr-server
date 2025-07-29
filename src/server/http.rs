//! HTTP服务器设置模块
//! 负责创建和配置HTTP服务器

use crate::api::routes;
use crate::util::config::Config;
use crate::AppState;
use axum::Router;
use std::net::Ipv4Addr;
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use tracing::info;
use anyhow::Result;

/// HTTP服务器管理器
pub struct ServerManager;

impl ServerManager {
    /// 创建HTTP服务器
    pub async fn create_server(config: &Config, app_state: AppState) -> Result<HttpServer> {
        info!("🌐 创建HTTP服务器...");
        
        // 绑定监听地址
        let listener = Self::bind_listener(config.port).await?;
        let local_addr = listener.local_addr()?;
        
        // 创建路由
        let app_routes = Self::create_routes(app_state)?;
        
        // 添加监控路由（如果启用）
        #[cfg(feature = "monitoring")]
        let app_routes = Self::add_monitoring_routes(app_routes, config)?;
        
        info!("✅ HTTP服务器创建完成，监听地址: {}", local_addr);
        
        Ok(HttpServer {
            listener,
            app_routes,
            local_addr,
        })
    }

    /// 绑定监听端口
    async fn bind_listener(port: u16) -> Result<TcpListener> {
        info!("🔌 绑定监听端口: {}", port);
        
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).await
            .map_err(|e| anyhow::anyhow!("端口 {} 绑定失败: {}", port, e))?;
        
        Ok(listener)
    }

    /// 创建应用路由
    fn create_routes(app_state: AppState) -> Result<Router> {
        info!("🛣️ 创建应用路由...");
        Ok(routes(app_state))
    }

    /// 添加监控路由
    #[cfg(feature = "monitoring")]
    fn add_monitoring_routes(app_routes: Router, config: &Config) -> Result<Router> {
        if config.monitoring.enabled {
            info!("📊 添加监控路由...");
            // 这里可以添加监控相关的路由逻辑
            Ok(app_routes)
        } else {
            Ok(app_routes)
        }
    }

    /// 启动服务器
    pub async fn start_server(server: HttpServer) -> Result<()> {
        info!("🚀 启动HTTP服务器...");
        info!("Server started at {}", server.local_addr);
        
        // 启动服务器并处理优雅关闭
        axum::serve(server.listener, server.app_routes)
            .with_graceful_shutdown(Self::shutdown_signal())
            .await?;
        
        info!("HTTP服务器已关闭");
        Ok(())
    }

    /// 优雅关闭信号处理
    async fn shutdown_signal() {
        info!("等待关闭信号...");
        
        // 等待Ctrl+C信号
        if let Err(e) = ctrl_c().await {
            tracing::warn!("无法监听关闭信号: {}", e);
        }
        
        info!("收到关闭信号，正在关闭服务器...");
    }

    /// 验证服务器配置
    pub fn validate_server_config(config: &Config) -> Result<ServerConfigValidation> {
        let mut validation = ServerConfigValidation::new();
        
        // 验证端口
        if config.port == 0 {
            validation.add_error("端口不能为0");
        } else if config.port < 1024 {
            validation.add_warning("使用了特权端口，可能需要管理员权限");
        }
        
        // 验证主机配置
        if config.host.is_empty() {
            validation.add_error("主机配置不能为空");
        }
        
        // 验证会话配置
        if config.session_timeout <= 0 {
            validation.add_error("会话超时必须大于0");
        } else if config.session_timeout < 300 {
            validation.add_warning("会话超时时间较短，可能影响用户体验");
        }
        
        Ok(validation)
    }

    /// 获取服务器运行状态
    pub fn get_server_status() -> ServerStatus {
        ServerStatus {
            is_running: true,
            start_time: chrono::Utc::now(), // 实际应该存储真实启动时间
            uptime_seconds: 0, // 实际应该计算真实运行时间
            request_count: 0, // 实际应该从统计系统获取
            error_count: 0, // 实际应该从错误统计获取
        }
    }
}

/// HTTP服务器实例
pub struct HttpServer {
    listener: TcpListener,
    app_routes: Router,
    local_addr: std::net::SocketAddr,
}

/// 服务器配置验证结果
#[derive(Debug, Clone)]
pub struct ServerConfigValidation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ServerConfigValidation {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
        }
    }
    
    pub fn add_error(&mut self, message: &str) {
        self.errors.push(message.to_string());
    }
    
    pub fn add_warning(&mut self, message: &str) {
        self.warnings.push(message.to_string());
    }
    
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
    
    pub fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// 服务器运行状态
#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub is_running: bool,
    pub start_time: chrono::DateTime<chrono::Utc>,
    pub uptime_seconds: u64,
    pub request_count: u64,
    pub error_count: u64,
}

impl Default for ServerConfigValidation {
    fn default() -> Self {
        Self::new()
    }
}