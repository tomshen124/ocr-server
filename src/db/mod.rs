// 数据库抽象层模块

#[cfg(feature = "dm_go")]
pub mod dm;
pub mod factory;
pub mod failover;
pub mod models;
pub mod sqlite;
pub mod traits;

#[cfg(feature = "dm_go")]
pub use factory::{
    create_database, DatabaseConfig, DatabaseType, DmConfig, SmartDatabaseManager, SqliteConfig,
};
#[cfg(not(feature = "dm_go"))]
pub use factory::{create_database, DatabaseConfig, DatabaseType, SqliteConfig};
pub use failover::FailoverDatabase;
pub use models::*;
pub use traits::Database;
