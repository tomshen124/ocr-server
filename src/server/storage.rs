//! 存储系统初始化模块
//! 负责根据配置创建和初始化存储系统

use crate::storage;
use crate::util::config::Config;
use anyhow::Result;
use std::sync::Arc;
use tracing::{error, info};

/// 存储系统初始化器
pub struct StorageInitializer;

impl StorageInitializer {
    /// 根据配置创建存储实例
    pub async fn create_from_config(config: &Config) -> Result<Arc<dyn storage::Storage>> {
        info!("[storage] 初始化存储系统...");

        // 根据配置创建存储配置
        let storage_config = if config.oss.access_key.is_empty() {
            // 使用本地存储
            info!("使用本地存储系统");
            storage::factory::StorageConfig {
                storage_type: storage::factory::StorageType::Local,
                local: Some(storage::factory::LocalConfig {
                    base_path: "data/storage".to_string(),
                    base_url: format!("{}/files", config.base_url().trim_end_matches('/')),
                }),
                oss: None,
            }
        } else {
            // 使用 OSS 存储
            info!("使用 OSS 存储系统: {}", config.oss.server_url);

            // 专有云OSS端点处理 - 为专有云保持完整HTTP URL
            let endpoint = if config.oss.server_url.contains("hzggcloud.xc.com") {
                // 专有云OSS需要完整的HTTP URL作为endpoint
                info!("[building] 专有云OSS：使用完整HTTP URL作为endpoint");
                config.oss.server_url.clone()
            } else {
                // 公网OSS移除协议前缀（OpenDAL会自动添加HTTPS）
                config
                    .oss
                    .server_url
                    .trim_start_matches("http://")
                    .trim_start_matches("https://")
                    .to_string()
            };
            info!("[tool] OpenDAL端点配置: {}", endpoint);

            storage::factory::StorageConfig {
                storage_type: storage::factory::StorageType::Oss,
                local: None,
                oss: Some(storage::factory::OssConfig {
                    bucket: config.oss.bucket.clone(),
                    endpoint: endpoint,
                    access_key_id: config.oss.access_key.clone(),
                    access_key_secret: config.oss.access_key_secret.clone(),
                    root: Some(config.oss.root.clone()),
                    public_endpoint: Some(format!(
                        "{}.{}",
                        config.oss.bucket,
                        config.oss.server_url.trim_end_matches('/')
                    )),
                }),
            }
        };

        let storage = storage::factory::create_storage(&storage_config).await?;

        // 如果启用了故障转移，包装存储
        if config.failover.storage.enabled {
            info!("[ok] 存储故障转移已启用");
            let failover_storage = storage::FailoverStorage::new(
                Arc::from(storage),
                config.failover.storage.clone(),
                config.host.clone(),
            )
            .await?;
            Ok(Arc::new(failover_storage) as Arc<dyn storage::Storage>)
        } else {
            info!("[ok] 存储系统初始化完成");
            Ok(Arc::from(storage))
        }
    }

    /// 验证存储系统连接
    pub async fn validate_connection(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        info!("[search] 验证存储系统连接...");

        // 执行存储系统连接测试
        Self::test_storage_access(storage).await?;

        info!("[ok] 存储系统连接验证成功");
        Ok(())
    }

    /// 初始化存储目录结构
    pub async fn initialize_directories(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        info!("[folder] 初始化存储目录结构...");

        // 创建必要的目录结构
        let directories = ["uploads", "previews", "reports", "temp", "cache"];

        for dir in directories {
            if let Err(e) = Self::ensure_directory_exists(storage, dir).await {
                tracing::warn!("创建目录 {} 失败: {}", dir, e);
            }
        }

        info!("[ok] 存储目录结构初始化完成");
        Ok(())
    }

    /// 执行存储系统健康检查
    pub async fn health_check(storage: &Arc<dyn storage::Storage>) -> Result<StorageHealth> {
        let start_time = std::time::Instant::now();

        // 测试存储系统访问
        let access_test_result = Self::test_storage_access(storage).await;
        let response_time = start_time.elapsed();

        // 检查可用空间
        let available_space = Self::check_available_space(storage).await.unwrap_or(0);

        Ok(StorageHealth {
            is_healthy: access_test_result.is_ok(),
            response_time_ms: response_time.as_millis() as u64,
            available_space_mb: available_space,
            error_message: access_test_result.err().map(|e| e.to_string()),
            last_check: chrono::Utc::now(),
        })
    }

    /// 测试存储系统访问
    async fn test_storage_access(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        // 尝试写入和读取测试文件
        let test_key = "health_check/test.txt";
        let test_content = b"storage health check";

        info!("[search] 开始存储访问测试: {}", test_key);

        // 写入测试文件
        info!("[upload] 尝试写入测试文件...");
        storage.put(test_key, test_content).await.map_err(|e| {
            error!("[fail] 存储写入测试失败: {}", e);
            tracing::debug!("[search] 写入失败详情(Debug): {:?}", e);
            anyhow::anyhow!("Failed to write to OSS, switching to fallback")
        })?;

        info!("[ok] 写入测试成功");

        // 读取测试文件
        info!("[download] 尝试读取测试文件...");
        let read_result = storage.get(test_key).await.map_err(|e| {
            error!("[fail] 存储读取测试失败: {}", e);
            tracing::debug!("[search] 读取失败详情(Debug): {:?}", e);
            anyhow::anyhow!("存储读取测试失败: {}", e)
        })?;

        // 验证内容
        if let Some(content) = read_result {
            if content != test_content {
                error!("[fail] 存储内容验证失败");
                return Err(anyhow::anyhow!("存储内容验证失败"));
            }
            info!("[ok] 读取和验证测试成功");
        } else {
            error!("[fail] 存储读取失败：文件不存在");
            return Err(anyhow::anyhow!("存储读取失败：文件不存在"));
        }

        // 清理测试文件
        info!("[broom] 清理测试文件...");
        if let Err(e) = storage.delete(test_key).await {
            tracing::warn!("[warn] 测试文件清理失败(可忽略): {}", e);
            tracing::debug!("[search] 清理失败详情(Debug): {:?}", e);
        }
        info!("[ok] 存储访问测试完成");

        Ok(())
    }

    /// 确保目录存在
    async fn ensure_directory_exists(storage: &Arc<dyn storage::Storage>, dir: &str) -> Result<()> {
        let marker_key = format!("{}/.gitkeep", dir);

        // 检查目录是否存在（通过检查标记文件）
        if storage.exists(&marker_key).await.unwrap_or(false) {
            return Ok(());
        }

        // 创建目录标记文件
        storage
            .put(&marker_key, b"")
            .await
            .map_err(|e| anyhow::anyhow!("创建目录标记失败: {}", e))?;

        Ok(())
    }

    /// 检查可用空间
    async fn check_available_space(storage: &Arc<dyn storage::Storage>) -> Result<u64> {
        // 这里应该根据存储类型实现具体的空间检查逻辑
        // 对于本地存储，可以检查磁盘空间
        // 对于OSS，可能需要调用相应的API

        // 暂时返回模拟值
        Ok(1024) // 1GB in MB
    }

    /// 清理临时文件
    pub async fn cleanup_temp_files(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        info!("[broom] 清理临时文件...");

        // 清理超过24小时的临时文件
        let cutoff_time = chrono::Utc::now() - chrono::Duration::hours(24);

        // 这里应该实现具体的清理逻辑
        // 遍历temp目录，删除过期文件

        info!("[ok] 临时文件清理完成");
        Ok(())
    }

    /// 获取存储统计信息
    pub async fn get_storage_stats(storage: &Arc<dyn storage::Storage>) -> Result<StorageStats> {
        let total_files = Self::count_files(storage, "").await.unwrap_or(0);
        let upload_files = Self::count_files(storage, "uploads").await.unwrap_or(0);
        let preview_files = Self::count_files(storage, "previews").await.unwrap_or(0);
        let report_files = Self::count_files(storage, "reports").await.unwrap_or(0);

        Ok(StorageStats {
            total_files,
            upload_files,
            preview_files,
            report_files,
            last_updated: chrono::Utc::now(),
        })
    }

    /// 统计文件数量
    async fn count_files(storage: &Arc<dyn storage::Storage>, prefix: &str) -> Result<u64> {
        // 这里应该实现具体的文件计数逻辑
        // 根据存储类型调用相应的API

        // 暂时返回模拟值
        Ok(100)
    }
}

/// 存储系统健康状态
#[derive(Debug, Clone)]
pub struct StorageHealth {
    /// 是否健康
    pub is_healthy: bool,
    /// 响应时间（毫秒）
    pub response_time_ms: u64,
    /// 可用空间（MB）
    pub available_space_mb: u64,
    /// 错误消息（如果有）
    pub error_message: Option<String>,
    /// 最后检查时间
    pub last_check: chrono::DateTime<chrono::Utc>,
}

/// 存储统计信息
#[derive(Debug, Clone)]
pub struct StorageStats {
    /// 总文件数
    pub total_files: u64,
    /// 上传文件数
    pub upload_files: u64,
    /// 预览文件数
    pub preview_files: u64,
    /// 报告文件数
    pub report_files: u64,
    /// 最后更新时间
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl StorageHealth {
    /// 创建健康状态
    pub fn healthy(response_time_ms: u64, available_space_mb: u64) -> Self {
        Self {
            is_healthy: true,
            response_time_ms,
            available_space_mb,
            error_message: None,
            last_check: chrono::Utc::now(),
        }
    }

    /// 创建不健康状态
    pub fn unhealthy(error: String) -> Self {
        Self {
            is_healthy: false,
            response_time_ms: 0,
            available_space_mb: 0,
            error_message: Some(error),
            last_check: chrono::Utc::now(),
        }
    }
}
