//! 文件下载器 - 简化版，支持 MUSL 编译
//! 处理HTTP/HTTPS和file://协议的文件下载，兼容静态编译

use anyhow::{anyhow, Result};
use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, warn};

use crate::util::logging::standards::events;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;

const DOWNLOAD_SLOW_THRESHOLD_MS: u64 = 5_000;
const RETRY_QUEUE_MAX: usize = 200;

#[derive(Clone)]
struct CacheEntry {
    expires_at: Instant,
    bytes: Vec<u8>,
}

static DOWNLOAD_CACHE: LazyLock<Mutex<HashMap<String, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static RETRY_QUEUE: LazyLock<Mutex<VecDeque<String>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));
static RETRY_WORKER_STARTED: LazyLock<Mutex<bool>> = LazyLock::new(|| Mutex::new(false));

fn cache_ttl() -> Duration {
    std::env::var("DOWNLOAD_CACHE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(60))
}

fn cache_capacity() -> usize {
    std::env::var("DOWNLOAD_CACHE_CAPACITY")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(64)
}

fn cache_get(url: &str) -> Option<Vec<u8>> {
    let mut cache = DOWNLOAD_CACHE.lock().ok()?;
    if let Some(entry) = cache.get(url) {
        if entry.expires_at > Instant::now() {
            return Some(entry.bytes.clone());
        }
    }
    cache.remove(url);
    None
}

fn cache_put(url: &str, bytes: Vec<u8>) {
    let mut cache = match DOWNLOAD_CACHE.lock() {
        Ok(c) => c,
        Err(_) => return,
    };
    if cache.len() >= cache_capacity() {
        if let Some(key) = cache.keys().next().cloned() {
            cache.remove(&key);
        }
    }
    cache.insert(
        url.to_string(),
        CacheEntry {
            expires_at: Instant::now() + cache_ttl(),
            bytes,
        },
    );
}

fn download_timeout_secs() -> u64 {
    std::env::var("DOWNLOAD_TIMEOUT_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(30)
}

fn enqueue_retry(url: &str) {
    if url.is_empty() {
        return;
    }
    let mut queue = match RETRY_QUEUE.lock() {
        Ok(q) => q,
        Err(_) => return,
    };
    if queue.len() >= RETRY_QUEUE_MAX {
        queue.pop_front();
    }
    queue.push_back(url.to_string());
    drop(queue);
    spawn_record_retry(url.to_string(), "timeout_or_connect_error".to_string());
    start_retry_worker();
}

fn start_retry_worker() {
    let mut started = match RETRY_WORKER_STARTED.lock() {
        Ok(flag) => flag,
        Err(_) => return,
    };
    if *started {
        return;
    }
    *started = true;
    drop(started);

    tokio::spawn(async move {
        loop {
            let next = {
                let mut queue = match RETRY_QUEUE.lock() {
                    Ok(q) => q,
                    Err(_) => return,
                };
                queue.pop_front()
            };
            let Some(url) = next else {
                // 无任务，休眠后再检查
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            };

            #[cfg(feature = "reqwest")]
            {
                if let Some(client) = crate::CLIENT.as_ref() {
                    let result = match client
                        .get(&url)
                        .timeout(Duration::from_secs(download_timeout_secs().max(1)))
                        .send()
                        .await
                    {
                        Ok(resp) => resp.bytes().await.map(|bytes| {
                            cache_put(&url, bytes.to_vec());
                        }),
                        Err(e) => Err(e),
                    };
                    if result.is_err() {
                        spawn_record_retry(url.clone(), "retry_failed".to_string());
                    } else {
                        spawn_mark_done(url.clone());
                    }
                }
            }

            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    });
}

fn spawn_record_retry(url: String, reason: String) {
    #[cfg(feature = "dm_go")]
    tokio::spawn(async move {
        if let Err(e) = crate::db::dm::record_download_retry(&url, &reason).await {
            tracing::warn!("写入下载重试记录失败: {}", e);
        }
    });
    #[cfg(not(feature = "dm_go"))]
    let _ = (url, reason);
}

fn spawn_mark_done(url: String) {
    #[cfg(feature = "dm_go")]
    tokio::spawn(async move {
        if let Err(e) = crate::db::dm::mark_download_retry_done(&url).await {
            tracing::warn!("更新下载重试记录失败: {}", e);
        }
    });
    #[cfg(not(feature = "dm_go"))]
    let _ = url;
}

/// 下载文件的主入口函数
pub async fn download_file_content(url: &str) -> Result<Vec<u8>> {
    let stage_start = Instant::now();
    let source_label = get_url_type(url).to_string();

    if let Some(bytes) = cache_get(url) {
        return Ok(bytes);
    }

    if url.starts_with("file://") {
        match download_file_url(url).await {
            Ok(bytes) => {
                let mut labels = HashMap::new();
                labels.insert("source".to_string(), source_label.clone());
                METRICS_COLLECTOR.record_pipeline_stage(
                    "download",
                    true,
                    stage_start.elapsed(),
                    Some(labels),
                    None,
                );
                Ok(bytes)
            }
            Err(err) => {
                let mut labels = HashMap::new();
                labels.insert("source".to_string(), source_label.clone());
                let err_msg = err.to_string();
                METRICS_COLLECTOR.record_pipeline_stage(
                    "download",
                    false,
                    stage_start.elapsed(),
                    Some(labels),
                    Some(&err_msg),
                );
                Err(err)
            }
        }
    } else if url.starts_with("http://") || url.starts_with("https://") {
        match download_http_url(url).await {
            Ok(bytes) => {
                let mut labels = HashMap::new();
                labels.insert("source".to_string(), source_label.clone());
                METRICS_COLLECTOR.record_pipeline_stage(
                    "download",
                    true,
                    stage_start.elapsed(),
                    Some(labels),
                    None,
                );
                Ok(bytes)
            }
            Err(err) => {
                let mut labels = HashMap::new();
                labels.insert("source".to_string(), source_label.clone());
                let err_msg = err.to_string();
                METRICS_COLLECTOR.record_pipeline_stage(
                    "download",
                    false,
                    stage_start.elapsed(),
                    Some(labels),
                    Some(&err_msg),
                );
                Err(err)
            }
        }
    } else {
        let err = anyhow!("不支持的协议: {}", url);
        let mut labels = HashMap::new();
        labels.insert("source".to_string(), "unknown".to_string());
        METRICS_COLLECTOR.record_pipeline_stage(
            "download",
            false,
            stage_start.elapsed(),
            Some(labels),
            Some("unsupported_scheme"),
        );
        Err(err)
    }
}

/// 处理HTTP/HTTPS URL
async fn download_http_url(url: &str) -> Result<Vec<u8>> {
    #[cfg(feature = "reqwest")]
    {
        let download_start = Instant::now();
        tracing::debug!(
            target: "material.downloader",
            event = events::ATTACHMENT_DOWNLOAD_START,
            url = %url
        );

        // 先进行连接测试
        match test_network_connectivity(url).await {
            Ok(_) => {
                tracing::debug!(
                    target: "material.downloader",
                    event = events::ATTACHMENT_START,
                    step = "connectivity",
                    url = %url,
                    status = "ok"
                );
            }
            Err(e) => {
                tracing::warn!(
                    target: "material.downloader",
                    event = events::ATTACHMENT_ERROR,
                    step = "connectivity",
                    url = %url,
                    error = %e,
                    "网络连接测试失败，继续尝试下载"
                );
            }
        }

        // 增加重试机制
        let mut attempts = 0;
        let max_attempts = 3;

        // 检查HTTP客户端是否可用
        let client = match crate::CLIENT.as_ref() {
            Some(client) => client,
            None => {
                return Err(anyhow!("HTTP客户端不可用，无法下载文件"));
            }
        };

        while attempts < max_attempts {
            attempts += 1;

            match client
                .get(url)
                .timeout(std::time::Duration::from_secs(
                    download_timeout_secs().max(1),
                ))
                .send()
                .await
            {
                Ok(response) => {
                    if !response.status().is_success() {
                        tracing::warn!(
                            "HTTP请求失败: {} (尝试 {}/{})",
                            response.status(),
                            attempts,
                            max_attempts
                        );
                        if attempts == max_attempts {
                            return Err(anyhow!("HTTP请求失败: {}", response.status()));
                        }
                        // 等待后重试
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                    // 基于Content-Type/URL估计类型，决定大小上限
                    let ct = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_lowercase();
                    let is_pdf =
                        ct.contains("application/pdf") || url.to_lowercase().ends_with(".pdf");
                    let limits = &crate::CONFIG.download_limits;
                    let max_mb = if is_pdf {
                        limits.max_pdf_mb
                    } else {
                        limits.max_file_mb
                    };
                    let max_bytes = (max_mb as usize) * 1024 * 1024;
                    if let Some(len) = response.content_length() {
                        if len as usize > max_bytes {
                            return Err(anyhow!("文件过大: {} bytes, 超过上限 {} MB", len, max_mb));
                        }
                    }
                    // 逐块读取，强制限流
                    let mut stream = response.bytes_stream();
                    use futures::StreamExt;
                    let mut out: Vec<u8> = Vec::with_capacity(128 * 1024);
                    while let Some(chunk) = stream.next().await {
                        let chunk = chunk.map_err(|e| anyhow!("HTTP响应读取失败: {}", e))?;
                        if out.len() + chunk.len() > max_bytes {
                            return Err(anyhow!("文件过大: 超过上限 {} MB", max_mb));
                        }
                        out.extend_from_slice(&chunk);
                    }
                    cache_put(url, out.clone());
                    let elapsed_ms = download_start.elapsed().as_millis() as u64;
                    if elapsed_ms > DOWNLOAD_SLOW_THRESHOLD_MS {
                        tracing::warn!(
                            target: "material.downloader",
                            event = events::ATTACHMENT_SLOW,
                            url = %url,
                            bytes = out.len(),
                            attempts,
                            elapsed_ms,
                            threshold_ms = DOWNLOAD_SLOW_THRESHOLD_MS
                        );
                    } else {
                        tracing::debug!(
                            target: "material.downloader",
                            event = events::ATTACHMENT_DOWNLOAD_COMPLETE,
                            url = %url,
                            bytes = out.len(),
                            attempts,
                            elapsed_ms
                        );
                    }
                    return Ok(out);
                }
                Err(e) => {
                    tracing::warn!("HTTP连接失败: {} (尝试 {}/{})", e, attempts, max_attempts);

                    // 提供网络诊断信息
                    if e.is_timeout() {
                        tracing::warn!("连接超时，可能是网络延迟问题");
                    } else if e.is_connect() {
                        tracing::warn!("连接被拒绝，可能是防火墙或代理问题");
                    } else if e.is_request() {
                        tracing::warn!("请求格式错误");
                    } else {
                        tracing::warn!("其他网络错误: {}", e);
                    }

                    if attempts == max_attempts {
                        // 入队重试但不阻塞当前流程
                        enqueue_retry(url);
                        return Err(anyhow!("HTTP连接失败: {}。已加入后台重试队列", e));
                    }
                }
            }

            // 等待后重试
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }

        Err(anyhow!("HTTP下载失败，已达到最大重试次数"))
    }

    #[cfg(not(feature = "reqwest"))]
    {
        tracing::warn!("HTTP下载功能在MUSL环境下未启用");
        Err(anyhow!("HTTP下载功能未启用"))
    }
}

/// 测试网络连接性
#[cfg(feature = "reqwest")]
async fn test_network_connectivity(url: &str) -> Result<()> {
    use tokio::time::timeout;
    use url::Url;

    let parsed_url = Url::parse(url)?;
    let host = parsed_url
        .host_str()
        .ok_or_else(|| anyhow!("无法解析主机名"))?;
    let port = parsed_url.port_or_known_default().unwrap_or(80);

    tracing::debug!(
        target: "material.downloader",
        event = events::ATTACHMENT_START,
        step = "tcp_probe",
        host = %host,
        port
    );

    // 简单的TCP连接测试（带超时，避免阻塞过久）
    match timeout(
        Duration::from_secs(download_timeout_secs().max(1)),
        tokio::net::TcpStream::connect(format!("{}:{}", host, port)),
    )
    .await
    .map_err(|_| anyhow!("TCP连接测试超时"))?
    {
        Ok(_) => {
            tracing::debug!(
                target: "material.downloader",
                event = events::ATTACHMENT_COMPLETE,
                step = "tcp_probe",
                host = %host,
                port
            );
            Ok(())
        }
        Err(e) => Err(anyhow!("TCP连接测试失败: {}", e)),
    }
}

/// 处理 file:// URL
async fn download_file_url(url: &str) -> Result<Vec<u8>> {
    let file_path = &url[7..]; // 去掉 "file://" 前缀
    debug!(
        target: "material.downloader",
        event = events::ATTACHMENT_START,
        step = "file://",
        path = %file_path
    );

    // 大小限制
    if let Ok(meta) = tokio::fs::metadata(file_path).await {
        let limits = &crate::CONFIG.download_limits;
        let max_bytes = (limits.max_file_mb as u64) * 1024 * 1024;
        if meta.len() > max_bytes {
            return Err(anyhow!(
                "本地文件过大: {} bytes, 超过上限 {} MB",
                meta.len(),
                limits.max_file_mb
            ));
        }
    }

    let bytes = tokio::fs::read(file_path)
        .await
        .map_err(|e| anyhow!("读取文件失败 {}: {}", file_path, e))?;
    debug!(
        target: "material.downloader",
        event = events::ATTACHMENT_COMPLETE,
        step = "file://",
        path = %file_path,
        bytes = bytes.len()
    );
    Ok(bytes)
}

/// 获取URL类型（兼容性函数）
pub fn get_url_type(url: &str) -> &'static str {
    if url.starts_with("http://") || url.starts_with("https://") {
        "http"
    } else if url.starts_with("file://") {
        "file"
    } else {
        "unknown"
    }
}
