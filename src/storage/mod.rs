// 存储抽象层模块

pub mod factory;
pub mod failover;
pub mod local;
pub mod oss;
pub mod traits;

pub use failover::FailoverStorage;
pub use traits::Storage;
