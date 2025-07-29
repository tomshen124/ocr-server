//! 规则引擎模块
//! 处理zen规则内容的加载、缓存和管理

use crate::util::WebResult;
use axum::extract::Multipart;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::read_to_string;
use std::sync::LazyLock;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use zen_engine::model::DecisionContent;
use ocr_conn::CURRENT_DIR;

// 规则引擎相关结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulateRequest {
    pub context: serde_json::Value,
    pub content: DecisionContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub code: String,
    pub content: String,
}

// 规则内容缓存 - 存储DecisionContent
static RULES_CACHE: LazyLock<RwLock<HashMap<String, DecisionContent>>> = LazyLock::new(|| {
    let mut cache = HashMap::new();

    // 加载默认规则（向后兼容）
    if let Ok(rule_content) = read_to_string(CURRENT_DIR.join("graph.json")) {
        if let Ok(decision_content) = serde_json::from_str::<DecisionContent>(&rule_content) {
            cache.insert("default".to_string(), decision_content);
            info!("已加载默认规则文件: graph.json");
        }
    }

    // 加载所有主题规则
    let themes_config = super::theme::get_available_themes();
    for theme in &themes_config {
        if !theme.enabled {
            info!("跳过禁用的主题: {} ({})", theme.id, theme.name);
            continue;
        }

        let rule_path = CURRENT_DIR.join(format!("rules/{}.json", theme.id));
        match read_to_string(&rule_path) {
            Ok(rule_content) => {
                match serde_json::from_str::<DecisionContent>(&rule_content) {
                    Ok(decision_content) => {
                        cache.insert(theme.id.clone(), decision_content);
                        info!("已加载主题规则: {} ({}) -> rules/{}.json", theme.id, theme.name, theme.id);
                    }
                    Err(e) => {
                        error!("解析主题规则文件失败: rules/{}.json - {}", theme.id, e);
                    }
                }
            }
            Err(e) => {
                warn!("主题规则文件不存在或读取失败: rules/{}.json - {}", theme.id, e);
            }
        }
    }

    info!("规则缓存初始化完成，共加载 {} 个规则", cache.len());
    RwLock::new(cache)
});

// 向后兼容：保持原有的RULE静态变量
static RULE: LazyLock<RwLock<DecisionContent>> = LazyLock::new(|| {
    let rule = read_to_string(CURRENT_DIR.join("graph.json")).unwrap_or_default();
    let decision_content: DecisionContent = serde_json::from_str(&rule).unwrap_or_else(|_| {
        // 创建一个空的DecisionContent
        DecisionContent {
            nodes: vec![],
            edges: vec![],
        }
    });
    RwLock::new(decision_content)
});

/// 根据主题ID获取规则
pub async fn get_rule_by_theme(theme_id: Option<&str>) -> DecisionContent {
    let cache = RULES_CACHE.read().await;

    if let Some(theme_id) = theme_id {
        info!("尝试获取主题规则: {}", theme_id);

        if let Some(rule) = cache.get(theme_id) {
            info!("✅ 找到主题规则: {}", theme_id);
            return rule.clone();
        } else {
            warn!("⚠️  主题规则不存在: {}，使用默认规则", theme_id);
        }
    }

    // 回退到默认规则
    if let Some(default_rule) = cache.get("default") {
        info!("使用默认规则");
        default_rule.clone()
    } else {
        warn!("⚠️  默认规则也不存在，返回空规则");
        DecisionContent {
            nodes: vec![],
            edges: vec![],
        }
    }
}

/// 重新加载指定主题的规则
pub async fn reload_theme_rule(theme_id: &str) -> anyhow::Result<WebResult> {
    info!("重新加载主题规则: {}", theme_id);
    
    let rule_path = if theme_id == "default" {
        CURRENT_DIR.join("graph.json")
    } else {
        CURRENT_DIR.join(format!("rules/{}.json", theme_id))
    };
    
    match read_to_string(&rule_path) {
        Ok(rule_content) => {
            match serde_json::from_str::<DecisionContent>(&rule_content) {
                Ok(decision_content) => {
                    // 更新缓存
                    let mut cache = RULES_CACHE.write().await;
                    cache.insert(theme_id.to_string(), decision_content.clone());
                    
                    // 如果是默认规则，同时更新RULE
                    if theme_id == "default" {
                        *RULE.write().await = decision_content;
                    }
                    
                    info!("✅ 主题规则重新加载成功: {}", theme_id);
                    Ok(WebResult::ok(format!("主题规则 {} 重新加载成功", theme_id)))
                }
                Err(e) => {
                    error!("解析主题规则文件失败: {:?} - {}", rule_path, e);
                    Err(anyhow::anyhow!("解析规则文件失败: {}", e))
                }
            }
        }
        Err(e) => {
            error!("读取主题规则文件失败: {:?} - {}", rule_path, e);
            Err(anyhow::anyhow!("读取规则文件失败: {}", e))
        }
    }
}

/// 更新默认规则文件
pub async fn update_rule(mut multipart: Multipart) -> anyhow::Result<WebResult> {
    if let Some(file) = multipart.next_field().await? {
        let rule = file.text().await?;

        // 验证规则格式
        let decision_content: DecisionContent = serde_json::from_str(&rule)?;

        // 更新默认规则
        *RULE.write().await = decision_content.clone();
        tokio::fs::write("graph.json", rule.as_bytes()).await?;

        // 同时更新缓存中的默认规则
        let mut cache = RULES_CACHE.write().await;
        cache.insert("default".to_string(), decision_content);
        info!("✅ 默认规则已更新并同步到缓存");

        return Ok(WebResult::ok("Update success"));
    }

    Err(anyhow::anyhow!("No file found in multipart request"))
}

/// 获取默认规则（向后兼容）
pub async fn get_default_rule() -> DecisionContent {
    RULE.read().await.clone()
}

/// 获取所有已加载的规则主题ID列表
pub async fn get_loaded_rule_themes() -> Vec<String> {
    let cache = RULES_CACHE.read().await;
    cache.keys().cloned().collect()
}

/// 检查指定主题的规则是否已加载
pub async fn is_rule_loaded(theme_id: &str) -> bool {
    let cache = RULES_CACHE.read().await;
    cache.contains_key(theme_id)
}

/// 清空规则缓存（谨慎使用）
pub async fn clear_rules_cache() {
    let mut cache = RULES_CACHE.write().await;
    cache.clear();
    info!("⚠️  规则缓存已清空");
}

/// 重新初始化所有规则缓存
pub async fn reinitialize_rules_cache() {
    clear_rules_cache().await;
    
    // 重新加载默认规则
    if let Ok(rule_content) = read_to_string(CURRENT_DIR.join("graph.json")) {
        if let Ok(decision_content) = serde_json::from_str::<DecisionContent>(&rule_content) {
            let mut cache = RULES_CACHE.write().await;
            cache.insert("default".to_string(), decision_content.clone());
            *RULE.write().await = decision_content;
            info!("已重新加载默认规则文件: graph.json");
        }
    }
    
    // 重新加载所有主题规则
    let themes_config = super::theme::get_available_themes();
    let mut cache = RULES_CACHE.write().await;
    
    for theme in &themes_config {
        if !theme.enabled {
            continue;
        }

        let rule_path = CURRENT_DIR.join(format!("rules/{}.json", theme.id));
        match read_to_string(&rule_path) {
            Ok(rule_content) => {
                match serde_json::from_str::<DecisionContent>(&rule_content) {
                    Ok(decision_content) => {
                        cache.insert(theme.id.clone(), decision_content);
                        info!("已重新加载主题规则: {} -> rules/{}.json", theme.id, theme.id);
                    }
                    Err(e) => {
                        error!("解析主题规则文件失败: rules/{}.json - {}", theme.id, e);
                    }
                }
            }
            Err(e) => {
                warn!("主题规则文件不存在或读取失败: rules/{}.json - {}", theme.id, e);
            }
        }
    }
    
    info!("✅ 规则缓存重新初始化完成，共加载 {} 个规则", cache.len());
}