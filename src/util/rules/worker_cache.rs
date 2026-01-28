use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::sync::Mutex;
use tracing::debug;

use super::{compute_definition_fingerprint, MatterRuleDefinition};

const FALLBACK_MAX_ENTRIES: usize = 128;
const FALLBACK_TTL_SECS: u64 = 900;

#[derive(Clone)]
struct CachedRule {
    json: Arc<Value>,
    definition: Arc<MatterRuleDefinition>,
    fingerprint: String,
    inserted_at: Instant,
}

struct CacheState {
    entries: HashMap<String, CachedRule>,
    order: VecDeque<String>,
}

impl CacheState {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
        self.order.push_back(key.to_string());
    }

    fn remove(&mut self, key: &str) {
        self.entries.remove(key);
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
    }
}

/// Worker 节点规则缓存句柄，暴露给调用方使用
#[derive(Clone)]
pub struct WorkerCachedRuleHandle {
    pub json: Arc<Value>,
    pub definition: Arc<MatterRuleDefinition>,
    pub fingerprint: String,
}

impl From<&CachedRule> for WorkerCachedRuleHandle {
    fn from(rule: &CachedRule) -> Self {
        Self {
            json: rule.json.clone(),
            definition: rule.definition.clone(),
            fingerprint: rule.fingerprint.clone(),
        }
    }
}

/// Worker 规则缓存，实现简单的 TTL + LRU 淘汰
pub struct WorkerRuleCache {
    enabled: bool,
    ttl: Option<Duration>,
    max_entries: usize,
    state: Mutex<CacheState>,
}

impl WorkerRuleCache {
    pub fn new(enabled: bool, ttl: Option<Duration>, max_entries: usize) -> Self {
        Self {
            enabled,
            ttl,
            max_entries: max_entries.max(1),
            state: Mutex::new(CacheState::new()),
        }
    }

    fn from_config() -> Self {
        let deployment = crate::CONFIG.deployment.worker.clone();
        if let Some(worker_cfg) = deployment {
            let cache_cfg = worker_cfg.rule_cache;
            let ttl = if !cache_cfg.enabled || cache_cfg.ttl_secs == 0 {
                None
            } else {
                Some(Duration::from_secs(cache_cfg.ttl_secs))
            };
            return Self::new(cache_cfg.enabled, ttl, cache_cfg.max_entries.max(1));
        }

        // 默认开启缓存，但使用较小容量
        Self::new(
            true,
            Some(Duration::from_secs(FALLBACK_TTL_SECS)),
            FALLBACK_MAX_ENTRIES,
        )
    }

    pub fn global() -> &'static WorkerRuleCache {
        static INSTANCE: Lazy<WorkerRuleCache> = Lazy::new(WorkerRuleCache::from_config);
        &INSTANCE
    }

    fn is_expired(&self, rule: &CachedRule) -> bool {
        match self.ttl {
            Some(ttl) => rule.inserted_at.elapsed() > ttl,
            None => false,
        }
    }

    async fn store_handle(&self, matter_id: &str, handle: &WorkerCachedRuleHandle) {
        if !self.enabled {
            return;
        }

        let mut guard = self.state.lock().await;

        let entry = CachedRule {
            json: handle.json.clone(),
            definition: handle.definition.clone(),
            fingerprint: handle.fingerprint.clone(),
            inserted_at: Instant::now(),
        };

        if let Some(existing) = guard.entries.get_mut(matter_id) {
            if existing.fingerprint == entry.fingerprint {
                existing.inserted_at = Instant::now();
                guard.touch(matter_id);
                return;
            }
        }

        guard.entries.insert(matter_id.to_string(), entry);
        guard.touch(matter_id);
        self.enforce_capacity(&mut guard);
    }

    fn enforce_capacity(&self, state: &mut CacheState) {
        if !self.enabled {
            return;
        }

        while state.entries.len() > self.max_entries {
            if let Some(evicted_key) = state.order.pop_front() {
                state.entries.remove(&evicted_key);
                debug!(
                    matter_id = %evicted_key,
                    max_entries = self.max_entries,
                    "Worker 规则缓存达到容量，驱逐最旧记录"
                );
            } else {
                break;
            }
        }
    }

    /// 记忆最新的规则定义，返回可复用的句柄
    pub async fn remember(&self, matter_id: &str, value: Value) -> Result<WorkerCachedRuleHandle> {
        let definition: MatterRuleDefinition =
            serde_json::from_value(value.clone()).context("解析事项规则JSON失败")?;
        let fingerprint =
            compute_definition_fingerprint(&definition).context("计算事项规则指纹失败")?;

        let handle = WorkerCachedRuleHandle {
            json: Arc::new(value),
            definition: Arc::new(definition),
            fingerprint,
        };

        self.store_handle(matter_id, &handle).await;

        Ok(handle)
    }

    /// 尝试从缓存中读取规则定义
    pub async fn get(&self, matter_id: &str) -> Option<WorkerCachedRuleHandle> {
        if !self.enabled {
            return None;
        }

        let mut guard = self.state.lock().await;
        if let Some(entry) = guard.entries.get(matter_id).cloned() {
            if self.is_expired(&entry) {
                guard.remove(matter_id);
                debug!(
                    matter_id = %matter_id,
                    "Worker 规则缓存条目已过期，执行驱逐"
                );
                return None;
            }

            guard.touch(matter_id);
            let handle: WorkerCachedRuleHandle = (&entry).into();
            return Some(handle);
        }

        None
    }

    /// 清理全部缓存（测试或热更新使用）
    #[allow(dead_code)]
    pub async fn clear(&self) {
        let mut guard = self.state.lock().await;
        guard.entries.clear();
        guard.order.clear();
    }
}

/// 工具函数：判定失败原因是否命中OCR关键字
pub fn matches_ocr_failure(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    lower.contains("ocr")
        || lower.contains("获取ocr引擎失败")
        || lower.contains("ocr失败")
        || lower.contains("ocr引擎无响应")
}
