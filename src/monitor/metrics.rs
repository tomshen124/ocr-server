use chrono::{DateTime, Utc};
#[cfg(feature = "monitoring")]
use serde::{Deserialize, Serialize};

/// 系统指标数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    #[serde(with = "chrono::serde::ts_seconds")]
    pub timestamp: DateTime<Utc>,
    pub cpu_usage: f32,
    pub memory_usage: f32,
    pub disk_usage: f32,
    pub total_memory: u64,
    pub used_memory: u64,
    pub process_count: u32,
}

impl SystemMetrics {
    pub fn new() -> Self {
        Self {
            timestamp: Utc::now(),
            cpu_usage: 0.0,
            memory_usage: 0.0,
            disk_usage: 0.0,
            total_memory: 0,
            used_memory: 0,
            process_count: 0,
        }
    }
}

/// OCR服务指标
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrMetrics {
    pub timestamp: DateTime<Utc>,
    pub is_running: bool,
    pub port_listening: bool,
    pub api_responsive: bool,
    pub memory_usage_mb: u64,
    pub cpu_usage: f32,
    pub connection_count: u32,
    pub response_time_ms: Option<u64>,
}

impl OcrMetrics {
    pub fn new() -> Self {
        Self {
            timestamp: Utc::now(),
            is_running: false,
            port_listening: false,
            api_responsive: false,
            memory_usage_mb: 0,
            cpu_usage: 0.0,
            connection_count: 0,
            response_time_ms: None,
        }
    }
}

/// 监控统计数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoringStats {
    pub system: SystemMetrics,
    pub ocr_service: OcrMetrics,
    pub uptime_seconds: u64,
    pub last_check: DateTime<Utc>,
}

impl MonitoringStats {
    pub fn new() -> Self {
        Self {
            system: SystemMetrics::new(),
            ocr_service: OcrMetrics::new(),
            uptime_seconds: 0,
            last_check: Utc::now(),
        }
    }
}

/// 告警级别
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertLevel {
    Info,
    Warning,
    Error,
    Critical,
}

/// 告警信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub timestamp: DateTime<Utc>,
    pub level: AlertLevel,
    pub component: String,
    pub message: String,
    pub value: Option<f64>,
    pub threshold: Option<f64>,
}

impl Alert {
    pub fn new(level: AlertLevel, component: &str, message: &str) -> Self {
        Self {
            timestamp: Utc::now(),
            level,
            component: component.to_string(),
            message: message.to_string(),
            value: None,
            threshold: None,
        }
    }

    pub fn with_values(mut self, value: f64, threshold: f64) -> Self {
        self.value = Some(value);
        self.threshold = Some(threshold);
        self
    }
}

/// 性能指标历史记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsHistory {
    pub system_metrics: Vec<SystemMetrics>,
    pub ocr_metrics: Vec<OcrMetrics>,
    pub alerts: Vec<Alert>,
    pub max_records: usize,
}

impl MetricsHistory {
    pub fn new(max_records: usize) -> Self {
        Self {
            system_metrics: Vec::new(),
            ocr_metrics: Vec::new(),
            alerts: Vec::new(),
            max_records,
        }
    }

    pub fn add_system_metric(&mut self, metric: SystemMetrics) {
        self.system_metrics.push(metric);
        if self.system_metrics.len() > self.max_records {
            self.system_metrics.remove(0);
        }
    }

    pub fn add_ocr_metric(&mut self, metric: OcrMetrics) {
        self.ocr_metrics.push(metric);
        if self.ocr_metrics.len() > self.max_records {
            self.ocr_metrics.remove(0);
        }
    }

    pub fn add_alert(&mut self, alert: Alert) {
        self.alerts.push(alert);
        if self.alerts.len() > self.max_records {
            self.alerts.remove(0);
        }
    }

    pub fn get_latest_system_metric(&self) -> Option<&SystemMetrics> {
        self.system_metrics.last()
    }

    pub fn get_latest_ocr_metric(&self) -> Option<&OcrMetrics> {
        self.ocr_metrics.last()
    }

    pub fn get_recent_alerts(&self, count: usize) -> Vec<&Alert> {
        let start = if self.alerts.len() > count {
            self.alerts.len() - count
        } else {
            0
        };
        self.alerts[start..].iter().collect()
    }
}
