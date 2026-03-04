
use super::types::*;
use anyhow::Result;
use std::fs;
use std::path::Path;
use url::Url;

pub struct ConfigLoader;

impl ConfigLoader {
    pub fn read_yaml(path: impl AsRef<Path>) -> Result<Config> {
        let config_str = fs::read_to_string(path)?;
        let config = serde_yaml::from_str(&config_str)?;
        Ok(config)
    }

    pub fn apply_env_overrides(mut config: Config) -> Config {
        tracing::info!("[tool] 应用环境变量配置覆盖...");

        if let Ok(host) = std::env::var("OCR_HOST") {
            config.server.host = host.clone();
            config.host = format!("http://{}", host);
            tracing::info!("[ok] 环境变量覆盖服务器地址: {}", host);
        }

        if let Ok(port_str) = std::env::var("OCR_PORT") {
            if let Ok(port_num) = port_str.parse::<u16>() {
                config.server.port = port_num;
                config.port = port_num;
                tracing::info!("[ok] 环境变量覆盖服务器端口: {}", port_num);
            }
        }

        if let Ok(db_password) = std::env::var("DB_PASSWORD") {
            if let Some(ref mut database) = config.database {
                if let Some(ref mut dm) = database.dm {
                    dm.password = db_password.clone();
                }
            }
            config.dm_sql.database_password = db_password;
            tracing::info!("[ok] 环境变量覆盖数据库密码: [安全隐藏]");
        }

        if let Ok(db_host) = std::env::var("DB_HOST") {
            if let Some(ref mut database) = config.database {
                if let Some(ref mut dm) = database.dm {
                    dm.host = db_host.clone();
                }
            }
            config.dm_sql.database_host = db_host.clone();
            tracing::info!("[ok] 环境变量覆盖数据库地址: {}", db_host);
        }

        if let Some(ref mut database) = config.database {
            if let Some(ref mut dm) = database.dm {
                if let Some(ref mut gw) = dm.go_gateway {
                    if let Ok(gw_url) = std::env::var("DM_GATEWAY_URL") {
                        gw.url = gw_url.clone();
                        tracing::info!("[ok] 环境变量覆盖DM网关URL: {}", gw_url);
                    }
                    if let Ok(gw_key) = std::env::var("DM_GATEWAY_API_KEY") {
                        gw.api_key = gw_key;
                        tracing::info!("[ok] 环境变量覆盖DM网关API Key: [隐藏]");
                    }

                    if gw.url.contains("localhost") {
                        let candidate_ip = std::env::var("HOST_IP")
                            .ok()
                            .or_else(|| std::env::var("OCR_HOST").ok())
                            .unwrap_or_else(|| config.server.host.clone());

                        let invalid = candidate_ip.is_empty()
                            || candidate_ip == "0.0.0.0"
                            || candidate_ip == "127.0.0.1"
                            || candidate_ip == "::1";

                        if !invalid {
                            let port = std::env::var("DM_GATEWAY_PORT")
                                .ok()
                                .and_then(|p| p.parse::<u16>().ok())
                                .unwrap_or(8080);
                            gw.url = format!("http://{}:{}", candidate_ip, port);
                            tracing::info!("[tool] 修正DM网关URL为本机IP: {}", gw.url);
                        } else {
                            tracing::warn!("[warn] 检测到DM网关URL使用localhost，且无法确定有效本机IP，保持不变: {}", gw.url);
                        }
                    }
                }
            }
        }

        if let Ok(oss_key) = std::env::var("OSS_ACCESS_KEY") {
            config.oss.access_key = oss_key;
            tracing::info!("[ok] 环境变量覆盖OSS访问密钥: [安全隐藏]");
        }

        if let Ok(oss_secret) = std::env::var("OSS_ACCESS_SECRET") {
            config.oss.access_key_secret = oss_secret;
            tracing::info!("[ok] 环境变量覆盖OSS密钥: [安全隐藏]");
        }
        if let Ok(oss_bucket) = std::env::var("OSS_BUCKET") {
            config.oss.bucket = oss_bucket.clone();
            tracing::info!("[ok] 环境变量覆盖OSS存储桶: {}", oss_bucket);
        }
        if let Ok(oss_root) = std::env::var("OSS_ROOT") {
            config.oss.root = oss_root.clone();
            tracing::info!("[ok] 环境变量覆盖OSS根目录: {}", oss_root);
        }

        if let Ok(callback_url) = std::env::var("OCR_THIRD_PARTY_CALLBACK_URL") {
            let trimmed = callback_url.trim();
            if trimmed.is_empty() {
                config.third_party_callback_url = None;
                tracing::warn!("[warn] OCR_THIRD_PARTY_CALLBACK_URL 为空，禁用第三方结果回调");
            } else {
                config.third_party_callback_url = Some(trimmed.to_string());
                tracing::info!("[ok] 环境变量覆盖第三方回调URL: {}", trimmed);
            }
        }

        if let Ok(role_str) = std::env::var("OCR_DEPLOYMENT_ROLE") {
            let role_clean = role_str.trim().to_ascii_lowercase();
            use crate::util::config::types::DeploymentRole;
            let new_role = match role_clean.as_str() {
                "master" => Some(DeploymentRole::Master),
                "worker" => Some(DeploymentRole::Worker),
                "hybrid" => Some(DeploymentRole::Hybrid),
                "standalone" => Some(DeploymentRole::Standalone),
                _ => None,
            };
            if let Some(role) = new_role {
                config.deployment.role = role;
                tracing::info!("[ok] 环境变量覆盖部署角色: {}", role_clean);
            } else {
                tracing::warn!("[warn] OCR_DEPLOYMENT_ROLE 无效: {}", role_str);
            }
        }

        if let Ok(node_id) = std::env::var("OCR_NODE_ID") {
            config.deployment.node_id = node_id.clone();
            tracing::info!("[ok] 环境变量覆盖节点ID: {}", node_id);
        }

        if let Ok(flag) = std::env::var("OCR_DISTRIBUTED_ENABLED") {
            if let Ok(enabled) = Self::parse_bool(&flag) {
                config.distributed.enabled = enabled;
                tracing::info!("[ok] 环境变量覆盖分布式开关: {}", enabled);
            } else {
                tracing::warn!("[warn] OCR_DISTRIBUTED_ENABLED 无法解析为布尔值: {}", flag);
            }
        }

        if let Ok(worker_id) = std::env::var("OCR_WORKER_ID") {
            let worker_cfg = config.deployment.worker.get_or_insert_with(|| {
                crate::util::config::types::WorkerDeploymentConfig {
                    id: worker_id.clone(),
                    secret: String::new(),
                    master_url: String::new(),
                    capabilities: None,
                    heartbeat_interval_secs: None,
                    rule_cache: crate::util::config::types::WorkerRuleCacheConfig::default(),
                }
            });
            worker_cfg.id = worker_id.clone();
            tracing::info!("[ok] 环境变量覆盖Worker ID: {}", worker_id);
        }

        if let Ok(worker_secret) = std::env::var("OCR_WORKER_SECRET") {
            if let Some(worker_cfg) = config.deployment.worker.as_mut() {
                worker_cfg.secret = worker_secret;
                tracing::info!("[ok] 环境变量覆盖Worker Secret: [隐藏]");
            }
        }

        if let Ok(master_url) = std::env::var("OCR_MASTER_URL") {
            if let Some(worker_cfg) = config.deployment.worker.as_mut() {
                worker_cfg.master_url = master_url.clone();
                tracing::info!("[ok] 环境变量覆盖Worker Master URL: {}", master_url);
            }
        }

        if let Ok(nats_url) = std::env::var("OCR_NATS_URL") {
            let nats_config = config
                .task_queue
                .nats
                .get_or_insert_with(crate::util::config::types::NatsQueueConfig::default);
            nats_config.server_url = nats_url.clone();
            config.task_queue.driver = crate::util::config::types::TaskQueueDriver::Nats;
            tracing::info!("[ok] 环境变量覆盖NATS服务地址: {}", nats_url);
        }

        if let Ok(inline_flag) = std::env::var("OCR_NATS_INLINE_WORKER") {
            if let Ok(enabled) = Self::parse_bool(&inline_flag) {
                if let Some(nats_config) = config.task_queue.nats.as_mut() {
                    nats_config.inline_worker = enabled;
                    tracing::info!("[ok] 环境变量覆盖NATS inline worker: {}", enabled);
                }
            } else {
                tracing::warn!(
                    "[warn] OCR_NATS_INLINE_WORKER 无法解析为布尔值: {}",
                    inline_flag
                );
            }
        }

        if let Ok(tls_enabled_flag) = std::env::var("OCR_NATS_TLS_ENABLED") {
            if let Ok(enabled) = Self::parse_bool(&tls_enabled_flag) {
                let nats_config = config
                    .task_queue
                    .nats
                    .get_or_insert_with(crate::util::config::types::NatsQueueConfig::default);
                let tls_cfg = nats_config
                    .tls
                    .get_or_insert_with(crate::util::config::types::NatsTlsConfig::default);
                tls_cfg.enabled = enabled;
                tracing::info!("[ok] 环境变量覆盖NATS TLS开关: {}", enabled);
            } else {
                tracing::warn!(
                    "[warn] OCR_NATS_TLS_ENABLED 无法解析为布尔值: {}",
                    tls_enabled_flag
                );
            }
        }

        if let Ok(tls_require_flag) = std::env::var("OCR_NATS_TLS_REQUIRE") {
            if let Ok(required) = Self::parse_bool(&tls_require_flag) {
                if let Some(nats_config) = config.task_queue.nats.as_mut() {
                    if let Some(tls_cfg) = nats_config.tls.as_mut() {
                        tls_cfg.require_tls = required;
                        tracing::info!("[ok] 环境变量覆盖NATS TLS require: {}", required);
                    }
                }
            } else {
                tracing::warn!(
                    "[warn] OCR_NATS_TLS_REQUIRE 无法解析为布尔值: {}",
                    tls_require_flag
                );
            }
        }

        if let Ok(ca_file) = std::env::var("OCR_NATS_TLS_CA") {
            if let Some(nats_config) = config.task_queue.nats.as_mut() {
                if let Some(tls_cfg) = nats_config.tls.as_mut() {
                    tls_cfg.ca_file = Some(ca_file.clone());
                    tracing::info!("[ok] 环境变量覆盖NATS TLS CA文件: {}", ca_file);
                }
            }
        }

        if let Ok(client_cert) = std::env::var("OCR_NATS_TLS_CLIENT_CERT") {
            if let Some(nats_config) = config.task_queue.nats.as_mut() {
                if let Some(tls_cfg) = nats_config.tls.as_mut() {
                    tls_cfg.client_cert = Some(client_cert.clone());
                    tracing::info!("[ok] 环境变量覆盖NATS客户端证书: {}", client_cert);
                }
            }
        }

        if let Ok(client_key) = std::env::var("OCR_NATS_TLS_CLIENT_KEY") {
            if let Some(nats_config) = config.task_queue.nats.as_mut() {
                if let Some(tls_cfg) = nats_config.tls.as_mut() {
                    tls_cfg.client_key = Some(client_key);
                    tracing::info!("[ok] 环境变量覆盖NATS客户端私钥: [隐藏]");
                }
            }
        }

        if let Ok(debug_str) = std::env::var("OCR_DEBUG_ENABLED") {
            let debug_enabled = debug_str.to_lowercase() == "true";
            config.debug.enabled = debug_enabled;
            config.runtime_mode.development.debug_enabled = debug_enabled;
            tracing::info!("[ok] 环境变量覆盖调试模式: {}", debug_enabled);
        }

        if let Ok(runtime_mode) = std::env::var("OCR_RUNTIME_MODE") {
            config.runtime_mode.mode = runtime_mode.clone();
            tracing::info!("[ok] 环境变量覆盖运行时模式: {}", runtime_mode);
        }

        if let Ok(log_retention) = std::env::var("OCR_LOG_RETENTION") {
            if let Ok(retention_days) = log_retention.parse::<u32>() {
                if let Some(file_config) = &mut config.logging.file.retention_days {
                    *file_config = retention_days;
                    tracing::info!("[ok] 环境变量覆盖日志保留天数: {}", retention_days);
                }
            }
        }

        Self::update_dependent_fields(&mut config);

        tracing::info!("[tool] 环境变量覆盖配置应用完成");

        tracing::info!("=== 配置加载诊断信息 ===");
        tracing::info!("📋 服务器配置:");
        tracing::info!("  - server.host: {}", config.server.host);
        tracing::info!("  - server.port: {}", config.server.port);
        tracing::info!("  - server.protocol: {}", config.server.protocol);
        tracing::info!("📋 URL配置:");
        tracing::info!("  - public_base_url: {:?}", config.public_base_url);
        tracing::info!("  - callback_url: {}", config.callback_url);
        tracing::info!("  - preview_url: {}", config.preview_url);
        tracing::info!("  - base_url(): {}", config.base_url());
        tracing::info!("  - callback_url(): {}", config.callback_url());
        tracing::info!("📋 SSO配置:");
        tracing::info!("  - app_id: {}", config.app_id);
        tracing::info!("  - sso_login_url: {}", config.login.sso_login_url);
        tracing::info!("  - use_callback: {}", config.login.use_callback);
        tracing::info!("=== 配置加载诊断信息结束 ===");

        config
    }

    fn parse_bool(value: &str) -> Result<bool, ()> {
        match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "y" => Ok(true),
            "false" | "0" | "no" | "n" => Ok(false),
            _ => Err(()),
        }
    }

    fn update_dependent_fields(config: &mut Config) {
        let base_url = format!(
            "{}://{}:{}",
            config.server.protocol, config.server.host, config.server.port
        );

        config.host = base_url.clone();
        config.port = config.server.port;

        if config.preview_url.is_empty() {
            config.preview_url = base_url.clone();
        }
        if config.callback_url.is_empty() {
            config.callback_url = format!("{}/api/sso/callback", base_url);
        }
    }

    pub fn load_with_env_overrides(path: impl AsRef<Path>) -> Result<Config> {
        let base_config = Self::read_yaml(path)?;

        let config = Self::apply_env_overrides(base_config);

        Self::validate_config(&config)?;

        tracing::info!("[ok] 智能配置加载完成");
        Ok(config)
    }

    pub fn validate_config(config: &Config) -> Result<()> {
        let is_worker = matches!(config.deployment.role, super::types::DeploymentRole::Worker);

        if !is_worker && config.port == 0 {
            return Err(anyhow::anyhow!("无效的端口号: {}", config.port));
        }

        if !config.host.starts_with("http://") && !config.host.starts_with("https://") {
            return Err(anyhow::anyhow!("无效的主机URL格式: {}", config.host));
        }

        if config.session_timeout <= 0 {
            return Err(anyhow::anyhow!("会话超时必须大于0"));
        }

        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&config.logging.level.as_str()) {
            return Err(anyhow::anyhow!("无效的日志级别: {}", config.logging.level));
        }

        let public_base = config
            .public_base_url
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());

        if let Some(url_str) = public_base {
            let normalized = url_str.trim_end_matches('/');
            if !normalized.starts_with("http://") && !normalized.starts_with("https://") {
                return Err(anyhow::anyhow!(
                    "public_base_url 必须以 http:// 或 https:// 开头: {}",
                    url_str
                ));
            }
            let parsed = Url::parse(normalized)
                .map_err(|e| anyhow::anyhow!("public_base_url 解析失败: {}", e))?;
            if let Some(host) = parsed.host_str() {
                if is_internal_host(host) {
                    return Err(anyhow::anyhow!(
                        "public_base_url 指向内网地址 ({}), 请配置对外可访问的域名或IP",
                        host
                    ));
                }
            }
        } else if !is_worker && Self::public_base_required(config) {
            return Err(anyhow::anyhow!(
                "生产环境需要配置 public_base_url，用于构建对外访问链接"
            ));
        }

        Ok(())
    }

    fn public_base_required(config: &Config) -> bool {
        let runtime_mode = config.runtime_mode.mode.to_ascii_lowercase();
        let is_production = matches!(runtime_mode.as_str(), "production" | "prod" | "release");

        if !is_production {
            return false;
        }

        Self::base_host_is_internal(config)
    }

    fn base_host_is_internal(config: &Config) -> bool {
        let base_url = config.base_url();
        Url::parse(&base_url)
            .ok()
            .and_then(|url| url.host_str().map(is_internal_host))
            .unwrap_or(true)
    }

    pub fn generate_template() -> Config {
        Config::default()
    }
}

pub struct ConfigWriter;

impl ConfigWriter {
    pub fn write_yaml(config: &Config, path: impl AsRef<Path>) -> Result<()> {
        let yaml_content = serde_yaml::to_string(config)?;
        fs::write(path, yaml_content)?;
        Ok(())
    }

    pub fn write_yaml_with_dir(config: &Config, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let yaml_content = serde_yaml::to_string(config)?;
        std::fs::write(path, yaml_content)?;
        Ok(())
    }

    pub fn generate_example_config(path: &Path) -> Result<()> {
        let example_config = Self::create_example_config();
        Self::write_yaml_with_dir(&example_config, path)?;
        Ok(())
    }

    pub fn generate_template() -> Config {
        Self::create_example_config()
    }

    fn create_example_config() -> Config {
        Config {
            host: "".to_string(),
            port: 0,
            preview_url: "".to_string(),
            callback_url: "".to_string(),
            third_party_callback_url: Some(
                "https://third-party.example.com/ocr/callback".to_string(),
            ),
            public_base_url: Some("https://ocr.example.com".to_string()),

            server: crate::util::config::types::ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8964,
                protocol: "http".to_string(),
            },

            database: None,

            app_id: "your_app_id".to_string(),
            session_timeout: 86400,
            login: Login {
                sso_login_url: "https://your-sso-provider.com/login".to_string(),
                access_token_url: "https://your-sso-provider.com/token".to_string(),
                get_user_info_url: "https://your-sso-provider.com/userinfo".to_string(),
                access_key: "your_access_key".to_string(),
                secret_key: "your_secret_key".to_string(),
                use_callback: true,
            },
            oss: Oss {
                root: "ocr-files".to_string(),
                bucket: "your-bucket".to_string(),
                server_url: "https://your-oss-endpoint.com".to_string(),
                access_key: "".to_string(),
                access_key_secret: "".to_string(),
            },
            dm_sql: DmSql {
                enabled: false,
                strict_mode: false,
                database_host: "".to_string(),
                database_port: "5236".to_string(),
                database_user: "SYSDBA".to_string(),
                database_password: "SYSDBA".to_string(),
                database_name: "OCR_DB".to_string(),
                connection_timeout: 30,
                max_retries: 3,
                retry_delay: 1000,
                max_connections: 10,
                min_connections: 2,
                idle_timeout: 600,
                health_check: None,
            },
            approve: Approve {
                submit_url: "".to_string(),
                access_key: "".to_string(),
                secret_key: "".to_string(),
            },
            runtime_mode: RuntimeModeConfig {
                mode: "development".to_string(),
                development: DevelopmentConfig {
                    debug_enabled: true,
                    mock_login: true,
                    mock_ocr: false,
                    test_tools: true,
                    auto_login: false,
                    detailed_logs: true,
                },
                testing: TestingConfig {
                    mock_data: true,
                    mock_delay: 100,
                    test_scenarios: true,
                    performance_test: false,
                },
                production: ProductionConfig {
                    debug_enabled: false,
                    mock_login: false,
                    mock_ocr: false,
                    test_tools: false,
                    security_strict: true,
                },
            },
            debug: Debug {
                enabled: true,
                tools_enabled: DebugToolsConfig {
                    api_test: true,
                    preview_demo: true,
                    flow_test: true,
                    system_monitor: true,
                    data_manager: true,
                },
            },
            test_mode: Some(TestModeConfig {
                enabled: true,
                auto_login: true,
                mock_ocr: false,
                mock_delay: 100,
                test_user: TestUserConfig {
                    id: "test_user_001".to_string(),
                    username: "测试用户".to_string(),
                    email: "test@example.com".to_string(),
                    role: "tester".to_string(),
                },
            }),
            logging: LoggingConfig {
                level: "info".to_string(),
                structured: Some(false),
                file: LogFileConfig {
                    enabled: true,
                    directory: "runtime/logs".to_string(),
                    retention_days: Some(7),
                },
                business_logging: None,
                level_config: None,
                attachment_logging: AttachmentLoggingConfig::default(),
                enable_debug_file: false,
            },
            monitoring: MonitoringConfig {
                enabled: false,
                performance: None,
                business_metrics: None,
            },
            third_party_access: ThirdPartyAccessConfig {
                enabled: false,
                clients: vec![ThirdPartyClient {
                    client_id: "demo_client".to_string(),
                    secret_key: "demo_secret_key_change_in_production".to_string(),
                    name: "演示客户端".to_string(),
                    source_type: "direct_api".to_string(),
                    enabled: false,
                    permissions: vec!["preview".to_string(), "query".to_string()],
                }],
                signature: SignatureConfig {
                    required: true,
                    timestamp_tolerance: 300,
                },
                rate_limiting: RateLimitingConfig {
                    enabled: true,
                    requests_per_minute: 100,
                    requests_per_hour: 1000,
                },
            },
            failover: FailoverConfig {
                database: DatabaseFailoverConfig {
                    enabled: true,
                    health_check_interval: 30,
                    max_retries: 3,
                    retry_delay: 1000,
                    fallback_to_local: true,
                    local_data_dir: "runtime/fallback/db".to_string(),
                },
                storage: StorageFailoverConfig {
                    enabled: true,
                    health_check_interval: 30,
                    max_retries: 3,
                    retry_delay: 1000,
                    auto_switch_to_local: true,
                    sync_when_recovered: true,
                    local_fallback_dir: "runtime/fallback/storage".to_string(),
                },
            },
            api_enhancement: ApiEnhancementConfig {
                enhanced_error_handling: false,
                trace_id_enabled: false,
                structured_response: false,
            },
            concurrency: None,
            business_metrics: None,
            user_data_encryption: UserDataEncryptionConfig::default(),
            api_call_tracking: Some(ApiCallTrackingConfig::default()),
            report_export: ReportExportConfig::default(),
            distributed_tracing: Some(DistributedTracingConfig::default()),
            download_limits: super::types::DownloadLimitsConfig {
                max_file_mb: 40,
                max_pdf_mb: 40,
                pdf_max_pages: 100,
                oversize_action: "truncate".to_string(),
                pdf_render_dpi: 150,
                pdf_jpeg_quality: 85,
            },
            ocr_tuning: super::types::OcrTuningConfig::default(),
            ocr_pool: super::types::OcrPoolConfig::default(),
            ocr_engine: None,
            task_queue: super::types::TaskQueueConfig::default(),
            worker_proxy: super::types::WorkerProxyConfig::default(),
            distributed: super::types::DistributedConfig::default(),
            deployment: super::types::DeploymentConfig::default(),
            master: super::types::MasterNodeConfig::default(),
            dynamic_worker: None,
            outbox: super::types::OutboxConfig::default(),
            service_watchdog: super::types::ServiceWatchdogConfig::default(),
            adaptive_concurrency: Some(super::types::AdaptiveConcurrencyConfig::default()),
        }
    }
}

impl Config {
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let mut config = ConfigLoader::read_yaml(path)?;
        config = ConfigLoader::apply_env_overrides(config);
        ConfigLoader::validate_config(&config)?;
        Ok(config)
    }

    pub fn save_to_file(&self, path: impl AsRef<Path>) -> Result<()> {
        ConfigWriter::write_yaml(self, path)
    }

    pub fn get_current_mode_config(&self) -> RuntimeModeInfo {
        match self.runtime_mode.mode.as_str() {
            "development" => RuntimeModeInfo {
                name: "development".to_string(),
                debug_enabled: self.runtime_mode.development.debug_enabled,
                mock_login: self.runtime_mode.development.mock_login,
                test_tools: self.runtime_mode.development.test_tools,
            },
            "testing" => RuntimeModeInfo {
                name: "testing".to_string(),
                debug_enabled: true,
                mock_login: true,
                test_tools: true,
            },
            "production" => RuntimeModeInfo {
                name: "production".to_string(),
                debug_enabled: self.runtime_mode.production.debug_enabled,
                mock_login: self.runtime_mode.production.mock_login,
                test_tools: self.runtime_mode.production.test_tools,
            },
            _ => RuntimeModeInfo {
                name: "unknown".to_string(),
                debug_enabled: false,
                mock_login: false,
                test_tools: false,
            },
        }
    }

    pub fn is_debug_enabled(&self) -> bool {
        self.debug.enabled || self.get_current_mode_config().debug_enabled
    }

    pub fn is_development_mode(&self) -> bool {
        self.debug.enabled && self.runtime_mode.mode == "development"
    }
}

#[derive(Debug, Clone)]
pub struct RuntimeModeInfo {
    pub name: String,
    pub debug_enabled: bool,
    pub mock_login: bool,
    pub test_tools: bool,
}
