//! 文件下载器模块
//! 处理多种URL格式的文件下载和数据解码

use base64::{engine::general_purpose, Engine as _};
use ocr_conn::CURRENT_DIR;
use tracing::{debug, info};

/// 下载文件内容（支持多种URL格式）
pub async fn download_file_content(url: &str) -> anyhow::Result<Vec<u8>> {
    debug!("尝试下载文件: {}", url);
    
    if url.starts_with("data:") {
        download_data_url(url).await
    } else if url.starts_with("http://") || url.starts_with("https://") {
        download_http_url(url).await
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
                    info!("✅ Base64数据解码成功，长度: {} bytes", decoded.len());
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
            info!("✅ URL编码数据解码成功，长度: {} bytes", decoded.len());
            Ok(decoded.to_string().into_bytes())
        }
    } else {
        Err(anyhow::anyhow!("无效的data URL格式: 缺少逗号分隔符"))
    }
}

/// 处理HTTP/HTTPS URL
async fn download_http_url(url: &str) -> anyhow::Result<Vec<u8>> {
    info!("正在从网络下载文件: {}", url);
    
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("OCR-Preview-Service/1.0")
        .build()?;
        
    let response = client.get(url).send().await?;
    
    if !response.status().is_success() {
        return Err(anyhow::anyhow!("HTTP请求失败: {}", response.status()));
    }
    
    let bytes = response.bytes().await?;
    info!("✅ 网络文件下载成功，长度: {} bytes", bytes.len());
    Ok(bytes.to_vec())
}

/// 处理 file:// URL
async fn download_file_url(url: &str) -> anyhow::Result<Vec<u8>> {
    let file_path = &url[7..]; // 去掉 "file://" 前缀
    info!("正在读取文件协议路径: {}", file_path);
    
    let bytes = tokio::fs::read(file_path).await
        .map_err(|e| anyhow::anyhow!("读取文件失败 {}: {}", file_path, e))?;
    info!("✅ 文件读取成功，长度: {} bytes", bytes.len());
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
    info!("✅ 本地文件读取成功，长度: {} bytes", bytes.len());
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