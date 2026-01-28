//! 真正的AES加密实现模块
//! 使用AES-256-GCM算法对敏感用户信息进行加密

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

/// AES加密配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    pub encryption_key: String, // 32字节的加密密钥 (hex编码)
    pub key_version: String,    // 密钥版本，用于密钥轮换
    pub algorithm: String,      // 加密算法标识
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            encryption_key: "".to_string(), // 需要在配置中指定
            key_version: "v1".to_string(),
            algorithm: "AES-256-GCM".to_string(),
        }
    }
}

/// AES加密管理器
pub struct AesEncryption {
    config: EncryptionConfig,
    key_bytes: Vec<u8>,
}

impl AesEncryption {
    /// 创建新的AES加密管理器
    pub fn new(config: EncryptionConfig) -> Result<Self> {
        // 验证密钥长度 (AES-256需要32字节)
        if config.encryption_key.is_empty() {
            return Err(anyhow!("加密密钥不能为空"));
        }

        // 从hex字符串解析密钥字节
        let key_bytes =
            hex::decode(&config.encryption_key).map_err(|e| anyhow!("无效的密钥格式: {}", e))?;

        if key_bytes.len() != 32 {
            return Err(anyhow!(
                "AES-256密钥必须是32字节长度，当前: {}",
                key_bytes.len()
            ));
        }

        Ok(Self { config, key_bytes })
    }

    /// 加密字符串
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }

        // 使用AES-256-GCM加密
        let encrypted_data = self.encrypt_with_aes_gcm(plaintext.as_bytes())?;

        // 格式：版本|加密数据(base64)
        let result = format!(
            "{}|{}",
            self.config.key_version,
            general_purpose::STANDARD.encode(&encrypted_data)
        );

        Ok(result)
    }

    /// 解密字符串
    pub fn decrypt(&self, ciphertext: &str) -> Result<String> {
        if ciphertext.is_empty() {
            return Ok(String::new());
        }

        // 解析版本和加密数据
        let parts: Vec<&str> = ciphertext.split('|').collect();
        if parts.len() != 2 {
            return Err(anyhow!("无效的加密数据格式"));
        }

        let version = parts[0];
        let encrypted_base64 = parts[1];

        // 验证版本
        if version != self.config.key_version {
            return Err(anyhow!(
                "密钥版本不匹配: 期望 {}, 实际 {}",
                self.config.key_version,
                version
            ));
        }

        // 解码base64
        let encrypted_data = general_purpose::STANDARD
            .decode(encrypted_base64)
            .map_err(|e| anyhow!("解码加密数据失败: {}", e))?;

        // 解密
        let plaintext_bytes = self.decrypt_with_aes_gcm(&encrypted_data)?;

        // 转换为字符串
        let plaintext = String::from_utf8(plaintext_bytes)
            .map_err(|e| anyhow!("解密数据不是有效的UTF-8: {}", e))?;

        Ok(plaintext)
    }

    /// AES-GCM加密实现
    fn encrypt_with_aes_gcm(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        // 创建cipher
        let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
        let cipher = Aes256Gcm::new(key);

        // 生成随机nonce (96位/12字节)
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        // 加密
        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow!("AES-GCM加密失败: {}", e))?;

        // 组合nonce和密文
        let mut result = Vec::new();
        result.extend_from_slice(&nonce); // 前12字节是nonce
        result.extend_from_slice(&ciphertext); // 后面是密文（包含认证标签）

        Ok(result)
    }

    /// AES-GCM解密实现
    fn decrypt_with_aes_gcm(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 12 {
            return Err(anyhow!("密文太短，至少需要12字节的nonce"));
        }

        // 分离nonce和密文
        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        // 创建cipher
        let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
        let cipher = Aes256Gcm::new(key);

        // 解密
        let plaintext = cipher
            .decrypt(nonce, encrypted)
            .map_err(|e| anyhow!("AES-GCM解密失败: {}", e))?;

        Ok(plaintext)
    }

    /// 生成新的32字节加密密钥 (用于初始化)
    pub fn generate_key() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let key_bytes: [u8; 32] = rng.gen();
        hex::encode(key_bytes)
    }

    /// 从环境变量或配置获取密钥
    pub fn get_default_key() -> String {
        // 优先从环境变量读取
        if let Ok(key) = std::env::var("AES_ENCRYPTION_KEY") {
            return key;
        }

        // 警告：这是示例密钥，生产环境必须更换！
        tracing::warn!("使用默认AES密钥，生产环境必须通过AES_ENCRYPTION_KEY环境变量设置！");
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
    }
}

/// 为生产环境生成真正的AES-GCM加密实现
#[cfg(feature = "production_crypto")]
mod production_aes {
    use super::*;
    use aes_gcm::{
        aead::{Aead, KeyInit, OsRng},
        Aes256Gcm, Key, Nonce,
    };

    impl AesEncryption {
        /// 生产环境的AES-GCM加密
        fn encrypt_with_aes_gcm(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
            let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
            let cipher = Aes256Gcm::new(key);

            // 生成随机nonce
            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

            // 加密
            let ciphertext = cipher
                .encrypt(&nonce, plaintext)
                .map_err(|e| anyhow!("AES加密失败: {}", e))?;

            // 组合nonce + ciphertext
            let mut result = Vec::new();
            result.extend_from_slice(&nonce);
            result.extend_from_slice(&ciphertext);

            Ok(result)
        }

        /// 生产环境的AES-GCM解密
        fn decrypt_with_aes_gcm(&self, data: &[u8]) -> Result<Vec<u8>> {
            if data.len() < 12 {
                return Err(anyhow!("加密数据长度不足"));
            }

            let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
            let cipher = Aes256Gcm::new(key);

            // 分离nonce和ciphertext
            let (nonce_bytes, ciphertext) = data.split_at(12);
            let nonce = Nonce::from_slice(nonce_bytes);

            // 解密
            let plaintext = cipher
                .decrypt(nonce, ciphertext)
                .map_err(|e| anyhow!("AES解密失败: {}", e))?;

            Ok(plaintext)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aes_encryption() {
        let config = EncryptionConfig {
            encryption_key: AesEncryption::generate_key(),
            key_version: "v1".to_string(),
            algorithm: "AES-256-GCM".to_string(),
        };

        let encryptor = AesEncryption::new(config).unwrap();

        let plaintext = "330102199001011234"; // 身份证号
        let encrypted = encryptor.encrypt(plaintext).unwrap();
        let decrypted = encryptor.decrypt(&encrypted).unwrap();

        assert_eq!(plaintext, decrypted);
        assert_ne!(plaintext, encrypted); // 确保已加密
        assert!(encrypted.contains("v1|")); // 包含版本信息
    }

    #[test]
    fn test_phone_encryption() {
        let config = EncryptionConfig {
            encryption_key: AesEncryption::generate_key(),
            key_version: "v1".to_string(),
            algorithm: "AES-256-GCM".to_string(),
        };

        let encryptor = AesEncryption::new(config).unwrap();

        let phone = "13800138000";
        let encrypted = encryptor.encrypt(phone).unwrap();
        let decrypted = encryptor.decrypt(&encrypted).unwrap();

        assert_eq!(phone, decrypted);
        println!("原始手机号: {}", phone);
        println!("加密后: {}", encrypted);
    }
}
