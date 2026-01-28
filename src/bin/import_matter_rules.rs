use std::path::{Path, PathBuf};
use std::{env, fs};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use ocr_server::db::traits::MatterRuleConfigRecord;
use ocr_server::server::database::DatabaseInitializer;
use ocr_server::util::rules::{MatterRuleDefinition, RuleRepository};
use ocr_server::CONFIG;
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let inputs: Vec<PathBuf> = {
        let args: Vec<String> = env::args().skip(1).collect();
        if args.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            args.into_iter().map(PathBuf::from).collect()
        }
    };

    let mut files = Vec::new();
    for input in inputs {
        if input.is_dir() {
            collect_matter_files_from_dir(&input, &mut files)?;
        } else if input.is_file() {
            if is_matter_file(&input) {
                files.push(input);
            } else {
                warn!("忽略非规则文件: {}", input.display());
            }
        } else {
            warn!("路径不存在，已忽略: {}", input.display());
        }
    }

    if files.is_empty() {
        return Err(anyhow!("未找到任何 matter-*.json 规则文件"));
    }

    info!("共发现 {} 个规则文件，开始导入", files.len());

    let database = DatabaseInitializer::create_from_config(&CONFIG)
        .await
        .context("初始化数据库连接失败")?;
    let repository = RuleRepository::new(database);

    let mut success = 0usize;
    for path in &files {
        match import_single_file(&repository, path).await {
            Ok(_) => {
                success += 1;
            }
            Err(err) => {
                error!("导入失败 [{}]: {}", path.display(), err);
            }
        }
    }

    info!(
        "规则导入完成: 成功 {} 条，失败 {} 条",
        success,
        files.len() - success
    );

    if success == 0 {
        Err(anyhow!("没有任何规则成功导入"))
    } else {
        Ok(())
    }
}

fn collect_matter_files_from_dir(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("读取目录失败: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if is_matter_file(&path) {
                files.push(path);
            }
        }
    }
    Ok(())
}

fn is_matter_file(path: &Path) -> bool {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        name.starts_with("matter-") && name.ends_with(".json")
    } else {
        false
    }
}

async fn import_single_file(repository: &RuleRepository, path: &Path) -> Result<()> {
    info!("导入规则文件: {}", path.display());
    let raw = fs::read_to_string(path)
        .with_context(|| format!("读取规则文件失败: {}", path.display()))?;

    let definition: MatterRuleDefinition = serde_json::from_str(&raw)
        .with_context(|| format!("解析规则JSON失败: {}", path.display()))?;

    if definition.matter_id.trim().is_empty() {
        return Err(anyhow!("规则文件缺少 matterId 字段: {}", path.display()));
    }

    let mut checksum_hasher = Sha256::new();
    checksum_hasher.update(raw.as_bytes());
    let checksum = format!("{:x}", checksum_hasher.finalize());

    let now = Utc::now();
    let updated_by = detect_updated_by();

    let existing = repository.fetch(&definition.matter_id).await?;

    let (id, created_at, status, description) = if let Some(existing) = existing {
        info!(
            "事项 {} 已存在规则记录，执行更新",
            existing.record.matter_id
        );
        (
            existing.record.id.clone(),
            existing.record.created_at,
            existing.record.status.clone(),
            definition
                .description
                .clone()
                .or(existing.record.description.clone()),
        )
    } else {
        let desc = definition.description.clone();
        (
            uuid::Uuid::new_v4().to_string(),
            now,
            "active".to_string(),
            desc,
        )
    };

    let record = MatterRuleConfigRecord {
        id,
        matter_id: definition.matter_id.clone(),
        matter_name: definition.matter_name.clone(),
        spec_version: definition.spec_version.clone(),
        mode: definition.mode.as_str().to_string(),
        rule_payload: raw,
        status,
        description,
        checksum: Some(checksum),
        updated_by,
        created_at,
        updated_at: now,
    };

    repository
        .upsert(record)
        .await
        .with_context(|| format!("写入数据库失败: {}", path.display()))?;

    info!("规则导入成功: matter_id={} ", definition.matter_id);
    Ok(())
}

fn detect_updated_by() -> Option<String> {
    let candidates = [
        env::var("UPDATED_BY").ok(),
        env::var("USER").ok(),
        env::var("USERNAME").ok(),
    ];

    candidates
        .into_iter()
        .flatten()
        .find(|s| !s.trim().is_empty())
}
