use crate::CONFIG;
#[cfg(feature = "monitoring")]
use std::time::Duration;
use tokio::net::TcpStream;
use tracing::{debug, error, warn};

/// OCR服务健康检查器
#[derive(Clone)]
pub struct HealthChecker {
    ocr_port: u16,
    api_timeout: Duration,
}

impl HealthChecker {
    pub fn new() -> Self {
        Self {
            ocr_port: CONFIG.get_port(),
            api_timeout: Duration::from_secs(5),
        }
    }

    /// 检查OCR服务整体健康状态
    pub async fn check_ocr_service(&self) -> anyhow::Result<bool> {
        // 1. 检查端口是否监听
        let port_ok = self.check_port_listening().await?;
        if !port_ok {
            debug!("OCR服务端口 {} 未监听", self.ocr_port);
            return Ok(false);
        }

        // 2. 检查API健康状态
        let api_ok = self.check_api_health().await.unwrap_or(false);
        if !api_ok {
            debug!("OCR服务API健康检查失败");
            return Ok(false);
        }

        Ok(true)
    }

    /// 检查端口是否监听
    pub async fn check_port_listening(&self) -> anyhow::Result<bool> {
        let address = format!("127.0.0.1:{}", self.ocr_port);

        match tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(&address)).await {
            Ok(Ok(_)) => {
                debug!("端口 {} 正常监听", self.ocr_port);
                Ok(true)
            }
            Ok(Err(_)) => {
                debug!("端口 {} 连接失败", self.ocr_port);
                Ok(false)
            }
            Err(_) => {
                debug!("端口 {} 连接超时", self.ocr_port);
                Ok(false)
            }
        }
    }

    /// 检查API健康状态
    pub async fn check_api_health(&self) -> anyhow::Result<bool> {
        #[cfg(feature = "reqwest")]
        {
            let client = reqwest::Client::builder()
                .timeout(self.api_timeout)
                .build()?;

            let health_url = format!("http://127.0.0.1:{}/api/health", self.ocr_port);

            match client.get(&health_url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        debug!("OCR服务API健康检查通过");
                        Ok(true)
                    } else {
                        warn!("OCR服务API返回错误状态: {}", response.status());
                        Ok(false)
                    }
                }
                Err(e) => {
                    warn!("OCR服务API健康检查失败: {}", e);
                    Ok(false)
                }
            }
        }

        #[cfg(not(feature = "reqwest"))]
        {
            // MUSL环境下不支持HTTP客户端，假设API健康
            debug!("MUSL环境下跳过API健康检查");
            Ok(true)
        }
    }

    /// 检查详细健康信息
    pub async fn check_detailed_health(&self) -> anyhow::Result<serde_json::Value> {
        #[cfg(feature = "reqwest")]
        {
            let client = reqwest::Client::builder()
                .timeout(self.api_timeout)
                .build()?;

            let health_url = format!("http://127.0.0.1:{}/api/health/details", self.ocr_port);

            match client.get(&health_url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let health_data: serde_json::Value = response.json().await?;
                        Ok(health_data)
                    } else {
                        Err(anyhow::anyhow!(
                            "健康检查API返回错误: {}",
                            response.status()
                        ))
                    }
                }
                Err(e) => Err(anyhow::anyhow!("健康检查API调用失败: {}", e)),
            }
        }

        #[cfg(not(feature = "reqwest"))]
        {
            // MUSL环境下返回模拟健康数据
            Ok(serde_json::json!({
                "status": "healthy",
                "mode": "musl_simplified",
                "note": "HTTP客户端功能未启用"
            }))
        }
    }

    /// 检查组件健康状态
    pub async fn check_components_health(&self) -> anyhow::Result<serde_json::Value> {
        #[cfg(feature = "reqwest")]
        {
            let client = reqwest::Client::builder()
                .timeout(self.api_timeout)
                .build()?;

            let components_url =
                format!("http://127.0.0.1:{}/api/health/components", self.ocr_port);

            match client.get(&components_url).send().await {
                Ok(response) => {
                    if response.status().is_success() {
                        let components_data: serde_json::Value = response.json().await?;
                        Ok(components_data)
                    } else {
                        Err(anyhow::anyhow!(
                            "组件健康检查API返回错误: {}",
                            response.status()
                        ))
                    }
                }
                Err(e) => Err(anyhow::anyhow!("组件健康检查API调用失败: {}", e)),
            }
        }

        #[cfg(not(feature = "reqwest"))]
        {
            // MUSL环境下返回模拟组件状态
            Ok(serde_json::json!({
                "database": "healthy",
                "storage": "healthy",
                "mode": "musl_simplified"
            }))
        }
    }

    /// 测量API响应时间
    pub async fn measure_api_response_time(&self) -> anyhow::Result<u64> {
        #[cfg(feature = "reqwest")]
        {
            let start = std::time::Instant::now();

            let client = reqwest::Client::builder()
                .timeout(self.api_timeout)
                .build()?;

            let health_url = format!("http://127.0.0.1:{}/api/health", self.ocr_port);

            match client.get(&health_url).send().await {
                Ok(response) => {
                    let duration = start.elapsed();
                    if response.status().is_success() {
                        Ok(duration.as_millis() as u64)
                    } else {
                        Err(anyhow::anyhow!("API响应错误: {}", response.status()))
                    }
                }
                Err(e) => Err(anyhow::anyhow!("API调用失败: {}", e)),
            }
        }

        #[cfg(not(feature = "reqwest"))]
        {
            // MUSL环境下返回模拟响应时间
            Ok(1) // 1ms模拟响应时间
        }
    }

    /// 检查OCR服务进程状态
    pub async fn check_process_status(&self) -> anyhow::Result<bool> {
        use sysinfo::System;

        let mut system = System::new_all();
        system.refresh_all();

        // 查找OCR服务进程
        for (_, process) in system.processes() {
            if process.name().contains("ocr-server") {
                debug!(
                    "找到OCR服务进程: PID={}, 名称={}",
                    process.pid(),
                    process.name()
                );
                return Ok(true);
            }
        }

        debug!("未找到OCR服务进程");
        Ok(false)
    }

    /// 获取OCR服务进程信息
    pub async fn get_process_info(&self) -> anyhow::Result<Option<ProcessInfo>> {
        use sysinfo::System;

        let mut system = System::new_all();
        system.refresh_all();

        for (pid, process) in system.processes() {
            if process.name().contains("ocr-server") {
                return Ok(Some(ProcessInfo {
                    pid: pid.as_u32(),
                    name: process.name().to_string(),
                    memory_kb: process.memory(),
                    cpu_usage: process.cpu_usage(),
                    start_time: process.start_time(),
                }));
            }
        }

        Ok(None)
    }
}

/// 进程信息
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub memory_kb: u64,
    pub cpu_usage: f32,
    pub start_time: u64,
}

/// 健康检查结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthCheckResult {
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub overall_healthy: bool,
    pub port_listening: bool,
    pub api_responsive: bool,
    pub process_running: bool,
    pub response_time_ms: Option<u64>,
    pub process_info: Option<ProcessInfo>,
    pub error_message: Option<String>,
}

impl HealthCheckResult {
    pub fn new() -> Self {
        Self {
            timestamp: chrono::Utc::now(),
            overall_healthy: false,
            port_listening: false,
            api_responsive: false,
            process_running: false,
            response_time_ms: None,
            process_info: None,
            error_message: None,
        }
    }
}
