//! Zen模块 - OCR智能预审的核心业务逻辑
//!
//! 本模块包含：
//! - evaluation: OCR评估逻辑
//! - enhanced_evaluator: 并发优化后的评估器
//! - downloader: 文件下载工具

pub mod downloader;
pub mod enhanced_evaluator;
pub mod evaluation;

// 重新导出核心公共接口以保持向后兼容性

// 增强版评估器相关
pub use enhanced_evaluator::{EnhancedOcrEvaluator, PreviewEvaluationResult};
