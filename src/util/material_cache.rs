use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use tokio::sync::RwLock;
use tokio::time::interval;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub const WORKER_CACHE_SCHEME: &str = "worker-cache://";

#[derive(Clone, Debug)]
pub struct MaterialToken {
    pub token: String,
    pub filename: String,
    pub content_type: Option<String>,
}

/// 根据 preview/material/attachment 获取缓存记录唯一ID
pub fn cached_record_id(preview_id: &str, material_code: &str, attachment_index: usize) -> String {
    format!("{}:{}:{}", preview_id, material_code, attachment_index)
}

#[derive(Clone, Debug)]
struct MaterialCacheEntry {
    path: PathBuf,
    filename: String,
    content_type: Option<String>,
    preview_id: String,
    material_code: String,
    expires_at: Instant,
}

struct MaterialCacheInner {
    base_dir: PathBuf,
    ttl: Duration,
    entries: HashMap<String, MaterialCacheEntry>,
    preview_index: HashMap<String, HashSet<String>>,
}

impl MaterialCacheInner {
    fn new(base_dir: PathBuf, ttl: Duration) -> Self {
        Self {
            base_dir,
            ttl,
            entries: HashMap::new(),
            preview_index: HashMap::new(),
        }
    }
}

static MATERIAL_CACHE: OnceCell<Arc<RwLock<MaterialCacheInner>>> = OnceCell::new();
static CLEANUP_TASK: OnceCell<()> = OnceCell::new();

const CLEANUP_INTERVAL_SECS: u64 = 60;
const CLEANUP_BATCH_SIZE: usize = 1024;

fn ensure_initialized() -> Result<Arc<RwLock<MaterialCacheInner>>> {
    MATERIAL_CACHE
        .get()
        .cloned()
        .ok_or_else(|| anyhow!("material cache 尚未初始化"))
}

pub async fn init(base_dir: impl AsRef<Path>, ttl: Duration) -> Result<()> {
    if MATERIAL_CACHE.get().is_some() {
        return Ok(());
    }

    let base_dir = base_dir.as_ref().to_path_buf();
    if !base_dir.exists() {
        tokio::fs::create_dir_all(&base_dir)
            .await
            .with_context(|| format!("创建材料缓存目录失败: {}", base_dir.display()))?;
    }

    let base_dir_for_cache = base_dir.clone();
    MATERIAL_CACHE
        .set(Arc::new(RwLock::new(MaterialCacheInner::new(
            base_dir_for_cache,
            ttl,
        ))))
        .map_err(|_| anyhow!("material cache 已初始化"))?;

    spawn_cleanup_task();

    info!(
        base = %base_dir.display(),
        ttl_secs = ttl.as_secs(),
        "材料缓存初始化完成"
    );

    Ok(())
}

pub async fn store_material(
    preview_id: &str,
    material_code: &str,
    filename: &str,
    bytes: &[u8],
    content_type: Option<String>,
) -> Result<MaterialToken> {
    let cache = ensure_initialized()?;
    let token = Uuid::new_v4().to_string();
    let sanitized_filename = sanitize_filename(filename);
    let sanitized_material_code = sanitize_filename(material_code);
    let base_dir = {
        let guard = cache.read().await;
        guard.base_dir.clone()
    };

    let preview_dir = base_dir.join(preview_id);
    let material_dir = preview_dir.join(&sanitized_material_code);

    tokio::fs::create_dir_all(&material_dir)
        .await
        .with_context(|| format!("创建材料目录失败: {}", material_dir.display()))?;

    let file_path = material_dir.join(&sanitized_filename);
    tokio::fs::write(&file_path, bytes)
        .await
        .with_context(|| format!("写入材料文件失败: {}", file_path.display()))?;

    let mut guard = cache.write().await;
    let expires_at = Instant::now() + guard.ttl;
    guard.entries.insert(
        token.clone(),
        MaterialCacheEntry {
            path: file_path.clone(),
            filename: sanitized_filename.clone(),
            content_type: content_type.clone(),
            preview_id: preview_id.to_string(),
            material_code: material_code.to_string(),
            expires_at,
        },
    );
    guard
        .preview_index
        .entry(preview_id.to_string())
        .or_default()
        .insert(token.clone());

    debug!(
        preview_id = %preview_id,
        material_code = %material_code,
        token = %token,
        path = %file_path.display(),
        "材料已缓存"
    );

    Ok(MaterialToken {
        token,
        filename: sanitized_filename,
        content_type,
    })
}

pub async fn store_material_with_token(
    token: &str,
    preview_id: &str,
    material_code: &str,
    filename: &str,
    bytes: &[u8],
    content_type: Option<String>,
) -> Result<PathBuf> {
    let cache = ensure_initialized()?;
    let sanitized_filename = sanitize_filename(filename);
    let sanitized_material_code = sanitize_filename(material_code);
    let base_dir = {
        let guard = cache.read().await;
        guard.base_dir.clone()
    };

    let preview_dir = base_dir.join(preview_id);
    let material_dir = preview_dir.join(&sanitized_material_code);

    tokio::fs::create_dir_all(&material_dir)
        .await
        .with_context(|| format!("创建材料目录失败: {}", material_dir.display()))?;

    let file_path = material_dir.join(&sanitized_filename);
    tokio::fs::write(&file_path, bytes)
        .await
        .with_context(|| format!("写入材料文件失败: {}", file_path.display()))?;

    let mut guard = cache.write().await;
    let expires_at = Instant::now() + guard.ttl;
    guard.entries.insert(
        token.to_string(),
        MaterialCacheEntry {
            path: file_path.clone(),
            filename: sanitized_filename.clone(),
            content_type: content_type.clone(),
            preview_id: preview_id.to_string(),
            material_code: material_code.to_string(),
            expires_at,
        },
    );
    guard
        .preview_index
        .entry(preview_id.to_string())
        .or_default()
        .insert(token.to_string());

    debug!(
        preview_id = %preview_id,
        material_code = %material_code,
        token = %token,
        path = %file_path.display(),
        "材料已缓存 (指定Token)"
    );

    Ok(file_path)
}

pub async fn read_material(token: &str) -> Result<Vec<u8>> {
    let cache = ensure_initialized()?;
    let (path, expires_at) = {
        let guard = cache.read().await;
        guard
            .entries
            .get(token)
            .map(|entry| (entry.path.clone(), entry.expires_at))
    }
    .ok_or_else(|| anyhow!("未知的材料令牌"))?;

    if Instant::now() > expires_at {
        warn!(token = %token, "材料令牌已过期，移除缓存");
        let mut guard = cache.write().await;
        remove_entry_locked(&mut guard, token);
        return Err(anyhow!("材料令牌已过期"));
    }

    tokio::fs::read(&path)
        .await
        .with_context(|| format!("读取缓存材料失败: {}", path.display()))
}

/// 获取缓存材料的本地路径
pub async fn get_material_path(token: &str) -> Option<PathBuf> {
    let cache = ensure_initialized().ok()?;
    let guard = cache.read().await;
    guard.entries.get(token).map(|entry| entry.path.clone())
}

pub async fn get_material_metadata(token: &str) -> Option<(String, Option<String>)> {
    let cache = ensure_initialized().ok()?;
    let guard = cache.read().await;
    guard
        .entries
        .get(token)
        .map(|entry| (entry.filename.clone(), entry.content_type.clone()))
}

pub async fn cleanup_preview(preview_id: &str) -> Result<()> {
    let cache = ensure_initialized()?;
    let mut guard = cache.write().await;

    if let Some(tokens) = guard.preview_index.remove(preview_id) {
        for token in tokens {
            remove_entry_locked(&mut guard, &token);
        }
    }

    let preview_dir = guard.base_dir.join(preview_id);
    if let Err(err) = tokio::fs::remove_dir_all(&preview_dir).await {
        if err.kind() != std::io::ErrorKind::NotFound {
            warn!(
                preview_id = %preview_id,
                error = %err,
                "删除材料缓存目录失败"
            );
        }
    }

    Ok(())
}

fn spawn_cleanup_task() {
    if CLEANUP_TASK.set(()).is_err() {
        return;
    }

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(CLEANUP_INTERVAL_SECS));
        ticker.tick().await; // 等待首个周期，避免刚启动就打扰

        loop {
            ticker.tick().await;
            match purge_expired_entries(CLEANUP_BATCH_SIZE).await {
                Ok(removed) if removed > 0 => {
                    info!(removed, batch = CLEANUP_BATCH_SIZE, "材料缓存后台清理完成");
                }
                Ok(_) => {}
                Err(err) => {
                    warn!(error = %err, "材料缓存后台清理失败");
                }
            }
        }
    });
}

async fn purge_expired_entries(limit: usize) -> Result<usize> {
    let cache = ensure_initialized()?;
    let mut guard = cache.write().await;
    let now = Instant::now();

    let expired_tokens: Vec<String> = guard
        .entries
        .iter()
        .filter_map(|(token, entry)| {
            if entry.expires_at <= now {
                Some(token.clone())
            } else {
                None
            }
        })
        .take(limit)
        .collect();

    let removed = expired_tokens.len();
    for token in expired_tokens {
        remove_entry_locked(&mut guard, &token);
    }

    Ok(removed)
}

fn remove_entry_locked(cache: &mut MaterialCacheInner, token: &str) {
    if let Some(entry) = cache.entries.remove(token) {
        if let Some(tokens) = cache.preview_index.get_mut(&entry.preview_id) {
            tokens.remove(token);
            if tokens.is_empty() {
                cache.preview_index.remove(&entry.preview_id);
            }
        }
        if let Err(err) = std::fs::remove_file(&entry.path) {
            if err.kind() != ErrorKind::NotFound {
                warn!(
                    preview_id = %entry.preview_id,
                    path = %entry.path.display(),
                    error = %err,
                    "删除材料缓存文件失败"
                );
            }
        }

        if !cache.preview_index.contains_key(&entry.preview_id) {
            let preview_dir = cache.base_dir.join(&entry.preview_id);
            if let Err(err) = std::fs::remove_dir_all(&preview_dir) {
                if err.kind() != ErrorKind::NotFound {
                    warn!(
                        preview_id = %entry.preview_id,
                        dir = %preview_dir.display(),
                        error = %err,
                        "删除材料缓存目录失败"
                    );
                }
            }
        }
    }
}

fn sanitize_filename(name: &str) -> String {
    let fallback = "attachment.bin";
    let mut result = name.trim();
    if result.is_empty() {
        return fallback.to_string();
    }
    if result.len() > 200 {
        result = &result[..200];
    }
    let sanitized: String = result
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}
