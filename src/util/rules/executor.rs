use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;

use crate::db::traits::Database;

use super::cache::RuleCache;
use super::model::MatterRuleConfig;
use super::repository::RuleRepository;

/// 规则引擎入口
pub struct RuleEngine {
    repository: RuleRepository,
    cache: Arc<RuleCache>,
}

impl RuleEngine {
    pub fn new(database: Arc<dyn Database>) -> Self {
        Self {
            repository: RuleRepository::new(database),
            cache: Arc::new(RuleCache::new(Duration::from_secs(300))),
        }
    }

    pub async fn get_config(&self, matter_id: &str) -> Result<Option<Arc<MatterRuleConfig>>> {
        self.cache.get_or_load(matter_id, &self.repository).await
    }

    pub async fn reload(&self, matter_id: &str) {
        self.cache.invalidate(matter_id).await;
    }

    pub async fn reload_all(&self) {
        self.cache.invalidate_all().await;
    }
}
