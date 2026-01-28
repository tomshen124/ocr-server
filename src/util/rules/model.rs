use std::collections::HashMap;
use std::fmt;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::db::traits::MatterRuleConfigRecord;

/// 完整的事项规则配置（数据库元信息 + JSON定义）
#[derive(Debug, Clone)]
pub struct MatterRuleConfig {
    pub record: MatterRuleConfigRecord,
    pub definition: MatterRuleDefinition,
    pub mode: RuleMode,
}

impl MatterRuleConfig {
    pub fn new(record: MatterRuleConfigRecord, definition: MatterRuleDefinition) -> Result<Self> {
        if record.matter_id != definition.matter_id {
            return Err(anyhow!(
                "matter_id mismatch between record ({}) and definition ({})",
                record.matter_id,
                definition.matter_id
            ));
        }

        let record_mode = RuleMode::from_str(&record.mode);
        if record_mode != definition.mode && !definition.mode.matches_str(&record.mode) {
            warn!(
                "Rule mode mismatch for matter {}: record='{}', definition='{}'",
                record.matter_id,
                record.mode,
                definition.mode.as_str()
            );
        }

        let mode = definition.mode.clone();

        Ok(Self {
            record,
            definition,
            mode,
        })
    }

    pub fn matter_id(&self) -> &str {
        &self.record.matter_id
    }

    pub fn status(&self) -> &str {
        &self.record.status
    }

    pub fn is_active(&self) -> bool {
        self.record.status.eq_ignore_ascii_case("active")
    }
}

/// JSON定义主体
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatterRuleDefinition {
    pub spec_version: String,
    #[serde(default)]
    pub mode: RuleMode,
    #[serde(default)]
    pub generated_at: Option<String>,
    pub matter_id: String,
    #[serde(default)]
    pub matter_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub materials: Vec<MaterialRule>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}

/// 规则模式
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleMode {
    PresentOnly,
    Strict,
    Full,
    Custom(String),
}

impl RuleMode {
    pub fn as_str(&self) -> &str {
        match self {
            RuleMode::PresentOnly => "presentOnly",
            RuleMode::Strict => "strict",
            RuleMode::Full => "full",
            RuleMode::Custom(value) => value.as_str(),
        }
    }

    pub fn matches_str(&self, other: &str) -> bool {
        self.as_str().eq_ignore_ascii_case(other)
    }

    pub fn from_str(value: &str) -> Self {
        match value {
            "presentOnly" | "presentonly" => RuleMode::PresentOnly,
            "strict" => RuleMode::Strict,
            "full" => RuleMode::Full,
            other => RuleMode::Custom(other.to_owned()),
        }
    }
}

impl Default for RuleMode {
    fn default() -> Self {
        RuleMode::PresentOnly
    }
}

impl<'de> Deserialize<'de> for RuleMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = Option::<String>::deserialize(deserializer)?;
        Ok(match raw {
            None => RuleMode::PresentOnly,
            Some(value) => RuleMode::from_str(&value),
        })
    }
}

impl Serialize for RuleMode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

/// 材料规则
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialRule {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub max_files: Option<u32>,
    #[serde(default)]
    pub min_files: Option<u32>,
    #[serde(default)]
    pub allowed_types: Vec<String>,
    #[serde(default)]
    pub scope: MaterialScope,
    #[serde(default)]
    pub repeat: Option<MaterialRepeat>,
    #[serde(default)]
    pub validity: Option<MaterialValidity>,
    #[serde(default)]
    pub checks: Option<MaterialChecks>,
    #[serde(default)]
    pub pairing: Option<MaterialPairing>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub min_pairs: Option<u32>,
    #[serde(flatten, default)]
    pub extra: HashMap<String, Value>,
}

/// 材料作用域
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialScope {
    Global,
    PerVehicle,
    Custom(String),
}

impl Default for MaterialScope {
    fn default() -> Self {
        MaterialScope::Global
    }
}

impl<'de> Deserialize<'de> for MaterialScope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = Option::<String>::deserialize(deserializer)?;
        Ok(match raw.as_deref() {
            None | Some("global") => MaterialScope::Global,
            Some("perVehicle") => MaterialScope::PerVehicle,
            Some(other) => MaterialScope::Custom(other.to_owned()),
        })
    }
}

impl Serialize for MaterialScope {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            MaterialScope::Global => serializer.serialize_str("global"),
            MaterialScope::PerVehicle => serializer.serialize_str("perVehicle"),
            MaterialScope::Custom(value) => serializer.serialize_str(value),
        }
    }
}

/// 材料重复配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialRepeat {
    pub case_list: String,
    pub case_key_field: String,
    pub ocr_key_field: String,
}

/// 材料有效期配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum MaterialValidity {
    None,
    ExpiryField { field: String },
    IssuePlusDays { days: u32 },
}

/// 材料校验配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialChecks {
    #[serde(default)]
    pub must_have_seal: bool,
    #[serde(default)]
    pub must_have_signature: bool,
    #[serde(default)]
    pub matches: Vec<FieldMatchRule>,
}

/// 字段比对规则
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldMatchRule {
    pub ocr_field: String,
    pub case_field: String,
    #[serde(default, deserialize_with = "deserialize_normalize_ops")]
    pub normalize: Vec<String>,
}

/// 材料配对要求（如45°照片）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialPairing {
    pub angle_field: Option<String>,
    #[serde(default)]
    pub required_angles: Vec<String>,
    #[serde(default)]
    pub fallback_name_regex: Vec<PairingFallback>,
}

/// 配对名称回退
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingFallback {
    pub pattern: String,
    pub map_to: String,
}

fn deserialize_normalize_ops<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    let mut ops = Vec::new();

    match value {
        None => {}
        Some(Value::String(s)) => {
            ops.extend(split_normalize_ops(&s));
        }
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    ops.extend(split_normalize_ops(s));
                }
            }
        }
        Some(other) => {
            return Err(serde::de::Error::custom(format!(
                "unsupported normalize value: {other}"
            )));
        }
    }

    Ok(ops)
}

fn split_normalize_ops(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

impl fmt::Display for RuleMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
