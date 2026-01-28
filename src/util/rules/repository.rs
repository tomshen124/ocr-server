use std::sync::Arc;

use anyhow::{anyhow, Context, Result};

use crate::db::traits::{Database, MatterRuleConfigRecord};

use super::model::{MatterRuleConfig, MatterRuleDefinition};

/// 事项规则仓储 - 负责从数据库加载配置
pub struct RuleRepository {
    db: Arc<dyn Database>,
}

impl RuleRepository {
    pub fn new(db: Arc<dyn Database>) -> Self {
        Self { db }
    }

    pub async fn fetch(&self, matter_id: &str) -> Result<Option<MatterRuleConfig>> {
        let record = match self.db.get_matter_rule_config(matter_id).await? {
            Some(record) => record,
            None => return Ok(None),
        };

        let config = Self::build_config(record)?;
        Ok(Some(config))
    }

    pub async fn list(&self, status: Option<&str>) -> Result<Vec<MatterRuleConfig>> {
        let records = self.db.list_matter_rule_configs(status).await?;
        records
            .into_iter()
            .map(Self::build_config)
            .collect::<Result<Vec<_>>>()
    }

    pub async fn upsert(&self, config: MatterRuleConfigRecord) -> Result<()> {
        self.db.upsert_matter_rule_config(&config).await
    }

    fn build_config(record: MatterRuleConfigRecord) -> Result<MatterRuleConfig> {
        let definition: MatterRuleDefinition = serde_json::from_str(&record.rule_payload)
            .with_context(|| {
                format!(
                    "failed to parse rule_payload for matter {}",
                    record.matter_id
                )
            })?;

        if !record.status.eq_ignore_ascii_case("active")
            && !record.status.eq_ignore_ascii_case("draft")
        {
            return Err(anyhow!(
                "unsupported matter rule status '{}'",
                record.status
            ));
        }

        MatterRuleConfig::new(record, definition)
    }
}
