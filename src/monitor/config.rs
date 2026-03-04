#[cfg(feature = "monitoring")]
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    pub enabled: bool,
    pub check_interval: u64,
    pub system: SystemMonitorConfig,
    pub ocr_service: OcrServiceMonitorConfig,
    pub alerts: AlertConfig,
    pub retention: RetentionConfig,
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval: 60,
            system: SystemMonitorConfig::default(),
            ocr_service: OcrServiceMonitorConfig::default(),
            alerts: AlertConfig::default(),
            retention: RetentionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMonitorConfig {
    pub cpu_threshold: f32,
    pub memory_threshold: f32,
    pub disk_threshold: f32,
    pub monitor_processes: bool,
}

impl Default for SystemMonitorConfig {
    fn default() -> Self {
        Self {
            cpu_threshold: 90.0,
            memory_threshold: 90.0,
            disk_threshold: 90.0,
            monitor_processes: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrServiceMonitorConfig {
    pub health_check_interval: u64,
    pub api_timeout: u64,
    pub memory_threshold_mb: u64,
    pub auto_restart: bool,
    pub restart_delay: u64,
    pub max_restarts: u32,
}

impl Default for OcrServiceMonitorConfig {
    fn default() -> Self {
        Self {
            health_check_interval: 300,
            api_timeout: 5,
            memory_threshold_mb: 500,
            auto_restart: false,
            restart_delay: 10,
            max_restarts: 3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    pub enabled: bool,
    pub cooldown_seconds: u64,
    pub max_alerts: usize,
    pub notification: NotificationConfig,
}

impl Default for AlertConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cooldown_seconds: 300,
            max_alerts: 100,
            notification: NotificationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    pub log: bool,
    pub email: bool,
    pub email_config: Option<EmailConfig>,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            log: true,
            email: false,
            email_config: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    pub system_metrics_hours: u32,
    pub ocr_metrics_hours: u32,
    pub alerts_hours: u32,
    pub max_records: usize,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            system_metrics_hours: 24,
            ocr_metrics_hours: 24,
            alerts_hours: 72,
            max_records: 1440,
        }
    }
}

pub struct MonitorConfigBuilder {
    config: MonitorConfig,
}

impl MonitorConfigBuilder {
    pub fn new() -> Self {
        Self {
            config: MonitorConfig::default(),
        }
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.config.enabled = enabled;
        self
    }

    pub fn check_interval(mut self, seconds: u64) -> Self {
        self.config.check_interval = seconds;
        self
    }

    pub fn cpu_threshold(mut self, threshold: f32) -> Self {
        self.config.system.cpu_threshold = threshold;
        self
    }

    pub fn memory_threshold(mut self, threshold: f32) -> Self {
        self.config.system.memory_threshold = threshold;
        self
    }

    pub fn disk_threshold(mut self, threshold: f32) -> Self {
        self.config.system.disk_threshold = threshold;
        self
    }

    pub fn ocr_memory_threshold(mut self, mb: u64) -> Self {
        self.config.ocr_service.memory_threshold_mb = mb;
        self
    }

    pub fn auto_restart(mut self, enabled: bool) -> Self {
        self.config.ocr_service.auto_restart = enabled;
        self
    }

    pub fn alerts_enabled(mut self, enabled: bool) -> Self {
        self.config.alerts.enabled = enabled;
        self
    }

    pub fn build(self) -> MonitorConfig {
        self.config
    }
}

pub fn load_monitor_config() -> MonitorConfig {
    MonitorConfig::default()
}
