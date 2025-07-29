// 存储抽象层模块

pub mod traits;
pub mod local;
pub mod oss;
pub mod factory;
pub mod failover;

pub use traits::Storage;
pub use failover::FailoverStorage;