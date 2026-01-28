use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use async_nats::jetstream::{self, consumer, stream, AckKind};
use async_nats::ConnectOptions;
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::any::Any;
use tokio::sync::{mpsc, RwLock};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::model::preview::PreviewBody;
use crate::util::config::types::{
    LocalQueueConfig, NatsQueueConfig, TaskQueueConfig, TaskQueueDriver,
};
use crate::util::logging::standards::events;
use crate::util::tracing::metrics_collector::METRICS_COLLECTOR;

pub const PREVIEW_QUEUE_NAME: &str = "preview";

/// 预审任务载荷，供队列传递
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewTask {
    pub preview_body: PreviewBody,
    pub preview_id: String,
    pub third_party_request_id: String,
}

impl PreviewTask {
    pub fn new(
        preview_body: PreviewBody,
        preview_id: String,
        third_party_request_id: String,
    ) -> Self {
        Self {
            preview_body,
            preview_id,
            third_party_request_id,
        }
    }
}

#[async_trait]
pub trait PreviewTaskHandler: Send + Sync {
    async fn handle_preview_task(&self, task: PreviewTask) -> Result<()>;
}

#[async_trait]
pub trait TaskQueue: Send + Sync + Any {
    async fn enqueue(&self, task: PreviewTask) -> Result<()>;

    /// 类型转换支持，用于dynamic_worker模块的downcast
    fn as_any(&self) -> &dyn std::any::Any;
}

/// 根据配置初始化任务队列实现
pub async fn initialize_task_queue(
    distributed_enabled: bool,
    config: &TaskQueueConfig,
    handler: Arc<dyn PreviewTaskHandler>,
) -> Result<Arc<dyn TaskQueue>> {
    if !distributed_enabled {
        return Ok(Arc::new(DirectTaskQueue::new(handler)));
    }

    let driver = config.driver.clone();
    match driver {
        TaskQueueDriver::Local => Ok(create_local_queue(
            PREVIEW_QUEUE_NAME,
            &config.local,
            handler,
        )),
        TaskQueueDriver::Nats => {
            let nats_config = config
                .nats
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow!("缺少 NATS 队列配置"))?;

            let queue_impl =
                Arc::new(NatsTaskQueue::connect(PREVIEW_QUEUE_NAME, &nats_config).await?);

            if nats_config.inline_worker {
                let consumer = NatsTaskQueueConsumer::new(
                    PREVIEW_QUEUE_NAME,
                    queue_impl.jetstream_context(),
                    nats_config.clone(),
                );
                let handler_clone = Arc::clone(&handler);
                tokio::spawn(async move {
                    if let Err(err) = consumer.run(handler_clone).await {
                        error!("内联 NATS 队列消费失败: {:#}", err);
                    }
                });
            }

            let queue: Arc<dyn TaskQueue> = queue_impl;
            Ok(queue)
        }
        TaskQueueDriver::Database => Err(anyhow!(
            "数据库任务队列驱动尚未实现，请修改配置或使用 NATS/Local 驱动"
        )),
    }
}

fn create_local_queue(
    queue_name: &'static str,
    local_config: &LocalQueueConfig,
    handler: Arc<dyn PreviewTaskHandler>,
) -> Arc<dyn TaskQueue> {
    let capacity = local_config.channel_capacity.max(16);
    Arc::new(LocalTaskQueue::new(queue_name, handler, capacity))
}

/// 单机队列实现，使用 tokio mpsc 连接本地 handler
pub struct LocalTaskQueue {
    sender: mpsc::Sender<PreviewTask>,
    #[allow(dead_code)]
    handler: Arc<dyn PreviewTaskHandler>,
    queue_name: &'static str,
    pending_tasks: Arc<AtomicU64>,
}

impl LocalTaskQueue {
    pub fn new(
        queue_name: &'static str,
        handler: Arc<dyn PreviewTaskHandler>,
        capacity: usize,
    ) -> Self {
        let (sender, mut receiver) = mpsc::channel(capacity);
        let worker_handler = handler.clone();
        let pending_tasks = Arc::new(AtomicU64::new(0));
        let pending_for_loop = Arc::clone(&pending_tasks);
        let queue_name_for_loop = queue_name;
        tokio::spawn(async move {
            while let Some(task) = receiver.recv().await {
                let result = worker_handler.handle_preview_task(task).await;
                let success = result.is_ok();
                if let Err(err) = result {
                    error!("本地预审任务执行失败: {:?}", err);
                }
                let previous = pending_for_loop
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                        current.checked_sub(1)
                    })
                    .unwrap_or(0);
                let depth = previous.saturating_sub(1);
                METRICS_COLLECTOR.record_queue_dequeue(queue_name_for_loop, success, Some(depth));
                METRICS_COLLECTOR.record_worker_inflight("local", depth);
            }
            METRICS_COLLECTOR.record_queue_depth(queue_name_for_loop, 0);
            METRICS_COLLECTOR.record_worker_inflight("local", 0);
        });
        METRICS_COLLECTOR.record_queue_depth(queue_name, 0);
        METRICS_COLLECTOR.record_worker_inflight("local", 0);
        Self {
            sender,
            handler,
            queue_name,
            pending_tasks,
        }
    }
}

#[async_trait]
impl TaskQueue for LocalTaskQueue {
    async fn enqueue(&self, task: PreviewTask) -> Result<()> {
        let new_depth = self.pending_tasks.fetch_add(1, Ordering::SeqCst) + 1;
        METRICS_COLLECTOR.record_queue_enqueue(self.queue_name, Some(new_depth));
        METRICS_COLLECTOR.record_worker_inflight("local", new_depth);

        match self.sender.send(task).await {
            Ok(()) => Ok(()),
            Err(e) => {
                let previous = self
                    .pending_tasks
                    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                        current.checked_sub(1)
                    })
                    .unwrap_or(0);
                let depth = previous.saturating_sub(1);
                METRICS_COLLECTOR.record_queue_depth(self.queue_name, depth);
                METRICS_COLLECTOR.record_worker_inflight("local", depth);
                Err(anyhow!("发送预审任务失败: {}", e))
            }
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// 基于 NATS JetStream 的任务队列实现
#[derive(Clone)]
pub struct NatsTaskQueue {
    context: Arc<RwLock<jetstream::Context>>,
    subject: String,
    stream: String,
    queue_name: &'static str,
    config: NatsQueueConfig, // 保存配置以供重连使用
    /// 断线重连状态标记
    reconnecting: Arc<AtomicBool>,
    /// 连接健康状态
    healthy: Arc<AtomicBool>,
}

/// JetStream 队列运行指标
#[derive(Debug, Clone)]
pub struct QueueStreamMetrics {
    pub backlog_messages: u64,
    pub backlog_bytes: u64,
    pub max_messages: i64,
    pub max_bytes: i64,
    pub max_age_seconds: Option<u64>,
}

impl NatsTaskQueue {
    /// 连接NATS服务器，带超时和重试机制
    pub async fn connect(queue_name: &'static str, config: &NatsQueueConfig) -> Result<Self> {
        info!("[plug] 正在连接NATS服务器: {}", config.server_url);

        // [tool] 连接重试配置
        const MAX_RETRIES: u32 = 3;
        const CONNECT_TIMEOUT_SECS: u64 = 10;
        let mut retry_delay = Duration::from_secs(1);

        for attempt in 1..=MAX_RETRIES {
            info!("尝试连接NATS (第{}/{}次)...", attempt, MAX_RETRIES);

            // [ok] 带超时的连接尝试
            let connect_result = tokio::time::timeout(
                Duration::from_secs(CONNECT_TIMEOUT_SECS),
                Self::try_connect(config),
            )
            .await;

            match connect_result {
                Ok(Ok(client)) => {
                    info!("[ok] NATS连接成功 (尝试 {}/{})", attempt, MAX_RETRIES);

                    // 创建JetStream上下文
                    let context = jetstream::new(client);

                    // 创建或获取Stream
                    context
                        .get_or_create_stream(build_stream_config(config))
                        .await
                        .with_context(|| {
                            format!("创建/获取 JetStream Stream [{}] 失败", config.stream)
                        })?;

                    info!("[ok] JetStream Stream [{}] 就绪", config.stream);

                    let queue = Self {
                        context: Arc::new(RwLock::new(context)),
                        subject: config.subject.clone(),
                        stream: config.stream.clone(),
                        queue_name,
                        config: config.clone(),
                        reconnecting: Arc::new(AtomicBool::new(false)),
                        healthy: Arc::new(AtomicBool::new(true)),
                    };

                    // [loop] 启动后台健康监控任务
                    queue.start_health_monitor();

                    return Ok(queue);
                }
                Ok(Err(e)) => {
                    warn!(
                        "[fail] NATS连接失败 (尝试 {}/{}): {}",
                        attempt, MAX_RETRIES, e
                    );
                    METRICS_COLLECTOR.record_nats_connection_failure();
                }
                Err(_timeout) => {
                    warn!(
                        "[stopwatch] NATS连接超时 (尝试 {}/{}, {}秒)",
                        attempt, MAX_RETRIES, CONNECT_TIMEOUT_SECS
                    );
                    METRICS_COLLECTOR.record_nats_connection_timeout();
                }
            }

            // 如果还有重试次数，等待后重试
            if attempt < MAX_RETRIES {
                info!("等待 {:?} 后重试...", retry_delay);
                tokio::time::sleep(retry_delay).await;
                retry_delay *= 2; // 指数退避
            }
        }

        // 所有重试都失败
        let error_msg = format!(
            "NATS连接失败，已重试{}次。请检查: 1) NATS服务是否运行 2) 网络连接 3) 配置地址: {}",
            MAX_RETRIES, config.server_url
        );
        error!("[fail] {}", error_msg);
        Err(anyhow!(error_msg))
    }

    /// 尝试连接NATS（不带超时）
    async fn try_connect(config: &NatsQueueConfig) -> Result<async_nats::Client> {
        if let Some(tls) = config.tls.as_ref().filter(|tls| tls.enabled) {
            let mut options = ConnectOptions::new();

            if tls.require_tls {
                options = options.require_tls(true);
            }

            let ca_file = tls
                .ca_file
                .as_ref()
                .ok_or_else(|| anyhow!("NATS TLS 配置缺少 ca_file"))?;
            options = options.add_root_certificates(PathBuf::from(ca_file));

            match (&tls.client_cert, &tls.client_key) {
                (Some(cert), Some(key)) => {
                    options =
                        options.add_client_certificate(PathBuf::from(cert), PathBuf::from(key));
                }
                (None, None) => {}
                _ => {
                    return Err(anyhow!("NATS TLS 配置 client_cert/client_key 需要同时提供"));
                }
            }

            options
                .connect(&config.server_url)
                .await
                .with_context(|| format!("TLS连接NATS失败: {}", config.server_url))
        } else {
            async_nats::connect(&config.server_url)
                .await
                .with_context(|| format!("连接NATS失败: {}", config.server_url))
        }
    }

    pub fn jetstream_context(&self) -> jetstream::Context {
        if let Ok(guard) = self.context.try_read() {
            return guard.clone();
        }

        // 退化为阻塞等待，避免在重连期间直接 panic
        warn!("[warn] JetStream context 读取被阻塞，等待重连完成");
        self.context.blocking_read().clone()
    }

    /// 关联的 JetStream 队列名称
    pub fn queue_name(&self) -> &'static str {
        self.queue_name
    }

    /// NEW 获取队列深度 (pending消息数)
    pub async fn get_queue_depth(&self) -> Result<u64> {
        let context = self.context.read().await;
        let mut stream = context
            .get_stream(&self.stream)
            .await
            .context("获取Stream失败")?;

        let info = stream.info().await.context("获取Stream信息失败")?;

        // pending消息数 = 总消息数
        Ok(info.state.messages)
    }

    /// NEW 获取流量统计信息（backlog 与保留策略）
    pub async fn stream_metrics(&self) -> Result<QueueStreamMetrics> {
        let context = self.context.read().await;
        let mut stream = context
            .get_stream(&self.stream)
            .await
            .context("获取Stream失败")?;

        let info = stream.info().await.context("获取Stream信息失败")?;
        let max_age = info.config.max_age;
        let max_age_seconds = if max_age.is_zero() {
            None
        } else {
            Some(max_age.as_secs())
        };

        Ok(QueueStreamMetrics {
            backlog_messages: info.state.messages,
            backlog_bytes: info.state.bytes,
            max_messages: info.config.max_messages,
            max_bytes: info.config.max_bytes,
            max_age_seconds,
        })
    }

    /// NEW 获取配置（用于dynamic_worker的Consumer工厂）
    pub fn get_config(&self) -> &NatsQueueConfig {
        &self.config
    }

    /// [loop] 启动后台健康监控任务
    fn start_health_monitor(&self) {
        let queue_clone = self.clone();
        tokio::spawn(async move {
            queue_clone.health_monitor_loop().await;
        });
    }

    /// 健康监控循环
    async fn health_monitor_loop(&self) {
        let mut check_interval = tokio::time::interval(Duration::from_secs(30)); // 每30秒检查一次

        loop {
            check_interval.tick().await;

            // 检查连接健康状态
            if let Err(e) = self.check_connection_health().await {
                warn!("[warn] NATS连接健康检查失败: {}", e);
                self.healthy.store(false, Ordering::SeqCst);

                // 如果未在重连中，触发重连
                if !self.reconnecting.load(Ordering::SeqCst) {
                    info!("[loop] 触发NATS断线重连...");
                    self.reconnect().await;
                }
            } else {
                // 连接健康，更新状态
                if !self.healthy.load(Ordering::SeqCst) {
                    info!("[ok] NATS连接已恢复健康");
                    self.healthy.store(true, Ordering::SeqCst);
                }
            }
        }
    }

    /// 检查NATS连接健康状态
    async fn check_connection_health(&self) -> Result<()> {
        let context = self.context.read().await;

        // 尝试获取stream信息作为健康检查
        match tokio::time::timeout(Duration::from_secs(5), context.get_stream(&self.stream)).await {
            Ok(Ok(mut stream)) => {
                // 尝试获取stream info
                match stream.info().await {
                    Ok(_) => Ok(()),
                    Err(e) => Err(anyhow!("获取Stream信息失败: {}", e)),
                }
            }
            Ok(Err(e)) => Err(anyhow!("获取Stream失败: {}", e)),
            Err(_) => Err(anyhow!("健康检查超时（5秒）")),
        }
    }

    /// [loop] 执行断线重连
    async fn reconnect(&self) {
        // 设置重连标记，防止并发重连
        if self.reconnecting.swap(true, Ordering::SeqCst) {
            warn!("[warn] 已有重连任务在进行中，跳过");
            return;
        }

        info!("[loop] 开始NATS断线重连流程...");
        METRICS_COLLECTOR.record_nats_connection_failure();

        const MAX_RECONNECT_ATTEMPTS: u32 = 5;
        let mut retry_delay = Duration::from_secs(2);

        for attempt in 1..=MAX_RECONNECT_ATTEMPTS {
            info!(
                "尝试重连NATS (第{}/{}次)...",
                attempt, MAX_RECONNECT_ATTEMPTS
            );

            match tokio::time::timeout(Duration::from_secs(10), Self::try_connect(&self.config))
                .await
            {
                Ok(Ok(client)) => {
                    // 连接成功，创建新的JetStream context
                    let new_context = jetstream::new(client);

                    // 确保Stream存在
                    match new_context
                        .get_or_create_stream(build_stream_config(&self.config))
                        .await
                    {
                        Ok(_) => {
                            // 更新context
                            let mut context_guard = self.context.write().await;
                            *context_guard = new_context;
                            drop(context_guard);

                            info!(
                                "[ok] NATS重连成功 (尝试 {}/{})",
                                attempt, MAX_RECONNECT_ATTEMPTS
                            );
                            self.healthy.store(true, Ordering::SeqCst);
                            self.reconnecting.store(false, Ordering::SeqCst);
                            return;
                        }
                        Err(e) => {
                            warn!("[fail] 创建/获取Stream失败: {}", e);
                        }
                    }
                }
                Ok(Err(e)) => {
                    warn!(
                        "[fail] NATS重连失败 (尝试 {}/{}): {}",
                        attempt, MAX_RECONNECT_ATTEMPTS, e
                    );
                }
                Err(_) => {
                    warn!(
                        "[stopwatch] NATS重连超时 (尝试 {}/{}, 10秒)",
                        attempt, MAX_RECONNECT_ATTEMPTS
                    );
                    METRICS_COLLECTOR.record_nats_connection_timeout();
                }
            }

            // 如果还有重试次数，等待后重试
            if attempt < MAX_RECONNECT_ATTEMPTS {
                info!("等待 {:?} 后重试重连...", retry_delay);
                tokio::time::sleep(retry_delay).await;
                retry_delay = (retry_delay * 2).min(Duration::from_secs(60)); // 最大60秒
            }
        }

        // 所有重连尝试都失败
        error!(
            "[fail] NATS重连失败，已尝试{}次。连接将在下次健康检查时继续重试。",
            MAX_RECONNECT_ATTEMPTS
        );
        self.reconnecting.store(false, Ordering::SeqCst);
    }

    /// 获取连接健康状态
    pub fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    /// 获取是否正在重连
    pub fn is_reconnecting(&self) -> bool {
        self.reconnecting.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl TaskQueue for NatsTaskQueue {
    async fn enqueue(&self, task: PreviewTask) -> Result<()> {
        let payload = serde_json::to_vec(&task).context("序列化预审任务失败")?;

        // 通过RwLock获取context
        let context = self.context.read().await;
        let ack = context
            .publish(self.subject.clone(), payload.into())
            .await
            .context("发布预审任务消息失败")?;

        ack.await.context("等待 JetStream 确认失败")?;
        METRICS_COLLECTOR.record_queue_enqueue(self.queue_name, None);
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// NATS JetStream 任务消费者
pub struct NatsTaskQueueConsumer {
    queue_name: &'static str,
    context: jetstream::Context,
    config: NatsQueueConfig,
}

impl NatsTaskQueueConsumer {
    pub fn new(
        queue_name: &'static str,
        context: jetstream::Context,
        config: NatsQueueConfig,
    ) -> Self {
        Self {
            queue_name,
            context,
            config,
        }
    }

    pub async fn connect(queue_name: &'static str, config: NatsQueueConfig) -> Result<Self> {
        let queue = NatsTaskQueue::connect(queue_name, &config).await?;
        Ok(Self::new(queue_name, queue.jetstream_context(), config))
    }

    pub async fn run(self, handler: Arc<dyn PreviewTaskHandler>) -> Result<()> {
        info!(
            stream = %self.config.stream,
            subject = %self.config.subject,
            durable_consumer = %self.config.durable_consumer,
            ack_wait_ms = self.config.ack_wait_ms,
            max_batch = self.config.max_batch,
            pull_wait_ms = self.config.pull_wait_ms,
            "启动 NATS 任务队列消费者"
        );

        loop {
            info!(
                stream = %self.config.stream,
                durable_consumer = %self.config.durable_consumer,
                "准备连接 JetStream stream/consumer"
            );
            let stream = self
                .context
                .get_or_create_stream(build_stream_config(&self.config))
                .await
                .with_context(|| {
                    format!("创建/获取 JetStream Stream [{}] 失败", self.config.stream)
                })?;

            let consumer = stream
                .get_or_create_consumer(
                    &self.config.durable_consumer,
                    build_consumer_config(&self.config),
                )
                .await
                .with_context(|| {
                    format!("创建/获取消费者 [{}] 失败", self.config.durable_consumer)
                })?;

            let inflight = Arc::new(AtomicU64::new(0));
            METRICS_COLLECTOR.record_worker_inflight(&self.config.durable_consumer, 0);

            let mut messages = consumer
                .stream()
                .max_messages_per_batch(self.config.max_batch)
                .expires(Duration::from_millis(self.config.pull_wait_ms))
                .messages()
                .await
                .context("获取 JetStream 消息流失败")?;

            while let Some(item) = messages.next().await {
                match item {
                    Ok(message) => {
                        let inflight_now = inflight.fetch_add(1, Ordering::SeqCst) + 1;
                        METRICS_COLLECTOR
                            .record_worker_inflight(&self.config.durable_consumer, inflight_now);

                        let payload = message.payload.clone();
                        let mut success = false;
                        let mut retry = false;

                        match serde_json::from_slice::<PreviewTask>(&payload) {
                            Ok(task) => {
                                let preview_id = task.preview_id.clone();
                                let message_info = message.info();
                                let delivered = message_info
                                    .as_ref()
                                    .map(|info| info.delivered)
                                    .unwrap_or(0);
                                let pending =
                                    message_info.as_ref().map(|info| info.pending).unwrap_or(0);
                                let stream_sequence = message_info
                                    .as_ref()
                                    .map(|info| info.stream_sequence)
                                    .unwrap_or(0);
                                let consumer_sequence = message_info
                                    .as_ref()
                                    .map(|info| info.consumer_sequence)
                                    .unwrap_or(0);
                                tracing::info!(
                                    target: "queue.consumer",
                                    event = events::QUEUE_DEQUEUE,
                                    preview_id = %preview_id,
                                    stream = %self.config.stream,
                                    consumer = %self.config.durable_consumer,
                                    delivered_attempts = delivered,
                                    pending,
                                    stream_sequence,
                                    consumer_sequence
                                );
                                tracing::debug!(
                                    preview_id = %preview_id,
                                    stream = %self.config.stream,
                                    consumer = %self.config.durable_consumer,
                                    ack_wait_ms = self.config.ack_wait_ms,
                                    "收到预审任务消息"
                                );
                                match handler.handle_preview_task(task).await {
                                    Ok(_) => {
                                        success = true;
                                        debug!(
                                            preview_id = %preview_id,
                                            stream = %self.config.stream,
                                            consumer = %self.config.durable_consumer,
                                            "预审任务处理完成，准备 ACK"
                                        );
                                        if let Err(err) = message.ack().await {
                                            warn!(
                                                preview_id = %preview_id,
                                                error = %err,
                                                "预审任务完成但 ACK 失败"
                                            );
                                            success = false;
                                            retry = true;
                                        } else {
                                            debug!(preview_id = %preview_id, "NATS 消息 ACK 成功");
                                        }
                                    }
                                    Err(err) => {
                                        error!(
                                            preview_id = %preview_id,
                                            error = %err,
                                            "预审任务处理失败，将重试"
                                        );
                                        retry = true;
                                        if let Err(nak_err) =
                                            message.ack_with(AckKind::Nak(None)).await
                                        {
                                            warn!("发送 NAK 失败: {:#}", nak_err);
                                        } else {
                                            warn!(
                                                preview_id = %preview_id,
                                                "已发送 NAK，等待 JetStream 重试"
                                            );
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                error!("无法解析任务消息，将终止该消息: {:#}", err);
                                if let Err(term_err) = message.ack_with(AckKind::Term).await {
                                    warn!("终止消息失败: {:#}", term_err);
                                } else {
                                    warn!("因解析失败终止消息，防止重复投递");
                                }
                            }
                        }

                        METRICS_COLLECTOR.record_queue_dequeue(self.queue_name, success, None);
                        if retry {
                            METRICS_COLLECTOR.record_queue_retry(self.queue_name);
                        }

                        let previous = inflight
                            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |current| {
                                current.checked_sub(1)
                            })
                            .unwrap_or(0);
                        let remaining = previous.saturating_sub(1);
                        METRICS_COLLECTOR
                            .record_worker_inflight(&self.config.durable_consumer, remaining);
                    }
                    Err(err) => {
                        warn!("从 NATS 拉取消息失败: {:#}", err);
                        break;
                    }
                }
            }

            METRICS_COLLECTOR.record_worker_inflight(&self.config.durable_consumer, 0);

            warn!(
                stream = %self.config.stream,
                consumer = %self.config.durable_consumer,
                wait_ms = self.config.pull_wait_ms.max(500),
                "NATS 消息流结束，等待后重建"
            );
            sleep(Duration::from_millis(self.config.pull_wait_ms.max(500))).await;
        }
    }
}

/// 直接执行处理（单机模式）
pub struct DirectTaskQueue {
    handler: Arc<dyn PreviewTaskHandler>,
}

impl DirectTaskQueue {
    pub fn new(handler: Arc<dyn PreviewTaskHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait]
impl TaskQueue for DirectTaskQueue {
    async fn enqueue(&self, task: PreviewTask) -> Result<()> {
        let handler = Arc::clone(&self.handler);
        tokio::spawn(async move {
            if let Err(err) = handler.handle_preview_task(task).await {
                error!("直接处理预审任务失败: {:?}", err);
            }
        });
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn build_stream_config(config: &NatsQueueConfig) -> stream::Config {
    let mut stream_config = stream::Config::default();
    stream_config.name = config.stream.clone();
    stream_config.subjects = vec![config.subject.clone()];
    stream_config.retention = stream::RetentionPolicy::WorkQueue;
    let max_messages = config.max_messages.unwrap_or(-1);
    stream_config.max_messages = max_messages;
    stream_config.max_messages_per_subject = max_messages;
    stream_config.max_bytes = config.max_bytes.unwrap_or(-1);
    if let Some(age_secs) = config.max_age_seconds {
        stream_config.max_age = Duration::from_secs(age_secs);
    }
    stream_config
}

fn build_consumer_config(config: &NatsQueueConfig) -> consumer::pull::Config {
    consumer::pull::Config {
        durable_name: Some(config.durable_consumer.clone()),
        ack_policy: consumer::AckPolicy::Explicit,
        ack_wait: Duration::from_millis(config.ack_wait_ms),
        max_deliver: config.max_deliver as i64,
        filter_subject: config.subject.clone(),
        max_batch: config.max_batch as i64,
        ..Default::default()
    }
}

/// 启动任务队列 worker（独立进程）
pub async fn start_queue_worker(
    config: &TaskQueueConfig,
    handler: Arc<dyn PreviewTaskHandler>,
) -> Result<()> {
    match config.driver.clone() {
        TaskQueueDriver::Local => {
            info!("当前配置使用本地任务队列，无需独立 worker，任务在主节点内处理");
            Ok(())
        }
        TaskQueueDriver::Nats => {
            let nats_config = config
                .nats
                .as_ref()
                .cloned()
                .ok_or_else(|| anyhow!("缺少 NATS 队列配置"))?;
            let consumer = NatsTaskQueueConsumer::connect(PREVIEW_QUEUE_NAME, nats_config).await?;
            consumer.run(handler).await
        }
        TaskQueueDriver::Database => Err(anyhow!("数据库任务队列驱动尚未实现，无法启动 worker")),
    }
}
