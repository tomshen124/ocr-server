
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub user_id: String,
    pub user_name: Option<String>,
    pub user_name_encrypted: Option<String>,
    pub certificate_type: String,
    pub certificate_number_encrypted: Option<String>,
    pub phone_number_encrypted: Option<String>,
    pub email: Option<String>,
    pub organization_name: Option<String>,
    pub organization_code: Option<String>,
    pub login_count: i64,
    pub first_login_at: DateTime<Utc>,
    pub last_login_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub is_active: bool,
    pub data_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDataEncryptionConfig {
    pub encrypt_certificate_number: bool,
    pub encrypt_phone_number: bool,
    pub encrypt_user_name: bool,
    pub encrypt_email: bool,
    pub encryption_key_id: String,
}

impl Default for UserDataEncryptionConfig {
    fn default() -> Self {
        Self {
            encrypt_certificate_number: true,
            encrypt_phone_number: true,
            encrypt_user_name: false,
            encrypt_email: false,
            encryption_key_id: "user_data_key_v1".to_string(),
        }
    }
}

pub struct UserInfoEncryption {
    config: UserDataEncryptionConfig,
    encryption_key: String,
}

impl UserInfoEncryption {
    pub fn new(config: UserDataEncryptionConfig, encryption_key: String) -> Self {
        Self {
            config,
            encryption_key,
        }
    }

    pub fn encrypt_user_info(&self, session_user: &crate::model::SessionUser) -> Result<UserInfo> {
        Ok(UserInfo {
            user_id: session_user.user_id.clone(),

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

            certificate_number_encrypted: if self.config.encrypt_certificate_number {
                session_user
                    .certificate_number
                    .as_ref()
                    .map(|cert| self.encrypt_string(cert))
                    .transpose()?
            } else {
                session_user.certificate_number.clone()
            },

            phone_number_encrypted: if self.config.encrypt_phone_number {
                session_user
                    .phone_number
                    .as_ref()
                    .map(|phone| self.encrypt_string(phone))
                    .transpose()?
            } else {
                session_user.phone_number.clone()
            },

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

    pub fn decrypt_to_session_user(
        &self,
        user_info: &UserInfo,
    ) -> Result<crate::model::SessionUser> {
        Ok(crate::model::SessionUser {
            user_id: user_info.user_id.clone(),

            user_name: if let Some(encrypted_name) = &user_info.user_name_encrypted {
                Some(self.decrypt_string(encrypted_name)?)
            } else {
                user_info.user_name.clone()
            },

            certificate_type: user_info.certificate_type.clone(),

            certificate_number: if let Some(encrypted_cert) =
                &user_info.certificate_number_encrypted
            {
                Some(self.decrypt_string(encrypted_cert)?)
            } else {
                None
            },

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

    pub fn update_login_info(&self, user_info: &mut UserInfo) {
        user_info.login_count += 1;
        user_info.last_login_at = Utc::now();
        user_info.updated_at = Utc::now();
    }

    ///
    ///
    ///
    ///
    /// ```rust
    /// use crate::util::crypto::aes;
    /// let key = env::var("ENCRYPTION_KEY")?;
    /// let encrypted = aes::encrypt_aes_gcm(plaintext.as_bytes(), key.as_bytes())?;
    /// let encoded = format!("ENC_{}_{}", self.config.encryption_key_id, base64::encode(&encrypted));
    /// ```
    fn encrypt_string(&self, plaintext: &str) -> Result<String> {
        use base64::{engine::general_purpose, Engine as _};

        let encrypted = format!(
            "ENC_{}_{}",
            self.config.encryption_key_id,
            general_purpose::STANDARD.encode(plaintext.as_bytes())
        );

        Ok(encrypted)
    }

    ///
    ///
    ///
    ///
    /// ```rust
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
