use async_trait::async_trait;
use anyhow::{Context, Result};
use opendal::{Operator, services::Oss as OssService};
use std::time::Duration;

use super::traits::{Storage, FileMetadata};

/// 阿里云OSS存储实现
pub struct OssStorage {
    operator: Operator,
    bucket: String,
    public_endpoint: Option<String>,
}

impl OssStorage {
    pub fn new(config: OssConfig) -> Result<Self> {
        // 构建OSS服务
        let builder = OssService::default()
            .root(&config.root.unwrap_or_default())
            .bucket(&config.bucket)
            .endpoint(&config.endpoint)
            .access_key_id(&config.access_key_id)
            .access_key_secret(&config.access_key_secret);
        
        let operator = Operator::new(builder)?.finish();
        
        Ok(Self {
            operator,
            bucket: config.bucket,
            public_endpoint: config.public_endpoint,
        })
    }
}

#[async_trait]
impl Storage for OssStorage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        // 将数据转换为Vec<u8>以满足所有权要求
        let data_vec = data.to_vec();
        self.operator.write(key, data_vec).await
            .context("Failed to write to OSS")?;
        Ok(())
    }
    
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        match self.operator.read(key).await {
            Ok(data) => Ok(Some(data.to_vec())),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("Failed to read from OSS")?,
        }
    }
    
    async fn delete(&self, key: &str) -> Result<()> {
        self.operator.delete(key).await
            .context("Failed to delete from OSS")?;
        Ok(())
    }
    
    async fn exists(&self, key: &str) -> Result<bool> {
        match self.operator.stat(key).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e).context("Failed to check existence in OSS")?,
        }
    }
    
    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let lister = self.operator.list(prefix).await
            .context("Failed to list OSS objects")?;
        
        let mut files = Vec::new();
        for entry in lister {
            let metadata = entry.metadata();
            if metadata.is_file() {
                files.push(entry.path().to_string());
            }
        }
        
        Ok(files)
    }
    
    async fn get_public_url(&self, key: &str) -> Result<String> {
        if let Some(endpoint) = &self.public_endpoint {
            Ok(format!("{}/{}", endpoint.trim_end_matches('/'), key.trim_start_matches('/')))
        } else {
            // 如果没有配置公开端点，返回内部端点URL
            Ok(format!("https://{}.oss.aliyuncs.com/{}", self.bucket, key.trim_start_matches('/')))
        }
    }
    
    async fn get_presigned_url(&self, key: &str, _expires: Duration) -> Result<String> {
        // 简化实现，返回公开URL
        // 实际实现应该生成带签名的临时URL
        self.get_public_url(key).await
    }
    
    async fn get_metadata(&self, key: &str) -> Result<FileMetadata> {
        let metadata = self.operator.stat(key).await
            .context("Failed to get metadata from OSS")?;
        
        Ok(FileMetadata {
            size: metadata.content_length(),
            content_type: metadata.content_type().map(|s| s.to_string()),
            last_modified: metadata.last_modified().unwrap_or_else(chrono::Utc::now),
            etag: metadata.etag().map(|s| s.to_string()),
        })
    }
    
    async fn health_check(&self) -> Result<bool> {
        // 简单的健康检查：尝试列出根目录
        match self.operator.list("").await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

/// OSS配置
#[derive(Debug, Clone)]
pub struct OssConfig {
    pub bucket: String,
    pub endpoint: String,
    pub access_key_id: String,
    pub access_key_secret: String,
    pub root: Option<String>,
    pub public_endpoint: Option<String>,
}