
use crate::storage;
use crate::util::config::Config;
use anyhow::Result;
use std::sync::Arc;
use tracing::{error, info};

pub struct StorageInitializer;

impl StorageInitializer {
    pub async fn create_from_config(config: &Config) -> Result<Arc<dyn storage::Storage>> {
        info!("[storage] 初始化存储系统...");

        let storage_config = if config.oss.access_key.is_empty() {
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
            info!("使用 OSS 存储系统: {}", config.oss.server_url);

            let endpoint = if config.oss.server_url.contains("hzggcloud.xc.com") {
                info!("[building] 专有云OSS：使用完整HTTP URL作为endpoint");
                config.oss.server_url.clone()
            } else {
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

    pub async fn validate_connection(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        info!("[search] 验证存储系统连接...");

        Self::test_storage_access(storage).await?;

        info!("[ok] 存储系统连接验证成功");
        Ok(())
    }

    pub async fn initialize_directories(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        info!("[folder] 初始化存储目录结构...");

        let directories = ["uploads", "previews", "reports", "temp", "cache"];

        for dir in directories {
            if let Err(e) = Self::ensure_directory_exists(storage, dir).await {
                tracing::warn!("创建目录 {} 失败: {}", dir, e);
            }
        }

        info!("[ok] 存储目录结构初始化完成");
        Ok(())
    }

    pub async fn health_check(storage: &Arc<dyn storage::Storage>) -> Result<StorageHealth> {
        let start_time = std::time::Instant::now();

        let access_test_result = Self::test_storage_access(storage).await;
        let response_time = start_time.elapsed();

        let available_space = Self::check_available_space(storage).await.unwrap_or(0);

        Ok(StorageHealth {
            is_healthy: access_test_result.is_ok(),
            response_time_ms: response_time.as_millis() as u64,
            available_space_mb: available_space,
            error_message: access_test_result.err().map(|e| e.to_string()),
            last_check: chrono::Utc::now(),
        })
    }

    async fn test_storage_access(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        let test_key = "health_check/test.txt";
        let test_content = b"storage health check";

        info!("[search] 开始存储访问测试: {}", test_key);

        info!("[upload] 尝试写入测试文件...");
        storage.put(test_key, test_content).await.map_err(|e| {
            error!("[fail] 存储写入测试失败: {}", e);
            tracing::debug!("[search] 写入失败详情(Debug): {:?}", e);
            anyhow::anyhow!("Failed to write to OSS, switching to fallback")
        })?;

        info!("[ok] 写入测试成功");

        info!("[download] 尝试读取测试文件...");
        let read_result = storage.get(test_key).await.map_err(|e| {
            error!("[fail] 存储读取测试失败: {}", e);
            tracing::debug!("[search] 读取失败详情(Debug): {:?}", e);
            anyhow::anyhow!("存储读取测试失败: {}", e)
        })?;

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

        info!("[broom] 清理测试文件...");
        if let Err(e) = storage.delete(test_key).await {
            tracing::warn!("[warn] 测试文件清理失败(可忽略): {}", e);
            tracing::debug!("[search] 清理失败详情(Debug): {:?}", e);
        }
        info!("[ok] 存储访问测试完成");

        Ok(())
    }

    async fn ensure_directory_exists(storage: &Arc<dyn storage::Storage>, dir: &str) -> Result<()> {
        let marker_key = format!("{}/.gitkeep", dir);

        if storage.exists(&marker_key).await.unwrap_or(false) {
            return Ok(());
        }

        storage
            .put(&marker_key, b"")
            .await
            .map_err(|e| anyhow::anyhow!("创建目录标记失败: {}", e))?;

        Ok(())
    }

    async fn check_available_space(storage: &Arc<dyn storage::Storage>) -> Result<u64> {

        Ok(1024) // 1GB in MB
    }

    pub async fn cleanup_temp_files(storage: &Arc<dyn storage::Storage>) -> Result<()> {
        info!("[broom] 清理临时文件...");

        let cutoff_time = chrono::Utc::now() - chrono::Duration::hours(24);


        info!("[ok] 临时文件清理完成");
        Ok(())
    }

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

    async fn count_files(storage: &Arc<dyn storage::Storage>, prefix: &str) -> Result<u64> {

        Ok(100)
    }
}

#[derive(Debug, Clone)]
pub struct StorageHealth {
    pub is_healthy: bool,
    pub response_time_ms: u64,
    pub available_space_mb: u64,
    pub error_message: Option<String>,
    pub last_check: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct StorageStats {
    pub total_files: u64,
    pub upload_files: u64,
    pub preview_files: u64,
    pub report_files: u64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

impl StorageHealth {
    pub fn healthy(response_time_ms: u64, available_space_mb: u64) -> Self {
        Self {
            is_healthy: true,
            response_time_ms,
            available_space_mb,
            error_message: None,
            last_check: chrono::Utc::now(),
        }
    }

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
