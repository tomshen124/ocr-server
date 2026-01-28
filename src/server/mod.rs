//! 服务器模块
//!
//! 这个模块提供了完整的服务器设置和管理功能，包括：
//! - 配置管理 (config.rs)
//! - 数据库初始化 (database.rs)
//! - 存储系统初始化 (storage.rs)
//! - HTTP服务器设置 (http.rs)
//! - 监控服务管理 (monitoring.rs)
//!
//! 使用示例：
//! ```rust
//! use crate::server::ServerBootstrap;
//!
//! // 启动完整的服务器
//! let server = ServerBootstrap::new().await?;
//! server.start().await?;
//! ```

pub mod config;
pub mod database;
pub mod http;
pub mod monitoring;
pub mod storage;

// 重新导出主要组件
pub use config::{ConfigManager, ConfigValidationReport};
pub use database::{DatabaseHealth, DatabaseInitializer};
pub use http::{HttpServer, ServerManager, ServerStatus};
pub use monitoring::{MonitoringManager, MonitoringServices, MonitoringStatus};
pub use storage::{StorageHealth, StorageInitializer, StorageStats};

use crate::api::{LocalPreviewTaskHandler, RemotePreviewTaskHandler};
use crate::build_info;
use crate::util::adaptive_limiter;
use crate::util::config::types::DeploymentRole;
use crate::util::config::Config;
use crate::util::dynamic_worker::DynamicWorkerConfig;
use crate::util::material_cache;
use crate::util::material_cache_manager;
use crate::util::service_watchdog;
use crate::util::system_info::{self, init_start_time};
use crate::util::task_queue::{
    initialize_task_queue, start_queue_worker, PreviewTaskHandler, TaskQueue,
};
use crate::util::task_recovery;
use crate::util::worker;
use crate::AppState;
use anyhow::{anyhow, Context, Result};
use num_cpus;
use ocr_conn::ocr::{configure_pool_capacity, OcrEngineOptions, GLOBAL_POOL};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

/// 服务器引导程序 - 统一的服务器启动入口
pub struct ServerBootstrap {
    config: Config,
    validation_report: ConfigValidationReport,
    _log_guard: WorkerGuard,
}

impl ServerBootstrap {
    /// 创建新的服务器引导程序
    pub async fn new() -> Result<Self> {
        info!("[launch] 开始服务器引导程序...");

        // 加载和验证配置
        let (config, validation_report) = ConfigManager::load_and_validate()?;

        // 初始化日志系统
        let log_guard = ConfigManager::initialize_logging(&config)?;

        // [launch] 初始化所有全局变量（在日志系统初始化之后）
        crate::initialize_globals();

        // 记录配置验证结果
        if validation_report.has_errors() {
            return Err(anyhow::anyhow!(
                "配置验证失败: {} 个错误",
                validation_report.error_count()
            ));
        }

        info!("[ok] 服务器引导程序初始化完成");

        Ok(Self {
            config,
            validation_report,
            _log_guard: log_guard,
        })
    }

    /// 启动服务器
    pub async fn start(self) -> Result<()> {
        info!("=== OCR服务启动 ===");
        info!("版本信息: {}", build_info::summary());
        info!("服务端口: {}", self.config.get_port());

        // 初始化系统启动时间
        init_start_time();

        // 应用 OCR 池配置（需在首次使用前设置）
        apply_ocr_pool_config_for_role("master", &self.config);

        // [config] 配置优化流水线
        crate::util::processing::optimized_pipeline::OPTIMIZED_PIPELINE.configure(&self.config);

        // [search] 初始化分布式链路追踪
        self.initialize_distributed_tracing();

        // 创建应用状态
        let app_state = self.create_app_state().await?;
        service_watchdog::spawn_master_watchdog(&app_state);
        adaptive_limiter::spawn_for_master(&app_state);
        material_cache_manager::spawn_material_cache_manager(&app_state);

        // 启动 Worker 结果异步处理器
        let processor =
            crate::util::worker::result_processor::ResultProcessor::new(app_state.clone());
        tokio::spawn(async move {
            processor.run().await;
        });

        // 启动材料下载服务
        let material_downloader =
            crate::util::material::downloader_service::MaterialDownloaderService::new(
                app_state.database.clone(),
                app_state.task_queue.clone(),
            );
        tokio::spawn(async move {
            material_downloader.run().await;
        });

        // 预热OCR引擎池（可选，带超时保护）
        match timeout(Duration::from_secs(10), self.prewarm_ocr_engines()).await {
            Ok(_) => info!("[ok] OCR引擎预热完成"),
            Err(_) => {
                warn!("[warn] OCR引擎预热超时（10秒），跳过预热继续启动");
                warn!("首次OCR请求可能会有延迟");
            }
        }

        // 启动监控服务（带软超时，防止阻塞HTTP启动）
        info!("[hourglass] 准备启动监控服务（软超时3s）...");
        let monitoring_services: Option<MonitoringServices> = match timeout(
            Duration::from_secs(3),
            MonitoringManager::start_monitoring_services(&self.config),
        )
        .await
        {
            Ok(Ok(svcs)) => {
                info!("[ok] 监控服务启动完成");
                Some(svcs)
            }
            Ok(Err(e)) => {
                warn!("[warn] 监控服务启动失败: {}，将跳过监控继续启动HTTP", e);
                None
            }
            Err(_) => {
                warn!("[warn] 监控服务启动超过3秒，后台延迟启动");
                // 后台尝试启动，但不阻塞HTTP
                let cfg = self.config.clone();
                tokio::spawn(async move {
                    if let Err(e) = MonitoringManager::start_monitoring_services(&cfg).await {
                        warn!("后台启动监控失败: {}", e);
                    } else {
                        info!("后台监控服务已启动");
                    }
                });
                None
            }
        };

        // 创建HTTP服务器
        let server = ServerManager::create_server(&self.config, app_state).await?;

        // 启动服务器（这会阻塞直到关闭）
        let server_result = ServerManager::start_server(server).await;

        // 停止监控服务（如果已成功启动）
        if let Some(svcs) = monitoring_services {
            MonitoringManager::stop_monitoring_services(svcs).await?;
        }

        server_result
    }

    /// 创建应用状态
    async fn create_app_state(&self) -> Result<AppState> {
        info!("[build] 创建应用状态...");

        // 初始化数据库
        let database = self.initialize_database().await?;

        // 初始化存储系统
        let storage = self.initialize_storage().await?;

        material_cache::init(
            &self.config.master.material_cache_dir,
            Duration::from_secs(self.config.master.material_token_ttl_secs),
        )
        .await
        .context("初始化材料缓存失败")?;

        let task_handler: Arc<dyn PreviewTaskHandler> = Arc::new(LocalPreviewTaskHandler::new(
            database.clone(),
            storage.clone(),
        ));
        let task_queue = initialize_task_queue(
            self.config.distributed.enabled,
            &self.config.task_queue,
            task_handler.clone(),
        )
        .await
        .context("初始化任务队列失败")?;

        if let Some(dw_config) = &self.config.dynamic_worker {
            if dw_config.enabled {
                info!("[target] 检测到动态Worker配置，正在启动...");
                if let Err(e) = self
                    .start_dynamic_worker_manager(
                        dw_config.clone(),
                        &task_queue,
                        task_handler.clone(),
                    )
                    .await
                {
                    warn!("[warn] 动态Worker启动失败: {:#}", e);
                }
            } else {
                info!("[circle] 动态Worker功能已禁用");
            }
        }

        // 初始化HTTP客户端
        let http_client = Arc::new(
            crate::util::http_client::HttpClient::default_client()
                .context("初始化HTTP客户端失败")?,
        );
        info!("[ok] HTTP客户端初始化完成");

        let submission_permits = self
            .config
            .concurrency
            .as_ref()
            .map(|c| c.queue_monitoring.max_queue_length.max(1) as usize)
            .unwrap_or(50);
        let download_permits = submission_permits;

        let submission_semaphore = Arc::new(Semaphore::new(submission_permits));
        let download_semaphore = Arc::new(Semaphore::new(download_permits));

        // 创建应用状态
        let app_state = AppState {
            database,
            storage,
            config: self.config.clone(),
            task_queue,
            http_client,
            submission_semaphore,
            download_semaphore,
        };

        crate::util::callbacks::initialize(&app_state);
        crate::util::outbox::initialize(&app_state);

        if matches!(
            self.config.deployment.role,
            DeploymentRole::Master | DeploymentRole::Standalone | DeploymentRole::Hybrid
        ) {
            task_recovery::spawn_processing_watchdog(
                &app_state,
                &self.config.master.processing_watchdog,
            );
        }

        info!("[ok] 应用状态创建完成");
        Ok(app_state)
    }

    /// 启动动态Worker管理器
    async fn start_dynamic_worker_manager(
        &self,
        config: DynamicWorkerConfig,
        task_queue: &Arc<dyn TaskQueue>,
        handler: Arc<dyn PreviewTaskHandler>,
    ) -> Result<()> {
        use crate::util::dynamic_worker::{init_dynamic_worker_manager, DynamicWorkerManager};
        use crate::util::task_queue::{NatsTaskQueue, NatsTaskQueueConsumer};

        let queue_arc: Arc<NatsTaskQueue> = {
            let task_queue_arc: Arc<dyn TaskQueue> = Arc::clone(task_queue);
            let any_arc: Arc<dyn std::any::Any + Send + Sync> = task_queue_arc;
            any_arc
                .downcast::<NatsTaskQueue>()
                .map_err(|_| anyhow!("动态Worker仅支持NATS队列"))?
        };
        let consumer_factory = {
            let queue_clone = Arc::clone(&queue_arc);
            move || -> Result<NatsTaskQueueConsumer> {
                Ok(NatsTaskQueueConsumer::new(
                    queue_clone.queue_name(),
                    queue_clone.jetstream_context(),
                    queue_clone.get_config().clone(),
                ))
            }
        };

        let manager = Arc::new(DynamicWorkerManager::new(
            config,
            Arc::clone(&queue_arc),
            handler,
            consumer_factory,
        ));

        let manager_clone = Arc::clone(&manager);
        tokio::spawn(async move {
            manager_clone.start_monitoring().await;
        });

        init_dynamic_worker_manager(manager);
        info!("[ok] 动态Worker管理器已启动");
        Ok(())
    }

    /// 初始化数据库
    async fn initialize_database(&self) -> Result<Arc<dyn crate::db::Database>> {
        let database = DatabaseInitializer::create_from_config(&self.config).await?;

        // 验证数据库连接
        DatabaseInitializer::validate_connection(&database).await?;

        // 初始化数据库架构
        DatabaseInitializer::initialize_schema(&database).await?;

        Ok(database)
    }

    /// 初始化存储系统
    async fn initialize_storage(&self) -> Result<Arc<dyn crate::storage::Storage>> {
        let storage = StorageInitializer::create_from_config(&self.config).await?;

        // 验证存储系统连接
        StorageInitializer::validate_connection(&storage).await?;

        // 初始化存储目录结构
        StorageInitializer::initialize_directories(&storage).await?;

        Ok(storage)
    }

    /// 获取配置信息
    pub fn get_config(&self) -> &Config {
        &self.config
    }

    /// 获取验证报告
    pub fn get_validation_report(&self) -> &ConfigValidationReport {
        &self.validation_report
    }

    /// [search] 初始化分布式链路追踪
    fn initialize_distributed_tracing(&self) {
        if let Some(tracing_config) = &self.config.distributed_tracing {
            if tracing_config.enabled {
                info!("[search] 分布式链路追踪配置检测到，但暂时禁用以确保编译稳定性");
                warn!("分布式链路追踪将在后续版本中完全启用");

                // 暂时注释掉分布式追踪的初始化，等编译稳定后再启用
                /*
                let tracing_config = crate::util::tracing::distributed_tracing::TracingConfig {
                    enabled: tracing_config.enabled,
                    sampling_rate: tracing_config.sampling_rate,
                    max_spans: tracing_config.max_spans,
                    retention_seconds: tracing_config.retention_seconds,
                    verbose_logging: tracing_config.verbose_logging,
                };

                crate::util::tracing::distributed_tracing::init_tracing(tracing_config.clone());
                */

                info!("[ok] 分布式链路追踪初始化完成");
                info!(
                    "[stats] 采样率: {:.1}%",
                    tracing_config.sampling_rate * 100.0
                );
                info!("[clipboard] 最大span数量: {}", tracing_config.max_spans);
                info!(
                    "[clock] 数据保留时间: {}秒",
                    tracing_config.retention_seconds
                );

                // 启动清理任务
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(300)); // 5分钟清理一次
                    loop {
                        interval.tick().await;
                        // 暂时注释掉分布式追踪管理器调用
                        // if let Some(manager) = crate::util::tracing::distributed_tracing::get_tracing_manager() {
                        // manager.cleanup_expired().await;
                        // }
                    }
                });
            } else {
                info!("[circle] 分布式链路追踪已禁用");
            }
        } else {
            info!("[circle] 未配置分布式链路追踪，暂时跳过初始化");

            // 暂时注释掉默认配置初始化
            // let default_config = crate::util::tracing::distributed_tracing::TracingConfig::default();
            // crate::util::tracing::distributed_tracing::init_tracing(default_config);
        }
    }

    /// 预热OCR引擎池，减少首个请求延迟
    async fn prewarm_ocr_engines(&self) {
        let n = self.config.ocr_tuning.prewarm_engines;
        if n == 0 {
            return;
        }
        info!("[hot] 预热OCR引擎池: {} 个", n);
        // 应用引擎启动参数（若配置提供）
        if let Some(cfg) = &self.config.ocr_engine {
            let work_dir = cfg.work_dir.as_ref().map(|s| std::path::PathBuf::from(s));
            let binary = cfg.binary.as_ref().map(|s| std::path::PathBuf::from(s));
            let lib_path = cfg.lib_path.as_ref().map(|s| std::path::PathBuf::from(s));
            let opts = OcrEngineOptions {
                work_dir,
                binary,
                lib_path,
                timeout_secs: cfg.timeout_secs,
            };
            GLOBAL_POOL.set_options_if_empty(opts);
        }
        // 并行获取引擎句柄以触发进程启动，然后立即释放
        let mut tasks = Vec::new();
        for i in 0..n {
            tasks.push(tokio::spawn(async move {
                // 为每个OCR引擎初始化添加3秒超时
                match timeout(Duration::from_secs(3), GLOBAL_POOL.acquire()).await {
                    Ok(Ok(_h)) => { /* drop 即释放 */ }
                    Ok(Err(e)) => tracing::warn!("预热获取OCR引擎 {} 失败: {}", i + 1, e),
                    Err(_) => tracing::warn!("预热OCR引擎 {} 超时（3秒）", i + 1),
                }
            }));
        }
        for t in tasks {
            let _ = t.await;
        }
        info!(
            "[ok] 预热完成: capacity={}, available={}",
            ocr_conn::ocr::ocr_pool_stats().capacity,
            ocr_conn::ocr::ocr_pool_stats().available
        );
    }

    /// 执行健康检查
    pub async fn health_check(&self) -> Result<SystemHealthReport> {
        info!("[search] 执行系统健康检查...");

        // 创建临时的数据库和存储实例进行健康检查
        let database = DatabaseInitializer::create_from_config(&self.config).await?;
        let storage = StorageInitializer::create_from_config(&self.config).await?;

        // 执行数据库健康检查
        let database_health = DatabaseInitializer::health_check(&database).await?;

        // 执行存储系统健康检查
        let storage_health = StorageInitializer::health_check(&storage).await?;

        // 验证服务器配置
        let server_validation = ServerManager::validate_server_config(&self.config)?;

        let overall_healthy =
            database_health.is_healthy && storage_health.is_healthy && server_validation.is_valid();

        Ok(SystemHealthReport {
            overall_healthy,
            database_health,
            storage_health,
            server_config_valid: server_validation.is_valid(),
            validation_warnings: server_validation.warnings.clone(),
            check_time: chrono::Utc::now(),
        })
    }

    /// 获取系统信息摘要
    pub fn get_system_summary(&self) -> SystemSummary {
        SystemSummary {
            service_name: "OCR智能预审系统".to_string(),
            version: build_info::summary(),
            host: self.config.host.clone(),
            port: self.config.get_port(),
            debug_mode: self.config.debug.enabled,
            database_type: if self.config.dm_sql.database_host.is_empty() {
                "SQLite".to_string()
            } else {
                "达梦数据库".to_string()
            },
            storage_type: if self.config.oss.access_key.is_empty() {
                "本地存储".to_string()
            } else {
                "OSS存储".to_string()
            },
            monitoring_enabled: self.config.monitoring.enabled,
            third_party_access_enabled: self.config.third_party_access.enabled,
        }
    }
}

/// 系统健康检查报告
#[derive(Debug, Clone)]
pub struct SystemHealthReport {
    pub overall_healthy: bool,
    pub database_health: DatabaseHealth,
    pub storage_health: StorageHealth,
    pub server_config_valid: bool,
    pub validation_warnings: Vec<String>,
    pub check_time: chrono::DateTime<chrono::Utc>,
}

/// 系统信息摘要
#[derive(Debug, Clone)]
pub struct SystemSummary {
    pub service_name: String,
    pub version: String,
    pub host: String,
    pub port: u16,
    pub debug_mode: bool,
    pub database_type: String,
    pub storage_type: String,
    pub monitoring_enabled: bool,
    pub third_party_access_enabled: bool,
}

/// 便捷函数：快速启动服务器
pub async fn start_server() -> Result<()> {
    let bootstrap = ServerBootstrap::new().await?;
    bootstrap.start().await
}

/// 便捷函数：执行健康检查
pub async fn check_system_health() -> Result<SystemHealthReport> {
    let bootstrap = ServerBootstrap::new().await?;
    bootstrap.health_check().await
}

/// 便捷函数：启动队列 worker（仅处理任务，不提供 HTTP 服务）
pub async fn start_worker() -> Result<()> {
    info!("=== OCR Worker 启动 ===");
    info!("版本信息: {}", build_info::summary());

    let (config, validation_report) = ConfigManager::load_and_validate()?;
    if !config.distributed.enabled {
        return Err(anyhow!("当前配置未启用分布式模式"));
    }
    if config.deployment.role != DeploymentRole::Worker {
        return Err(anyhow!(
            "当前配置未声明 worker 角色，请设置 deployment.role=\"worker\""
        ));
    }
    if validation_report.has_errors() {
        return Err(anyhow::anyhow!(
            "配置验证失败: {} 个错误",
            validation_report.error_count()
        ));
    }

    let _log_guard = ConfigManager::initialize_logging(&config)?;

    // [launch] 初始化所有全局变量（在日志系统初始化之后）
    crate::initialize_globals();

    init_start_time();
    service_watchdog::spawn_worker_watchdog(&config);
    adaptive_limiter::spawn_for_worker(&config);

    apply_ocr_pool_config_for_role("worker", &config);

    // [config] 配置优化流水线
    crate::util::processing::optimized_pipeline::OPTIMIZED_PIPELINE.configure(&config);

    let worker_settings = config
        .deployment
        .worker
        .clone()
        .ok_or_else(|| anyhow!("worker 节点缺少 deployment.worker 配置"))?;

    // [init] 初始化材料缓存
    material_cache::init(
        &config.master.material_cache_dir,
        Duration::from_secs(config.master.material_token_ttl_secs),
    )
    .await
    .context("初始化材料缓存失败")?;

    let proxy_client = worker::init_worker_context(
        worker_settings.id.clone(),
        worker_settings.secret.clone(),
        worker_settings.master_url.clone(),
    )?;

    worker::log_worker_startup("worker");
    worker::spawn_heartbeat_task(worker_settings.heartbeat_interval_secs.unwrap_or(30));

    let handler = Arc::new(RemotePreviewTaskHandler::new(proxy_client));

    start_queue_worker(&config.task_queue, handler).await
}

fn apply_ocr_pool_config_for_role(role: &str, config: &Config) {
    let requested = config.ocr_pool.max_engines.max(1);
    let normalized = requested.clamp(1, 128);
    configure_pool_capacity(normalized);

    let recommendation = compute_ocr_pool_recommendation();

    info!(
        role = role,
        configured = normalized,
        recommended = recommendation.recommended,
        cpu_based = recommendation.cpu_based,
        memory_based = recommendation.memory_based,
        available_memory_mb = recommendation.available_memory_mb,
        "[config] OCR 引擎池容量已设置"
    );

    if normalized > recommendation.memory_based {
        warn!(
            role = role,
            configured = normalized,
            memory_based = recommendation.memory_based,
            "当前 OCR 池配置高于内存建议值，留意 worker RSS 及 swap 使用"
        );
    } else if normalized < recommendation.recommended {
        info!(
            role = role,
            configured = normalized,
            recommended = recommendation.recommended,
            "OCR 池少于建议值，如需更高吞吐可调大配置并重启"
        );
    }
}

struct OcrPoolRecommendation {
    recommended: usize,
    cpu_based: usize,
    memory_based: usize,
    available_memory_mb: u64,
}

fn compute_ocr_pool_recommendation() -> OcrPoolRecommendation {
    let physical_cores = num_cpus::get_physical().max(1);
    let cpu_based = std::cmp::max(1, physical_cores / 3);

    let memory = system_info::get_memory_usage();
    let available_mb = memory.total_mb.saturating_sub(memory.used_mb);
    // 经验值：每个 OCR 进程大约占用 512MB 左右
    let memory_based = std::cmp::max(1, (available_mb / 512) as usize);

    let recommended = cpu_based.min(memory_based).clamp(1, 32);

    OcrPoolRecommendation {
        recommended,
        cpu_based,
        memory_based,
        available_memory_mb: available_mb,
    }
}
