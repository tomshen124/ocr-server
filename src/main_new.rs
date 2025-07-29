use crate::api::routes;
use crate::util::config::Config;
use crate::util::log::{log_init_with_config, cleanup_old_logs};
use crate::util::system_info::init_start_time;
use opendal::services::Oss;
use opendal::Operator;
use reqwest::Client;
use std::net::Ipv4Addr;
use std::sync::{Arc, LazyLock};
use tokio::net::TcpListener;
use tokio::signal::ctrl_c;
use tokio::sync::Semaphore;
use tracing::info;

mod api;
mod model;
mod util;
mod monitor;
mod db;
mod storage;
mod server; // 新增server模块

/// 应用状态结构
#[derive(Clone)]
pub struct AppState {
    pub database: Arc<dyn db::Database>,
    pub storage: Arc<dyn storage::Storage>,
    pub config: Config,
}

pub static CLIENT: LazyLock<Client> = LazyLock::new(Client::new);

/// 全局OCR并发控制信号量 - 针对32核64G服务器优化
/// 限制同时进行的OCR任务数量，防止系统过载
pub static OCR_SEMAPHORE: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    // 根据32核CPU和64GB内存，设置合理的并发限制
    // 考虑OCR + 文件下载 + 规则引擎处理的资源消耗
    let max_concurrent = 12; // 保守设置，确保系统稳定
    tracing::info!("🚀 初始化OCR并发控制: 最大{}个并发任务", max_concurrent);
    Arc::new(Semaphore::new(max_concurrent))
});

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    // 智能查找配置文件路径
    let config_path = find_config_file_path("config.yaml");
    
    match Config::read_yaml(&config_path) {
        Ok(config) => {
            tracing::info!("✅ 成功加载配置文件: {}", config_path.display());
            config
        }
        Err(e) => {
            tracing::warn!("⚠️  配置文件读取失败: {} - {}", config_path.display(), e);
            
            // 只在配置文件不存在时才创建默认配置
            if !config_path.exists() {
                tracing::info!("📝 创建默认配置文件: {}", config_path.display());
        let config = Config::default();
                if let Err(write_err) = config.write_yaml_to_path(&config_path) {
                    tracing::error!("❌ 创建默认配置文件失败: {}", write_err);
                }
    config
            } else {
                tracing::error!("❌ 配置文件存在但无法解析，请检查语法");
                tracing::error!("🔧 使用默认配置启动，但不会覆盖现有文件");
                Config::default()
            }
        }
    }
});

/// 智能查找配置文件路径，适应开发和生产环境
fn find_config_file_path(filename: &str) -> std::path::PathBuf {
    let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    
    // 获取可执行文件路径，用于判断我们在生产环境的哪个目录
    let exe_path = std::env::current_exe().ok();
    
    // 情况1：如果当前目录就有config子目录，直接使用（开发环境或生产根目录）
    let config_in_current = current_dir.join("config").join(filename);
    if config_in_current.exists() {
        return config_in_current;
    }
    
    // 情况2：如果我们在bin/目录，尝试上级目录的config/（生产环境）
    if let Some(parent) = current_dir.parent() {
        let config_in_parent = parent.join("config").join(filename);
        if config_in_parent.exists() {
            return config_in_parent;
        }
    }
    
    // 情况3：检查可执行文件同级目录是否有config/
    if let Some(exe_path) = exe_path {
        if let Some(exe_dir) = exe_path.parent() {
            // 如果在bin/目录，检查上级目录
            if exe_dir.file_name() == Some(std::ffi::OsStr::new("bin")) {
                if let Some(project_root) = exe_dir.parent() {
                    let config_in_root = project_root.join("config").join(filename);
                    if config_in_root.exists() {
                        return config_in_root;
                    }
                }
            }
        }
    }
    
    // 情况4：开发环境路径 (直接在当前目录)
    let dev_path = current_dir.join(filename);
    if dev_path.exists() {
        return dev_path;
    }
    
    // 如果都不存在，生产环境优先返回上级目录的config/，开发环境返回当前目录
    if current_dir.file_name() == Some(std::ffi::OsStr::new("bin")) {
        if let Some(parent) = current_dir.parent() {
            parent.join("config").join(filename)
        } else {
            current_dir.join(filename)
        }
    } else {
        current_dir.join(filename)
    }
}

pub static OSS: LazyLock<Operator> = LazyLock::new(|| {
    info!(
        "Connect OSS {}",
        format!("https://{}", CONFIG.oss.server_url)
    );
    let builder = Oss::default()
        .root(&CONFIG.oss.root)
        .bucket(&CONFIG.oss.bucket)
        .endpoint(&format!("https://{}", CONFIG.oss.server_url))
        .access_key_id(&CONFIG.oss.access_key)
        .access_key_secret(&CONFIG.oss.access_key_secret);
    Operator::new(builder).unwrap().finish()
});

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 使用新的server模块启动服务器
    server::start_server().await
}