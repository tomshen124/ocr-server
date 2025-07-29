//! 预审处理模块
//! 处理预审请求、状态查询、结果展示等核心业务逻辑

use crate::model::preview::PreviewBody;
use crate::model::SessionUser;
use crate::util::IntoJson;
use crate::{CONFIG, AppState};
use chrono::Utc;
use axum::extract::State;
use axum::response::IntoResponse;
use axum::Json;
use std::sync::Arc;

/// 主预审处理函数
pub async fn preview(State(app_state): State<AppState>, req: axum::extract::Request) -> impl IntoResponse {
    // 从认证中间件获取SessionUser
    let session_user = req.extensions().get::<SessionUser>().cloned();
    
    // 提取请求体
    let (_parts, body) = req.into_parts();
    let bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!("读取请求体失败: {}", e);
            return crate::util::WebResult::err_custom("无效的请求体").into_json().into_response();
        }
    };
    
    // 尝试解析为标准格式，如果失败则尝试生产环境格式
    let mut preview_body: PreviewBody = match serde_json::from_slice::<PreviewBody>(&bytes) {
        Ok(body) => {
            tracing::info!("✅ 解析为标准格式成功");
            body
        },
        Err(_) => {
            // 尝试解析为生产环境格式
            match serde_json::from_slice::<crate::model::preview::ProductionPreviewRequest>(&bytes) {
                Ok(prod_request) => {
                    tracing::info!("✅ 解析为生产环境格式成功，正在转换...");
                    prod_request.to_preview_body()
                },
                Err(e) => {
                    tracing::error!("解析请求体失败（尝试了标准格式和生产环境格式）: {}", e);
                    return crate::util::WebResult::err_custom("无效的JSON格式").into_json().into_response();
                }
            }
        }
    };
    
    // 简化的用户ID验证
    if let Some(session_user) = session_user {
        tracing::info!("=== 用户身份验证 ===");
        tracing::info!("会话用户ID: {}", session_user.user_id);
        tracing::info!("请求用户ID: {}", preview_body.user_id);
        
        // 只验证用户ID匹配即可
        if preview_body.user_id != session_user.user_id {
            tracing::warn!("❌ 用户ID不匹配: 请求用户={}, 会话用户={}", 
                          preview_body.user_id, session_user.user_id);
            return crate::util::WebResult::err_custom("用户身份验证失败：用户ID不匹配").into_json().into_response();
        }
        
        tracing::info!("✅ 用户身份验证通过: {}", session_user.user_id);
    } else {
        tracing::error!("❌ 认证中间件未提供用户信息");
        return crate::util::WebResult::err_custom("认证信息缺失").into_json().into_response();
    }
    
    // 保留第三方系统的原始requestId
    let third_party_request_id = preview_body.preview.request_id.clone();
    
    // 服务端生成我们自己的安全previewId
    let our_preview_id = crate::api::utils::generate_secure_preview_id();
    tracing::info!("第三方请求ID: {}", third_party_request_id);
    tracing::info!("我们的预审ID: {}", our_preview_id);
    
    // 使用我们的previewId作为文件名（确保安全）
    preview_body.preview.request_id = our_preview_id.clone();
    
    // 🔒 安全改进：验证第三方提供的用户ID格式
    if preview_body.user_id.is_empty() || preview_body.user_id.len() > 50 {
        tracing::warn!("❌ 无效的用户ID格式: {}", preview_body.user_id);
        return crate::util::WebResult::err_custom("无效的用户ID").into_json().into_response();
    }

    // 建立ID映射关系（使用数据库替代文件操作）
    if let Err(e) = save_id_mapping_to_database(&app_state.database, &our_preview_id, &third_party_request_id, &preview_body.user_id).await {
        tracing::error!("保存ID映射失败: {}", e);
        return crate::util::WebResult::err_custom("系统错误").into_json().into_response();
    }
    
    // 立即返回预审访问URL，不等待预审完成
    let view_url = format!("{}/api/preview/view/{}", CONFIG.host, our_preview_id);

    tracing::info!("立即返回预审访问URL: {}", view_url);

    // 🔄 回滚到原始设计：立即异步处理，提供最佳用户体验
    // 原因：政务服务场景下，用户期望即时反馈，延迟处理严重影响体验
    // 安全性通过严格的身份验证和权限控制来保障
    let mut preview_clone = preview_body.clone();
    let preview_id_clone = our_preview_id.clone();
    let third_party_id_clone = third_party_request_id.clone();
    let database_clone = app_state.database.clone();
    let storage_clone = app_state.storage.clone();
    
    tokio::spawn(async move {
        // 🔥 立即获取OCR并发控制许可
        // 如果系统繁忙，这里会等待，避免系统过载
        let permit = match crate::OCR_SEMAPHORE.try_acquire() {
            Ok(permit) => {
                tracing::info!("✅ 获取OCR处理许可成功，当前可用许可: {}", 
                             crate::OCR_SEMAPHORE.available_permits());
                Some(permit)
            },
            Err(_) => {
                tracing::warn!("⏳ 系统繁忙，OCR任务排队等待...");
                // 如果try_acquire失败，使用acquire等待
                match crate::OCR_SEMAPHORE.acquire().await {
                    Ok(permit) => {
                        tracing::info!("✅ 等待后获取OCR处理许可成功");
                        Some(permit)
                    },
                    Err(e) => {
                        tracing::error!("❌ 获取OCR处理许可失败: {}", e);
                        // 更新数据库状态为失败
                        if let Err(db_err) = database_clone.update_preview_status(&preview_id_clone, crate::db::PreviewStatus::Failed).await {
                            tracing::error!("更新预审状态失败: {}", db_err);
                        }
                        return;
                    }
                }
            }
        };
        
        tracing::info!("=== 开始自动预审任务（并发控制） ===");
        tracing::info!("预审ID: {}", preview_id_clone);
        tracing::info!("第三方请求ID: {}", third_party_id_clone);
        tracing::info!("当前系统可用OCR处理槽位: {}", crate::OCR_SEMAPHORE.available_permits());
        
        // 智能主题匹配
        let theme_id = if let Some(manual_theme) = &preview_clone.preview.theme_id {
            tracing::info!("✅ 使用手动指定的主题ID: {}", manual_theme);
            manual_theme.clone()
        } else {
            // 自动匹配主题
            let auto_theme = crate::util::zen::find_theme_by_matter(
                Some(&preview_clone.preview.matter_id),
                Some(&preview_clone.preview.matter_name)
            );
            tracing::info!("✅ 自动匹配主题ID: {}", auto_theme);
            auto_theme
        };
        
        // 设置主题ID
        preview_clone.preview.theme_id = Some(theme_id);
        
        // 更新预审状态为"处理中"
        if let Err(e) = database_clone.update_preview_status(&preview_id_clone, crate::db::PreviewStatus::Processing).await {
            tracing::error!("更新预审状态失败: {}", e);
        }
        
        // 🔥 克隆一份用于后续的evaluation操作
        let preview_for_evaluation = preview_clone.clone();
        
        // 执行预审逻辑（使用存储抽象层）
        let preview_result = preview_clone.preview_with_storage(&storage_clone).await;
        
        // 根据结果更新数据库状态
        let status = if preview_result.is_ok() {
            crate::db::PreviewStatus::Completed
        } else {
            crate::db::PreviewStatus::Failed
        };
        
        if let Err(e) = database_clone.update_preview_status(&preview_id_clone, status).await {
            tracing::error!("更新最终预审状态失败: {}", e);
        }
        
        // 🔥 新增：保存evaluation_result到数据库
        // 尝试获取evaluation_result并保存到数据库
        if preview_result.is_ok() {
            match preview_for_evaluation.preview.clone().evaluate().await {
                Ok(evaluation_result) => {
                    match serde_json::to_string(&evaluation_result) {
                        Ok(evaluation_json) => {
                            if let Err(e) = database_clone.update_preview_evaluation_result(&preview_id_clone, &evaluation_json).await {
                                tracing::error!("保存evaluation_result到数据库失败: {}", e);
                            } else {
                                tracing::info!("✅ 成功保存evaluation_result到数据库: {}", preview_id_clone);
                            }
                        }
                        Err(e) => {
                            tracing::error!("序列化evaluation_result失败: {}", e);
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("重新评估预审结果失败: {}", e);
                }
            }
        }
        
        match preview_result {
            Ok(result) => {
                tracing::info!("✅ 预审任务完成成功");
                tracing::info!("预审结果: {:?}", result);
                
                // 可选：通知第三方系统（如果配置了回调）
                if let Err(e) = notify_third_party_system(&third_party_id_clone, "completed", Some(&result)).await {
                    tracing::warn!("通知第三方系统失败: {}", e);
                }
            }
            Err(e) => {
                tracing::error!("❌ 预审任务失败: {}", e);
                
                // 通知第三方系统失败
                if let Err(notify_err) = notify_third_party_system(&third_party_id_clone, "failed", None).await {
                    tracing::warn!("通知第三方系统失败: {}", notify_err);
                }
            }
        }
        
        // 🔥 释放OCR处理许可 - 确保许可被正确释放
        if let Some(_permit) = permit {
            tracing::info!("🔓 释放OCR处理许可，当前可用许可: {}", 
                         crate::OCR_SEMAPHORE.available_permits() + 1);
        }
        
        tracing::info!("=== 自动预审任务结束（并发控制） ===");
    });
    
    // 构建响应数据 - 第三方系统只需要知道提交成功
    let response_data = serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "previewId": our_preview_id,
            "thirdPartyRequestId": third_party_request_id,
            "status": "submitted",
            "message": "预审任务已提交，正在后台处理"
        }
    });

    // 预审访问URL是给用户的，不是给第三方系统的
    // 用户会从政务系统跳转到: /api/preview/view/{previewId}
    tracing::info!("用户预审访问URL: {}", view_url);
    
    Json(response_data).into_response()
}

// 使用数据库保存ID映射关系（替代文件操作）
pub async fn save_id_mapping_to_database(database: &Arc<dyn crate::db::Database>, preview_id: &str, third_party_request_id: &str, user_id: &str) -> anyhow::Result<()> {
    use crate::db::{PreviewRecord, PreviewStatus};
    
    tracing::info!("保存ID映射到数据库: {} -> {}", preview_id, third_party_request_id);
    
    let record = PreviewRecord {
        id: preview_id.to_string(),
        user_id: user_id.to_string(),
        file_name: format!("{}.html", preview_id),
        ocr_text: "".to_string(), // 将在后续处理中填充
        theme_id: None,
        evaluation_result: None,
        preview_url: format!("{}/api/preview/view/{}", CONFIG.host, preview_id),
        status: PreviewStatus::Pending,
        created_at: Utc::now(),
        updated_at: Utc::now(),
        third_party_request_id: Some(third_party_request_id.to_string()),
    };
    
    database.save_preview_record(&record).await?;
    
    tracing::info!("✅ ID映射已保存到数据库");
    Ok(())
}

// 从数据库获取ID映射信息（替代文件操作）
pub async fn get_id_mapping_from_database(database: &Arc<dyn crate::db::Database>, preview_id: &str) -> anyhow::Result<Option<crate::db::PreviewRecord>> {
    database.get_preview_record(preview_id).await
}

/// 通知第三方系统预审结果
pub async fn notify_third_party_system(
    third_party_request_id: &str, 
    status: &str, 
    result: Option<&crate::util::WebResult>
) -> anyhow::Result<()> {
    tracing::info!("=== 准备通知第三方系统 ===");
    tracing::info!("第三方请求ID: {}", third_party_request_id);
    tracing::info!("预审状态: {}", status);
    
    // 检查是否配置了回调URL
    let callback_url = &CONFIG.callback_url;
    if callback_url.is_empty() {
        tracing::info!("⚠️  未配置第三方回调URL，跳过结果推送");
        return Ok(());
    }
    
    // 注意：callback_url是SSO回调URL(GET)，不是第三方系统通知URL(POST)
    // 这里应该使用专门的第三方系统回调配置，而不是SSO callback URL
    
    tracing::info!("回调URL: {}", callback_url);
    
    // 构建回调数据
    let mut callback_data = serde_json::json!({
        "previewId": third_party_request_id, // 使用第三方requestId作为previewId
        "thirdPartyRequestId": third_party_request_id,
        "status": status,
        "timestamp": Utc::now().to_rfc3339(),
        "callbackType": "preview_result"
    });
    
    // 根据状态添加不同的数据
    match status {
        "completed" => {
            if let Some(web_result) = result {
                callback_data["result"] = serde_json::json!({
                    "success": web_result.success,
                    "data": web_result.data,
                    "message": "预审完成"
                });
                
                // 添加文件下载URL（如果需要）
                let view_url = format!("{}/api/preview/view/{}", CONFIG.host, third_party_request_id);
                callback_data["viewUrl"] = serde_json::json!(view_url);
            }
        }
        "failed" => {
            callback_data["result"] = serde_json::json!({
                "success": false,
                "message": "预审处理失败"
            });
        }
        _ => {}
    }
    
    // 发送回调请求
    match send_callback_request(callback_url, &callback_data).await {
        Ok(_) => {
            tracing::info!("✅ 第三方系统回调成功");
        }
        Err(e) => {
            tracing::error!("❌ 第三方系统回调失败: {}", e);
            // 这里可以考虑重试机制
        }
    }
    
    tracing::info!("=== 第三方系统通知结束 ===");
    Ok(())
}

// 发送回调请求
pub async fn send_callback_request(callback_url: &str, data: &serde_json::Value) -> anyhow::Result<()> {
    tracing::info!("发送回调请求到: {}", callback_url);
    
    // 构建第三方系统通知URL（区别于SSO callback URL）
    // TODO: 应该在配置中添加专门的第三方系统回调URL配置
    // 暂时跳过回调，避免混淆SSO callback和第三方系统通知
    tracing::warn!("⚠️  暂时跳过第三方系统回调，避免与SSO callback混淆");
    tracing::info!("建议在配置中添加 third_party_callback_url 独立配置");
    return Ok(());
    
    // 原代码保留用于参考：
    // let client = reqwest::Client::new();
    // let response = client
    //     .post(callback_url)
    //     .header("Content-Type", "application/json")
    //     .header("User-Agent", "OCR-Preview-Service/1.0")
    //     .json(data)
    //     .timeout(std::time::Duration::from_secs(30))
    //     .send()
    //     .await?;
    
    // 由于已跳过回调，不需要处理响应
    // let status_code = response.status();
    // let response_text = response.text().await?;
    // 
    // if status_code.is_success() {
    //     tracing::info!("回调请求成功: {} - {}", status_code, response_text);
    // } else {
    //     tracing::warn!("回调请求失败: {} - {}", status_code, response_text);
    //     return Err(anyhow::anyhow!("回调请求失败: {}", status_code));
    // }
    
    Ok(())
}

// preview_submit测试占位符函数已删除 - 使用主要的preview函数