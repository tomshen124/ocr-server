use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, LazyLock};

use opendal::services::Oss;
use opendal::Operator;
#[cfg(feature = "reqwest")]
use reqwest::Client;
use tokio::sync::Semaphore;
use tracing::info;

pub mod api;
pub mod build_info;
pub mod db;
pub mod model;
pub mod monitor;
pub mod server;
pub mod storage;
pub mod util;

use util::config::loader::ConfigLoader;
use util::config::Config;

/// 应用状态结构
#[derive(Clone)]
pub struct AppState {
    pub database: Arc<dyn db::Database>,
    pub storage: Arc<dyn storage::Storage>,
    pub config: Config,
    pub task_queue: Arc<dyn util::task_queue::TaskQueue>,
    /// HTTP客户端（支持依赖注入和配置管理）
    pub http_client: Arc<util::http_client::HttpClient>,
    /// 控制后台提交并发的信号量
    pub submission_semaphore: Arc<Semaphore>,
    /// 控制材料下载并发的信号量
    pub download_semaphore: Arc<Semaphore>,
}

/// 全局初始化状态标记
static GLOBALS_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// 显式初始化所有全局变量
///
/// 初始化顺序：
/// 1. CONFIG - 配置文件（无依赖）
/// 2. 日志系统 - 依赖CONFIG（在server模块中处理）
/// 3. CLIENT - 依赖日志系统
/// 4. OCR_SEMAPHORE - 依赖CONFIG和日志
/// 5. OSS - 依赖CONFIG和日志
///
/// 注意：必须在程序入口处调用，确保 tracing 初始化后再调用
pub fn initialize_globals() {
    if GLOBALS_INITIALIZED.swap(true, AtomicOrdering::SeqCst) {
        tracing::debug!("全局变量已初始化，跳过");
        return;
    }

    tracing::info!(event = "globals.init.start", "开始初始化全局变量");

    tracing::debug!("→ 初始化CONFIG");
    let _ = &*CONFIG;

    #[cfg(feature = "reqwest")]
    {
        tracing::debug!("→ 初始化HTTP CLIENT");
        let _ = &*CLIENT;
    }

    tracing::debug!("→ 初始化OCR_SEMAPHORE");
    let _ = &*OCR_SEMAPHORE;

    tracing::debug!("→ 初始化OSS");
    let _ = &*OSS;

    tracing::info!(
        event = "globals.init.complete",
        "全局变量初始化完成 (CONFIG → CLIENT → OCR_SEMAPHORE → OSS)"
    );
}

/// [1] CONFIG - 配置文件（无依赖，最先初始化）
pub static CONFIG: LazyLock<Config> = LazyLock::new(load_config_or_exit);

fn load_config_or_exit() -> Config {
    let config_path = find_config_file_path("config.yaml");
    match ConfigLoader::load_with_env_overrides(&config_path) {
        Ok(config) => {
            tracing::info!(event = "config.load.success", path = %config_path.display());
            config
        }
        Err(err) => {
            tracing::error!(
                event = "config.load.failed",
                path = %config_path.display(),
                error = %err
            );

            if !config_path.exists() {
                tracing::warn!(
                    event = "config.missing",
                    "配置文件不存在，正在生成模板: {}",
                    config_path.display()
                );
                let template = Config::default();
                if let Err(write_err) = template.write_yaml_to_path(&config_path) {
                    tracing::error!(
                        event = "config.template.write_failed",
                        error = %write_err
                    );
                }
            }

            eprintln!("❌ FATAL: 配置加载失败");
            eprintln!("路径: {}", config_path.display());
            eprintln!("错误: {err}");
            eprintln!("请修复配置文件后重新启动服务。");
            std::process::exit(1);
        }
    }
}

/// 智能查找配置文件路径，适应开发和生产环境
pub fn find_config_file_path(filename: &str) -> std::path::PathBuf {
    server::config::ConfigManager::find_config_file_path(filename)
}

/// [2] CLIENT - HTTP客户端（依赖：日志系统）
#[cfg(feature = "reqwest")]
pub static CLIENT: LazyLock<Option<Client>> = LazyLock::new(|| {
    let mut client_builder = Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .connect_timeout(std::time::Duration::from_secs(30))
        .danger_accept_invalid_certs(true)
        .user_agent("OCR-Preview-Service/1.0")
        .tcp_keepalive(std::time::Duration::from_secs(60))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .pool_max_idle_per_host(10);

    if let Ok(proxy_url) = std::env::var("HTTP_PROXY") {
        if let Ok(proxy) = reqwest::Proxy::http(&proxy_url) {
            tracing::info!(event = "client.http_proxy.enabled", proxy = %proxy_url);
            client_builder = client_builder.proxy(proxy);
        }
    }

    if let Ok(proxy_url) = std::env::var("HTTPS_PROXY") {
        if let Ok(proxy) = reqwest::Proxy::https(&proxy_url) {
            tracing::info!(event = "client.https_proxy.enabled", proxy = %proxy_url);
            client_builder = client_builder.proxy(proxy);
        }
    }

    match client_builder.build() {
        Ok(client) => {
            tracing::info!(event = "client.init.success");
            Some(client)
        }
        Err(e) => {
            tracing::error!(event = "client.init.failed", error = %e);
            tracing::warn!("检查: 1) 代理配置是否正确 2) TLS/SSL库是否可用");
            tracing::warn!("系统将在需要HTTP请求时返回错误");
            None
        }
    }
});

/// [3] OCR_SEMAPHORE - 全局OCR并发控制信号量
pub static OCR_SEMAPHORE: LazyLock<Arc<Semaphore>> = LazyLock::new(|| {
    let max_concurrent = CONFIG
        .concurrency
        .as_ref()
        .map(|c| c.ocr_processing.max_concurrent_tasks)
        .unwrap_or(6);

    let ocr_pool_size = 6;
    if max_concurrent as usize != ocr_pool_size {
        tracing::error!(
            "配置错误：OCR并发任务数({})与引擎池大小({})不一致，可能导致死锁",
            max_concurrent,
            ocr_pool_size
        );
        tracing::warn!("自动调整为引擎池大小: {}", ocr_pool_size);
        return Arc::new(Semaphore::new(ocr_pool_size));
    }

    tracing::info!(
        "初始化OCR并发控制: 最大{}个并发任务 ({})",
        max_concurrent,
        if CONFIG.concurrency.is_some() {
            "配置文件"
        } else {
            "默认值"
        }
    );
    Arc::new(Semaphore::new(max_concurrent as usize))
});

/// [4] OSS - OSS存储 Operator
pub static OSS: LazyLock<Option<Operator>> = LazyLock::new(|| {
    info!(
        target: "storage.oss",
        event = "oss.connect.start",
        endpoint = %CONFIG.oss.server_url
    );

    let builder = Oss::default()
        .root(&CONFIG.oss.root)
        .bucket(&CONFIG.oss.bucket)
        .endpoint(&CONFIG.oss.server_url)
        .access_key_id(&CONFIG.oss.access_key)
        .access_key_secret(&CONFIG.oss.access_key_secret);

    match Operator::new(builder) {
        Ok(operator) => {
            info!(
                target: "storage.oss",
                event = "oss.connect.success",
                bucket = %CONFIG.oss.bucket,
                root = %CONFIG.oss.root
            );
            Some(operator.finish())
        }
        Err(e) => {
            tracing::error!(target: "storage.oss", event = "oss.connect.failed", error = %e);
            tracing::error!(
                "OSS配置详情: server={}, bucket={}, root={}",
                CONFIG.oss.server_url,
                CONFIG.oss.bucket,
                CONFIG.oss.root
            );
            tracing::warn!("系统将自动降级到本地存储模式");
            tracing::warn!("请检查: 1) OSS配置是否正确 2) 网络连接 3) AccessKey权限");
            None
        }
    }
});
