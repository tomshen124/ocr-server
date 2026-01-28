use crate::model::{CpuStatus, DiskStatus, MemoryStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime};
use sysinfo::{Disks, Networks, System};

#[derive(Debug, Clone, Default)]
pub struct LoadAverages {
    pub one: f64,
    pub five: f64,
    pub fifteen: f64,
}

#[derive(Debug, Clone, Default)]
pub struct NetworkUsage {
    pub bytes_in_per_sec: u64,
    pub bytes_out_per_sec: u64,
    pub active_interfaces: u32,
}

struct SystemCache {
    system: System,
    disks: Disks,
    networks: Networks,
    last_memory_refresh: Instant,
    cached_memory: MemoryStatus,
    last_cpu_refresh: Instant,
    cached_cpu: CpuStatus,
    last_disk_refresh: Instant,
    cached_disk: DiskStatus,
    last_load_refresh: Instant,
    cached_load: LoadAverages,
    last_network_refresh: Instant,
    cached_network: NetworkUsage,
    network_initialized: bool,
}

impl SystemCache {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            system: System::new_all(),
            disks: Disks::new_with_refreshed_list(),
            networks: Networks::new_with_refreshed_list(),
            last_memory_refresh: now.checked_sub(Duration::from_secs(10)).unwrap_or(now),
            cached_memory: MemoryStatus {
                total_mb: 0,
                used_mb: 0,
                usage_percent: 0.0,
            },
            last_cpu_refresh: now.checked_sub(Duration::from_secs(2)).unwrap_or(now),
            cached_cpu: CpuStatus { usage_percent: 0.0 },
            last_disk_refresh: now.checked_sub(Duration::from_secs(10)).unwrap_or(now),
            cached_disk: DiskStatus {
                total_gb: 0,
                used_gb: 0,
                usage_percent: 0.0,
            },
            last_load_refresh: now.checked_sub(Duration::from_secs(5)).unwrap_or(now),
            cached_load: LoadAverages::default(),
            last_network_refresh: now.checked_sub(Duration::from_secs(1)).unwrap_or(now),
            cached_network: NetworkUsage::default(),
            network_initialized: false,
        }
    }

    fn refresh_all(&mut self) {
        self.refresh_memory();
        self.refresh_cpu();
        self.refresh_disk();
        self.refresh_load();
        self.refresh_network();
    }

    fn refresh_memory(&mut self) {
        self.system.refresh_memory();
        let total_mb = self.system.total_memory() / (1024 * 1024);
        let used_mb = self.system.used_memory() / (1024 * 1024);
        let usage_percent = if total_mb > 0 {
            (used_mb as f32 / total_mb as f32) * 100.0
        } else {
            0.0
        };

        self.cached_memory = MemoryStatus {
            total_mb,
            used_mb,
            usage_percent,
        };
        self.last_memory_refresh = Instant::now();
    }

    fn refresh_cpu(&mut self) {
        self.system.refresh_cpu();
        std::thread::sleep(Duration::from_millis(100));
        self.system.refresh_cpu();

        let cpu_count = self.system.cpus().len();
        let total_cpu_usage: f32 = self.system.cpus().iter().map(|cpu| cpu.cpu_usage()).sum();
        let avg_cpu_usage = if cpu_count > 0 {
            total_cpu_usage / cpu_count as f32
        } else {
            0.0
        };

        self.cached_cpu = CpuStatus {
            usage_percent: avg_cpu_usage,
        };
        self.last_cpu_refresh = Instant::now();
    }

    fn refresh_disk(&mut self) {
        self.disks.refresh_list();
        self.disks.refresh();

        let mut total_bytes: u128 = 0;
        let mut used_bytes: u128 = 0;

        for disk in self.disks.list() {
            let total = disk.total_space() as u128;
            let available = disk.available_space() as u128;
            total_bytes += total;
            used_bytes += total.saturating_sub(available);
        }

        let total_gb = (total_bytes / (1024 * 1024 * 1024)) as u64;
        let used_gb = (used_bytes / (1024 * 1024 * 1024)) as u64;
        let usage_percent = if total_bytes > 0 {
            (used_bytes as f64 / total_bytes as f64) * 100.0
        } else {
            0.0
        };

        self.cached_disk = DiskStatus {
            total_gb,
            used_gb,
            usage_percent: usage_percent as f32,
        };
        self.last_disk_refresh = Instant::now();
    }

    fn refresh_load(&mut self) {
        let load = System::load_average();
        self.cached_load = LoadAverages {
            one: load.one,
            five: load.five,
            fifteen: load.fifteen,
        };
        self.last_load_refresh = Instant::now();
    }

    fn refresh_network(&mut self) {
        if !self.network_initialized {
            self.networks.refresh_list();
        }
        self.networks.refresh();

        let now = Instant::now();
        let elapsed = now
            .checked_duration_since(self.last_network_refresh)
            .unwrap_or_else(|| Duration::from_millis(1));
        let elapsed_secs = elapsed.as_secs_f64().max(0.001);

        let mut total_received: u64 = 0;
        let mut total_transmitted: u64 = 0;
        let mut active_interfaces: u32 = 0;

        for (_name, data) in &self.networks {
            total_received = total_received.saturating_add(data.received());
            total_transmitted = total_transmitted.saturating_add(data.transmitted());
            if data.received() > 0 || data.transmitted() > 0 {
                active_interfaces = active_interfaces.saturating_add(1);
            }
        }

        self.cached_network = NetworkUsage {
            bytes_in_per_sec: (total_received as f64 / elapsed_secs) as u64,
            bytes_out_per_sec: (total_transmitted as f64 / elapsed_secs) as u64,
            active_interfaces,
        };
        self.last_network_refresh = now;
        self.network_initialized = true;
    }
}

static SYSTEM_MONITOR: LazyLock<Arc<SystemMonitor>> = LazyLock::new(|| {
    let monitor = Arc::new(SystemMonitor::new());
    monitor.refresh_once();
    monitor.spawn_refresh_loop();
    monitor
});

const SYSTEM_REFRESH_INTERVAL_SECS: u64 = 1;

struct SystemMonitor {
    cache: Mutex<SystemCache>,
}

impl SystemMonitor {
    fn new() -> Self {
        Self {
            cache: Mutex::new(SystemCache::new()),
        }
    }

    fn refresh_once(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.refresh_all();
        }
    }

    fn spawn_refresh_loop(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        std::thread::Builder::new()
            .name("system-info-refresh".into())
            .spawn(move || {
                let interval = Duration::from_secs(SYSTEM_REFRESH_INTERVAL_SECS);
                while let Some(strong) = weak.upgrade() {
                    strong.refresh_once();
                    std::thread::sleep(interval);
                }
            })
            .expect("无法启动系统信息刷新线程");
    }

    fn snapshot<T>(&self, f: impl FnOnce(&SystemCache) -> T) -> T {
        let guard = self.cache.lock().expect("system cache poisoned");
        f(&guard)
    }
}

// 服务启动时间
static START_TIME: AtomicU64 = AtomicU64::new(0);

// 初始化服务启动时间
pub fn init_start_time() {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs();
    START_TIME.store(now, Ordering::SeqCst);
}

// 获取服务运行时间（秒）
pub fn get_uptime_seconds() -> u64 {
    let start_time = START_TIME.load(Ordering::SeqCst);
    if start_time == 0 {
        return 0;
    }

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs();

    now.saturating_sub(start_time)
}

// 获取内存使用情况
pub fn get_memory_usage() -> MemoryStatus {
    system_monitor().snapshot(|cache| cache.cached_memory.clone())
}

// 获取CPU使用情况
pub fn get_cpu_usage() -> CpuStatus {
    system_monitor().snapshot(|cache| cache.cached_cpu.clone())
}

// 获取磁盘使用情况
pub fn get_disk_usage() -> DiskStatus {
    system_monitor().snapshot(|cache| cache.cached_disk.clone())
}

pub fn get_load_average() -> LoadAverages {
    system_monitor().snapshot(|cache| cache.cached_load.clone())
}

pub fn get_network_usage() -> NetworkUsage {
    system_monitor().snapshot(|cache| cache.cached_network.clone())
}

// 检查数据库连接（示例函数，需要根据实际情况实现）
pub async fn check_database_connection() -> bool {
    // 这里应该实现实际的数据库连接检查
    // 由于当前项目可能没有数据库，这里只是一个示例
    true
}

// 获取队列状态（示例函数，需要根据实际情况实现）
pub async fn get_queue_status() -> crate::model::QueueStatus {
    // 这里应该实现实际的队列状态获取
    // 由于当前项目可能没有队列，这里只是一个示例
    crate::model::QueueStatus {
        pending: 0,
        processing: 0,
        completed_last_hour: 0,
        failed_last_hour: 0,
    }
}

fn system_monitor() -> &'static Arc<SystemMonitor> {
    &*SYSTEM_MONITOR
}
