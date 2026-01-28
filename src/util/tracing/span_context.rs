//! Span上下文管理
//! 提供分布式追踪的上下文传播和管理

use crate::util::tracing::GlobalTraceId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// 追踪ID类型别名
pub type TraceId = String;

/// Span ID类型别名  
pub type SpanId = String;

/// Span上下文
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SpanContext {
    /// 追踪ID - 整个请求链路的唯一标识
    pub trace_id: TraceId,

    /// Span ID - 当前操作的唯一标识
    pub span_id: SpanId,

    /// 父Span ID - 上级操作的标识
    pub parent_span_id: Option<SpanId>,

    /// 追踪标志
    pub trace_flags: TraceFlags,

    /// 追踪状态
    pub trace_state: TraceState,

    /// 采样决策
    pub sampled: bool,

    /// 创建时间戳
    pub created_at: u64,

    /// baggage - 跨服务传播的键值对
    pub baggage: HashMap<String, String>,
}

/// 追踪标志
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceFlags {
    /// 是否被采样
    pub sampled: bool,

    /// 是否为调试模式
    pub debug: bool,

    /// 是否为远程调用
    pub remote: bool,
}

impl Default for TraceFlags {
    fn default() -> Self {
        Self {
            sampled: true,
            debug: false,
            remote: false,
        }
    }
}

/// 追踪状态 - 存储厂商特定的追踪信息
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TraceState {
    /// 状态条目
    pub entries: HashMap<String, String>,
}

impl Default for TraceState {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }
}

impl TraceState {
    /// 添加状态条目
    pub fn add(&mut self, key: String, value: String) {
        self.entries.insert(key, value);
    }

    /// 获取状态条目
    pub fn get(&self, key: &str) -> Option<&String> {
        self.entries.get(key)
    }

    /// 移除状态条目
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.entries.remove(key)
    }

    /// 转换为W3C TraceState头格式
    pub fn to_w3c_header(&self) -> String {
        self.entries
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect::<Vec<_>>()
            .join(",")
    }

    /// 从W3C TraceState头格式解析
    pub fn from_w3c_header(header: &str) -> Self {
        let mut entries = HashMap::new();

        for entry in header.split(',') {
            if let Some((key, value)) = entry.trim().split_once('=') {
                entries.insert(key.to_string(), value.to_string());
            }
        }

        Self { entries }
    }
}

impl SpanContext {
    /// 创建新的根Span上下文
    pub fn new_root() -> Self {
        let trace_id = generate_trace_id();
        let span_id = generate_span_id();

        Self {
            trace_id,
            span_id,
            parent_span_id: None,
            trace_flags: TraceFlags::default(),
            trace_state: TraceState::default(),
            sampled: true,
            created_at: current_timestamp(),
            baggage: HashMap::new(),
        }
    }

    /// 创建子Span上下文
    pub fn create_child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: generate_span_id(),
            parent_span_id: Some(self.span_id.clone()),
            trace_flags: self.trace_flags.clone(),
            trace_state: self.trace_state.clone(),
            sampled: self.sampled,
            created_at: current_timestamp(),
            baggage: self.baggage.clone(),
        }
    }

    /// 从追踪ID创建上下文
    pub fn from_trace_id(trace_id: TraceId, parent_span_id: Option<SpanId>) -> Self {
        Self {
            trace_id,
            span_id: generate_span_id(),
            parent_span_id,
            trace_flags: TraceFlags::default(),
            trace_state: TraceState::default(),
            sampled: true,
            created_at: current_timestamp(),
            baggage: HashMap::new(),
        }
    }

    /// 是否为根Span
    pub fn is_root(&self) -> bool {
        self.parent_span_id.is_none()
    }

    /// 是否有效
    pub fn is_valid(&self) -> bool {
        !self.trace_id.is_empty() && !self.span_id.is_empty()
    }

    /// 是否被采样
    pub fn is_sampled(&self) -> bool {
        self.sampled && self.trace_flags.sampled
    }

    /// 是否为远程上下文
    pub fn is_remote(&self) -> bool {
        self.trace_flags.remote
    }

    /// 设置baggage
    pub fn set_baggage(&mut self, key: String, value: String) {
        self.baggage.insert(key, value);
    }

    /// 获取baggage
    pub fn get_baggage(&self, key: &str) -> Option<&String> {
        self.baggage.get(key)
    }

    /// 移除baggage
    pub fn remove_baggage(&mut self, key: &str) -> Option<String> {
        self.baggage.remove(key)
    }

    /// 转换为W3C Traceparent头格式
    /// 格式: version-trace_id-span_id-flags
    pub fn to_w3c_traceparent(&self) -> String {
        let flags = if self.is_sampled() { "01" } else { "00" };
        format!("00-{}-{}-{}", self.trace_id, self.span_id, flags)
    }

    /// 从W3C Traceparent头格式解析
    pub fn from_w3c_traceparent(traceparent: &str) -> Option<Self> {
        let parts: Vec<&str> = traceparent.trim().split('-').collect();
        if parts.len() != 4 {
            return None;
        }

        let version = parts[0];
        let trace_id = parts[1];
        let span_id = parts[2];
        let flags = parts[3];

        // 目前只支持版本00
        if version != "00" {
            return None;
        }

        // 解析标志
        let flags_int = u8::from_str_radix(flags, 16).ok()?;
        let sampled = (flags_int & 0x01) != 0;

        Some(Self {
            trace_id: trace_id.to_string(),
            span_id: generate_span_id(),               // 生成新的span ID
            parent_span_id: Some(span_id.to_string()), // 原来的span ID成为父ID
            trace_flags: TraceFlags {
                sampled,
                debug: (flags_int & 0x02) != 0,
                remote: true,
            },
            trace_state: TraceState::default(),
            sampled,
            created_at: current_timestamp(),
            baggage: HashMap::new(),
        })
    }

    /// 创建用于传播的头部映射
    pub fn to_headers(&self) -> HashMap<String, String> {
        let mut headers = HashMap::new();

        headers.insert("traceparent".to_string(), self.to_w3c_traceparent());

        if !self.trace_state.entries.is_empty() {
            headers.insert("tracestate".to_string(), self.trace_state.to_w3c_header());
        }

        // 添加自定义头部
        headers.insert("x-trace-id".to_string(), self.trace_id.clone());
        headers.insert("x-span-id".to_string(), self.span_id.clone());

        if let Some(parent_id) = &self.parent_span_id {
            headers.insert("x-parent-span-id".to_string(), parent_id.clone());
        }

        // 添加baggage
        if !self.baggage.is_empty() {
            let baggage_str = self
                .baggage
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect::<Vec<_>>()
                .join(",");
            headers.insert("baggage".to_string(), baggage_str);
        }

        headers
    }

    /// 从头部映射创建上下文
    pub fn from_headers(headers: &HashMap<String, String>) -> Option<Self> {
        // 尝试从W3C traceparent头解析
        if let Some(traceparent) = headers.get("traceparent") {
            let mut context = Self::from_w3c_traceparent(traceparent)?;

            // 解析tracestate
            if let Some(tracestate) = headers.get("tracestate") {
                context.trace_state = TraceState::from_w3c_header(tracestate);
            }

            // 解析baggage
            if let Some(baggage) = headers.get("baggage") {
                for entry in baggage.split(',') {
                    if let Some((key, value)) = entry.trim().split_once('=') {
                        context.baggage.insert(key.to_string(), value.to_string());
                    }
                }
            }

            return Some(context);
        }

        // 尝试从自定义头部解析
        let trace_id = headers.get("x-trace-id")?.clone();
        let parent_span_id = headers.get("x-parent-span-id").cloned();

        Some(Self::from_trace_id(trace_id, parent_span_id))
    }

    /// 获取追踪的层级深度
    pub fn depth(&self) -> usize {
        // 这里可以根据实际需求实现深度计算
        // 比如从trace_id或baggage中获取深度信息
        if self.is_root() {
            0
        } else {
            1
        }
    }

    /// 克隆用于传播
    pub fn clone_for_propagation(&self) -> Self {
        let mut cloned = self.clone();
        cloned.trace_flags.remote = true;
        cloned
    }
}

/// 生成追踪ID
fn generate_trace_id() -> String {
    // W3C trace ID是32个十六进制字符（16字节）
    let uuid1 = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();
    format!("{:x}{:x}", uuid1.as_u128(), uuid2.as_u128())[0..32].to_string()
}

/// 生成Span ID
fn generate_span_id() -> String {
    // W3C span ID是16个十六进制字符（8字节）
    let uuid = Uuid::new_v4();
    format!("{:x}", uuid.as_u128())[0..16].to_string()
}

/// 获取当前时间戳（毫秒）
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// 全局上下文管理器
pub struct SpanContextManager {
    /// 当前活跃的上下文栈
    context_stack: Vec<SpanContext>,
}

impl SpanContextManager {
    /// 创建新的上下文管理器
    pub fn new() -> Self {
        Self {
            context_stack: Vec::new(),
        }
    }

    /// 推入新的上下文
    pub fn push(&mut self, context: SpanContext) {
        self.context_stack.push(context);
    }

    /// 弹出当前上下文
    pub fn pop(&mut self) -> Option<SpanContext> {
        self.context_stack.pop()
    }

    /// 获取当前上下文
    pub fn current(&self) -> Option<&SpanContext> {
        self.context_stack.last()
    }

    /// 获取当前上下文的可变引用
    pub fn current_mut(&mut self) -> Option<&mut SpanContext> {
        self.context_stack.last_mut()
    }

    /// 获取上下文栈深度
    pub fn depth(&self) -> usize {
        self.context_stack.len()
    }

    /// 清空上下文栈
    pub fn clear(&mut self) {
        self.context_stack.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_context_creation() {
        let context = SpanContext::new_root();

        assert!(context.is_root());
        assert!(context.is_valid());
        assert!(context.is_sampled());
        assert!(!context.is_remote());
        assert!(!context.trace_id.is_empty());
        assert!(!context.span_id.is_empty());
    }

    #[test]
    fn test_child_context() {
        let root = SpanContext::new_root();
        let child = root.create_child();

        assert_eq!(child.trace_id, root.trace_id);
        assert_ne!(child.span_id, root.span_id);
        assert_eq!(child.parent_span_id, Some(root.span_id));
        assert!(!child.is_root());
    }

    #[test]
    fn test_w3c_traceparent() {
        let context = SpanContext::new_root();
        let traceparent = context.to_w3c_traceparent();

        assert!(traceparent.starts_with("00-"));
        assert_eq!(traceparent.matches('-').count(), 3);

        let parsed = SpanContext::from_w3c_traceparent(&traceparent);
        assert!(parsed.is_some());

        let parsed = parsed.unwrap();
        assert_eq!(parsed.trace_id, context.trace_id);
    }

    #[test]
    fn test_trace_state() {
        let mut state = TraceState::default();
        state.add("vendor".to_string(), "value123".to_string());

        let header = state.to_w3c_header();
        assert_eq!(header, "vendor=value123");

        let parsed = TraceState::from_w3c_header(&header);
        assert_eq!(parsed.get("vendor"), Some(&"value123".to_string()));
    }

    #[test]
    fn test_baggage() {
        let mut context = SpanContext::new_root();
        context.set_baggage("user_id".to_string(), "123".to_string());
        context.set_baggage("session_id".to_string(), "abc".to_string());

        assert_eq!(context.get_baggage("user_id"), Some(&"123".to_string()));
        assert_eq!(context.get_baggage("session_id"), Some(&"abc".to_string()));

        let headers = context.to_headers();
        assert!(headers.contains_key("baggage"));
    }

    #[test]
    fn test_context_manager() {
        let mut manager = SpanContextManager::new();

        assert_eq!(manager.depth(), 0);
        assert!(manager.current().is_none());

        let context1 = SpanContext::new_root();
        manager.push(context1.clone());

        assert_eq!(manager.depth(), 1);
        assert_eq!(manager.current().unwrap().trace_id, context1.trace_id);

        let context2 = context1.create_child();
        manager.push(context2.clone());

        assert_eq!(manager.depth(), 2);
        assert_eq!(manager.current().unwrap().span_id, context2.span_id);

        let popped = manager.pop();
        assert_eq!(popped.unwrap().span_id, context2.span_id);
        assert_eq!(manager.depth(), 1);
    }

    #[test]
    fn test_headers_roundtrip() {
        let mut original = SpanContext::new_root();
        original.set_baggage("test".to_string(), "value".to_string());
        original
            .trace_state
            .add("vendor".to_string(), "data".to_string());

        let headers = original.to_headers();
        let parsed = SpanContext::from_headers(&headers).unwrap();

        assert_eq!(parsed.trace_id, original.trace_id);
        assert_eq!(parsed.get_baggage("test"), Some(&"value".to_string()));
    }
}
