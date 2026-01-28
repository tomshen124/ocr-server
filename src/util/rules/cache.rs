use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tokio::sync::RwLock;

use super::model::MatterRuleConfig;
use super::repository::RuleRepository;

/// 规则缓存，减少频繁的数据库访问
pub struct RuleCache {
    ttl: Duration,
    inner: RwLock<HashMap<String, CachedRule>>,
}

struct CachedRule {
    rule: Arc<MatterRuleConfig>,
    fetched_at: Instant,
}

impl RuleCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            inner: RwLock::new(HashMap::new()),
        }
    }

    pub async fn get_or_load(
        &self,
        matter_id: &str,
        repository: &RuleRepository,
    ) -> Result<Option<Arc<MatterRuleConfig>>> {
        if let Some(entry) = self.read_hit(matter_id).await {
            return Ok(Some(entry));
        }

        let mut guard = self.inner.write().await;
        if let Some(entry) = guard.get(matter_id) {
            if entry.fetched_at.elapsed() < self.ttl {
                return Ok(Some(entry.rule.clone()));
            }
        }

        let rule = match repository.fetch(matter_id).await? {
            Some(rule) => {
                let arc = Arc::new(rule);
                guard.insert(
                    matter_id.to_string(),
                    CachedRule {
                        rule: arc.clone(),
                        fetched_at: Instant::now(),
                    },
                );
                Some(arc)
            }
            None => {
                guard.remove(matter_id);
                None
            }
        };
        Ok(rule)
    }

    pub async fn invalidate(&self, matter_id: &str) {
        let mut guard = self.inner.write().await;
        guard.remove(matter_id);
    }

    pub async fn invalidate_all(&self) {
        let mut guard = self.inner.write().await;
        guard.clear();
    }

    async fn read_hit(&self, matter_id: &str) -> Option<Arc<MatterRuleConfig>> {
        let guard = self.inner.read().await;
        guard.get(matter_id).and_then(|entry| {
            if entry.fetched_at.elapsed() < self.ttl {
                Some(entry.rule.clone())
            } else {
                None
            }
        })
    }
}
