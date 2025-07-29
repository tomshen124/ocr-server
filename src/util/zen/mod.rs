//! Zen模块 - OCR智能预审的核心业务逻辑
//! 
//! 本模块包含：
//! - theme: 主题管理和事项映射
//! - rules: 规则引擎管理
//! - evaluation: OCR评估逻辑
//! - downloader: 文件下载工具

pub mod theme;
pub mod rules;
pub mod evaluation;
pub mod downloader;

// 重新导出核心公共接口以保持向后兼容性

// 主题管理相关
pub use theme::{
    find_theme_by_matter, get_theme_name, get_available_themes
};

// 规则引擎相关  
pub use rules::{
    reload_theme_rule, update_rule
};