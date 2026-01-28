//! 配置管理模块
//! 提供前端配置、调试配置等接口

use axum::{extract::State, response::IntoResponse, Json};
use serde_json::json;
use tracing::error;

use crate::{util::rules::RuleRepository, AppState, CONFIG};

/// 获取前端配置
pub async fn get_frontend_config(State(state): State<AppState>) -> impl IntoResponse {
    tracing::info!("获取前端配置");

    let repo = RuleRepository::new(state.database.clone());
    let rule_summaries = match repo.list(Some("active")).await {
        Ok(configs) => configs
            .into_iter()
            .map(|config| {
                json!({
                    "matterId": config.record.matter_id,
                    "matterName": config.record.matter_name,
                    "specVersion": config.record.spec_version,
                    "mode": config.mode.as_str(),
                    "updatedAt": config.record.updated_at.to_rfc3339(),
                })
            })
            .collect::<Vec<_>>(),
        Err(err) => {
            error!("获取规则配置失败: {}", err);
            Vec::new()
        }
    };

    // 测试模式已移除，确保生产环境安全

    // 构建前端需要的配置数据
    let frontend_config = json!({
        "rules": rule_summaries,
        "features": {
            "theme_selection": false,
            "ocr_preview": true,
            "pdf_download": true,
            "elder_mode": true
        },
        "ui": {
            "title": "材料智能预审",
            "upload_hint": "支持PDF、JPG、PNG、BMP等格式",
            "max_file_size": "10MB"
        },
        "api": {
            "base_url": "/api",
            "timeout": 30000
        },
        "test_mode": if let Some(test_config) = &CONFIG.test_mode {
            serde_json::json!({
                "enabled": test_config.enabled,
                "auto_login": test_config.auto_login,
                "mock_ocr": test_config.mock_ocr,
                "test_user": {
                    "id": test_config.test_user.id,
                    "username": test_config.test_user.username,
                    "email": test_config.test_user.email,
                    "role": test_config.test_user.role
                }
            })
        } else {
            serde_json::json!({
                "enabled": false,
                "auto_login": false,
                "mock_ocr": false,
                "test_user": null
            })
        },
        "debug": {
            "enabled": CONFIG.debug.enabled,
            "development_mode": CONFIG.runtime_mode.mode == "development"
        }
    });

    Json(json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": frontend_config
    }))
}

/// 获取Debug配置
pub async fn get_debug_config() -> impl IntoResponse {
    tracing::info!("获取Debug配置");

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "Debug配置获取成功",
        "data": {
            "enabled": CONFIG.debug.enabled,
            "development_mode": CONFIG.runtime_mode.mode == "development",
            "tools": {
                "api_test": CONFIG.debug.tools_enabled.api_test,
                // 移除mock_login工具
                "preview_demo": CONFIG.debug.tools_enabled.preview_demo,
                "flow_test": CONFIG.debug.tools_enabled.flow_test,
                "system_monitor": CONFIG.debug.tools_enabled.system_monitor,
                "data_manager": CONFIG.debug.tools_enabled.data_manager
            }
        }
    }))
}

/// 根路由重定向到主页面
pub async fn root_redirect() -> impl IntoResponse {
    tracing::info!("根路由访问，重定向到主页面");
    axum::response::Redirect::to("/static/index.html")
}
