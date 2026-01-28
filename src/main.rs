use std::io::Write;

use ocr_server::server;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    std::panic::set_hook(Box::new(|panic_info| {
        let payload = panic_info.payload();
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            s
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s
        } else {
            "Unknown panic payload"
        };

        let location = panic_info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "Unknown location".to_string());

        eprintln!("[PANIC] 程序异常退出");
        eprintln!("位置: {}", location);
        eprintln!("原因: {}", message);
        eprintln!(
            "时间: {}",
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        );
        eprintln!("建议: 请查看完整日志并报告此问题");

        if let Ok(_) = std::panic::catch_unwind(|| {
            tracing::error!(event = "panic.raised", location = %location, reason = %message, time = %chrono::Utc::now());
        }) {}

        let panic_msg = format!(
            "PANIC OCCURRED\nLocation: {}\nReason: {}\nTime: {}\n\n",
            location,
            message,
            chrono::Utc::now()
        );

        if let Err(e) = std::fs::write("./panic.log", &panic_msg) {
            eprintln!("[WARN] 无法写入panic.log: {}", e);
        } else {
            eprintln!("[OK] Panic信息已保存到 ./panic.log");
        }

        if let Ok(()) = std::fs::create_dir_all("./runtime/logs") {
            let panic_file = format!(
                "./runtime/logs/panic-{}.log",
                chrono::Utc::now().format("%Y%m%d-%H%M%S")
            );
            if let Err(e) = std::fs::write(&panic_file, &panic_msg) {
                eprintln!("[WARN] 无法写入运行时panic日志: {}", e);
            } else {
                eprintln!("[OK] Panic信息也保存到 {}", panic_file);
            }
        }

        std::io::stderr().flush().ok();
    }));

    let mut args = std::env::args();
    let _ = args.next();

    match args.next().as_deref() {
        Some("worker") | Some("--worker") => server::start_worker().await,
        Some("health-check") | Some("--health-check") => {
            let report = server::check_system_health().await?;
            println!(
                "健康检查: overall={}, db={:?}, storage={:?}",
                report.overall_healthy, report.database_health, report.storage_health
            );
            Ok(())
        }
        _ => server::start_server().await,
    }
}
