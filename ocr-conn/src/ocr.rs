#![allow(warnings)]

use crate::{preprocess, CURRENT_DIR};
use crossbeam::channel::{self, bounded};
use crossbeam::channel::{Receiver, Sender};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::ops::DerefMut;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{fmt, io, process};
use tracing::{debug, error, info, warn};
use base64::Engine;
use image::{GenericImageView, ImageFormat};

type Point = [usize; 2];

const ENGINE_FORCE_RESTART_FAILURES: u32 = 3;
const CIRCUIT_MAX_CONSECUTIVE_FAILURES: u32 = 5;
const CIRCUIT_COOLDOWN_SECS: u64 = 30;
const SLOW_CALL_WARN_THRESHOLD_MS: u128 = 8_000;
const DEFAULT_MAX_INPUT_BYTES: usize = 10 * 1024 * 1024; // 10MB
const DEFAULT_MAX_PIXELS: u64 = 25_000_000; // ~25MP, 5000x5000
const DEFAULT_MIN_DIMENSION: u32 = 16; // 过小图片直接判为无效

/// PaddleOCR-json 官方错误码
pub mod error_code {
    // 成功类 (正常返回)
    pub const OK_WITH_TEXT: u32 = 100; // 识别到文字
    pub const OK_NO_TEXT: u32 = 101; // 未识别到文字（正常）

    // 路径/文件类错误 (2xx) - 数据问题，不需重启
    pub const ERR_PATH_NOT_EXIST: u32 = 200;
    pub const ERR_PATH_ENCODE: u32 = 201;
    pub const ERR_FILE_OPEN: u32 = 202;
    pub const ERR_IMAGE_DECODE: u32 = 203;

    // 剪贴板类错误 (210-217) - 不适用于服务端

    // Base64类错误 (3xx) - 数据问题，不需重启
    pub const ERR_BASE64_DECODE: u32 = 300;
    pub const ERR_BASE64_IMDECODE: u32 = 301;

    // JSON/引擎类错误 (4xx) - 可能需要重启
    pub const ERR_JSON_DUMP: u32 = 400;
    pub const ERR_JSON_PARSE: u32 = 401;
    pub const ERR_JSON_KEY: u32 = 402;
    pub const ERR_NO_TASK: u32 = 403;

    /// 判断是否为"成功"结果（包括空文本）
    #[inline]
    pub fn is_success(code: u32) -> bool {
        matches!(code, OK_WITH_TEXT | OK_NO_TEXT)
    }

    /// 判断是否为数据问题（不需要重启引擎）
    #[inline]
    pub fn is_data_error(code: u32) -> bool {
        matches!(code, 200..=217 | 300..=301)
    }

    /// 判断是否可能需要重启引擎（引擎内部问题）
    #[inline]
    pub fn should_restart(code: u32) -> bool {
        matches!(code, 400..=403)
    }

    /// 获取错误码的人类可读描述
    pub fn description(code: u32) -> &'static str {
        match code {
            100 => "识别成功",
            101 => "未识别到文字",
            200 => "图片路径不存在",
            201 => "路径编码转换失败",
            202 => "无法打开文件",
            203 => "图片无法解码",
            210 => "剪贴板打开失败",
            211 => "剪贴板为空",
            212 => "剪贴板格式不支持",
            213 => "剪贴板句柄获取失败",
            214 => "剪贴板文件数量无效",
            215 => "剪贴板位图信息获取失败",
            216 => "剪贴板位图数据获取失败",
            217 => "剪贴板图片通道数无效",
            300 => "Base64解析失败",
            301 => "Base64图片解码失败",
            400 => "JSON序列化失败",
            401 => "JSON反序列化失败",
            402 => "JSON键解析失败",
            403 => "未发现有效任务",
            _ => "未知错误",
        }
    }
}

#[derive(Deserialize, Debug, Clone)]

pub struct Content {
    code: u32,
    #[serde(default)]
    data: serde_json::Value, // 可以是数组或字符串
}

#[derive(Deserialize, Debug, Clone)]
pub struct ContentData {
    #[serde(rename = "box")]
    pub rect: Rectangle,
    pub score: f64,
    pub text: String,
}

pub type Rectangle = [Point; 4];

#[derive(Debug, Serialize, Clone)]
#[serde(untagged)]
pub enum ImageData {
    ImagePathDict { image_path: String },
    ImageBase64Dict { image_base64: String },
}

impl ImageData {
    pub fn from_path<P>(path: P) -> ImageData
    where
        P: AsRef<Path>,
    {
        let provided = path.as_ref();
        let abs = if provided.is_absolute() {
            provided.to_path_buf()
        } else {
            CURRENT_DIR.join(provided)
        };
        if let Err(err) = preprocess::preprocess_file_in_place(&abs) {
            warn!(
                "预处理本地图片失败，使用原始文件: {}, err: {}",
                abs.display(),
                err
            );
        }
        ImageData::ImagePathDict {
            image_path: abs.to_string_lossy().to_string(),
        }
    }

    pub fn from_base64(base64: String) -> ImageData {
        ImageData::ImageBase64Dict {
            image_base64: base64,
        }
    }

    pub fn from_bytes<T>(bytes: T) -> ImageData
    where
        T: AsRef<[u8]>,
    {
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        let raw = bytes.as_ref();
        let processed = preprocess::preprocess_bytes(raw).unwrap_or_else(|| raw.to_vec());
        ImageData::ImageBase64Dict {
            image_base64: engine.encode(&processed),
        }
    }
}

impl From<&Path> for ImageData {
    fn from(path: &Path) -> Self {
        ImageData::from_path(path)
    }
}

impl From<PathBuf> for ImageData {
    fn from(path: PathBuf) -> Self {
        ImageData::from_path(path)
    }
}

impl From<Vec<u8>> for ImageData {
    fn from(value: Vec<u8>) -> Self {
        ImageData::from_bytes(value)
    }
}

pub struct Extractor {
    process: Child,
    receiver: Receiver<String>,
    stderr_recent: Arc<Mutex<VecDeque<String>>>,
    engine_opts: OcrEngineOptions,
    last_used: Instant,
    consecutive_failures: u32,
    last_failure_at: Option<SystemTime>,
}

/// 引擎启动选项（由上层传入）
#[derive(Debug, Clone, Default)]
pub struct OcrEngineOptions {
    pub work_dir: Option<std::path::PathBuf>,
    pub binary: Option<std::path::PathBuf>,
    pub lib_path: Option<std::path::PathBuf>,
    pub timeout_secs: Option<u64>,
}

impl Extractor {
    pub fn new() -> io::Result<Self> {
        Self::new_with_options(OcrEngineOptions::default())
    }

    /// 使用启动选项创建实例
    pub fn new_with_options(opts: OcrEngineOptions) -> io::Result<Self> {
        let (process, receiver, stderr_recent) = Self::spawn_process(&opts)?;
        Ok(Self {
            process,
            receiver,
            stderr_recent,
            engine_opts: opts,
            last_used: Instant::now(),
            consecutive_failures: 0,
            last_failure_at: None,
        })
    }

    fn spawn_process(
        opts: &OcrEngineOptions,
    ) -> io::Result<(Child, Receiver<String>, Arc<Mutex<VecDeque<String>>>)> {
        let default_work = CURRENT_DIR.join("ocr");
        let work_dir = opts.work_dir.clone().unwrap_or(default_work);
        let bin = opts
            .binary
            .clone()
            .unwrap_or(work_dir.join("PaddleOCR-json"));
        let lib = opts.lib_path.clone().unwrap_or(work_dir.join("lib"));

        let mut process = Command::new(bin)
            .env("LD_LIBRARY_PATH", lib)
            .current_dir(&work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let (sender, receiver) = bounded::<String>(0);
        // stdout reader thread
        let stdout = match process.stdout.take() {
            Some(stdout) => Mutex::new(stdout),
            None => {
                tracing::error!("❌ 无法获取OCR进程的stdout");
                return Err(io::Error::new(io::ErrorKind::Other, "OCR进程stdout不可用"));
            }
        };
        rayon::spawn(move || {
            let stdout = stdout;
            let mut guard = stdout.lock();
            let mut stdout = BufReader::new(guard.deref_mut());
            let mut content = String::new();
            while stdout.read_line(&mut content).is_ok() {
                if sender.send(content.to_string()).is_err() {
                    break;
                }
                content.clear();
            }
        });

        // stderr reader thread - keep only recent lines
        let stderr_recent: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(200)));
        let stderr_recent_cloned = stderr_recent.clone();
        let stderr = match process.stderr.take() {
            Some(stderr) => Mutex::new(stderr),
            None => {
                tracing::error!("❌ 无法获取OCR进程的stderr");
                return Err(io::Error::new(io::ErrorKind::Other, "OCR进程stderr不可用"));
            }
        };
        rayon::spawn(move || {
            let mut guard = stderr.lock();
            let mut reader = BufReader::new(guard.deref_mut());
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok() {
                if !line.is_empty() {
                    let mut buf = stderr_recent_cloned.lock();
                    if buf.len() >= 200 {
                        buf.pop_front();
                    }
                    buf.push_back(line.trim_end().to_string());
                }
                line.clear();
            }
        });

        // Drain any startup banner noise quickly
        while receiver.recv_timeout(Duration::from_millis(200)).is_ok() {}
        ENGINE_STARTED.fetch_add(1, Ordering::Relaxed);
        Ok((process, receiver, stderr_recent))
    }

    fn read_line(&mut self) -> String {
        // Read until we get a JSON line or timeout
        let timeout = self.engine_opts.timeout_secs.unwrap_or(10);
        let deadline = Instant::now() + Duration::from_secs(timeout);
        loop {
            let now = Instant::now();
            if now >= deadline {
                return String::new();
            }
            let remain = deadline - now;
            match self.receiver.recv_timeout(remain) {
                Ok(content) => {
                    let trimmed = content.trim_start();
                    if trimmed.starts_with('{') {
                        return content;
                    }
                    // Skip non-JSON lines like version banners or logs
                    continue;
                }
                Err(_) => return String::new(),
            }
        }
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        let inner = self
            .process
            .stdin
            .as_mut()
            .ok_or(io::Error::new(io::ErrorKind::Other, "stdin not piped"))?;
        inner.write_fmt(fmt)
    }

    pub fn ocr(&mut self, image: ImageData) -> io::Result<String> {
        let start = Instant::now();
        let image_type = match &image {
            ImageData::ImagePathDict { image_path } => {
                tracing::debug!("📄 OCR处理: 文件路径 = {}", image_path);
                "file_path"
            }
            ImageData::ImageBase64Dict { image_base64 } => {
                let size_kb = image_base64.len() / 1024;
                tracing::debug!("📄 OCR处理: Base64数据，大小 = {} KB", size_kb);
                "base64"
            }
        };

        let s = serde_json::to_string(&image)?;
        self.write_fmt(format_args!("{}\n", s.trim()))?;
        let result = self.read_line();

        let elapsed = start.elapsed();
        if result.is_empty() {
            tracing::warn!(
                "⚠️ OCR引擎无响应，耗时: {:?}，类型: {}",
                elapsed,
                image_type
            );
        } else {
            tracing::debug!(
                "✅ OCR调用完成，耗时: {:?}，类型: {}, 响应长度: {} bytes",
                elapsed,
                image_type,
                result.len()
            );
        }

        Ok(result)
    }

    pub fn ocr_and_parse(&mut self, image: ImageData) -> Result<Vec<ContentData>, String> {
        let call_start = Instant::now();
        tracing::info!("🔍 开始OCR识别和解析");

        if let Err(e) = validate_image_data(&image) {
            tracing::warn!("❌ 输入校验失败，直接返回数据错误: {}", e);
            return Err(e);
        }

        // try once, restart on failure, then one more try
        let attempt = |this: &mut Self,
                       img: &ImageData,
                       attempt_num: u8|
         -> Result<Vec<ContentData>, String> {
            tracing::debug!("🔄 OCR尝试 #{}", attempt_num);
            let ocr_result = this.ocr(img.clone());
            let Ok(ocr_string) = ocr_result.as_ref() else {
                let err = ocr_result.err().unwrap();
                tracing::warn!("❌ OCR引擎调用失败 (尝试 #{}): {}", attempt_num, err);
                return Err(format!("OCR调用失败: {}", err));
            };
            if ocr_string.trim().is_empty() {
                tracing::warn!(
                    "❌ OCR引擎返回空响应 (尝试 #{}), 可能是超时或已崩溃",
                    attempt_num
                );
                return Err("OCR引擎无响应 (超时或崩溃)".to_string());
            }
            match serde_json::from_str::<Content>(ocr_string) {
                Ok(content) => {
                    use error_code::*;

                    if is_success(content.code) {
                        // code=100 或 code=101 都是成功
                        if content.code == OK_NO_TEXT {
                            tracing::debug!("ℹ️ OCR未识别到文字 (尝试 #{})，空白图片", attempt_num);
                            return Ok(Vec::new());
                        }
                        // code=100: 解析data数组
                        if let Ok(data_vec) =
                            serde_json::from_value::<Vec<ContentData>>(content.data)
                        {
                            let text_count = data_vec.len();
                            tracing::info!(
                                "✅ OCR识别成功 (尝试 #{}): 识别到 {} 个文本块",
                                attempt_num,
                                text_count
                            );
                            Ok(data_vec)
                        } else {
                            tracing::debug!("ℹ️ OCR完成但data为空 (尝试 #{})", attempt_num);
                            Ok(Vec::new())
                        }
                    } else if is_data_error(content.code) {
                        // 数据问题 (2xx/3xx): 直接返回错误，不触发重启
                        let desc = description(content.code);
                        let detail = content.data.as_str().unwrap_or("Unknown");
                        tracing::warn!(
                            "❌ 数据错误 (code={}): {} - {}",
                            content.code,
                            desc,
                            detail
                        );
                        // 使用特殊前缀标记数据错误，避免重试
                        Err(format!("[DATA_ERR:{}] {} - {}", content.code, desc, detail))
                    } else {
                        // 引擎问题 (4xx): 允许重启
                        let desc = description(content.code);
                        let detail = content.data.as_str().unwrap_or("Unknown");
                        tracing::warn!(
                            "❌ 引擎错误 (code={}): {} - {}",
                            content.code,
                            desc,
                            detail
                        );
                        Err(format!(
                            "[ENGINE_ERR:{}] {} - {}",
                            content.code, desc, detail
                        ))
                    }
                }

                Err(e) => {
                    tracing::warn!("❌ 解析OCR响应失败 (尝试 #{}): {}", attempt_num, e);
                    tracing::debug!(
                        "原始响应 (前500字符): {}",
                        &ocr_string.chars().take(500).collect::<String>()
                    );
                    Err(format!("Response JSON parse failed: {}", e))
                }
            }
        };

        match attempt(self, &image, 1) {
            Ok(ok) => {
                let total_elapsed = call_start.elapsed();
                tracing::info!("🎉 OCR处理成功完成，总耗时: {:?}", total_elapsed);
                return Ok(ok);
            }
            Err(first_err) => {
                // 数据问题（2xx/3xx）不触发重启，直接返回错误
                if is_data_error_message(&first_err) {
                    let total_elapsed = call_start.elapsed();
                    tracing::info!(
                        "ℹ️ 数据问题，直接返回错误（不重启引擎），耗时: {:?}",
                        total_elapsed
                    );
                    return Err(first_err);
                }

                tracing::warn!("⚠️ 第一次OCR尝试失败（引擎问题），准备重启引擎并重试...");

                // Restart engine and try again once
                let stderr_snapshot = {
                    let buf = self.stderr_recent.lock();
                    buf.iter().cloned().collect::<Vec<_>>()
                };
                if !stderr_snapshot.is_empty() {
                    tracing::warn!(
                        "📋 [PaddleOCR-json stderr 摘要 - 重启前最近{}行]",
                        stderr_snapshot.len()
                    );
                    for line in stderr_snapshot.iter().rev().take(20).rev() {
                        // 打印最后20行
                        tracing::warn!("stderr> {}", line);
                    }
                }
                self.restart();
                tracing::info!("🔄 引擎已重启，进行第二次OCR尝试...");
                match attempt(self, &image, 2) {
                    Ok(ok) => {
                        let total_elapsed = call_start.elapsed();
                        tracing::info!("✅ OCR处理在重启后成功，总耗时: {:?}", total_elapsed);
                        Ok(ok)
                    }
                    Err(second_err) => {
                        let total_elapsed = call_start.elapsed();
                        tracing::error!("💥 OCR处理最终失败，总耗时: {:?}", total_elapsed);
                        let stderr_snapshot2 = {
                            let buf = self.stderr_recent.lock();
                            buf.iter().cloned().collect::<Vec<_>>()
                        };
                        let summary = if stderr_snapshot2.is_empty() {
                            "<无stderr输出>".to_string()
                        } else {
                            let mut tail = String::new();
                            let tail_lines = stderr_snapshot2
                                .iter()
                                .rev()
                                .take(20)
                                .cloned()
                                .collect::<Vec<_>>();
                            for line in tail_lines.into_iter().rev() {
                                tail.push_str(&format!("{}\n", line));
                            }
                            tail
                        };
                        let message = format!(
                            "OCR失败（已重启尝试）。第一次错误: {}; 第二次错误: {}。stderr摘要:\n{}",
                            first_err, second_err, summary
                        );
                        tracing::error!("📊 详细错误信息: {}", message.replace('\n', " | "));
                        Err(message)
                    }
                }
            }
        }
    }

    fn restart(&mut self) {
        let _ = self.process.kill();
        // Drop existing receiver by swapping a dummy; then respawn
        match Self::spawn_process(&self.engine_opts) {
            Ok((proc, recv, stderr_recent)) => {
                self.process = proc;
                self.receiver = recv;
                self.stderr_recent = stderr_recent;
                self.last_used = Instant::now();
                self.consecutive_failures = 0;
                self.last_failure_at = None;
                ENGINE_RESTARTED.fetch_add(1, Ordering::Relaxed);
                info!("OCR引擎重启成功");
            }
            Err(e) => {
                error!("OCR引擎重启失败: {}", e);
            }
        }
    }

    fn ensure_running(&mut self) {
        match self.process.try_wait() {
            Ok(Some(status)) => {
                warn!(?status, "检测到OCR引擎已退出，准备重启");
                self.restart();
            }
            Ok(None) => {}
            Err(err) => {
                warn!("检查OCR引擎状态失败，将重新拉起: {}", err);
                self.restart();
            }
        }
    }

    fn mark_success(&mut self) {
        self.consecutive_failures = 0;
        self.last_failure_at = None;
        self.last_used = Instant::now();
    }

    fn mark_failure(&mut self) {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.last_failure_at = Some(SystemTime::now());
        self.last_used = Instant::now();
        if self.consecutive_failures >= ENGINE_FORCE_RESTART_FAILURES {
            warn!(
                "OCR引擎连续失败{}次，执行强制重启",
                self.consecutive_failures
            );
            self.restart();
            self.consecutive_failures = 0;
        }
    }
}

/// 判断错误消息是否为"数据问题"（不需要重启引擎）
/// 数据错误使用 [DATA_ERR:xxx] 前缀标记
fn is_data_error_message(err: &str) -> bool {
    err.starts_with("[DATA_ERR:")
}

/// 输入校验：限制大小、像素、格式，提前将数据问题拦截为 DATA_ERR
fn validate_image_data(image: &ImageData) -> Result<(), String> {
    match image {
        ImageData::ImagePathDict { image_path } => {
            let bytes = std::fs::read(image_path).map_err(|e| {
                format!(
                    "[DATA_ERR:READ_FAILED] 无法读取文件: {} ({})",
                    image_path, e
                )
            })?;
            validate_image_bytes(&bytes)
        }
        ImageData::ImageBase64Dict { image_base64 } => {
            let limits = &*IMAGE_LIMITS;
            if image_base64.len() > limits.max_input_bytes.saturating_mul(2) {
                return Err(format!(
                    "[DATA_ERR:IMAGE_TOO_LARGE] Base64 长度过大: {} bytes",
                    image_base64.len()
                ));
            }
            let decoded = base64::engine::general_purpose::STANDARD
                .decode(image_base64)
                .map_err(|e| format!("[DATA_ERR:BASE64_DECODE] Base64 解码失败: {}", e))?;
            validate_image_bytes(&decoded)
        }
    }
}

fn validate_image_bytes(bytes: &[u8]) -> Result<(), String> {
    let limits = &*IMAGE_LIMITS;

    if bytes.len() > limits.max_input_bytes {
        return Err(format!(
            "[DATA_ERR:IMAGE_TOO_LARGE] 输入文件过大: {} bytes (上限 {} bytes)",
            bytes.len(),
            limits.max_input_bytes
        ));
    }

    let format = image::guess_format(bytes)
        .map_err(|e| format!("[DATA_ERR:UNSUPPORTED_FORMAT] 无法识别格式: {}", e))?;
    let allowed = matches!(
        format,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Tiff
    );
    if !allowed {
        return Err(format!(
            "[DATA_ERR:UNSUPPORTED_FORMAT] 不支持的图片格式: {:?}",
            format
        ));
    }

    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("[DATA_ERR:IMAGE_DECODE] 图片解码失败: {}", e))?;
    let (w, h) = img.dimensions();
    if w < limits.min_dimension || h < limits.min_dimension {
        return Err(format!(
            "[DATA_ERR:IMAGE_TOO_SMALL] 图片尺寸过小: {}x{} (最小 {}x{})",
            w, h, limits.min_dimension, limits.min_dimension
        ));
    }
    let pixels = w as u64 * h as u64;
    if pixels > limits.max_pixels {
        return Err(format!(
            "[DATA_ERR:IMAGE_TOO_LARGE] 像素总数过大: {} (上限 {})",
            pixels, limits.max_pixels
        ));
    }
    Ok(())
}

struct ImageLimits {
    max_input_bytes: usize,
    max_pixels: u64,
    min_dimension: u32,
}

static IMAGE_LIMITS: LazyLock<ImageLimits> = LazyLock::new(|| {
    let max_input_bytes = std::env::var("OCR_MAX_INPUT_BYTES")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_INPUT_BYTES);
    let max_pixels = std::env::var("OCR_MAX_PIXELS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MAX_PIXELS);
    let min_dimension = std::env::var("OCR_MIN_DIMENSION")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(DEFAULT_MIN_DIMENSION);

    tracing::info!(
        max_input_bytes,
        max_pixels,
        min_dimension,
        "OCR 图片校验阈值已初始化（可通过环境变量 OCR_MAX_INPUT_BYTES / OCR_MAX_PIXELS / OCR_MIN_DIMENSION 覆盖）"
    );

    ImageLimits {
        max_input_bytes,
        max_pixels,
        min_dimension,
    }
});

impl Drop for Extractor {
    fn drop(&mut self) {
        self.process.kill().ok();
    }
}

// =====================
// 简单双进程池（可复用OCR引擎）
// =====================

#[derive(Debug)]
struct CircuitState {
    consecutive_failures: u32,
    last_failure_at: Option<SystemTime>,
    open_until: Option<Instant>,
    open_until_epoch: Option<SystemTime>,
}

impl CircuitState {
    fn new() -> Self {
        Self {
            consecutive_failures: 0,
            last_failure_at: None,
            open_until: None,
            open_until_epoch: None,
        }
    }

    fn is_open(&self) -> bool {
        if let Some(until) = self.open_until {
            until > Instant::now()
        } else {
            false
        }
    }
}

struct PoolInner {
    engines: parking_lot::Mutex<Vec<Option<Extractor>>>,
    opts: parking_lot::Mutex<Option<OcrEngineOptions>>,
    circuit: parking_lot::Mutex<CircuitState>,
    max: usize,
}
impl PoolInner {
    fn record_success(&self) {
        let mut circuit = self.circuit.lock();
        if circuit.open_until.is_some() {
            info!("OCR进程池熔断状态解除");
        }
        circuit.consecutive_failures = 0;
        circuit.last_failure_at = None;
        circuit.open_until = None;
        circuit.open_until_epoch = None;
    }

    fn record_failure(&self) -> bool {
        let mut circuit = self.circuit.lock();
        circuit.consecutive_failures = circuit.consecutive_failures.saturating_add(1);
        circuit.last_failure_at = Some(SystemTime::now());
        if circuit.consecutive_failures >= CIRCUIT_MAX_CONSECUTIVE_FAILURES {
            let now_instant = Instant::now();
            let now_system = SystemTime::now();
            circuit.open_until = Some(now_instant + Duration::from_secs(CIRCUIT_COOLDOWN_SECS));
            circuit.open_until_epoch =
                Some(now_system + Duration::from_secs(CIRCUIT_COOLDOWN_SECS));
            circuit.consecutive_failures = 0;
            warn!(
                "OCR进程池因连续失败进入熔断，{}秒后尝试恢复",
                CIRCUIT_COOLDOWN_SECS
            );
            true
        } else {
            false
        }
    }

    fn ensure_circuit_allows_acquire(&self) -> Result<(), String> {
        let mut circuit = self.circuit.lock();
        if let Some(until) = circuit.open_until {
            let now = Instant::now();
            if until > now {
                let remaining = until.saturating_duration_since(now);
                let secs = remaining.as_secs().max(1);
                return Err(format!(
                    "ocr pool circuit open; retry after {}s (cooldown)",
                    secs
                ));
            } else {
                circuit.open_until = None;
                circuit.open_until_epoch = None;
                circuit.consecutive_failures = 0;
                info!("OCR进程池熔断期结束，恢复服务");
            }
        }
        Ok(())
    }

    fn circuit_snapshot(&self) -> (u32, bool, Option<SystemTime>) {
        let circuit = self.circuit.lock();
        let now = Instant::now();
        let is_open = circuit.open_until.map(|until| until > now).unwrap_or(false);
        (
            circuit.consecutive_failures,
            is_open,
            circuit.open_until_epoch,
        )
    }
}

pub struct ExtractorPool {
    inner: Arc<PoolInner>,
    semaphore: Arc<tokio::sync::Semaphore>,
}

pub struct ExtractorHandle {
    pool: Arc<PoolInner>,
    engine: Option<Extractor>,
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl ExtractorHandle {
    pub fn ocr_and_parse(&mut self, image: ImageData) -> Result<Vec<ContentData>, String> {
        if let Some(ref mut eng) = self.engine {
            eng.ensure_running();
            let start = Instant::now();
            let result = eng.ocr_and_parse(image);
            let elapsed = start.elapsed();
            if elapsed.as_millis() > SLOW_CALL_WARN_THRESHOLD_MS {
                warn!(
                    duration_ms = elapsed.as_millis() as u64,
                    "单次OCR调用耗时超出阈值"
                );
            }
            match result {
                Ok(contents) => {
                    eng.mark_success();
                    self.pool.record_success();
                    Ok(contents)
                }
                Err(err) => {
                    // 数据类错误不算引擎故障，避免触发熔断/重启
                    if is_data_error_message(&err) {
                        info!(
                            "收到数据错误，不计入引擎失败计数: {}",
                            err
                        );
                        return Err(err);
                    }

                    eng.mark_failure();
                    ENGINE_FAILURES.fetch_add(1, Ordering::Relaxed);
                    let opened = self.pool.record_failure();
                    if opened {
                        warn!("OCR进程池进入熔断窗口");
                    }
                    Err(err)
                }
            }
        } else {
            Err("invalid extractor handle".to_string())
        }
    }
}

impl Drop for ExtractorHandle {
    fn drop(&mut self) {
        // 归还引擎（若存在）
        if let Some(engine) = self.engine.take() {
            let mut engines = self.pool.engines.lock();
            // 放回第一个空位或追加
            if let Some(slot) = engines.iter_mut().find(|e| e.is_none()) {
                *slot = Some(engine);
            } else {
                engines.push(Some(engine));
            }
        }
        // _permit drop 将自动释放并发许可
    }
}

impl ExtractorPool {
    fn with_capacity(max: usize) -> Self {
        Self {
            inner: Arc::new(PoolInner {
                engines: parking_lot::Mutex::new(Vec::with_capacity(max)),
                opts: parking_lot::Mutex::new(None),
                circuit: parking_lot::Mutex::new(CircuitState::new()),
                max,
            }),
            semaphore: Arc::new(tokio::sync::Semaphore::new(max)),
        }
    }

    fn auto_size() -> usize {
        // 针对32核(16物理核心)服务器优化
        // OCR进程是CPU密集型，建议使用物理核心数的1/3到1/2
        // 16物理核心 / 3 ≈ 6个引擎（预留资源给系统和其他服务）
        6
    }

    pub fn new_auto() -> Self {
        Self::with_capacity(Self::auto_size())
    }

    /// 仅在未设置时设置引擎启动参数
    pub fn set_options_if_empty(&self, opts: OcrEngineOptions) {
        let mut guard = self.inner.opts.lock();
        if guard.is_none() {
            *guard = Some(opts);
        }
    }

    /// 获取一个可用的引擎句柄（异步等待并发许可）
    pub async fn acquire(&self) -> Result<ExtractorHandle, String> {
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| format!("semaphore closed: {}", e))?;

        if let Err(err) = self.inner.ensure_circuit_allows_acquire() {
            drop(permit);
            return Err(err);
        }

        // 取一个空闲引擎或在未满时创建新引擎
        let mut engines = self.inner.engines.lock();
        if let Some(pos) = engines.iter().position(|e| e.is_some()) {
            let mut eng = engines[pos].take();
            drop(engines);
            if let Some(ref mut engine) = eng {
                engine.ensure_running();
            }
            return Ok(ExtractorHandle {
                pool: self.inner.clone(),
                engine: eng,
                _permit: permit,
            });
        }

        // 没有空闲的，如果未达到上限则创建一个
        let active = engines.iter().filter(|e| e.is_some()).count();
        let max = self.inner.max;
        if active < max {
            // 获取启动参数
            let opts = self.inner.opts.lock().clone().unwrap_or_default();
            drop(engines);
            let mut eng = Extractor::new_with_options(opts)
                .map_err(|e| format!("spawn OCR engine failed: {}", e))?;
            eng.ensure_running();
            return Ok(ExtractorHandle {
                pool: self.inner.clone(),
                engine: Some(eng),
                _permit: permit,
            });
        }

        // 不应发生（许可数量与max一致），但为了安全放回许可并报错
        drop(engines);
        drop(permit);
        Err("no engine available".to_string())
    }
}

/// 全局引擎池（自动容量、延迟创建引擎）
static POOL_CAPACITY_OVERRIDE: OnceLock<usize> = OnceLock::new();

pub static GLOBAL_POOL: LazyLock<ExtractorPool> = LazyLock::new(|| {
    let capacity = POOL_CAPACITY_OVERRIDE
        .get()
        .copied()
        .filter(|cap| *cap > 0)
        .unwrap_or_else(ExtractorPool::auto_size);
    ExtractorPool::with_capacity(capacity)
});

static ENGINE_STARTED: AtomicU64 = AtomicU64::new(0);
static ENGINE_RESTARTED: AtomicU64 = AtomicU64::new(0);
static ENGINE_FAILURES: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, serde::Serialize)]
pub struct PoolStats {
    pub capacity: usize,
    pub available: usize,
    pub in_use: usize,
    pub total_started: u64,
    pub total_restarted: u64,
    pub total_failures: u64,
    pub consecutive_failures: u32,
    pub circuit_open: bool,
    pub circuit_open_until_epoch: Option<u64>,
}

impl ExtractorPool {
    pub fn stats(&self) -> PoolStats {
        let capacity = self.inner.max;
        let available = self.semaphore.available_permits();
        let (consecutive_failures, circuit_open, circuit_until_epoch) =
            self.inner.circuit_snapshot();
        let circuit_open_until_epoch = circuit_until_epoch
            .and_then(|ts| ts.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        PoolStats {
            capacity,
            available,
            in_use: capacity.saturating_sub(available),
            total_started: ENGINE_STARTED.load(Ordering::Relaxed),
            total_restarted: ENGINE_RESTARTED.load(Ordering::Relaxed),
            total_failures: ENGINE_FAILURES.load(Ordering::Relaxed),
            consecutive_failures,
            circuit_open,
            circuit_open_until_epoch,
        }
    }
}

pub fn ocr_pool_stats() -> PoolStats {
    GLOBAL_POOL.stats()
}

/// 配置全局OCR引擎池容量（需在首次使用前调用）
pub fn configure_pool_capacity(capacity: usize) {
    let normalized = capacity.clamp(1, 128);
    let _ = POOL_CAPACITY_OVERRIDE.set(normalized);
}
