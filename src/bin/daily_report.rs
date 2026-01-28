use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use chrono::{Duration, Local, TimeZone, Utc};
use tracing::info;

use ocr_server::db::{PreviewFilter, PreviewRecord, PreviewStatus};
use ocr_server::server::{ConfigManager, DatabaseInitializer};

#[tokio::main]
async fn main() -> Result<()> {
    let (config, validation) =
        ConfigManager::load_and_validate().context("加载配置文件失败，无法生成日报")?;

    if validation.has_errors() {
        return Err(anyhow!(
            "配置验证失败，共 {} 个错误，请先修复配置",
            validation.error_count()
        ));
    }

    let _log_guard = ConfigManager::initialize_logging(&config).context("初始化日志系统失败")?;

    ocr_server::initialize_globals();

    let database = DatabaseInitializer::create_from_config(&config)
        .await
        .context("初始化数据库失败")?;

    let (start_utc, end_utc) = today_time_range_utc();
    let mut filter = PreviewFilter::default();
    filter.start_date = Some(start_utc);
    filter.end_date = Some(end_utc);

    let records = database
        .list_preview_records(&filter)
        .await
        .context("查询当天预审记录失败")?;

    let markdown = build_report_markdown(&records, start_utc);
    let output_path = write_report(markdown)?;

    info!(
        "[ok] 日报生成完成：{}（记录数：{}）",
        output_path.display(),
        records.len()
    );
    println!(
        "[ok] 预审日报生成完成：{}（记录数：{}）",
        output_path.display(),
        records.len()
    );

    Ok(())
}

fn today_time_range_utc() -> (chrono::DateTime<Utc>, chrono::DateTime<Utc>) {
    use chrono::LocalResult;

    let now = Local::now();
    let date = now.date_naive();
    let naive_midnight = date.and_hms_opt(0, 0, 0).unwrap();

    let local_start = match Local.from_local_datetime(&naive_midnight) {
        LocalResult::Single(dt) => dt,
        LocalResult::Ambiguous(early, _) => early,
        LocalResult::None => {
            let fallback = naive_midnight + Duration::hours(1);
            Local
                .from_local_datetime(&fallback)
                .earliest()
                .expect("无法解析本地午夜时间")
                - Duration::hours(1)
        }
    };

    let local_end = local_start + Duration::days(1);

    (
        local_start.with_timezone(&Utc),
        local_end.with_timezone(&Utc),
    )
}

fn build_report_markdown(records: &[PreviewRecord], start_utc: chrono::DateTime<Utc>) -> String {
    let total = records.len();
    let completed = records
        .iter()
        .filter(|rec| matches!(rec.status, PreviewStatus::Completed))
        .count();
    let failed = records
        .iter()
        .filter(|rec| matches!(rec.status, PreviewStatus::Failed))
        .count();
    let processing = records
        .iter()
        .filter(|rec| matches!(rec.status, PreviewStatus::Processing))
        .count();
    let queued = records
        .iter()
        .filter(|rec| matches!(rec.status, PreviewStatus::Queued))
        .count();

    let mut user_counts: HashMap<&str, usize> = HashMap::new();
    let mut matter_counts: HashMap<&str, usize> = HashMap::new();

    for record in records {
        *user_counts.entry(record.user_id.as_str()).or_insert(0) += 1;
        *matter_counts.entry(record.file_name.as_str()).or_insert(0) += 1;
    }

    let duplicate_users: Vec<_> = user_counts
        .iter()
        .filter(|(_, count)| **count > 1)
        .collect();

    let mut top_matters: Vec<_> = matter_counts.into_iter().collect();
    top_matters.sort_by(|a, b| b.1.cmp(&a.1));
    if top_matters.len() > 10 {
        top_matters.truncate(10);
    }

    let report_date = start_utc
        .with_timezone(&Local)
        .format("%Y-%m-%d")
        .to_string();

    let mut markdown = String::new();
    markdown.push_str(&format!("# 预审处理日报 - {}\n\n", report_date));
    markdown.push_str(&format!(
        "- 总处理量：{}\n- 成功：{}\n- 失败：{}\n- 正在处理：{}\n- 排队等待：{}\n\n",
        total, completed, failed, processing, queued
    ));

    markdown.push_str("## 重复用户请求\n");
    if duplicate_users.is_empty() {
        markdown.push_str("- 无重复用户请求\n\n");
    } else {
        for (user_id, count) in duplicate_users {
            markdown.push_str(&format!("- 用户 `{}` 提交 {} 个请求\n", user_id, count));
        }
        markdown.push('\n');
    }

    markdown.push_str("## 热门事项（Top 10）\n");
    if top_matters.is_empty() {
        markdown.push_str("- 今日无事项数据\n");
    } else {
        for (matter, count) in top_matters {
            markdown.push_str(&format!("- {}：{} 次\n", matter, count));
        }
    }

    markdown
}

fn write_report(markdown: String) -> Result<PathBuf> {
    let report_dir = PathBuf::from("runtime/reports");
    fs::create_dir_all(&report_dir)
        .with_context(|| format!("创建报表目录失败: {}", report_dir.display()))?;

    let report_name = Local::now().format("preview-report-%Y%m%d.md").to_string();
    let report_path = report_dir.join(report_name);
    fs::write(&report_path, markdown)
        .with_context(|| format!("写入日报文件失败: {}", report_path.display()))?;

    Ok(report_path)
}
