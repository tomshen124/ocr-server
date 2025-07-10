#[cfg(feature = "monitoring")]
use serde::{Deserialize, Serialize};

/// 监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    /// 是否启用监控
    pub enabled: bool,
    /// 监控检查间隔（秒）
    pub check_interval: u64,
    /// 系统资源监控配置
    pub system: SystemMonitorConfig,
    /// OCR服务监控配置
    pub ocr_service: OcrServiceMonitorConfig,
    /// 告警配置
    pub alerts: AlertConfig,
    /// 数据保留配置
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

/// 系统监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMonitorConfig {
    /// CPU使用率告警阈值
    pub cpu_threshold: f32,
    /// 内存使用率告警阈值
    pub memory_threshold: f32,
    /// 磁盘使用率告警阈值
    pub disk_threshold: f32,
    /// 是否监控进程数量
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

/// OCR服务监控配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrServiceMonitorConfig {
    /// 健康检查间隔（秒）
    pub health_check_interval: u64,
    /// API超时时间（秒）
    pub api_timeout: u64,
    /// 内存使用阈值（MB）
    pub memory_threshold_mb: u64,
    /// 是否启用自动重启
    pub auto_restart: bool,
    /// 重启前等待时间（秒）
    pub restart_delay: u64,
    /// 最大重启次数
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

/// 告警配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertConfig {
    /// 是否启用告警
    pub enabled: bool,
    /// 告警冷却时间（秒）
    pub cooldown_seconds: u64,
    /// 最大告警数量
    pub max_alerts: usize,
    /// 告警通知方式
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

/// 通知配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationConfig {
    /// 是否记录到日志
    pub log: bool,
    /// 是否发送邮件
    pub email: bool,
    /// 邮件配置
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

/// 邮件配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
    pub to: Vec<String>,
}

/// 数据保留配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// 系统指标保留时间（小时）
    pub system_metrics_hours: u32,
    /// OCR指标保留时间（小时）
    pub ocr_metrics_hours: u32,
    /// 告警记录保留时间（小时）
    pub alerts_hours: u32,
    /// 最大记录数量
    pub max_records: usize,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            system_metrics_hours: 24,
            ocr_metrics_hours: 24,
            alerts_hours: 72,
            max_records: 1440, // 24小时，每分钟一条记录
        }
    }
}

/// 监控配置构建器
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

/// 从配置文件加载监控配置
pub fn load_monitor_config() -> MonitorConfig {
    // 这里可以从主配置文件中读取监控相关配置
    // 或者使用默认配置
    MonitorConfig::default()
}
