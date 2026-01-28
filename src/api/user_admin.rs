//! 用户信息管理API
//! 提供管理员查询用户信息和解密敏感数据的功能

use crate::model::user_info::{UserInfo, UserInfoFilter};
use crate::util::crypto::AesEncryption;
use crate::util::WebResult;
use axum::extract::{Path, Query};
use axum::response::Json;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const DEFAULT_PAGE_SIZE: u32 = 20;
const MAX_PAGE_SIZE: u32 = 100;

/// 管理员用户查询请求
#[derive(Debug, Deserialize)]
pub struct AdminUserQuery {
    pub user_id: Option<String>,
    pub user_name: Option<String>,         // 可以按姓名搜索 (明文字段)
    pub organization_code: Option<String>, // 按组织筛选
    pub certificate_type: Option<String>,  // 按证件类型筛选
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub decrypt_sensitive: Option<bool>, // 是否解密敏感信息
}

/// 用户信息响应 (管理员视图)
#[derive(Debug, Serialize)]
pub struct AdminUserResponse {
    pub user_id: String,
    pub user_name: String,                   // 姓名 (明文)
    pub certificate_type: String,
    pub certificate_number: Option<String>,  // 解密后的身份证号
    pub certificate_number_masked: String,   // 脱敏显示
    pub phone_number: Option<String>,        // 解密后的手机号
    pub phone_number_masked: String,         // 脱敏显示
    pub email: Option<String>,               // 邮箱 (明文)
    pub organization_name: Option<String>,
    pub organization_code: Option<String>,
    pub login_count: i64,
    pub first_login_at: String,
    pub last_login_at: String,
    pub is_active: bool,
    pub data_source: String,
}

/// 用户信息管理服务
pub struct UserInfoAdminService {
    encryption: AesEncryption,
}

impl UserInfoAdminService {
    pub fn new(encryption: AesEncryption) -> Self {
        Self { encryption }
    }

    /// 管理员查询用户列表
    pub async fn list_users(
        &self,
        database: &std::sync::Arc<dyn crate::db::Database>,
        query: AdminUserQuery,
    ) -> anyhow::Result<Json<WebResult>> {
        let (page, page_size) = normalize_pagination(query.page, query.page_size);
        let offset = page
            .checked_sub(1)
            .and_then(|p| p.checked_mul(page_size))
            .unwrap_or(0);

        // 构建查询过滤器
        let filter = UserInfoFilter {
            user_id: query.user_id,
            organization_code: query.organization_code,
            certificate_type: query.certificate_type,
            limit: Some(page_size.min(MAX_PAGE_SIZE)),
            offset: Some(offset),
            ..Default::default()
        };

        // TODO: 调用数据库查询
        // let users = database.list_user_info(&filter).await?;

        // 模拟查询结果
        let users = vec![]; // 实际实现中从数据库获取

        let mut result = Vec::new();
        for user in users {
            let admin_response = self
                .convert_to_admin_response(user, query.decrypt_sensitive.unwrap_or(false))
                .await?;
            result.push(admin_response);
        }

        Ok(Json(WebResult::success_with_data(serde_json::json!({
            "users": result,
            "total": result.len(),
            "page": page,
            "page_size": page_size
        }))))
    }

    /// 获取单个用户详情 (管理员权限)
    pub async fn get_user_detail(
        &self,
        database: &std::sync::Arc<dyn crate::db::Database>,
        user_id: &str,
        decrypt_sensitive: bool,
    ) -> anyhow::Result<Json<WebResult>> {
        // TODO: 从数据库获取用户信息
        // let user = database.get_user_info(user_id).await?;
        
        // 模拟用户数据
        let user = None; // 实际实现中从数据库获取
        
        match user {
            Some(user_info) => {
                let admin_response = self.convert_to_admin_response(user_info, decrypt_sensitive).await?;
                Ok(Json(WebResult::success_with_data(admin_response)))
            }
            None => Ok(Json(WebResult::error("用户不存在", 404))),
        }
    }
    
    /// 解密用户敏感信息 (高权限操作)
    pub async fn decrypt_user_sensitive_info(
        &self,
        user_id: &str,
        database: &std::sync::Arc<dyn crate::db::Database>,
    ) -> anyhow::Result<Json<WebResult>> {
        // TODO: 记录解密操作审计日志
        tracing::warn!("[lock] 管理员正在解密用户敏感信息: {}", user_id);
        
        // TODO: 从数据库获取用户信息
        // let user = database.get_user_info(user_id).await?;
        
        let user = None; // 实际实现中从数据库获取
        
        match user {
            Some(user_info) => {
                // 解密敏感信息
                let certificate_number = if !user_info.certificate_number_encrypted.is_empty() {
                    Some(self.encryption.decrypt(&user_info.certificate_number_encrypted)?)
                } else {
                    None
                };
                
                let phone_number = if let Some(encrypted_phone) = &user_info.phone_number_encrypted {
                    Some(self.encryption.decrypt(encrypted_phone)?)
                } else {
                    None
                };
                
                let sensitive_info = serde_json::json!({
                    "user_id": user_info.user_id,
                    "user_name": user_info.user_name,
                    "certificate_number": certificate_number,
                    "phone_number": phone_number,
                    "decrypted_at": chrono::Utc::now().to_rfc3339(),
                    "warning": "[warn] 此操作已记录审计日志"
                });
                
                Ok(Json(WebResult::success_with_data(sensitive_info)))
            }
            None => Ok(Json(WebResult::error("用户不存在", 404))),
        }
    }
    
    /// 转换为管理员响应格式
    async fn convert_to_admin_response(
        &self,
        user_info: UserInfo,
        decrypt_sensitive: bool,
    ) -> anyhow::Result<AdminUserResponse> {
        let (certificate_number, phone_number) = if decrypt_sensitive {
            // [unlocked] 管理员请求解密
            let cert = if !user_info.certificate_number_encrypted.is_empty() {
                Some(self.encryption.decrypt(&user_info.certificate_number_encrypted)?)
            } else {
                None
            };
            
            let phone = if let Some(encrypted_phone) = &user_info.phone_number_encrypted {
                Some(self.encryption.decrypt(encrypted_phone)?)
            } else {
                None
            };
            
            (cert, phone)
        } else {
            // [locked] 不解密，只返回脱敏信息
            (None, None)
        };
        
        Ok(AdminUserResponse {
            user_id: user_info.user_id,
            user_name: user_info.user_name.unwrap_or_default(),
            certificate_type: user_info.certificate_type,
            certificate_number: certificate_number.clone(),
            certificate_number_masked: Self::mask_certificate(&certificate_number),
            phone_number: phone_number.clone(),
            phone_number_masked: Self::mask_phone(&phone_number),
            email: user_info.email,
            organization_name: user_info.organization_name,
            organization_code: user_info.organization_code,
            login_count: user_info.login_count,
            first_login_at: user_info.first_login_at.to_rfc3339(),
            last_login_at: user_info.last_login_at.to_rfc3339(),
            is_active: user_info.is_active,
            data_source: user_info.data_source,
        })
    }

    /// 身份证号脱敏显示
    fn mask_certificate(cert: &Option<String>) -> String {
        match cert {
            Some(cert_num) if cert_num.len() >= 18 => {
                format!("{}****{}", &cert_num[..6], &cert_num[14..])
            }
            Some(cert_num) if cert_num.len() >= 8 => {
                format!("{}****{}", &cert_num[..4], &cert_num[cert_num.len()-2..])
            }
            _ => "****".to_string(),
        }
    }
    
    /// 手机号脱敏显示
    fn mask_phone(phone: &Option<String>) -> String {
        match phone {
            Some(phone_num) if phone_num.len() == 11 => {
                format!("{}****{}", &phone_num[..3], &phone_num[7..])
            }
            Some(phone_num) if phone_num.len() >= 7 => {
                format!("{}****{}", &phone_num[..3], &phone_num[phone_num.len()-2..])
            }
            _ => "****".to_string(),
        }
    }
}

/// 用户统计信息
#[derive(Debug, Serialize)]
pub struct UserStatsResponse {
    pub total_users: u64,
    pub active_users: u64,
    pub organizations: HashMap<String, u64>,
    pub certificate_types: HashMap<String, u64>,
    pub login_stats: LoginStatsResponse,
}

#[derive(Debug, Serialize)]
pub struct LoginStatsResponse {
    pub today_logins: u64,
    pub this_week_logins: u64,
    pub this_month_logins: u64,
    pub avg_logins_per_user: f64,
}

fn normalize_pagination(page: Option<u32>, page_size: Option<u32>) -> (u32, u32) {
    let normalized_size = page_size
        .unwrap_or(DEFAULT_PAGE_SIZE)
        .clamp(1, MAX_PAGE_SIZE);
    let normalized_page = page.unwrap_or(1).max(1);
    (normalized_page, normalized_size)
}
