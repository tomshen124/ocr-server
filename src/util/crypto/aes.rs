
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Result};
use base64::{engine::general_purpose, Engine as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    pub encryption_key: String,
    pub key_version: String,
    pub algorithm: String,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            encryption_key: "".to_string(),
            key_version: "v1".to_string(),
            algorithm: "AES-256-GCM".to_string(),
        }
    }
}

pub struct AesEncryption {
    config: EncryptionConfig,
    key_bytes: Vec<u8>,
}

impl AesEncryption {
    pub fn new(config: EncryptionConfig) -> Result<Self> {
        if config.encryption_key.is_empty() {
            return Err(anyhow!("加密密钥不能为空"));
        }

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

    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        if plaintext.is_empty() {
            return Ok(String::new());
        }

        let encrypted_data = self.encrypt_with_aes_gcm(plaintext.as_bytes())?;

        let result = format!(
            "{}|{}",
            self.config.key_version,
            general_purpose::STANDARD.encode(&encrypted_data)
        );

        Ok(result)
    }

    pub fn decrypt(&self, ciphertext: &str) -> Result<String> {
        if ciphertext.is_empty() {
            return Ok(String::new());
        }

        let parts: Vec<&str> = ciphertext.split('|').collect();
        if parts.len() != 2 {
            return Err(anyhow!("无效的加密数据格式"));
        }

        let version = parts[0];
        let encrypted_base64 = parts[1];

        if version != self.config.key_version {
            return Err(anyhow!(
                "密钥版本不匹配: 期望 {}, 实际 {}",
                self.config.key_version,
                version
            ));
        }

        let encrypted_data = general_purpose::STANDARD
            .decode(encrypted_base64)
            .map_err(|e| anyhow!("解码加密数据失败: {}", e))?;

        let plaintext_bytes = self.decrypt_with_aes_gcm(&encrypted_data)?;

        let plaintext = String::from_utf8(plaintext_bytes)
            .map_err(|e| anyhow!("解密数据不是有效的UTF-8: {}", e))?;

        Ok(plaintext)
    }

    fn encrypt_with_aes_gcm(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
        let cipher = Aes256Gcm::new(key);

        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

        let ciphertext = cipher
            .encrypt(&nonce, plaintext)
            .map_err(|e| anyhow!("AES-GCM加密失败: {}", e))?;

        let mut result = Vec::new();
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    fn decrypt_with_aes_gcm(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 12 {
            return Err(anyhow!("密文太短，至少需要12字节的nonce"));
        }

        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);

        let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
        let cipher = Aes256Gcm::new(key);

        let plaintext = cipher
            .decrypt(nonce, encrypted)
            .map_err(|e| anyhow!("AES-GCM解密失败: {}", e))?;

        Ok(plaintext)
    }

    pub fn generate_key() -> String {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let key_bytes: [u8; 32] = rng.gen();
        hex::encode(key_bytes)
    }

    pub fn get_default_key() -> String {
        if let Ok(key) = std::env::var("AES_ENCRYPTION_KEY") {
            return key;
        }

        tracing::warn!("使用默认AES密钥，生产环境必须通过AES_ENCRYPTION_KEY环境变量设置！");
        "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string()
    }
}

#[cfg(feature = "production_crypto")]
mod production_aes {
    use super::*;
    use aes_gcm::{
        aead::{Aead, KeyInit, OsRng},
        Aes256Gcm, Key, Nonce,
    };

    impl AesEncryption {
        fn encrypt_with_aes_gcm(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
            let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
            let cipher = Aes256Gcm::new(key);

            let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

            let ciphertext = cipher
                .encrypt(&nonce, plaintext)
                .map_err(|e| anyhow!("AES加密失败: {}", e))?;

            let mut result = Vec::new();
            result.extend_from_slice(&nonce);
            result.extend_from_slice(&ciphertext);

            Ok(result)
        }

        fn decrypt_with_aes_gcm(&self, data: &[u8]) -> Result<Vec<u8>> {
            if data.len() < 12 {
                return Err(anyhow!("加密数据长度不足"));
            }

            let key = Key::<Aes256Gcm>::from_slice(&self.key_bytes);
            let cipher = Aes256Gcm::new(key);

            let (nonce_bytes, ciphertext) = data.split_at(12);
            let nonce = Nonce::from_slice(nonce_bytes);

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

        let plaintext = "330102199001011234";
        let encrypted = encryptor.encrypt(plaintext).unwrap();
        let decrypted = encryptor.decrypt(&encrypted).unwrap();

        assert_eq!(plaintext, decrypted);
        assert_ne!(plaintext, encrypted);
        assert!(encrypted.contains("v1|"));
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
