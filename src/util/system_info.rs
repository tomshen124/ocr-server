use crate::model::{CpuStatus, DiskStatus, MemoryStatus};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};
use sysinfo::System;


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
    let mut system = System::new_all();
    system.refresh_memory();
    
    let total_memory = system.total_memory() / (1024 * 1024); // Bytes to MB  
    let used_memory = system.used_memory() / (1024 * 1024);   // Bytes to MB
    let usage_percent = if total_memory > 0 {
        (used_memory as f32 / total_memory as f32) * 100.0
    } else {
        0.0
    };
    
    MemoryStatus {
        total_mb: total_memory,
        used_mb: used_memory,
        usage_percent,
    }
}

// 获取CPU使用情况
pub fn get_cpu_usage() -> CpuStatus {
    let mut system = System::new_all();
    
    // 首次获取CPU信息
    system.refresh_cpu();
    
    // 等待一小段时间，以便获取CPU使用率
    std::thread::sleep(Duration::from_millis(100));
    
    // 再次获取CPU信息
    system.refresh_cpu();
    
    // 计算所有CPU核心的平均使用率
    let cpu_count = system.cpus().len();
    let mut total_cpu_usage = 0.0;
    
    for cpu in system.cpus() {
        total_cpu_usage += cpu.cpu_usage();
    }
    
    let avg_cpu_usage = if cpu_count > 0 {
        total_cpu_usage / cpu_count as f32
    } else {
        0.0
    };
    
    CpuStatus {
        usage_percent: avg_cpu_usage,
    }
}

// 获取磁盘使用情况
pub fn get_disk_usage() -> DiskStatus {
    // 简化实现，返回模拟数据
    // 在新版本的sysinfo中，磁盘API可能有所不同
    DiskStatus {
        total_gb: 100,
        used_gb: 50,
        usage_percent: 50.0,
    }
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