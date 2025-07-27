// 数据库抽象层模块

pub mod traits;
pub mod sqlite;
pub mod factory;
pub mod models;
pub mod failover;

pub use traits::Database;
pub use factory::{create_database, DatabaseConfig};
pub use models::*;
pub use failover::FailoverDatabase;

use anyhow::Result;
use async_trait::async_trait;