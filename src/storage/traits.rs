use anyhow::Result;
use async_trait::async_trait;
use std::time::Duration;

#[async_trait]
pub trait Storage: Send + Sync {
    async fn put(&self, key: &str, data: &[u8]) -> Result<()>;

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;

    async fn delete(&self, key: &str) -> Result<()>;

    async fn exists(&self, key: &str) -> Result<bool>;

    async fn list(&self, prefix: &str) -> Result<Vec<String>>;

    async fn get_public_url(&self, key: &str) -> Result<String>;

    async fn get_presigned_url(&self, key: &str, expires: Duration) -> Result<String>;

    async fn get_metadata(&self, key: &str) -> Result<FileMetadata>;

    async fn health_check(&self) -> Result<bool>;
}

#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub size: u64,
    pub content_type: Option<String>,
    pub last_modified: chrono::DateTime<chrono::Utc>,
    pub etag: Option<String>,
}
