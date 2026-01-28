/// Build-time metadata exposed at runtime.
pub const BUILD_VERSION: &str = env!("APP_BUILD_VERSION");
pub const BUILD_COMMIT: &str = env!("APP_BUILD_COMMIT");
pub const BUILD_TIMESTAMP: &str = env!("APP_BUILD_TIMESTAMP");

/// Human-readable summary combining Cargo version and build metadata.
pub fn summary() -> String {
    format!(
        "{} (build {}, commit {}, built at {})",
        env!("CARGO_PKG_VERSION"),
        BUILD_VERSION,
        BUILD_COMMIT,
        BUILD_TIMESTAMP
    )
}
