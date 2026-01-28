use crate::util::config::types::ServiceWatchdogConfig;
use crate::util::system_info::{get_cpu_usage, get_disk_usage, get_memory_usage};
use chrono::{DateTime, Utc};
use ocr_conn::CURRENT_DIR;
use parking_lot::RwLock;
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::process;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::time::interval;

use crate::AppState;

/// 启动 Master 节点服务看门狗
pub fn spawn_master_watchdog(app_state: &AppState) {
    spawn_watchdog_task("master", app_state.config.service_watchdog.clone());
}

/// 启动 Worker 节点服务看门狗
pub fn spawn_worker_watchdog(config: &crate::util::config::Config) {
    spawn_watchdog_task("worker", config.service_watchdog.clone());
}

fn spawn_watchdog_task(role: &'static str, cfg: ServiceWatchdogConfig) {
    if !cfg.enabled {
        tracing::info!(target: "watchdog", role = role, "服务看门狗已禁用");
        return;
    }

    let interval_secs = cfg.interval_secs.max(5);
    tracing::info!(target: "watchdog", role = role, interval_secs, cpu_threshold = cfg.cpu_threshold_percent, memory_threshold = cfg.memory_threshold_percent, disk_threshold = cfg.disk_threshold_percent, "服务看门狗已启动");

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(interval_secs));
        let mut consecutive_unhealthy = 0u32;
        let mut last_violation_at: Option<DateTime<Utc>> = None;
        let mut last_restart_at: Option<Instant> = None;
        loop {
            ticker.tick().await;
            let cpu = get_cpu_usage().usage_percent as f64;
            let memory = get_memory_usage().usage_percent as f64;
            let disk = get_disk_usage().usage_percent as f64;

            let mut unhealthy = false;
            if cpu > cfg.cpu_threshold_percent {
                tracing::warn!(
                    target = "watchdog",
                    role = role,
                    cpu_percent = cpu,
                    threshold = cfg.cpu_threshold_percent,
                    "CPU 使用率超过阈值"
                );
                unhealthy = true;
            }
            if memory > cfg.memory_threshold_percent {
                tracing::warn!(
                    target = "watchdog",
                    role = role,
                    memory_percent = memory,
                    threshold = cfg.memory_threshold_percent,
                    "内存使用率超过阈值"
                );
                unhealthy = true;
            }
            if disk > cfg.disk_threshold_percent {
                tracing::warn!(
                    target = "watchdog",
                    role = role,
                    disk_percent = disk,
                    threshold = cfg.disk_threshold_percent,
                    "磁盘使用率超过阈值"
                );
                unhealthy = true;
            }

            if unhealthy {
                consecutive_unhealthy = consecutive_unhealthy.saturating_add(1);
                last_violation_at = Some(Utc::now());
                if let Err(err) = log_watchdog_snapshot(role, cpu, memory, disk).await {
                    tracing::warn!(target = "watchdog", role = role, error = %err, "记录看门狗快照失败");
                }
            } else {
                tracing::debug!(
                    target = "watchdog",
                    role = role,
                    cpu_percent = cpu,
                    memory_percent = memory,
                    disk_percent = disk,
                    "资源使用情况正常"
                );
                consecutive_unhealthy = 0;
                last_violation_at = None;
            }

            update_watchdog_state(
                role,
                cpu,
                memory,
                disk,
                consecutive_unhealthy,
                last_violation_at,
                cfg.auto_restart_on_violation,
            );

            if cfg.auto_restart_on_violation {
                // 严重突刺：超过阈值 20% 直接触发退出
                let severe_spike = cpu > cfg.cpu_threshold_percent * 1.2
                    || memory > cfg.memory_threshold_percent * 1.2
                    || disk > cfg.disk_threshold_percent * 1.2;
                let restart_due_to_consecutive =
                    consecutive_unhealthy >= cfg.max_consecutive_violations;
                let cooldown_ok = last_restart_at
                    .map(|ts| ts.elapsed().as_secs() >= cfg.restart_cooldown_secs)
                    .unwrap_or(true);

                if (restart_due_to_consecutive || severe_spike) && cooldown_ok {
                    tracing::error!(
                        target = "watchdog",
                        role = role,
                        consecutive_violations = consecutive_unhealthy,
                        severe_spike = severe_spike,
                        cpu_percent = cpu,
                        memory_percent = memory,
                        disk_percent = disk,
                        restart_exit_code = cfg.restart_exit_code,
                        "资源异常，触发自我保护退出"
                    );
                    if let Err(err) = log_watchdog_snapshot(role, cpu, memory, disk).await {
                        tracing::warn!(
                            target = "watchdog",
                            role = role,
                            error = %err,
                            "退出前记录快照失败"
                        );
                    }
                    update_watchdog_restart(role);
                    // last_restart_at = Some(Instant::now()); // Unused assignment before exit
                    process::exit(cfg.restart_exit_code);
                }
            }
        }
    });
}

async fn log_watchdog_snapshot(role: &str, cpu: f64, memory: f64, disk: f64) -> anyhow::Result<()> {
    let logs_dir = CURRENT_DIR.join("runtime").join("logs").join("watchdog");
    fs::create_dir_all(&logs_dir).await?;
    let timestamp = chrono::Utc::now().format("%Y%m%d%H%M%S");
    let file_name = format!("watchdog-{}-{}.json", role, timestamp);
    let path = logs_dir.join(file_name);
    let payload = json!({
        "role": role,
        "timestamp": Utc::now().to_rfc3339(),
        "cpu_percent": cpu,
        "memory_percent": memory,
        "disk_percent": disk,
    });
    fs::write(path, payload.to_string()).await?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchdogStateSnapshot {
    pub role: String,
    pub last_checked_at: DateTime<Utc>,
    pub cpu_percent: f64,
    pub memory_percent: f64,
    pub disk_percent: f64,
    pub consecutive_violations: u32,
    pub last_violation_at: Option<DateTime<Utc>>,
    pub auto_restart_enabled: bool,
    pub last_restart_trigger_at: Option<DateTime<Utc>>,
}

static WATCHDOG_STATE: LazyLock<Arc<RwLock<HashMap<&'static str, WatchdogStateSnapshot>>>> =
    LazyLock::new(|| Arc::new(RwLock::new(HashMap::new())));

fn update_watchdog_state(
    role: &'static str,
    cpu: f64,
    memory: f64,
    disk: f64,
    consecutive: u32,
    last_violation: Option<DateTime<Utc>>,
    auto_restart: bool,
) {
    let mut guard = WATCHDOG_STATE.write();
    let last_restart = guard
        .get(role)
        .and_then(|prev| prev.last_restart_trigger_at);
    guard.insert(
        role,
        WatchdogStateSnapshot {
            role: role.to_string(),
            last_checked_at: Utc::now(),
            cpu_percent: cpu,
            memory_percent: memory,
            disk_percent: disk,
            consecutive_violations: consecutive,
            last_violation_at: last_violation,
            auto_restart_enabled: auto_restart,
            last_restart_trigger_at: last_restart,
        },
    );
}

fn update_watchdog_restart(role: &'static str) {
    let mut guard = WATCHDOG_STATE.write();
    if let Some(state) = guard.get_mut(role) {
        state.last_restart_trigger_at = Some(Utc::now());
    }
}

pub fn list_watchdog_states() -> Vec<WatchdogStateSnapshot> {
    let guard = WATCHDOG_STATE.read();
    guard.values().cloned().collect()
}

pub fn get_watchdog_state(role: &str) -> Option<WatchdogStateSnapshot> {
    let guard = WATCHDOG_STATE.read();
    guard.get(role).cloned()
}
