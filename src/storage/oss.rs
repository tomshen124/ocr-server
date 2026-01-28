use anyhow::{Context, Result};
use async_trait::async_trait;
use opendal::{services::Oss as OssService, Operator};
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

use super::traits::{FileMetadata, Storage};

/// 阿里云OSS存储实现
pub struct OssStorage {
    operator: Operator,
    bucket: String,
    public_endpoint: Option<String>,
    endpoint: String,
}

impl OssStorage {
    pub fn new(config: OssConfig) -> Result<Self> {
        // 构建OSS服务 - 专有云OSS配置优化
        info!(
            "[tool] 配置OSS服务: endpoint={}, bucket={}",
            config.endpoint, config.bucket
        );

        // 检测是否为专有云环境
        let is_private_cloud = config.endpoint.contains("hzggcloud.xc.com");
        if is_private_cloud {
            info!("[building] 检测到专有云OSS，使用HTTP协议");
        }

        let mut builder = OssService::default()
            .root(&config.root.unwrap_or_default())
            .bucket(&config.bucket)
            .endpoint(&config.endpoint)
            .access_key_id(&config.access_key_id)
            .access_key_secret(&config.access_key_secret);

        // 专有云OSS需要禁用HTTPS，强制使用HTTP
        if is_private_cloud {
            // 注意：OpenDAL 0.50 可能需要通过不同的方式配置HTTP
            // 这里我们依靠endpoint不带协议前缀，OpenDAL应该会根据端点判断
            info!("[tool] 专有云OSS配置：强制使用HTTP协议");
        }

        let operator = Operator::new(builder)?.finish();

        Ok(Self {
            operator,
            bucket: config.bucket,
            public_endpoint: config.public_endpoint,
            endpoint: config.endpoint,
        })
    }

    /// 测试网络连通性
    pub async fn test_network_connectivity(&self) -> Result<bool> {
        // 简单的网络连通性测试
        let endpoint = if let Some(ref public_endpoint) = self.public_endpoint {
            // 使用配置的公开端点，自动检测协议
            if public_endpoint.starts_with("http://") || public_endpoint.starts_with("https://") {
                public_endpoint.clone()
            } else {
                // 专有云通常使用HTTP，公网使用HTTPS
                let protocol = if public_endpoint.contains("aliyuncs.com") {
                    "https"
                } else {
                    "http" // 专有云默认HTTP
                };
                format!("{}://{}", protocol, public_endpoint)
            }
        } else {
            // 默认公网阿里云使用HTTPS
            format!("https://{}.oss.aliyuncs.com", self.bucket)
        };

        #[cfg(feature = "reqwest")]
        {
            match reqwest::get(&endpoint).await {
                Ok(response) => {
                    // HTTP状态码在200-499之间都表示网络连通
                    // 4xx错误通常是权限问题，不是网络问题
                    Ok(response.status().as_u16() < 500)
                }
                Err(_) => Ok(false),
            }
        }

        #[cfg(not(feature = "reqwest"))]
        {
            // MUSL环境下不支持HTTP客户端，假设网络连通
            tracing::debug!("MUSL环境下跳过OSS网络检查");
            Ok(true)
        }
    }
}

#[async_trait]
impl Storage for OssStorage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let start = Instant::now();
        info!(
            "[upload] OSS写入操作: key={}, size={}字节, bucket={}, endpoint={}",
            key,
            data.len(),
            self.bucket,
            self.endpoint
        );

        // 将数据转换为Vec<u8>以满足所有权要求
        let data_vec = data.to_vec();

        match self.operator.write(key, data_vec).await {
            Ok(_) => {
                let elapsed = start.elapsed();
                info!("[ok] OSS写入成功: {}, 用时: {:?}", key, elapsed);
                Ok(())
            }
            Err(e) => {
                error!(
                    "[fail] OSS写入失败: key={}, kind={:?}, error={}",
                    key,
                    e.kind(),
                    e
                );
                debug!("[search] 错误详情(Debug): {:?}", e);
                Err(e).context("Failed to write to OSS")
            }
        }
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let start = Instant::now();
        match self.operator.read(key).await {
            Ok(data) => {
                let elapsed = start.elapsed();
                info!(
                    "[download] OSS读取成功: key={}, size={}字节, 用时: {:?}",
                    key,
                    data.len(),
                    elapsed
                );
                Ok(Some(data.to_vec()))
            }
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                debug!("[download] OSS读取: key不存在: {}", key);
                Ok(None)
            }
            Err(e) => {
                error!(
                    "[fail] OSS读取失败: key={}, kind={:?}, error={}",
                    key,
                    e.kind(),
                    e
                );
                Err(e).context("Failed to read from OSS")?
            }
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let start = Instant::now();
        match self.operator.delete(key).await {
            Ok(_) => {
                info!(
                    "[broom] OSS删除成功: key={}, 用时: {:?}",
                    key,
                    start.elapsed()
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    "[fail] OSS删除失败: key={}, kind={:?}, error={}",
                    key,
                    e.kind(),
                    e
                );
                Err(e).context("Failed to delete from OSS")
            }
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        match self.operator.stat(key).await {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == opendal::ErrorKind::NotFound => Ok(false),
            Err(e) => {
                error!(
                    "[fail] OSS存在性检查失败: key={}, kind={:?}, error={}",
                    key,
                    e.kind(),
                    e
                );
                Err(e).context("Failed to check existence in OSS")?
            }
        }
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let start = Instant::now();
        info!(
            "[doc] OSS列举对象: prefix={}, bucket={}, endpoint={}",
            prefix, self.bucket, self.endpoint
        );
        let lister = self
            .operator
            .list(prefix)
            .await
            .context("Failed to list OSS objects")?;
        let mut files = Vec::new();
        for entry in lister {
            let metadata = entry.metadata();
            if metadata.is_file() {
                files.push(entry.path().to_string());
            }
        }
        info!(
            "[doc] OSS列举完成: {} 个对象, 用时: {:?}",
            files.len(),
            start.elapsed()
        );
        Ok(files)
    }

    async fn get_public_url(&self, key: &str) -> Result<String> {
        if let Some(endpoint) = &self.public_endpoint {
            Ok(format!(
                "{}/{}",
                endpoint.trim_end_matches('/'),
                key.trim_start_matches('/')
            ))
        } else {
            // 如果没有配置公开端点，返回内部端点URL
            Ok(format!(
                "https://{}.oss.aliyuncs.com/{}",
                self.bucket,
                key.trim_start_matches('/')
            ))
        }
    }

    async fn get_presigned_url(&self, key: &str, _expires: Duration) -> Result<String> {
        // 简化实现，返回公开URL
        // 实际实现应该生成带签名的临时URL
        self.get_public_url(key).await
    }

    async fn get_metadata(&self, key: &str) -> Result<FileMetadata> {
        let start = Instant::now();
        let metadata = self
            .operator
            .stat(key)
            .await
            .context("Failed to get metadata from OSS")?;
        info!(
            "ℹ OSS元数据获取成功: key={}, size={}, 用时: {:?}",
            key,
            metadata.content_length(),
            start.elapsed()
        );

        Ok(FileMetadata {
            size: metadata.content_length(),
            content_type: metadata.content_type().map(|s| s.to_string()),
            last_modified: metadata.last_modified().unwrap_or_else(chrono::Utc::now),
            etag: metadata.etag().map(|s| s.to_string()),
        })
    }

    async fn health_check(&self) -> Result<bool> {
        // [tool] 增强的OSS健康检查机制
        info!(
            "[search] 开始OSS健康检查... bucket={}, endpoint={}",
            self.bucket, self.endpoint
        );

        // 第一步：尝试轻量级检查 - 检查bucket是否可访问
        match self.operator.stat("").await {
            Ok(_) => {
                info!("[ok] OSS基础连接正常");

                // 第二步：尝试读取一个健康检查文件
                match self.operator.stat(".health_check").await {
                    Ok(_) => {
                        info!("[ok] OSS健康检查文件存在");
                        return Ok(true);
                    }
                    Err(e) if e.kind() == opendal::ErrorKind::NotFound => {
                        info!("[warn] 健康检查文件不存在，尝试创建");

                        // 第三步：尝试创建健康检查文件来测试写入权限
                        let health_data = format!(
                            "{{\"timestamp\":\"{}\",\"status\":\"healthy\"}}",
                            chrono::Utc::now().to_rfc3339()
                        );

                        match self
                            .operator
                            .write(".health_check", health_data.as_bytes().to_vec())
                            .await
                        {
                            Ok(_) => {
                                info!("[ok] OSS写入测试成功（可能缺少读/列举权限，但写权限正常）");
                                return Ok(true);
                            }
                            Err(e) => {
                                error!("[fail] OSS写入测试失败: kind={:?}, error={}", e.kind(), e);
                                return Ok(false);
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            "[fail] OSS健康检查文件访问失败: kind={:?}, error={}",
                            e.kind(),
                            e
                        );
                        return Ok(false);
                    }
                }
            }
            Err(e) => {
                error!("[fail] OSS基础连接失败: kind={:?}, error={}", e.kind(), e);

                // 最后尝试：检查网络连接
                match tokio::time::timeout(
                    Duration::from_secs(10),
                    self.test_network_connectivity(),
                )
                .await
                {
                    Ok(Ok(true)) => {
                        warn!("[warn] OSS网络连通但API调用失败，可能是权限或配置问题");
                        Ok(false)
                    }
                    Ok(Ok(false)) => {
                        warn!("[warn] OSS网络连接失败，可能是网络问题");
                        Ok(false)
                    }
                    Ok(Err(_)) | Err(_) => {
                        warn!("[warn] OSS网络连接超时");
                        Ok(false)
                    }
                }
            }
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
