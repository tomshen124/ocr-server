//! 配置管理模块 (重构版本)
//!
//! 该模块已重构为更小的子模块:
//! - types: 配置数据结构定义
//! - loader: 配置加载和环境变量处理
//! - validator: 配置验证和兼容性检查
//!
//! 这种模块化设计提供了更好的代码组织、验证和扩展性

pub mod loader;
pub mod types;
pub mod validator;

// 重新导出主要接口以保持向后兼容性
pub use loader::{ConfigLoader, ConfigWriter, RuntimeModeInfo};
pub use types::*;
pub use validator::{CompatibilityReport, ConfigValidator, ValidationIssue, ValidationReport};

// 为了完全向后兼容，保留原始的Config实现
impl Config {
    /// 从YAML文件读取配置 (向后兼容)
    pub fn read_yaml(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        ConfigLoader::read_yaml(path)
    }

    /// 写入YAML文件 (向后兼容)
    pub fn write_yaml(&self, path: &std::path::Path) {
        let _ = ConfigWriter::write_yaml(self, path);
    }

    /// 写入YAML到指定路径 (向后兼容)
    pub fn write_yaml_to_path(&self, path: &std::path::Path) -> anyhow::Result<()> {
        ConfigWriter::write_yaml_with_dir(self, path)
    }
}

// 提供默认实现以保持向后兼容性
impl Default for Config {
    fn default() -> Self {
        ConfigWriter::generate_template()
    }
}
