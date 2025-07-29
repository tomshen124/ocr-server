//! 主题管理模块
//! 处理OCR主题配置、加载、缓存和查询功能

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Theme {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeConfig {
    pub themes: Vec<Theme>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatterThemeMapping {
    #[serde(rename = "matterId")]
    pub matter_id: String,
    #[serde(rename = "matterName")]
    pub matter_name: String,
    #[serde(rename = "themeId")]
    pub theme_id: String,
    pub priority: Option<u32>,
    pub enabled: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatterMappingConfig {
    pub mappings: Vec<MatterThemeMapping>,
}

// 全局主题配置缓存
static THEME_CONFIG: LazyLock<RwLock<ThemeConfig>> = LazyLock::new(|| {
    RwLock::new(load_theme_config())
});

// 事项映射配置缓存
static MATTER_MAPPINGS: LazyLock<RwLock<MatterMappingConfig>> = LazyLock::new(|| {
    RwLock::new(load_matter_mappings())
});

/// 加载主题配置文件
fn load_theme_config() -> ThemeConfig {
    let themes_path = "themes.json";
    
    match std::fs::read_to_string(themes_path) {
        Ok(content) => {
            match serde_json::from_str::<ThemeConfig>(&content) {
                Ok(config) => {
                    info!("✅ 成功加载主题配置文件: {}", themes_path);
                    info!("配置的主题数量: {}", config.themes.len());
                    for theme in &config.themes {
                        let status = if theme.enabled { "启用" } else { "禁用" };
                        info!("  - {}: {} ({})", theme.id, theme.name, status);
                    }
                    config
                }
                Err(e) => {
                    warn!("主题配置文件解析失败: {}", e);
                    create_default_theme_config()
                }
            }
        }
        Err(_) => {
            warn!("⚠️  主题配置文件不存在，创建默认配置");
            create_default_theme_config()
        }
    }
}

/// 创建默认主题配置
fn create_default_theme_config() -> ThemeConfig {
    let default_config = ThemeConfig {
        themes: vec![
            Theme {
                id: "theme_001".to_string(),
                name: "工程渣土准运证核准".to_string(),
                description: "杭州市工程渣土准运证核准事项的材料预审".to_string(),
                enabled: true,
            },
            Theme {
                id: "theme_002".to_string(),
                name: "建筑工程施工许可证核发".to_string(),
                description: "建筑工程施工许可证核发事项的材料预审".to_string(),
                enabled: true,
            },
            Theme {
                id: "theme_003".to_string(),
                name: "食品经营许可证核发".to_string(),
                description: "食品经营许可证核发事项的材料预审".to_string(),
                enabled: true,
            },
            Theme {
                id: "default".to_string(),
                name: "默认主题".to_string(),
                description: "通用材料预审主题".to_string(),
                enabled: true,
            },
        ],
    };
    
    // 保存默认配置到文件
    if let Ok(json_content) = serde_json::to_string_pretty(&default_config) {
        if let Err(e) = std::fs::write("themes.json", json_content) {
            warn!("创建默认主题配置文件失败: {}", e);
        } else {
            info!("✅ 已创建默认主题配置文件: themes.json");
        }
    }
    
    default_config
}

/// 加载事项映射配置
fn load_matter_mappings() -> MatterMappingConfig {
    let mapping_path = "matter-theme-mapping.json";
    
    match std::fs::read_to_string(mapping_path) {
        Ok(content) => {
            match serde_json::from_str::<MatterMappingConfig>(&content) {
                Ok(config) => {
                    info!("✅ 成功加载事项映射配置: {}", mapping_path);
                    info!("映射数量: {}", config.mappings.len());
                    config
                }
                Err(e) => {
                    warn!("事项映射配置解析失败: {}", e);
                    create_default_matter_mappings()
                }
            }
        }
        Err(_) => {
            warn!("⚠️  事项映射配置不存在，创建默认配置");
            create_default_matter_mappings()
        }
    }
}

/// 创建默认事项映射配置
fn create_default_matter_mappings() -> MatterMappingConfig {
    let mapping_path = "matter-theme-mapping.json";
    let default_mappings = MatterMappingConfig {
        mappings: vec![
            MatterThemeMapping {
                matter_id: "11010216570147206221001".to_string(),
                matter_name: "杭州市工程渣土准运证核准".to_string(),
                theme_id: "theme_001".to_string(),
                priority: Some(1),
                enabled: Some(true),
                description: Some("工程渣土准运证核准事项".to_string()),
            },
            MatterThemeMapping {
                matter_id: "11010216570147206228001".to_string(),
                matter_name: "建筑工程施工许可证核发".to_string(),
                theme_id: "theme_002".to_string(),
                priority: Some(1),
                enabled: Some(true),
                description: Some("建筑工程施工许可证核发事项".to_string()),
            },
        ],
    };
    
    // 保存默认配置到文件
    if let Ok(json_content) = serde_json::to_string_pretty(&default_mappings) {
        if let Err(e) = std::fs::write(mapping_path, json_content) {
            warn!("创建默认事项映射配置失败: {}", e);
        } else {
            info!("✅ 已创建默认事项映射配置: {}", mapping_path);
        }
    }
    
    default_mappings
}

/// 根据主题ID获取主题名称
pub fn get_theme_name(theme_id: Option<&str>) -> Option<String> {
    if let Some(theme_id) = theme_id {
        if let Ok(config) = THEME_CONFIG.try_read() {
            for theme in &config.themes {
                if theme.id == theme_id {
                    return Some(theme.name.clone());
                }
            }
        }
    }
    None
}

/// 获取所有可用主题
pub fn get_available_themes() -> Vec<Theme> {
    THEME_CONFIG.try_read()
        .map(|config| config.themes.clone())
        .unwrap_or_default()
}

/// 获取事项映射配置
pub fn get_matter_mappings() -> Vec<MatterThemeMapping> {
    MATTER_MAPPINGS.try_read()
        .map(|config| config.mappings.clone())
        .unwrap_or_default()
}

/// 根据事项信息查找对应的主题ID
pub fn find_theme_by_matter(matter_id: Option<&str>, matter_name: Option<&str>) -> String {
    info!("=== 智能主题匹配 ===");
    info!("事项ID: {:?}", matter_id);
    info!("事项名称: {:?}", matter_name);

    let mappings = get_matter_mappings();
    
    // 优先级1: 精确匹配事项ID
    if let Some(matter_id) = matter_id {
        let normalized_input_id = normalize_matter_id(matter_id);
        info!("标准化输入事项ID: {}", normalized_input_id);
        
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) {
                let normalized_mapping_id = normalize_matter_id(&mapping.matter_id);
                info!("检查映射: {} -> {}", normalized_mapping_id, mapping.theme_id);
                
                if normalized_input_id == normalized_mapping_id {
                    info!("✅ 找到精确匹配的事项ID: {} -> {}", matter_id, mapping.theme_id);
                    return mapping.theme_id.clone();
                }
            }
        }
        
        // 优先级2: 部分匹配事项ID（数字重叠度分析）
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) {
                if has_significant_digit_overlap(matter_id, &mapping.matter_id) {
                    info!("✅ 找到数字重叠匹配的事项ID: {} -> {}", matter_id, mapping.theme_id);
                    return mapping.theme_id.clone();
                }
            }
        }
    }

    // 优先级3: 事项名称匹配
    if let Some(matter_name) = matter_name {
        info!("开始事项名称匹配: {}", matter_name);
        
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) {
                // 精确名称匹配
                if matter_name == mapping.matter_name {
                    info!("✅ 找到精确匹配的事项名称: {} -> {}", matter_name, mapping.theme_id);
                    return mapping.theme_id.clone();
                }
                
                // 包含关系匹配
                if matter_name.contains(&mapping.matter_name) || mapping.matter_name.contains(matter_name) {
                    info!("✅ 找到包含匹配的事项名称: {} <-> {} -> {}", 
                          matter_name, mapping.matter_name, mapping.theme_id);
                    return mapping.theme_id.clone();
                }
                
                // 关键词匹配
                let keywords = ["渣土", "施工许可", "食品经营", "营业执照"];
                for keyword in &keywords {
                    if matter_name.contains(keyword) && mapping.matter_name.contains(keyword) {
                        info!("✅ 找到关键词匹配的事项名称: {} (关键词: {}) -> {}", 
                              matter_name, keyword, mapping.theme_id);
                        return mapping.theme_id.clone();
                    }
                }
            }
        }
    }

    // 优先级4: 最长公共子串匹配（相似度分析）
    if let Some(matter_name) = matter_name {
        let mut best_match = ("default".to_string(), 0);
        
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) {
                let common_substring = find_longest_common_substring(matter_name, &mapping.matter_name);
                let similarity_score = common_substring.len();
                
                if similarity_score > 3 && similarity_score > best_match.1 {
                    best_match = (mapping.theme_id.clone(), similarity_score);
                    info!("发现相似匹配: {} <-> {} (相似度: {})", 
                          matter_name, mapping.matter_name, similarity_score);
                }
            }
        }
        
        if best_match.1 > 3 {
            info!("✅ 使用最佳相似匹配: {} (相似度: {})", best_match.0, best_match.1);
            return best_match.0;
        }
    }

    // 默认主题
    let default_theme = "theme_001".to_string();
    info!("⚠️  未找到匹配的主题，使用默认主题: {}", default_theme);
    default_theme
}

/// 标准化事项ID（移除特殊字符，保留数字和字母）
fn normalize_matter_id(id: &str) -> String {
    id.chars().filter(|c| c.is_alphanumeric()).collect()
}

/// 检查两个事项ID之间是否有显著的数字重叠
fn has_significant_digit_overlap(id1: &str, id2: &str) -> bool {
    let digits1: String = id1.chars().filter(|c| c.is_ascii_digit()).collect();
    let digits2: String = id2.chars().filter(|c| c.is_ascii_digit()).collect();
    
    if digits1.len() < 8 || digits2.len() < 8 {
        return false;
    }
    
    let common_substring = find_longest_common_substring(&digits1, &digits2);
    let overlap_ratio = common_substring.len() as f64 / digits1.len().min(digits2.len()) as f64;
    
    overlap_ratio >= 0.6
}

/// 查找两个字符串的最长公共子串
fn find_longest_common_substring(s1: &str, s2: &str) -> String {
    let chars1: Vec<char> = s1.chars().collect();
    let chars2: Vec<char> = s2.chars().collect();
    let mut result = String::new();
    
    for i in 0..chars1.len() {
        for j in 0..chars2.len() {
            let mut k = 0;
            while i + k < chars1.len() 
                && j + k < chars2.len() 
                && chars1[i + k] == chars2[j + k] {
                k += 1;
            }
            
            if k > result.len() {
                result = chars1[i..i + k].iter().collect();
            }
        }
    }
    
    result
}

/// 重新加载主题配置
pub async fn reload_theme_config() {
    let new_config = load_theme_config();
    *THEME_CONFIG.write().await = new_config;
    info!("✅ 主题配置已重新加载");
}

/// 重新加载事项映射配置
pub async fn reload_matter_mappings() {
    let new_config = load_matter_mappings();
    *MATTER_MAPPINGS.write().await = new_config;
    info!("✅ 事项映射配置已重新加载");
}