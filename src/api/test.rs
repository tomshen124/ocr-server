//! 测试模块
//! 处理测试模式模拟登录、测试数据生成等功能

use crate::model::SessionUser;
use crate::CONFIG;
use chrono::Utc;
use axum::Json;
use axum::http::StatusCode;
use tower_sessions::Session;

/// 测试模式模拟登录 - 仅在配置启用时可用
pub async fn mock_login_for_test(
    session: Session,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let config = &CONFIG;

    // 检查是否启用测试模式
    let test_mode_enabled = config.test_mode.as_ref()
        .map(|tm| tm.enabled && tm.auto_login)
        .unwrap_or(false);

    if !test_mode_enabled {
        return Ok(Json(serde_json::json!({
            "success": false,
            "msg": "测试模式未启用",
            "error": "TEST_MODE_DISABLED"
        })));
    }

    // 从配置或请求中获取用户信息
    let (user_id, user_name) = if let Some(test_config) = &config.test_mode {
        (
            test_config.test_user.id.clone(),
            test_config.test_user.username.clone()
        )
    } else {
        // 从请求中获取
        let user_id = payload.get("userId")
            .and_then(|v| v.as_str())
            .unwrap_or("test_user_001")
            .to_string();
        let user_name = payload.get("userName")
            .and_then(|v| v.as_str())
            .unwrap_or("测试用户")
            .to_string();
        (user_id, user_name)
    };

    // 创建会话用户
    let session_user = SessionUser {
        user_id: user_id.clone(),
        user_name: Some(user_name.clone()),
        certificate_type: "ID_CARD".to_string(),
        certificate_number: Some("test_cert_001".to_string()),
        email: Some("test@example.com".to_string()),
        phone_number: Some("13800000000".to_string()),
        organization_name: None,
        organization_code: None,
        login_time: Utc::now().to_string(),
        last_active: Utc::now().to_string(),
    };

    // 保存到会话
    if let Err(e) = session.insert("session_user", &session_user).await {
        tracing::error!("保存测试用户会话失败: {}", e);
        return Ok(Json(serde_json::json!({
            "success": false,
            "msg": "保存会话失败",
            "error": "SESSION_SAVE_FAILED"
        })));
    }

    tracing::info!("🧪 测试模式模拟登录成功: user_id={}", user_id);

    Ok(Json(serde_json::json!({
        "success": true,
        "msg": "模拟登录成功",
        "data": {
            "userId": user_id,
            "userName": user_name,
            "loginTime": Utc::now().to_string()
        }
    })))
}

/// 获取测试模拟数据
pub async fn get_mock_test_data(
    Json(request): Json<serde_json::Value>
) -> Result<Json<serde_json::Value>, StatusCode> {
    tracing::info!("获取测试模拟数据请求: {:?}", request);
    
    let data_type = request.get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("preview");
        
    match data_type {
        "preview" => {
            // 返回预审测试数据
            Ok(Json(serde_json::json!({
                "success": true,
                "data": {
                    "userId": "test_user_001",
                    "preview": {
                        "matterId": "MATTER_001",
                        "matterName": "测试事项",
                        "matterType": "test",
                        "requestId": format!("TEST_{}", Utc::now().timestamp()),
                        "sequenceNo": format!("SEQ_{}", Utc::now().timestamp()),
                        "copy": false,
                        "channel": "web",
                        "formData": [],
                        "materialData": [],
                        "agentInfo": {
                            "userId": "test_user_001",
                            "userName": "测试用户"
                        },
                        "subjectInfo": {
                            "name": "测试主体",
                            "type": "individual"
                        }
                    }
                }
            })))
        }
        "qingqiu" => {
            // 返回qingqiu.json格式的测试数据
            Ok(Json(serde_json::json!({
                "success": true,
                "data": {
                    "requestId": format!("QINGQIU_{}", Utc::now().timestamp()),
                    "sequenceNo": format!("SEQ_{}", Utc::now().timestamp()),
                    "userId": "test_user_001",
                    "materials": []
                }
            })))
        }
        _ => {
            Ok(Json(serde_json::json!({
                "success": false,
                "error": "未知的数据类型"
            })))
        }
    }
}