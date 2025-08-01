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

/// 根据配置创建数据库实例
async fn create_database_from_config(config: &Config) -> anyhow::Result<Arc<dyn db::Database>> {
    // 根据配置创建数据库配置
    let db_config = if config.dm_sql.database_host.is_empty() {
        // 使用 SQLite 作为默认数据库
        db::factory::DatabaseConfig {
            db_type: db::factory::DatabaseType::Sqlite,
            sqlite: Some(db::factory::SqliteConfig {
                path: "data/ocr.db".to_string(),
            }),
            dm: None,
        }
    } else {
        // 使用达梦数据库
        db::factory::DatabaseConfig {
            db_type: db::factory::DatabaseType::Dm,
            sqlite: None,
            dm: Some(db::factory::DmConfig {
                host: config.dm_sql.database_host.clone(),
                port: config.dm_sql.database_port.parse().unwrap_or(5236),
                username: config.dm_sql.database_user.clone(),
                password: config.dm_sql.database_password.clone(),
                database: config.dm_sql.database_name.clone(),
            }),
        }
    };

    let database = db::factory::create_database(&db_config).await?;
    
    // 如果启用了故障转移，包装数据库
    if config.failover.database.enabled {
        info!("数据库故障转移已启用");
        let failover_db = db::FailoverDatabase::new(
            Arc::from(database),
            config.failover.database.clone()
        ).await?;
        Ok(Arc::new(failover_db) as Arc<dyn db::Database>)
    } else {
        Ok(Arc::from(database))
    }
}

/// 根据配置创建存储实例
async fn create_storage_from_config(config: &Config) -> anyhow::Result<Arc<dyn storage::Storage>> {
    // 根据配置创建存储配置
    let storage_config = if config.oss.access_key.is_empty() {
        // 使用本地存储
        storage::factory::StorageConfig {
            storage_type: storage::factory::StorageType::Local,
            local: Some(storage::factory::LocalConfig {
                base_path: "data/storage".to_string(),
                base_url: format!("{}/files", config.host),
            }),
            oss: None,
        }
    } else {
        // 使用 OSS 存储
        storage::factory::StorageConfig {
            storage_type: storage::factory::StorageType::Oss,
            local: None,
            oss: Some(storage::factory::OssConfig {
                bucket: config.oss.bucket.clone(),
                endpoint: format!("https://{}", config.oss.server_url),
                access_key_id: config.oss.access_key.clone(),
                access_key_secret: config.oss.access_key_secret.clone(),
                root: Some(config.oss.root.clone()),
                public_endpoint: Some(format!("https://{}.{}", config.oss.bucket, config.oss.server_url)),
            }),
        }
    };

    let storage = storage::factory::create_storage(&storage_config).await?;
    
    // 如果启用了故障转移，包装存储
    if config.failover.storage.enabled {
        info!("存储故障转移已启用");
        let failover_storage = storage::FailoverStorage::new(
            Arc::from(storage),
            config.failover.storage.clone(),
            config.host.clone()
        ).await?;
        Ok(Arc::new(failover_storage) as Arc<dyn storage::Storage>)
    } else {
        Ok(Arc::from(storage))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 手动加载配置文件以获取日志配置
    let config_path = find_config_file_path("config.yaml");
    
    let config = match Config::read_yaml(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("⚠️  配置文件读取失败: {} - {}", config_path.display(), e);
            if !config_path.exists() {
                eprintln!("📝 创建默认配置文件: {}", config_path.display());
                let config = Config::default();
                if let Err(write_err) = config.write_yaml_to_path(&config_path) {
                    eprintln!("❌ 创建默认配置文件失败: {}", write_err);
                }
                config
            } else {
                eprintln!("❌ 配置文件存在但无法解析，请检查语法");
                eprintln!("🔧 使用默认配置启动，但不会覆盖现有文件");
                Config::default()
            }
        }
    };

    // 使用配置文件中的日志配置初始化日志系统
    let _log = log_init_with_config("logs", "ocr", config.logging.clone())?;

    info!("=== OCR服务启动 ===");
    info!("配置文件已加载: {}", config_path.display());
    info!("服务端口: {}", config.port);

    // 执行日志清理（如果配置了保留天数）
    if let Some(retention_days) = config.logging.file.retention_days {
        if config.logging.file.enabled {
            let log_path = std::path::Path::new(&config.logging.file.directory);
            if let Err(e) = cleanup_old_logs(log_path, retention_days) {
                tracing::warn!("日志清理失败: {}", e);
            }
        }
    }

    // 初始化服务启动时间
    init_start_time();

    // 创建数据库和存储实例
    info!("初始化数据库和存储系统...");
    let database = create_database_from_config(&config).await?;
    let storage = create_storage_from_config(&config).await?;
    
    // 创建应用状态
    let app_state = AppState {
        database,
        storage,
        config: config.clone(),
    };
    
    info!("✅ 数据库和存储系统初始化完成");

    // 启动监控服务（如果启用）
    #[cfg(feature = "monitoring")]
    let monitor_service = {
        if config.monitoring.enabled {
            let monitor = std::sync::Arc::new(monitor::MonitorService::new());
            monitor.start().await?;
            info!("集成监控服务已启动");
            Some(monitor)
        } else {
            info!("集成监控功能已禁用");
            None
        }
    };

    #[cfg(not(feature = "monitoring"))]
    let _monitor_service: Option<std::sync::Arc<monitor::MonitorService>> = {
        if config.monitoring.enabled {
            info!("监控功能已配置但未编译，请使用 --features monitoring 启动");
        }
        None
    };

    let Ok(listener) = TcpListener::bind((Ipv4Addr::UNSPECIFIED, config.port)).await else {
        info!("Port is occupied");
        return Ok(());
    };

    // 服务端测试代码已清理，使用前端工具进行测试

    info!("Server started at {}", listener.local_addr()?);

    // 创建路由，包含应用状态
    let app_routes = routes(app_state);
    
    #[cfg(feature = "monitoring")]
    let app_routes = if let Some(monitor) = monitor_service.clone() {
        let monitoring_routes = monitor::monitoring_routes().with_state(monitor);
        app_routes.nest("/", monitoring_routes)
    } else {
        app_routes
    };

    axum::serve(listener, app_routes)
        .with_graceful_shutdown(async {
            ctrl_c().await.ok();

            // 停止监控服务
            #[cfg(feature = "monitoring")]
            if let Some(monitor) = monitor_service {
                let _ = monitor.stop().await;
                info!("监控服务已停止");
            }
        })
        .await?;

    Ok(())
}
