use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod evaluation;
pub mod ocr;
pub mod preview;
pub mod user;
pub mod user_info;

// 会话中存储的用户信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUser {
    pub user_id: String,
    pub user_name: Option<String>,
    pub certificate_type: String,
    pub certificate_number: Option<String>,
    pub phone_number: Option<String>,
    pub email: Option<String>,
    pub organization_name: Option<String>,
    pub organization_code: Option<String>,
    pub login_time: String,  // 登录时间
    pub last_active: String, // 最后活跃时间
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Ticket {
    #[serde(rename = "ticketId")]
    ticket_id: String,
    #[serde(rename = "appId")]
    app_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccessToken {
    #[serde(rename = "accessToken")]
    access_token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThirdResult {
    success: Value,
    data: Value,
    #[serde(rename = "errorCode")]
    error_code: Value,
    #[serde(rename = "errorMsg")]
    error_msg: Value,
    #[serde(rename = "extraData")]
    extra_data: Value,
    #[serde(rename = "traceId")]
    trace_id: Value,
    env: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Token {
    token: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TicketId {
    #[serde(rename = "ticketId")]
    pub ticket_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PreviewInfo {
    #[serde(rename = "userId")]
    user_id: String,
    #[serde(rename = "previewUrl")]
    preview_url: String,
    /// 预审结果PDF下载地址（可选）
    #[serde(rename = "approvePdfFile", skip_serializing_if = "Option::is_none")]
    approve_pdf_file: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Goto {
    pub goto: String,
}

// 健康检查相关的数据结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub version: String,
    pub uptime: u64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailedHealthStatus {
    pub status: String,
    pub version: String,
    pub uptime: u64,
    pub timestamp: String,
    pub memory: MemoryStatus,
    pub cpu: CpuStatus,
    pub disk: DiskStatus,
    pub queue: QueueStatus,
    pub last_error: Option<ErrorInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStatus {
    pub total_mb: u64,
    pub used_mb: u64,
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CpuStatus {
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskStatus {
    pub total_gb: u64,
    pub used_gb: u64,
    pub usage_percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueStatus {
    pub pending: u32,
    pub processing: u32,
    pub completed_last_hour: u32,
    pub failed_last_hour: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub timestamp: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentsHealth {
    pub components: Vec<ComponentStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentStatus {
    pub name: String,
    pub status: String,
    pub details: Option<String>,
    pub response_time_ms: Option<u64>,
}
