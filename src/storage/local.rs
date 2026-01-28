use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::fs;

use super::traits::{FileMetadata, Storage};

/// 本地文件系统存储实现
pub struct LocalStorage {
    base_path: PathBuf,
    base_url: String,
}

impl LocalStorage {
    pub fn new(base_path: impl AsRef<Path>, base_url: &str) -> Result<Self> {
        let base_path = base_path.as_ref().to_path_buf();

        // 确保基础目录存在
        std::fs::create_dir_all(&base_path).context("Failed to create base directory")?;

        Ok(Self {
            base_path,
            base_url: base_url.trim_end_matches('/').to_string(),
        })
    }

    /// 获取文件的完整路径
    fn get_full_path(&self, key: &str) -> PathBuf {
        self.base_path.join(key.trim_start_matches('/'))
    }

    /// 确保文件的父目录存在
    async fn ensure_parent_dir(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .await
                .context("Failed to create parent directory")?;
        }
        Ok(())
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn put(&self, key: &str, data: &[u8]) -> Result<()> {
        let path = self.get_full_path(key);

        // 确保父目录存在
        self.ensure_parent_dir(&path).await?;

        // 写入文件
        fs::write(&path, data)
            .await
            .with_context(|| format!("Failed to write file: {}", path.display()))?;

        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        let path = self.get_full_path(key);

        match fs::read(&path).await {
            Ok(data) => Ok(Some(data)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e).context("Failed to read file")?,
        }
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.get_full_path(key);

        match fs::remove_file(&path).await {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e).context("Failed to delete file")?,
        }
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let path = self.get_full_path(key);
        Ok(path.exists())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let search_path = self.get_full_path(prefix);
        let _base_path_str = self.base_path.to_string_lossy();

        let mut files = Vec::new();

        // 如果搜索路径是目录，列出其中的文件
        if search_path.is_dir() {
            let mut entries = fs::read_dir(&search_path).await?;

            while let Some(entry) = entries.next_entry().await? {
                if entry.file_type().await?.is_file() {
                    let path = entry.path();
                    if let Ok(relative) = path.strip_prefix(&self.base_path) {
                        files.push(relative.to_string_lossy().to_string());
                    }
                }
            }
        } else {
            // 否则搜索匹配前缀的文件
            let parent = search_path.parent().unwrap_or(&self.base_path);
            let prefix_name = search_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if parent.exists() {
                let mut entries = fs::read_dir(parent).await?;

                while let Some(entry) = entries.next_entry().await? {
                    if entry.file_type().await?.is_file() {
                        let file_name = entry.file_name();
                        let file_name_str = file_name.to_string_lossy();

                        if file_name_str.starts_with(prefix_name) {
                            let path = entry.path();
                            if let Ok(relative) = path.strip_prefix(&self.base_path) {
                                files.push(relative.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }

        Ok(files)
    }

    async fn get_public_url(&self, key: &str) -> Result<String> {
        // 对于本地存储，返回基于base_url的URL
        Ok(format!("{}/{}", self.base_url, key.trim_start_matches('/')))
    }

    async fn get_presigned_url(&self, key: &str, _expires: Duration) -> Result<String> {
        // 本地存储不支持临时URL，返回永久URL
        self.get_public_url(key).await
    }

    async fn get_metadata(&self, key: &str) -> Result<FileMetadata> {
        let path = self.get_full_path(key);

        let metadata = fs::metadata(&path)
            .await
            .context("Failed to get file metadata")?;

        let modified = metadata
            .modified()
            .context("Failed to get modification time")?;

        let last_modified = chrono::DateTime::<Utc>::from(modified);

        Ok(FileMetadata {
            size: metadata.len(),
            content_type: mime_guess::from_path(&path).first().map(|m| m.to_string()),
            last_modified,
            etag: None, // 本地文件系统不生成ETag
        })
    }

    async fn health_check(&self) -> Result<bool> {
        // 检查基础目录是否可访问
        self.base_path
            .try_exists()
            .context("Failed to check base directory")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_local_storage() {
        let temp_dir = TempDir::new().unwrap();
        let storage = LocalStorage::new(temp_dir.path(), "http://localhost/files").unwrap();

        // 测试存储和获取
        let key = "test/file.txt";
        let data = b"Hello, World!";

        storage.put(key, data).await.unwrap();
        assert!(storage.exists(key).await.unwrap());

        let retrieved = storage.get(key).await.unwrap().unwrap();
        assert_eq!(retrieved, data);

        // 测试URL生成
        let url = storage.get_public_url(key).await.unwrap();
        assert_eq!(url, "http://localhost/files/test/file.txt");

        // 测试删除
        storage.delete(key).await.unwrap();
        assert!(!storage.exists(key).await.unwrap());
    }
}
