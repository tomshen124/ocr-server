use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

/// 存储操作trait
#[async_trait]
pub trait Storage: Send + Sync {
    /// 存储文件
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;

    /// 获取文件
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    /// 删除文件
    async fn delete(&self, key: &str) -> Result<()>;

    /// 检查文件是否存在
    async fn exists(&self, key: &str) -> Result<bool>;

    /// 列出指定前缀的所有文件
    async fn list(&self, prefix: &str) -> Result<Vec<String>>;

    /// 获取文件的公开访问URL（如果支持）
    async fn get_public_url(&self, key: &str) -> Result<String>;

    /// 获取文件的临时访问URL（带过期时间）
    async fn get_presigned_url(&self, key: &str, expires: Duration) -> Result<String>;

    /// 获取文件元数据
    async fn get_metadata(&self, key: &str) -> Result<FileMetadata>;

    /// 健康检查
    async fn health_check(&self) -> Result<bool>;
}

/// 文件元数据
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub content_type: Option<String>,
    pub last_modified: chrono::DateTime<chrono::Utc>,
    pub etag: Option<String>,
}
