//!
//!

pub mod loader;
pub mod types;
pub mod validator;

pub use loader::{ConfigLoader, ConfigWriter, RuntimeModeInfo};
pub use types::*;
pub use validator::{CompatibilityReport, ConfigValidator, ValidationIssue, ValidationReport};

impl Config {
    pub fn read_yaml(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        ConfigLoader::read_yaml(path)
    }

    pub fn write_yaml(&self, path: &std::path::Path) {
        let _ = ConfigWriter::write_yaml(self, path);
    }

    pub fn write_yaml_to_path(&self, path: &std::path::Path) -> anyhow::Result<()> {
        ConfigWriter::write_yaml_with_dir(self, path)
    }
}

impl Default for Config {
    fn default() -> Self {
        ConfigWriter::generate_template()
    }
}
