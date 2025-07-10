use crate::util::config::LoggingConfig;
use fastdate::offset_sec;
use std::io;
use std::path::Path;
use time::format_description::well_known::Rfc3339;
use time::UtcOffset;
use tracing::level_filters::LevelFilter;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::daily;
use tracing_subscriber::fmt::layer;
use tracing_subscriber::fmt::time::OffsetTime;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, Registry};

fn get_offset_time(offset: i32) -> anyhow::Result<OffsetTime<Rfc3339>> {
    Ok(OffsetTime::new(
        UtcOffset::from_whole_seconds(offset)?,
        Rfc3339,
    ))
}

pub fn log_init<P: AsRef<Path>>(path: P, filename: &str) -> WorkerGuard {
    let offset = offset_sec();

    let fmt_layer = layer()
        .with_timer(get_offset_time(offset).unwrap())
        .with_target(false)
        .with_writer(io::stdout)
        .with_filter(LevelFilter::INFO);

    let file_appender = daily(path.as_ref(), filename);
    let (no_blocking, guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = layer()
        .with_timer(get_offset_time(offset).unwrap())
        .with_target(false)
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
    let offset = offset_sec();
    
    let level_filter = match config.level.to_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        _ => LevelFilter::INFO,
    };

    if config.file.enabled {
        // 处理日志目录路径，支持相对路径转绝对路径
        let log_dir = if std::path::Path::new(&config.file.directory).is_absolute() {
            config.file.directory.clone()
        } else {
            // 如果是相对路径，基于当前工作目录的根目录（而不是bin目录）
            let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
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
        
        let fmt_layer = layer()
            .with_timer(get_offset_time(offset).unwrap())
            .with_target(false)
            .with_writer(io::stdout)
            .with_filter(level_filter);

        let file_appender = daily(&log_dir, file_prefix);
        let (no_blocking, guard) = tracing_appender::non_blocking(file_appender);
        let file_layer = layer()
            .with_timer(get_offset_time(offset).unwrap())
            .with_target(false)
            .with_ansi(false)
            .with_writer(no_blocking)
            .with_filter(level_filter);

        Registry::default().with(fmt_layer).with(file_layer).init();

        tracing::info!("=== 日志系统初始化完成 ===");
        tracing::info!("日志级别: {}", config.level);
        tracing::info!("控制台输出: true");
        tracing::info!("文件输出: true");
        tracing::info!("日志目录: {}", log_dir);
        tracing::info!("轮转策略: daily");
        if let Some(retention) = config.file.retention_days {
            tracing::info!("保留天数: {} 天", retention);
        }
        tracing::info!("结构化日志: false");

        Ok(Some(guard))
    } else {
        let fmt_layer = layer()
            .with_timer(get_offset_time(offset).unwrap())
            .with_target(false)
            .with_writer(io::stdout)
            .with_filter(level_filter);

        Registry::default().with(fmt_layer).init();

        tracing::info!("=== 日志系统初始化完成 ===");
        tracing::info!("日志级别: {}", config.level);
        tracing::info!("控制台输出: true");
        tracing::info!("文件输出: false");
        tracing::info!("结构化日志: false");

        Ok(None)
    }
}

pub fn cleanup_old_logs(log_dir: &Path, retention_days: u32) -> anyhow::Result<()> {
    // 简单的日志清理逻辑
    if !log_dir.exists() {
        tracing::debug!("日志目录不存在: {}", log_dir.display());
        return Ok(());
    }

    let cutoff_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() - (retention_days as u64 * 24 * 60 * 60);

    let mut deleted_count = 0;
    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        if let Ok(metadata) = entry.metadata() {
            if let Ok(created) = metadata.created() {
                if let Ok(created_secs) = created.duration_since(std::time::UNIX_EPOCH) {
                    if created_secs.as_secs() < cutoff_time {
                        if std::fs::remove_file(entry.path()).is_ok() {
                            deleted_count += 1;
                        }
                    }
                }
            }
        }
    }

    if deleted_count > 0 {
        tracing::info!("已清理 {} 个过期日志文件", deleted_count);
    } else {
        tracing::debug!("没有需要清理的旧日志文件");
    }

    Ok(())
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