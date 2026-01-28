//! 用户信息加密存储模块
//! 负责用户敏感信息的加密/解密处理

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 用户信息表结构
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_id: String,                              // 用户唯一标识 (明文)
    pub user_name: Option<String>,                    // 用户姓名 (明文)
    pub user_name_encrypted: Option<String>,          // 用户姓名 (加密)
    pub certificate_type: String,                     // 证件类型 (明文)
    pub certificate_number_encrypted: Option<String>, // 证件号码 (加密)
    pub phone_number_encrypted: Option<String>,       // 手机号 (加密)
    pub email: Option<String>,                        // 邮箱 (明文或加密)
    pub organization_name: Option<String>,            // 组织名称 (明文)
    pub organization_code: Option<String>,            // 组织代码 (明文)
    pub login_count: i64,                             // 登录次数
    pub first_login_at: DateTime<Utc>,                // 首次登录时间
    pub last_login_at: DateTime<Utc>,                 // 最后登录时间
    pub created_at: DateTime<Utc>,                    // 创建时间
    pub updated_at: DateTime<Utc>,                    // 更新时间
    pub is_active: bool,                              // 是否激活
    pub data_source: String,                          // 数据来源 (sso/manual)
}

/// 用户信息加密级别配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDataEncryptionConfig {
    pub encrypt_certificate_number: bool, // 是否加密证件号码
    pub encrypt_phone_number: bool,       // 是否加密手机号
    pub encrypt_user_name: bool,          // 是否加密用户姓名
    pub encrypt_email: bool,              // 是否加密邮箱
    pub encryption_key_id: String,        // 加密密钥ID
}

impl Default for UserDataEncryptionConfig {
    fn default() -> Self {
        Self {
            encrypt_certificate_number: true, // 默认加密身份证号
            encrypt_phone_number: true,       // 默认加密手机号
            encrypt_user_name: false,         // 默认不加密姓名
            encrypt_email: false,             // 默认不加密邮箱
            encryption_key_id: "user_data_key_v1".to_string(),
        }
    }
}

/// 用户信息加密管理器
pub struct UserInfoEncryption {
    config: UserDataEncryptionConfig,
    encryption_key: String,
}

impl UserInfoEncryption {
    /// 创建新的加密管理器
    pub fn new(config: UserDataEncryptionConfig, encryption_key: String) -> Self {
        Self {
            config,
            encryption_key,
        }
    }

    /// 加密敏感用户信息
    pub fn encrypt_user_info(&self, session_user: &crate::model::SessionUser) -> Result<UserInfo> {
        Ok(UserInfo {
            user_id: session_user.user_id.clone(),

            // 用户姓名处理
            user_name: if self.config.encrypt_user_name {
                None
            } else {
                session_user.user_name.clone()
            },
            user_name_encrypted: if self.config.encrypt_user_name {
                session_user
                    .user_name
                    .as_ref()
                    .map(|name| self.encrypt_string(name))
                    .transpose()?
            } else {
                None
            },

            certificate_type: session_user.certificate_type.clone(),

            // 证件号码加密
            certificate_number_encrypted: if self.config.encrypt_certificate_number {
                session_user
                    .certificate_number
                    .as_ref()
                    .map(|cert| self.encrypt_string(cert))
                    .transpose()?
            } else {
                session_user.certificate_number.clone()
            },

            // 手机号加密
            phone_number_encrypted: if self.config.encrypt_phone_number {
                session_user
                    .phone_number
                    .as_ref()
                    .map(|phone| self.encrypt_string(phone))
                    .transpose()?
            } else {
                session_user.phone_number.clone()
            },

            // 邮箱处理
            email: if self.config.encrypt_email {
                None
            } else {
                session_user.email.clone()
            },

            organization_name: session_user.organization_name.clone(),
            organization_code: session_user.organization_code.clone(),
            login_count: 1,
            first_login_at: Utc::now(),
            last_login_at: Utc::now(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            is_active: true,
            data_source: "sso".to_string(),
        })
    }

    /// 解密用户信息为会话用户格式
    pub fn decrypt_to_session_user(
        &self,
        user_info: &UserInfo,
    ) -> Result<crate::model::SessionUser> {
        Ok(crate::model::SessionUser {
            user_id: user_info.user_id.clone(),

            // 解密用户姓名
            user_name: if let Some(encrypted_name) = &user_info.user_name_encrypted {
                Some(self.decrypt_string(encrypted_name)?)
            } else {
                user_info.user_name.clone()
            },

            certificate_type: user_info.certificate_type.clone(),

            // 解密证件号码
            certificate_number: if let Some(encrypted_cert) =
                &user_info.certificate_number_encrypted
            {
                Some(self.decrypt_string(encrypted_cert)?)
            } else {
                None
            },

            // 解密手机号
            phone_number: if let Some(encrypted_phone) = &user_info.phone_number_encrypted {
                Some(self.decrypt_string(encrypted_phone)?)
            } else {
                None
            },

            email: user_info.email.clone(),
            organization_name: user_info.organization_name.clone(),
            organization_code: user_info.organization_code.clone(),
            login_time: user_info.last_login_at.to_rfc3339(),
            last_active: Utc::now().to_rfc3339(),
        })
    }

    /// 更新用户登录信息
    pub fn update_login_info(&self, user_info: &mut UserInfo) {
        user_info.login_count += 1;
        user_info.last_login_at = Utc::now();
        user_info.updated_at = Utc::now();
    }

    /// 加密字符串
    ///
    /// [warn] 安全说明: 当前实现为Base64编码(仅开发环境)
    ///
    /// [clipboard] 生产环境推荐方案:
    /// - 算法: AES-256-GCM (使用ring或rustcrypto库)
    /// - 密钥管理: 环境变量或密钥管理服务(KMS)
    /// - 初始向量: 每次加密生成随机IV(12字节)
    /// - 输出格式: IV + Ciphertext + Tag(16字节)
    /// - 密钥ID: 支持密钥轮换
    ///
    /// [book] 参考实现: src/util/crypto/aes.rs 已有AES加密模块
    ///
    /// ```rust
    /// // 生产环境示例代码:
    /// use crate::util::crypto::aes;
    /// let key = env::var("ENCRYPTION_KEY")?;
    /// let encrypted = aes::encrypt_aes_gcm(plaintext.as_bytes(), key.as_bytes())?;
    /// let encoded = format!("ENC_{}_{}", self.config.encryption_key_id, base64::encode(&encrypted));
    /// ```
    fn encrypt_string(&self, plaintext: &str) -> Result<String> {
        use base64::{engine::general_purpose, Engine as _};

        // 开发环境: Base64编码(明文传输)
        // [warn] 生产环境必须替换为真实AES-256-GCM加密
        let encrypted = format!(
            "ENC_{}_{}",
            self.config.encryption_key_id,
            general_purpose::STANDARD.encode(plaintext.as_bytes())
        );

        Ok(encrypted)
    }

    /// 解密字符串
    ///
    /// [warn] 安全说明: 当前实现为Base64解码(仅开发环境)
    ///
    /// [clipboard] 生产环境推荐方案:
    /// - 对应encrypt_string的AES-256-GCM解密
    /// - 验证密钥ID匹配
    /// - 验证GCM认证标签
    /// - 处理密钥轮换场景
    ///
    /// [book] 参考实现: src/util/crypto/aes.rs 已有AES解密模块
    ///
    /// ```rust
    /// // 生产环境示例代码:
    /// use crate::util::crypto::aes;
    /// let key = env::var("ENCRYPTION_KEY")?;
    /// let (key_id, encrypted_data) = parse_encrypted_format(ciphertext)?;
    /// let decrypted = aes::decrypt_aes_gcm(&encrypted_data, key.as_bytes())?;
    /// ```
    fn decrypt_string(&self, ciphertext: &str) -> Result<String> {
        use base64::{engine::general_purpose, Engine as _};

        if let Some(encoded_data) =
            ciphertext.strip_prefix(&format!("ENC_{}_", self.config.encryption_key_id))
        {
            let decoded = general_purpose::STANDARD.decode(encoded_data)?;
            let plaintext = String::from_utf8(decoded)?;
            Ok(plaintext)
        } else {
            Err(anyhow::anyhow!("无效的加密数据格式"))
        }
    }
}

/// 用户信息查询过滤器
#[derive(Debug, Clone, Default)]
pub struct UserInfoFilter {
    pub user_id: Option<String>,
    pub organization_code: Option<String>,
    pub certificate_type: Option<String>,
    pub is_active: Option<bool>,
    pub data_source: Option<String>,
    pub login_after: Option<DateTime<Utc>>,
    pub created_after: Option<DateTime<Utc>>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// 用户信息统计数据
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfoStats {
    pub total_users: u64,
    pub active_users: u64,
    pub sso_users: u64,
    pub total_logins_last_30_days: u64,
    pub unique_organizations: u64,
    pub avg_logins_per_user: f64,
    pub last_updated: DateTime<Utc>,
}
