use crate::util::config::{LevelConfig, LoggingConfig};
use std::io;
use std::path::Path;
use std::sync::OnceLock;
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::daily;
use tracing_subscriber::fmt::format::Format;
use tracing_subscriber::fmt::layer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{filter::EnvFilter, Layer, Registry};

static ACCESS_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static DEBUG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

pub fn log_init<P: AsRef<Path>>(path: P, filename: &str) -> WorkerGuard {
    let console_format = Format::default()
        .without_time()
        .with_level(false)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false);

    let file_format = Format::default()
        .without_time()
        .with_level(false)
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false);

    let fmt_layer = layer()
        .event_format(console_format)
        .with_writer(io::stdout)
        .with_filter(LevelFilter::INFO);

    let file_appender = daily(path.as_ref(), filename);
    let (no_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = layer()
        .event_format(file_format)
        .with_ansi(false)
        .with_writer(no_blocking)
        .with_filter(LevelFilter::INFO);

    Registry::default().with(fmt_layer).with(file_layer).init();

    guard
}

pub fn log_init_with_config(
    _log_dir: &str,
    file_prefix: &str,
    config: LoggingConfig,
) -> anyhow::Result<Option<WorkerGuard>> {
    let level_filter = match config.level.to_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        _ => LevelFilter::INFO,
    };

    let filter_expression = build_env_filter_expression(level_filter, config.level_config.as_ref());

    if config.file.enabled {
        // 处理日志目录路径，支持相对路径转绝对路径
        let log_dir = if std::path::Path::new(&config.file.directory).is_absolute() {
            config.file.directory.clone()
        } else {
            // 如果是相对路径，基于当前工作目录的根目录（而不是bin目录）
            let current_dir =
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            let abs_log_dir = if current_dir.file_name() == Some(std::ffi::OsStr::new("bin")) {
                // 如果在bin目录，使用上级目录
                if let Some(parent) = current_dir.parent() {
                    parent.join(&config.file.directory)
                } else {
                    current_dir.join(&config.file.directory)
                }
            } else {
                current_dir.join(&config.file.directory)
            };
            abs_log_dir.to_string_lossy().to_string()
        };

        // 确保日志目录存在
        std::fs::create_dir_all(&log_dir)?;

        // 检查是否启用结构化日志
        let use_json = config.structured.unwrap_or(false);

        // 简化实现：暂时不使用条件格式化，避免类型冲突
        let main_filter_expr = format!("{},http.server=off", filter_expression);
        let debug_filter_expr = format!(
            "{},http.server=off",
            build_env_filter_expression(LevelFilter::DEBUG, config.level_config.as_ref())
        );
        let access_filter_expr = format!("http.server={}", level_filter_to_str(level_filter));

        let stdout_filter = EnvFilter::try_new(filter_expression.as_str())
            .unwrap_or_else(|_| EnvFilter::new(level_filter_to_str(level_filter)));
        let file_filter = EnvFilter::try_new(main_filter_expr.as_str())
            .unwrap_or_else(|_| EnvFilter::new(level_filter_to_str(level_filter)));
        let access_filter = EnvFilter::try_new(access_filter_expr.as_str())
            .unwrap_or_else(|_| EnvFilter::new("http.server=info"));

        let file_appender = daily(&log_dir, format!("{}-info", file_prefix));
        let (no_blocking, guard) = tracing_appender::non_blocking(file_appender);
        let access_appender = daily(&log_dir, format!("{}-access", file_prefix));
        let (access_nb, access_guard) = tracing_appender::non_blocking(access_appender);
        let _ = ACCESS_GUARD.set(access_guard);
        let debug_guard = if config.enable_debug_file {
            let debug_appender = daily(&log_dir, format!("{}-debug", file_prefix));
            let (debug_nb, dbg_guard) = tracing_appender::non_blocking(debug_appender);
            let _ = DEBUG_GUARD.set(dbg_guard);
            Some(debug_nb)
        } else {
            None
        };

        if use_json {
            let stdout_layer = layer()
                .json()
                .with_target(false)
                .with_level(false)
                .with_writer(io::stdout)
                .with_filter(stdout_filter);

            let file_layer = layer()
                .json()
                .with_target(false)
                .with_level(false)
                .with_ansi(false)
                .with_writer(no_blocking)
                .with_filter(file_filter);

            let debug_layer = debug_guard.as_ref().map(|writer| {
                layer()
                    .json()
                    .with_target(false)
                    .with_level(false)
                    .with_ansi(false)
                    .with_writer(writer.clone())
                    .with_filter(
                        EnvFilter::try_new(debug_filter_expr.as_str()).unwrap_or_else(|_| {
                            EnvFilter::new(level_filter_to_str(LevelFilter::DEBUG))
                        }),
                    )
            });

            let access_layer = layer()
                .json()
                .with_target(false)
                .with_level(false)
                .with_ansi(false)
                .with_writer(access_nb)
                .with_filter(access_filter);

            let registry = Registry::default()
                .with(stdout_layer)
                .with(file_layer)
                .with(access_layer);

            if let Some(layer) = debug_layer {
                registry.with(layer).init();
            } else {
                registry.init();
            }
        } else {
            let console_format = Format::default()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false);

            let file_format = Format::default()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false);
            let access_format = Format::default()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false);
            let debug_format = Format::default()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false);

            let stdout_layer = layer()
                .event_format(console_format)
                .with_writer(io::stdout)
                .with_filter(stdout_filter);

            let file_layer = layer()
                .event_format(file_format)
                .with_ansi(false)
                .with_writer(no_blocking)
                .with_filter(file_filter);

            let debug_layer = debug_guard.as_ref().map(|writer| {
                layer()
                    .event_format(debug_format)
                    .with_ansi(false)
                    .with_writer(writer.clone())
                    .with_filter(
                        EnvFilter::try_new(debug_filter_expr.as_str()).unwrap_or_else(|_| {
                            EnvFilter::new(level_filter_to_str(LevelFilter::DEBUG))
                        }),
                    )
            });

            let access_layer = layer()
                .event_format(access_format)
                .with_ansi(false)
                .with_writer(access_nb)
                .with_filter(access_filter);

            let registry = Registry::default()
                .with(stdout_layer)
                .with(file_layer)
                .with(access_layer);

            if let Some(layer) = debug_layer {
                registry.with(layer).init();
            } else {
                registry.init();
            }
        }

        tracing::info!(
            event = "log.init",
            level = %config.level,
            console = true,
            file = true,
            directory = %log_dir,
            rotation = "daily",
            structured = use_json,
            split_access = true,
            access_file = format!("{}-access", file_prefix),
            split_debug = config.enable_debug_file,
            debug_file = format!("{}-debug", file_prefix)
        );
        if let Some(retention) = config.file.retention_days {
            tracing::info!(event = "log.retention", days = retention);
        }

        Ok(Some(guard))
    } else {
        let stdout_filter = EnvFilter::try_new(filter_expression.as_str())
            .unwrap_or_else(|_| EnvFilter::new(level_filter_to_str(level_filter)));

        let use_json = config.structured.unwrap_or(false);

        if use_json {
            let stdout_layer = layer()
                .json()
                .with_target(false)
                .with_level(false)
                .with_writer(io::stdout)
                .with_filter(stdout_filter);

            Registry::default().with(stdout_layer).init();
        } else {
            let console_format = Format::default()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false);

            let stdout_layer = layer()
                .event_format(console_format)
                .with_writer(io::stdout)
                .with_filter(stdout_filter);

            Registry::default().with(stdout_layer).init();
        }

        tracing::info!(event = "log.init", level = %config.level, console = true, file = false, structured = use_json);

        Ok(None)
    }
}

pub fn cleanup_old_logs(log_dir: &Path, retention_days: u32) -> anyhow::Result<()> {
    // 增强的日志清理逻辑
    if !log_dir.exists() {
        tracing::debug!("日志目录不存在: {}", log_dir.display());
        return Ok(());
    }

    let cutoff_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        - (retention_days as u64 * 24 * 60 * 60);

    let mut deleted_count = 0;
    let mut total_size_deleted = 0u64;
    let mut error_count = 0;

    tracing::info!("开始清理超过 {} 天的日志文件", retention_days);

    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();

        // 只处理日志文件（避免误删其他文件）
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        // 检查是否为日志文件（.log后缀或包含ocr-server的文件）
        if !file_name.ends_with(".log") && !file_name.contains("ocr-server") {
            continue;
        }

        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                // 使用修改时间而不是创建时间，更准确
                let check_time = metadata
                    .modified()
                    .or_else(|_| metadata.created()) // 如果没有修改时间，使用创建时间
                    .unwrap_or_else(|_| std::time::SystemTime::now());

                if let Ok(file_time) = check_time.duration_since(std::time::UNIX_EPOCH) {
                    if file_time.as_secs() < cutoff_time {
                        let file_size = metadata.len();
                        match std::fs::remove_file(&path) {
                            Ok(()) => {
                                deleted_count += 1;
                                total_size_deleted += file_size;
                                tracing::debug!("已删除过期日志: {}", path.display());
                            }
                            Err(e) => {
                                error_count += 1;
                                tracing::warn!("删除日志文件失败: {} - {}", path.display(), e);
                            }
                        }
                    }
                }
            }
        }
    }

    if deleted_count > 0 {
        let size_mb = total_size_deleted as f64 / (1024.0 * 1024.0);
        tracing::info!(
            "已清理 {} 个过期日志文件，释放空间 {:.2} MB",
            deleted_count,
            size_mb
        );
    } else {
        tracing::debug!("没有需要清理的旧日志文件");
    }

    if error_count > 0 {
        tracing::warn!("有 {} 个文件清理失败", error_count);
    }

    Ok(())
}

fn build_env_filter_expression(
    default_level: LevelFilter,
    level_config: Option<&LevelConfig>,
) -> String {
    let mut directives = vec![level_filter_to_str(default_level).to_string()];

    if let Some(cfg) = level_config {
        if let Some(level) = cfg.api.as_deref().and_then(normalize_level_str) {
            directives.push(format!("ocr_server::api={level}"));
        }
        if let Some(level) = cfg.business.as_deref().and_then(normalize_level_str) {
            // 业务日志默认挂在 util 层以及显式 business 目标
            directives.push(format!("ocr_server::util={level}"));
            directives.push(format!("business={level}"));
        }
        if let Some(level) = cfg.system.as_deref().and_then(normalize_level_str) {
            directives.push(format!("ocr_server::server={level}"));
            directives.push(format!("ocr_server::storage={level}"));
        }
        if let Some(level) = cfg.security.as_deref().and_then(normalize_level_str) {
            directives.push(format!("ocr_server::util::auth={level}"));
            directives.push(format!("security={level}"));
        }

        for (target, level_str) in &cfg.overrides {
            if let Some(level) = normalize_level_str(level_str) {
                directives.push(format!("{}={level}", normalize_directive_target(target)));
            }
        }
    }

    directives.join(",")
}

fn normalize_level_str(level: &str) -> Option<&'static str> {
    match level.to_lowercase().as_str() {
        "trace" => Some("trace"),
        "debug" => Some("debug"),
        "info" => Some("info"),
        "warn" => Some("warn"),
        "error" => Some("error"),
        _ => None,
    }
}

fn level_filter_to_str(level: LevelFilter) -> &'static str {
    match level {
        LevelFilter::OFF => "off",
        LevelFilter::ERROR => "error",
        LevelFilter::WARN => "warn",
        LevelFilter::INFO => "info",
        LevelFilter::DEBUG => "debug",
        LevelFilter::TRACE => "trace",
    }
}

fn normalize_directive_target(target: &str) -> String {
    if let Some(raw) = target.strip_prefix("target:") {
        raw.to_string()
    } else if target.contains("::") || target.starts_with("ocr_server::") {
        target.to_string()
    } else {
        let path = target.replace('.', "::");
        format!("ocr_server::{path}")
    }
}

pub fn get_log_stats(log_dir: &Path) -> anyhow::Result<serde_json::Value> {
    if !log_dir.exists() {
        return Ok(serde_json::json!({
            "total_files": 0,
            "total_size_mb": 0,
            "oldest_file": null,
            "newest_file": null
        }));
    }

    let mut total_files = 0;
    let mut total_size = 0u64;
    let mut oldest_time = None;
    let mut newest_time = None;

    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                total_files += 1;
                total_size += metadata.len();

                if let Ok(created) = metadata.created() {
                    match oldest_time {
                        None => oldest_time = Some(created),
                        Some(oldest) if created < oldest => oldest_time = Some(created),
                        _ => {}
                    }

                    match newest_time {
                        None => newest_time = Some(created),
                        Some(newest) if created > newest => newest_time = Some(created),
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(serde_json::json!({
        "total_files": total_files,
        "total_size_mb": total_size / (1024 * 1024),
        "oldest_file": oldest_time.map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()),
        "newest_file": newest_time.map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs())
    }))
}

pub fn check_log_health(log_dir: &Path) -> anyhow::Result<serde_json::Value> {
    let exists = log_dir.exists();
    let writable = if exists {
        // 尝试创建临时文件来测试写权限
        let test_file = log_dir.join(".write_test");
        let write_ok = std::fs::write(&test_file, "test").is_ok();
        if write_ok {
            let _ = std::fs::remove_file(&test_file);
        }
        write_ok
    } else {
        false
    };

    Ok(serde_json::json!({
        "status": if exists && writable { "healthy" } else { "error" },
        "directory_exists": exists,
        "directory_writable": writable,
        "path": log_dir.to_string_lossy()
    }))
}
