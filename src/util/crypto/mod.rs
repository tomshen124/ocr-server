//! 加密工具模块
//! 提供用户敏感信息的加密存储功能

pub mod aes;

pub use aes::{AesEncryption, EncryptionConfig};
