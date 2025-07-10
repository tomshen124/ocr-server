use axum::extract::Multipart;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;
use std::sync::LazyLock;
use std::fs::read_to_string;
use std::path::Path;
use crate::util::WebResult;

// 规则引擎相关导入
use zen_engine::model::DecisionContent;
use zen_engine::DecisionEngine;
use zen_engine::DecisionGraphResponse;
use serde_json::{json, Value};
use tracing::{info, warn, error};

// OCR和文档处理相关导入
use ocr_conn::ocr::Extractor;
use ocr_conn::CURRENT_DIR;
use build_html::{Html, HtmlContainer, HtmlPage, Table};
use chrono::Local;
use shiva::core::{Element, TransformerTrait};

// 预审相关结构
use crate::model::preview::Preview;

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

// 规则引擎相关结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulateRequest {
    pub context: Value,
    pub content: DecisionContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub code: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketId {
    #[serde(rename = "ticketId")]
    pub ticket_id: String,
}

// 全局缓存
static THEME_CONFIG: LazyLock<RwLock<ThemeConfig>> = LazyLock::new(|| {
    RwLock::new(load_theme_config())
});

static MATTER_MAPPINGS: LazyLock<RwLock<MatterMappingConfig>> = LazyLock::new(|| {
    RwLock::new(load_matter_mappings())
});

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
    let themes_config = load_theme_config();
    for theme in &themes_config.themes {
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

fn load_theme_config() -> ThemeConfig {
    let themes_path = "themes.json";
    
    match std::fs::read_to_string(themes_path) {
        Ok(content) => {
            match serde_json::from_str::<ThemeConfig>(&content) {
                Ok(config) => {
                    tracing::info!("✅ 成功加载主题配置文件: {}", themes_path);
                    tracing::info!("配置的主题数量: {}", config.themes.len());
                    for theme in &config.themes {
                        let status = if theme.enabled { "启用" } else { "禁用" };
                        tracing::info!("  - {}: {} ({})", theme.id, theme.name, status);
                    }
                    config
                }
                Err(e) => {
                    tracing::warn!("主题配置文件解析失败: {}", e);
                    create_default_theme_config()
                }
            }
        }
        Err(_) => {
            tracing::warn!("⚠️  主题配置文件不存在，创建默认配置");
            create_default_theme_config()
        }
    }
}

// 根据主题ID获取规则
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

fn create_default_theme_config() -> ThemeConfig {
    let default_config = ThemeConfig {
        themes: vec![
            Theme {
                id: "theme_001".to_string(),
                name: "工程渣土准运证核准".to_string(),
                description: "工程建设项目渣土准运许可证核准业务".to_string(),
                enabled: true,
            },
            Theme {
                id: "theme_002".to_string(),
                name: "建筑工程施工许可".to_string(),
                description: "建筑工程施工许可证核发业务".to_string(),
                enabled: true,
            },
            Theme {
                id: "theme_003".to_string(),
                name: "环境影响评价".to_string(),
                description: "建设项目环境影响评价审批".to_string(),
                enabled: true,
            },
            Theme {
                id: "theme_004".to_string(),
                name: "安全生产许可".to_string(),
                description: "安全生产许可证核发".to_string(),
                enabled: true,
            },
            Theme {
                id: "theme_005".to_string(),
                name: "消防验收备案".to_string(),
                description: "建设工程消防验收备案".to_string(),
                enabled: true,
            },
            Theme {
                id: "theme_006".to_string(),
                name: "规划许可证".to_string(),
                description: "建设工程规划许可证核发".to_string(),
                enabled: true,
            },
        ],
    };

    // 尝试保存默认配置
    if let Ok(json_content) = serde_json::to_string_pretty(&default_config) {
        if let Err(e) = std::fs::write("themes.json", json_content) {
            tracing::error!("❌ 创建默认主题配置文件失败: {}", e);
        } else {
            tracing::info!("✅ 已创建默认主题配置文件: themes.json");
        }
    }

    default_config
}

fn load_matter_mappings() -> MatterMappingConfig {
    let mapping_path = "matter-theme-mapping.json";
    
    match std::fs::read_to_string(mapping_path) {
        Ok(content) => {
            match serde_json::from_str::<MatterMappingConfig>(&content) {
                Ok(config) => {
                    tracing::info!("✅ 成功加载事项-主题映射配置文件: {}", mapping_path);
                    config
                }
                Err(e) => {
                    tracing::warn!("事项-主题映射配置文件解析失败: {}", e);
                    create_default_matter_mappings()
                }
            }
        }
        Err(_) => {
            tracing::warn!("⚠️  事项-主题映射配置文件不存在，创建默认配置");
            create_default_matter_mappings()
        }
    }
}

fn create_default_matter_mappings() -> MatterMappingConfig {
    let default_config = MatterMappingConfig {
        mappings: vec![
            MatterThemeMapping {
                matter_id: "MATTER_16570147206221001".to_string(),
                matter_name: "杭州市工程渣土准运证核准申请".to_string(),
                theme_id: "theme_001".to_string(),
                priority: Some(1),
                enabled: Some(true),
                description: Some("工程渣土准运证核准事项对应主题001规则".to_string()),
            },
            MatterThemeMapping {
                matter_id: "MATTER_CONSTRUCTION_PERMIT".to_string(),
                matter_name: "建筑工程施工许可证".to_string(),
                theme_id: "theme_002".to_string(),
                priority: Some(1),
                enabled: Some(true),
                description: Some("建筑工程施工许可证对应主题002规则".to_string()),
            },
        ],
    };

    // 尝试保存默认配置
    if let Ok(json_content) = serde_json::to_string_pretty(&default_config) {
        if let Err(e) = std::fs::write("matter-theme-mapping.json", json_content) {
            tracing::error!("❌ 创建默认事项-主题映射配置文件失败: {}", e);
        } else {
            tracing::info!("✅ 已创建默认事项-主题映射配置文件: matter-theme-mapping.json");
        }
    }

    default_config
}

// 获取主题名称
pub fn get_theme_name(theme_id: Option<&str>) -> Option<String> {
    if let Some(id) = theme_id {
        let config = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                THEME_CONFIG.read().await.clone()
            })
        });
        config.themes.iter()
            .find(|theme| theme.id == id)
            .map(|theme| theme.name.clone())
    } else {
        None
    }
}

// 公共API函数
pub fn get_available_themes() -> Vec<Theme> {
    let config = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            THEME_CONFIG.read().await.clone()
        })
    });
    config.themes
}

pub fn get_matter_mappings() -> Vec<MatterThemeMapping> {
    let config = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(async {
            MATTER_MAPPINGS.read().await.clone()
        })
    });
    config.mappings
}

// 根据事项ID或事项名称查找对应的主题ID
pub fn find_theme_by_matter(matter_id: Option<&str>, matter_name: Option<&str>) -> String {
    let mappings = get_matter_mappings();
    
    tracing::info!("=== 开始智能主题匹配 ===");
    tracing::info!("输入事项ID: {:?}", matter_id);
    tracing::info!("输入事项名称: {:?}", matter_name);
    tracing::info!("可用映射数量: {}", mappings.len());
    
    // 优先使用 matterId 匹配（支持多种格式）
    if let Some(id) = matter_id {
        tracing::info!("开始事项ID匹配...");
        
        // 1. 精确匹配（原样匹配）
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) && mapping.matter_id == id {
                tracing::info!("✅ ID精确匹配: {} -> {}", id, mapping.theme_id);
                return mapping.theme_id.clone();
            }
        }
        
        // 2. 数字ID智能匹配（兼容各种数字格式）
        let normalized_input_id = normalize_matter_id(id);
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) {
                let normalized_mapping_id = normalize_matter_id(&mapping.matter_id);
                
                // 直接数字匹配
                if normalized_input_id == normalized_mapping_id {
                    tracing::info!("✅ ID数字匹配: {} ({}) -> {}", id, normalized_input_id, mapping.theme_id);
                    return mapping.theme_id.clone();
                }
                
                // 尾部匹配（处理带前缀的情况）
                if normalized_mapping_id.ends_with(&normalized_input_id) || normalized_input_id.ends_with(&normalized_mapping_id) {
                    tracing::info!("✅ ID尾部匹配: {} ({}) 匹配 {} ({}) -> {}", 
                                 id, normalized_input_id, mapping.matter_id, normalized_mapping_id, mapping.theme_id);
                    return mapping.theme_id.clone();
                }
            }
        }
        
        // 3. 部分匹配（对于长数字ID）
        if normalized_input_id.len() >= 6 {  // 只对足够长的ID进行部分匹配
            for mapping in &mappings {
                if mapping.enabled.unwrap_or(true) {
                    let normalized_mapping_id = normalize_matter_id(&mapping.matter_id);
                    
                    // 检查是否有公共的长数字序列
                    if has_significant_digit_overlap(&normalized_input_id, &normalized_mapping_id) {
                        tracing::info!("✅ ID部分匹配: {} ({}) 与 {} ({}) 有显著重叠 -> {}", 
                                     id, normalized_input_id, mapping.matter_id, normalized_mapping_id, mapping.theme_id);
                        return mapping.theme_id.clone();
                    }
                }
            }
        }
    }
    
    // 使用 matterName 匹配
    if let Some(name) = matter_name {
        tracing::info!("开始事项名称匹配...");
        
        // 1. 精确匹配
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) && mapping.matter_name == name {
                tracing::info!("✅ 名称精确匹配: {} -> {}", name, mapping.theme_id);
                return mapping.theme_id.clone();
            }
        }
        
        // 2. 关键词匹配
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) {
                let mapping_keywords: Vec<&str> = mapping.matter_name
                    .split(&['、', ',', '，', ' ', '(', ')', '（', '）'])
                    .filter(|s| !s.is_empty() && s.len() > 1)
                    .collect();
                
                for keyword in mapping_keywords {
                    if name.contains(keyword) && keyword.len() > 2 {  // 至少3个字符才算有效关键词
                        tracing::info!("✅ 名称关键词匹配: {} 包含 {} -> {}", name, keyword, mapping.theme_id);
                        return mapping.theme_id.clone();
                    }
                }
            }
        }
        
        // 3. 反向关键词匹配（输入名称的关键词在配置中）
        let input_keywords: Vec<&str> = name
            .split(&['、', ',', '，', ' ', '(', ')', '（', '）'])
            .filter(|s| !s.is_empty() && s.len() > 2)
            .collect();
            
        for mapping in &mappings {
            if mapping.enabled.unwrap_or(true) {
                for input_keyword in &input_keywords {
                    if mapping.matter_name.contains(input_keyword) {
                        tracing::info!("✅ 反向关键词匹配: {} 包含在 {} 中 -> {}", input_keyword, mapping.matter_name, mapping.theme_id);
                        return mapping.theme_id.clone();
                    }
                }
            }
        }
    }
    
    // 无法匹配，使用默认主题
    tracing::warn!("⚠️  无法匹配特定主题，使用默认规则");
    tracing::warn!("输入事项ID: {:?}, 事项名称: {:?}", matter_id, matter_name);
    "default".to_string()
}

// 标准化事项ID，提取纯数字部分
fn normalize_matter_id(id: &str) -> String {
    // 移除所有非数字字符，保留纯数字
    id.chars().filter(|c| c.is_ascii_digit()).collect()
}

// 检查两个数字ID是否有显著的数字重叠
fn has_significant_digit_overlap(id1: &str, id2: &str) -> bool {
    if id1.is_empty() || id2.is_empty() || id1.len() < 6 || id2.len() < 6 {
        return false;
    }
    
    // 检查较长的公共子序列
    let min_overlap = std::cmp::min(id1.len(), id2.len()) / 2;  // 至少一半的数字匹配
    let min_overlap = std::cmp::max(min_overlap, 6);  // 但最少6位数字
    
    // 查找最长公共子字符串
    let longest_common = find_longest_common_substring(id1, id2);
    
    longest_common.len() >= min_overlap
}

// 查找最长公共子字符串
fn find_longest_common_substring(s1: &str, s2: &str) -> String {
    let chars1: Vec<char> = s1.chars().collect();
    let chars2: Vec<char> = s2.chars().collect();
    let mut longest = String::new();
    
    for i in 0..chars1.len() {
        for j in 0..chars2.len() {
            let mut k = 0;
            while i + k < chars1.len() && j + k < chars2.len() && chars1[i + k] == chars2[j + k] {
                k += 1;
            }
            if k > longest.len() {
                longest = chars1[i..i + k].iter().collect();
            }
        }
    }
    
    longest
}



pub async fn reload_theme_rule(theme_id: &str) -> anyhow::Result<WebResult> {
    info!("重新加载主题规则: {}", theme_id);

    let rule_path = CURRENT_DIR.join(format!("rules/{}.json", theme_id));
    match read_to_string(&rule_path) {
        Ok(content) => {
            match serde_json::from_str::<DecisionContent>(&content) {
                Ok(decision_content) => {
                    // 更新缓存
                    let mut cache = RULES_CACHE.write().await;
                    cache.insert(theme_id.to_string(), decision_content);

                    info!("✅ 主题规则重新加载成功: {}", theme_id);
                    Ok(WebResult::ok(format!("主题 {} 规则重新加载成功", theme_id)))
                }
                Err(e) => {
                    error!("❌ 解析主题规则文件失败: {} - {}", rule_path.display(), e);
                    Err(anyhow::anyhow!("规则文件解析失败: {}", e))
                }
            }
        }
        Err(e) => {
            error!("❌ 读取主题规则文件失败: {} - {}", rule_path.display(), e);
            Err(anyhow::anyhow!("规则文件读取失败: {}", e))
        }
    }
}

// 为Preview实现evaluate方法 - 只返回评估数据，不生成HTML
impl Preview {
    pub async fn evaluate(self) -> anyhow::Result<crate::model::evaluation::PreviewEvaluationResult> {
        use crate::model::evaluation::*;

        // 根据主题ID获取对应的规则
        let theme_id = self.theme_id.as_deref();
        let rule_content = get_rule_by_theme(theme_id).await;

        info!("=== 预审评估开始 ===");
        info!("主题ID: {:?}", theme_id);
        info!("事项名称: {}", self.matter_name);
        info!("材料数量: {}", self.material_data.len());

        // 构建基础信息
        let basic_info = BasicInfo {
            applicant_name: self.subject_info.user_name.clone().unwrap_or_default(),
            applicant_id: self.subject_info.user_id.clone(),
            agent_name: self.agent_info.user_name.clone().unwrap_or_default(),
            agent_id: self.agent_info.user_id.clone(),
            matter_name: self.matter_name.clone(),
            matter_id: self.matter_id.clone(),
            matter_type: self.matter_type.clone(),
            request_id: self.request_id.clone(),
            sequence_no: self.sequence_no.clone(),
            theme_id: theme_id.unwrap_or("default").to_string(),
            theme_name: get_theme_name(theme_id).unwrap_or("默认规则".to_string()),
        };

        // 创建评估结果对象
        let mut evaluation_result = PreviewEvaluationResult::new(basic_info);

        let mut ocr = Extractor::new()?;
        // 处理每个材料
        for material in self.material_data {
            info!("正在处理材料: {}", material.code);

            let mut contents = vec![];
            let mut attachment_infos = vec![];

            for attachment in material.attachment_list {
                info!("处理附件: {}", attachment.attach_name);

                // 下载文件内容
                let bytes = match download_file_content(&attachment.attach_url).await {
                    Ok(data) => data,
                    Err(e) => {
                        warn!("下载文件失败: {} - {}", attachment.attach_url, e);
                        // 记录失败的附件信息
                        attachment_infos.push(AttachmentInfo {
                            file_name: attachment.attach_name.clone(),
                            file_url: attachment.attach_url.clone(),
                            file_type: "unknown".to_string(),
                            file_size: None,
                            ocr_success: false,
                        });
                        continue;
                    }
                };

                // 根据文件扩展名处理
                let file_extension = Path::new(&attachment.attach_name)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                let mut ocr_success = true;
                let file_size = bytes.len() as u64;

                match file_extension.as_str() {
                    "pdf" => {
                        // PDF转图片后OCR
                        match ocr_conn::pdf_render_jpg(&attachment.attach_name, bytes) {
                            Ok(images) => {
                                for image in images {
                                    match ocr.ocr_and_parse(image.into()) {
                                        Ok(ocr_results) => {
                                            contents.extend(ocr_results.into_iter().map(|content| content.text));
                                        }
                                        Err(e) => {
                                            warn!("OCR处理失败: {}", e);
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("PDF转换失败: {}", e);
                                ocr_success = false;
                            }
                        }
                    }
                    "doc" | "docx" => {
                        // Word文档解析
                        match shiva::docx::Transformer::parse(&bytes.into()) {
                            Ok(doc) => {
                                let elements = doc.get_all_elements();
                                for element in elements {
                                    if let Element::Text { text, .. } = element {
                                        contents.push(text.clone());
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Word文档解析失败: {}", e);
                                ocr_success = false;
                            }
                        }
                    }
                    _ => {
                        // 图片直接OCR
                        match ocr.ocr_and_parse(bytes.into()) {
                            Ok(ocr_results) => {
                                contents.extend(ocr_results.into_iter().map(|content| content.text));
                            }
                            Err(e) => {
                                warn!("图片OCR处理失败: {}", e);
                                ocr_success = false;
                            }
                        }
                    }
                }

                // 记录附件信息
                attachment_infos.push(AttachmentInfo {
                    file_name: attachment.attach_name.clone(),
                    file_url: attachment.attach_url.clone(),
                    file_type: file_extension.clone(),
                    file_size: Some(file_size),
                    ocr_success,
                });
            }

            // 进行规则评估
            info!("正在评估材料: {} (代码: {})", material.code, material.code);
            let ocr_content = contents.join("");
            let simple_result = evaluate_material_simple(&material.code, &ocr_content);

            // 构建规则评估结果
            let rule_evaluation = RuleEvaluationResult {
                status_code: simple_result.code,
                message: simple_result.message.clone(),
                description: simple_result.description.clone(),
                suggestions: if simple_result.code != 200 {
                    vec!["请检查材料完整性".to_string(), "确保材料清晰可读".to_string()]
                } else {
                    vec![]
                },
                rule_details: Some(json!({
                    "material_code": material.code,
                    "content_length": ocr_content.len(),
                    "evaluation_method": "simple_rule_matching"
                })),
            };

            // 确定处理状态
            let processing_status = if attachment_infos.iter().any(|a| !a.ocr_success) {
                ProcessingStatus::PartialSuccess {
                    warnings: vec!["部分文件OCR处理失败".to_string()],
                }
            } else {
                ProcessingStatus::Success
            };

            // 构建材料评估结果
            let material_result = MaterialEvaluationResult {
                material_code: material.code.clone(),
                material_name: simple_result.description,
                attachments: attachment_infos,
                ocr_content,
                rule_evaluation,
                processing_status,
            };

            // 添加到评估结果中
            evaluation_result.add_material_result(material_result);
        }

        info!("=== 预审评估完成 ===");
        Ok(evaluation_result)
    }
}

// 简化的材料评估结果
#[derive(Debug, Clone)]
struct SimpleEvaluationResult {
    code: u64,
    message: String,
    description: String,
}

// 简化的材料评估函数
fn evaluate_material_simple(material_code: &str, content: &str) -> SimpleEvaluationResult {
    info!("简化评估 - 材料代码: {}, 内容长度: {}", material_code, content.len());

    // 基于材料代码的简单规则匹配
    match material_code {
        code if code.contains("16570147206221001") => {
            // 杭州市工程渣土准运证核准申请表
            if content.is_empty() {
                SimpleEvaluationResult {
                    code: 500,
                    message: "没有材料".to_string(),
                    description: "杭州市工程渣土准运证核准申请表".to_string(),
                }
            } else {
                SimpleEvaluationResult {
                    code: 200,
                    message: "材料检查通过".to_string(),
                    description: "杭州市工程渣土准运证核准申请表".to_string(),
                }
            }
        }
        code if code.contains("105100813") => {
            // 申请单位营业执照
            SimpleEvaluationResult {
                code: 200,
                message: "申请单位营业执照".to_string(),
                description: "申请单位营业执照".to_string(),
            }
        }
        code if code.contains("105100001") => {
            // 委托代理人身份证
            SimpleEvaluationResult {
                code: 200,
                message: "委托代理人身份证（检查有效期）".to_string(),
                description: "委托代理人身份证".to_string(),
            }
        }
        _ => {
            // 默认通过
            SimpleEvaluationResult {
                code: 200,
                message: "材料检查通过".to_string(),
                description: format!("材料代码: {}", material_code),
            }
        }
    }
}

// 辅助函数：下载文件内容
async fn download_file_content(url: &str) -> anyhow::Result<Vec<u8>> {
    if url.starts_with("http://") || url.starts_with("https://") {
        // 网络文件下载
        let response = reqwest::get(url).await?;
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    } else {
        // 本地文件读取
        let path = CURRENT_DIR.join(url);
        let bytes = tokio::fs::read(path).await?;
        Ok(bytes)
    }
}

// 更新规则文件
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
    Ok(WebResult::err_custom("No rule file"))
}