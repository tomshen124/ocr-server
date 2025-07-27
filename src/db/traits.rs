use async_trait::async_trait;
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// 预审记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewRecord {
    pub id: String,
    pub user_id: String,
    pub file_name: String,
    pub ocr_text: String,
    pub theme_id: Option<String>,
    pub evaluation_result: Option<String>,
    pub preview_url: String,
    pub status: PreviewStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub third_party_request_id: Option<String>,
}

/// 预审状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PreviewStatus {
    Pending,
    Processing,
    Completed,
    Failed,
}

impl fmt::Display for PreviewStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreviewStatus::Pending => write!(f, "pending"),
            PreviewStatus::Processing => write!(f, "processing"),
            PreviewStatus::Completed => write!(f, "completed"),
            PreviewStatus::Failed => write!(f, "failed"),
        }
    }
}

impl PreviewStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PreviewStatus::Pending => "pending",
            PreviewStatus::Processing => "processing",
            PreviewStatus::Completed => "completed",
            PreviewStatus::Failed => "failed",
        }
    }
}

/// API调用统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiStats {
    pub id: String,
    pub endpoint: String,
    pub method: String,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub status_code: u16,
    pub response_time_ms: u32,
    pub request_size: u32,
    pub response_size: u32,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 预审记录过滤条件
#[derive(Debug, Clone, Default)]
pub struct PreviewFilter {
    pub user_id: Option<String>,
    pub status: Option<PreviewStatus>,
    pub theme_id: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// API统计过滤条件
#[derive(Debug, Clone, Default)]
pub struct StatsFilter {
    pub endpoint: Option<String>,
    pub client_id: Option<String>,
    pub user_id: Option<String>,
    pub start_date: Option<DateTime<Utc>>,
    pub end_date: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// 数据库操作trait
#[async_trait]
pub trait Database: Send + Sync {
    /// 保存预审记录
    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()>;
    
    /// 获取预审记录
    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>>;
    
    /// 更新预审记录状态
    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()>;
    
    /// 查询预审记录列表
    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>>;
    
    /// 根据第三方请求ID和用户ID查找预审记录
    async fn find_preview_by_third_party_id(&self, third_party_id: &str, user_id: &str) -> Result<Option<PreviewRecord>>;
    
    /// 保存API调用统计
    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()>;
    
    /// 查询API统计数据
    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>>;
    
    /// 获取API调用汇总统计
    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary>;
    
    /// 健康检查
    async fn health_check(&self) -> Result<bool>;
    
    /// 初始化数据库（创建表等）
    async fn initialize(&self) -> Result<()>;
}

/// API调用汇总统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiSummary {
    pub total_calls: u64,
    pub success_calls: u64,
    pub failed_calls: u64,
    pub avg_response_time_ms: f64,
    pub total_request_size: u64,
    pub total_response_size: u64,
}