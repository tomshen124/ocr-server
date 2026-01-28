use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::local::LocalStorage;
use super::oss::OssConfig as InternalOssConfig;
use super::oss::OssStorage;
use super::traits::Storage;

/// 存储类型
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum StorageType {
    Local,
    Oss,
}

/// 存储配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StorageConfig {
    #[serde(rename = "type")]
    pub storage_type: StorageType,

    /// 本地存储配置
    pub local: Option<LocalConfig>,

    /// OSS存储配置
    pub oss: Option<OssConfig>,
}

/// 本地存储配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LocalConfig {
    pub base_path: String,
    pub base_url: String,
}

/// OSS存储配置
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OssConfig {
    pub bucket: String,
    pub endpoint: String,
    pub access_key_id: String,
    pub access_key_secret: String,
    pub root: Option<String>,
    pub public_endpoint: Option<String>,
}

/// 创建存储实例
pub async fn create_storage(config: &StorageConfig) -> Result<Box<dyn Storage>> {
    match config.storage_type {
        StorageType::Local => {
            let local_config = config
                .local
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Local storage configuration missing"))?;

            let storage = LocalStorage::new(&local_config.base_path, &local_config.base_url)?;

            tracing::info!("Local storage initialized at: {}", local_config.base_path);
            Ok(Box::new(storage))
        }

        StorageType::Oss => {
            let oss_config = config
                .oss
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("OSS configuration missing"))?;

            let internal_config = InternalOssConfig {
                bucket: oss_config.bucket.clone(),
                endpoint: oss_config.endpoint.clone(),
                access_key_id: oss_config.access_key_id.clone(),
                access_key_secret: oss_config.access_key_secret.clone(),
                root: oss_config.root.clone(),
                public_endpoint: oss_config.public_endpoint.clone(),
            };

            let storage = OssStorage::new(internal_config)?;

            tracing::info!("OSS storage initialized for bucket: {}", oss_config.bucket);
            Ok(Box::new(storage))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_local_storage() {
        let config = StorageConfig {
            storage_type: StorageType::Local,
            local: Some(LocalConfig {
                base_path: "/tmp/test-storage".to_string(),
                base_url: "http://localhost/files".to_string(),
            }),
            oss: None,
        };

        let storage = create_storage(&config).await.unwrap();
        assert!(storage.health_check().await.unwrap());
    }
}
