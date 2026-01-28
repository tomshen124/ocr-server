//! 文件下载器模块
//! 处理多种URL格式的文件下载和数据解码

use base64::{engine::general_purpose, Engine as _};
use ocr_conn::CURRENT_DIR;
use tracing::{debug, info};

/// 下载文件内容（支持多种URL格式）
pub async fn download_file_content(url: &str) -> anyhow::Result<Vec<u8>> {
    download_file_content_with_headers(url, None).await
}

/// 下载文件内容（支持自定义请求头）
pub async fn download_file_content_with_headers(
    url: &str,
    headers: Option<std::collections::HashMap<String, String>>
) -> anyhow::Result<Vec<u8>> {
    debug!("尝试下载文件: {}", url);

    if url.starts_with("data:") {
        download_data_url(url).await
    } else if url.starts_with("http://") || url.starts_with("https://") {
        download_http_url_with_headers(url, headers).await
    } else if url.starts_with("file://") {
        download_file_url(url).await
    } else if url.contains("://") {
        Err(anyhow::anyhow!("不支持的协议: {}", url))
    } else {
        download_local_file(url).await
    }
}

/// 处理 data URL 格式
async fn download_data_url(url: &str) -> anyhow::Result<Vec<u8>> {
    info!("检测到data URL格式，正在解析...");
    
    // 解析 data URL 格式: data:[<mediatype>][;base64],<data>
    if let Some(comma_pos) = url.find(',') {
        let header = &url[5..comma_pos]; // 去掉 "data:" 前缀
        let data_part = &url[comma_pos + 1..];
        
        if header.contains("base64") {
            // Base64 编码数据
            match general_purpose::STANDARD.decode(data_part) {
                Ok(decoded) => {
                    info!("[ok] Base64数据解码成功，长度: {} bytes", decoded.len());
                    Ok(decoded)
                }
                Err(e) => {
                    Err(anyhow::anyhow!("Base64解码失败: {}", e))
                }
            }
        } else {
            // URL编码的文本数据
            let decoded = urlencoding::decode(data_part)
                .map_err(|e| anyhow::anyhow!("URL解码失败: {}", e))?;
            info!("[ok] URL编码数据解码成功，长度: {} bytes", decoded.len());
            Ok(decoded.to_string().into_bytes())
        }
    } else {
        Err(anyhow::anyhow!("无效的data URL格式: 缺少逗号分隔符"))
    }
}

/// 处理HTTP/HTTPS URL
async fn download_http_url(url: &str) -> anyhow::Result<Vec<u8>> {
    download_http_url_with_headers(url, None).await
}

/// 处理HTTP/HTTPS URL（支持自定义头部）
async fn download_http_url_with_headers(
    url: &str,
    headers: Option<std::collections::HashMap<String, String>>
) -> anyhow::Result<Vec<u8>> {
    info!("=== 开始网络文件下载 ===");
    info!("目标URL: {}", url);

    // 解析URL获取域名和路径信息
    if let Ok(parsed_url) = url::Url::parse(url) {
        info!("域名: {}", parsed_url.host_str().unwrap_or("未知"));
        info!("路径: {}", parsed_url.path());
        if let Some(query) = parsed_url.query() {
            info!("查询参数: {}", query);
        }
    }

    // 记录开始时间
    let start_time = std::time::Instant::now();

    // 使用全局客户端（已配置SSL跳过）
    #[cfg(feature = "reqwest")]
    let mut request_builder = crate::CLIENT
        .get(url)
        .timeout(std::time::Duration::from_secs(30));

    #[cfg(not(feature = "reqwest"))]
    {
        tracing::warn!("HTTP下载功能在当前编译环境下未启用");
        return Err(anyhow::anyhow!("HTTP下载功能未启用"));
    }

    #[cfg(feature = "reqwest")]
    {
        // 添加自定义头部
        if let Some(headers) = &headers {
            info!("自定义请求头数量: {}", headers.len());
            for (key, value) in headers {
                request_builder = request_builder.header(key, value);
                // 敏感信息脱敏记录
                let safe_value = if key.to_lowercase().contains("auth") ||
                                   key.to_lowercase().contains("token") ||
                                   key.to_lowercase().contains("cookie") {
                    format!("{}***{}", &value[..2.min(value.len())], &value[value.len().saturating_sub(2)..])
                } else {
                    value.clone()
                };
                info!("请求头: {} = {}", key, safe_value);
            }
        } else {
            info!("未设置自定义请求头");
        }

        info!("发送HTTP请求...");
        let response = request_builder.send().await?;

        let elapsed = start_time.elapsed();
        info!("HTTP响应耗时: {:?}", elapsed);
        info!("响应状态: {}", response.status());

    // 记录响应头部信息
    info!("响应头部信息:");
    for (name, value) in response.headers() {
        let header_name = name.as_str();
        let header_value = value.to_str().unwrap_or("无法解析");

        // 记录关键响应头
        if header_name.to_lowercase().contains("content") ||
           header_name.to_lowercase().contains("server") ||
           header_name.to_lowercase().contains("location") ||
           header_name.to_lowercase().contains("set-cookie") {
            info!("  {}: {}", header_name, header_value);
        }
    }

    if !response.status().is_success() {
        let error_msg = format!("HTTP请求失败: {} - {}", response.status(), response.status().canonical_reason().unwrap_or("未知错误"));

        // 尝试读取错误响应体
        if let Ok(error_body) = response.text().await {
            if !error_body.is_empty() {
                info!("错误响应体: {}", error_body.chars().take(500).collect::<String>());
            }
        }

        return Err(anyhow::anyhow!(error_msg));
    }

    // 获取内容类型
    if let Some(content_type) = response.headers().get("content-type") {
        info!("文件类型: {}", content_type.to_str().unwrap_or("未知"));
    }

    // 获取文件大小
    if let Some(content_length) = response.headers().get("content-length") {
        info!("文件大小: {} bytes", content_length.to_str().unwrap_or("未知"));
    }

    let bytes = response.bytes().await?;
    let total_elapsed = start_time.elapsed();

    info!("[ok] 网络文件下载成功");
    info!("实际文件大小: {} bytes", bytes.len());
    info!("总耗时: {:?}", total_elapsed);
    info!("平均速度: {:.2} KB/s", bytes.len() as f64 / total_elapsed.as_secs_f64() / 1024.0);
    info!("=== 网络文件下载完成 ===");

    Ok(bytes.to_vec())
}

/// 处理 file:// URL
async fn download_file_url(url: &str) -> anyhow::Result<Vec<u8>> {
    let file_path = &url[7..]; // 去掉 "file://" 前缀
    info!("正在读取文件协议路径: {}", file_path);
    
    let bytes = tokio::fs::read(file_path).await
        .map_err(|e| anyhow::anyhow!("读取文件失败 {}: {}", file_path, e))?;
    info!("[ok] 文件读取成功，长度: {} bytes", bytes.len());
    Ok(bytes)
}

/// 处理本地文件路径
async fn download_local_file(url: &str) -> anyhow::Result<Vec<u8>> {
    info!("正在读取本地文件: {}", url);
    
    let path = if std::path::Path::new(url).is_absolute() {
        std::path::PathBuf::from(url)
    } else {
        CURRENT_DIR.join(url)
    };
    
    let bytes = tokio::fs::read(&path).await
        .map_err(|e| anyhow::anyhow!("读取本地文件失败 {:?}: {}", path, e))?;
    info!("[ok] 本地文件读取成功，长度: {} bytes", bytes.len());
    Ok(bytes)
}

/// 检查URL格式是否受支持
pub fn is_supported_url(url: &str) -> bool {
    url.starts_with("data:") 
        || url.starts_with("http://") 
        || url.starts_with("https://")
        || url.starts_with("file://")
        || !url.contains("://") // 本地文件路径
}

/// 获取URL类型描述
pub fn get_url_type(url: &str) -> &'static str {
    if url.starts_with("data:") {
        "Data URL"
    } else if url.starts_with("https://") {
        "HTTPS URL"
    } else if url.starts_with("http://") {
        "HTTP URL"
    } else if url.starts_with("file://") {
        "File URL"
    } else if url.contains("://") {
        "Unknown Protocol"
    } else {
        "Local File Path"
    }
}