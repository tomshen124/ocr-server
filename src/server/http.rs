
use crate::api::routes;
use crate::util::config::Config;
use crate::AppState;
use anyhow::Result;
use axum::Router;
use std::net::Ipv4Addr;
use tokio::net::TcpListener;
use tokio::signal::{
    ctrl_c,
    unix::{signal, SignalKind},
};
use tracing::{error, info, warn};

pub struct ServerManager;

impl ServerManager {
    pub async fn create_server(config: &Config, app_state: AppState) -> Result<HttpServer> {
        info!(
            target: "server.http",
            event = "http.server.create",
            "创建HTTP服务器"
        );

        let listener = Self::bind_listener(config.get_port()).await?;
        let local_addr = listener.local_addr()?;

        let app_routes = Self::create_routes(app_state)?;

        #[cfg(feature = "monitoring")]
        let app_routes = Self::add_monitoring_routes(app_routes, config)?;

        info!(
            target: "server.http",
            event = "http.server.ready",
            address = %local_addr
        );

        Ok(HttpServer {
            listener,
            app_routes,
            local_addr,
        })
    }

    async fn bind_listener(port: u16) -> Result<TcpListener> {
        info!(
            target: "server.http",
            event = "http.server.bind_start",
            port
        );
        let v6_addr = format!("[::]:{}", port);
        match TcpListener::bind(&v6_addr).await {
            Ok(listener) => {
                info!(
                    target: "server.http",
                    event = "http.server.bound",
                    protocol = "ipv6",
                    address = %v6_addr
                );
                Ok(listener)
            }
            Err(e6) => {
                warn!("IPv6绑定失败: {}，尝试IPv4", e6);
                let v4_addr = format!("0.0.0.0:{}", port);
                let listener = TcpListener::bind(&v4_addr).await.map_err(|e4| {
                    anyhow::anyhow!(
                        "端口 {} 绑定失败 (IPv4): {}；之前IPv6错误: {}",
                        port,
                        e4,
                        e6
                    )
                })?;
                info!(
                    target: "server.http",
                    event = "http.server.bound",
                    protocol = "ipv4",
                    address = %v4_addr
                );
                Ok(listener)
            }
        }
    }

    fn create_routes(app_state: AppState) -> Result<Router> {
        info!(
            target: "server.http",
            event = "http.router.build"
        );
        Ok(routes(app_state))
    }

    #[cfg(feature = "monitoring")]
    fn add_monitoring_routes(app_routes: Router, config: &Config) -> Result<Router> {
        if config.monitoring.enabled {
            info!(
                target: "server.http",
                event = "http.router.monitoring"
            );
            Ok(app_routes)
        } else {
            Ok(app_routes)
        }
    }

    pub async fn start_server(server: HttpServer) -> Result<()> {
        info!(
            target: "server.http",
            event = "http.server.start",
            address = %server.local_addr
        );

        axum::serve(server.listener, server.app_routes)
            .with_graceful_shutdown(Self::shutdown_signal())
            .await?;

        info!("HTTP服务器已关闭");
        Ok(())
    }

    async fn shutdown_signal() {
        info!(
            target: "server.http",
            event = "http.server.shutdown_wait"
        );

        tokio::select! {
            _ = ctrl_c() => {
                info!(
                    target: "server.http",
                    event = "http.server.signal",
                    signal = "SIGINT"
                );
            }
            _ = Self::wait_for_sigterm() => {
                info!(
                    target: "server.http",
                    event = "http.server.signal",
                    signal = "SIGTERM"
                );
            }
            _ = Self::wait_for_sighup() => {
                warn!(
                    target: "server.http",
                    event = "http.server.signal",
                    signal = "SIGHUP",
                    "暂不支持配置重载，准备退出"
                );
            }
        }

        info!(
            target: "server.http",
            event = "http.server.shutdown_begin"
        );
        info!(
            target: "server.http",
            event = "http.server.cleanup"
        );


        info!(
            target: "server.http",
            event = "http.server.shutdown_ready"
        );
    }

    async fn wait_for_sigterm() -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(unix)]
        {
            let mut term_signal = signal(SignalKind::terminate())?;
            term_signal.recv().await;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    async fn wait_for_sighup() -> Result<(), Box<dyn std::error::Error>> {
        #[cfg(unix)]
        {
            let mut hup_signal = signal(SignalKind::hangup())?;
            hup_signal.recv().await;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            std::future::pending::<()>().await;
            Ok(())
        }
    }

    pub fn validate_server_config(config: &Config) -> Result<ServerConfigValidation> {
        let mut validation = ServerConfigValidation::new();

        if config.get_port() == 0 {
            validation.add_error("端口不能为0");
        } else if config.get_port() < 1024 {
            validation.add_warning("使用了特权端口，可能需要管理员权限");
        }

        let base_url = config.base_url();
        if base_url.is_empty() {
            validation.add_error("主机配置不能为空");
        }

        if config.session_timeout <= 0 {
            validation.add_error("会话超时必须大于0");
        } else if config.session_timeout < 300 {
            validation.add_warning("会话超时时间较短，可能影响用户体验");
        }

        Ok(validation)
    }

    pub fn get_server_status() -> ServerStatus {
        ServerStatus {
            is_running: true,
            start_time: chrono::Utc::now(),
            uptime_seconds: 0,
            request_count: 0,
            error_count: 0,
        }
    }
}

pub struct HttpServer {
    listener: TcpListener,
    app_routes: Router,
    local_addr: std::net::SocketAddr,
}

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
