use anyhow::Result;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::MatterRuleDefinition;

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    format!("{:x}", digest)
}

/// 计算事项规则定义的指纹（SHA256，对序列化结果进行哈希）
pub fn compute_definition_fingerprint(definition: &MatterRuleDefinition) -> Result<String> {
    let serialized = serde_json::to_vec(definition)?;
    Ok(hash_bytes(&serialized))
}

/// 计算任意JSON规则定义的指纹（SHA256）
pub fn compute_value_fingerprint(value: &Value) -> Result<String> {
    let serialized = serde_json::to_vec(value)?;
    Ok(hash_bytes(&serialized))
}
