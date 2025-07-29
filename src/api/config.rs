//! 配置和主题管理模块
//! 处理系统配置、主题管理、规则更新等功能

use crate::{CONFIG};
use crate::util::{IntoJson};
use axum::extract::{Multipart, Path};
use axum::response::IntoResponse;
use axum::{Json};

/// 更新规则配置
pub async fn update_rule(multipart: Multipart) -> impl IntoResponse {
    let result = crate::util::zen::update_rule(multipart).await;
    result.into_json()
}

/// 获取所有可用主题
pub async fn get_themes() -> impl IntoResponse {
    tracing::info!("获取所有可用主题");
    let themes = crate::util::zen::get_available_themes();
    tracing::info!("可用主题数量: {}", themes.len());
    for theme in &themes {
        tracing::info!("  - {}: {} ({})", theme.id, theme.name, theme.description);
    }

    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "themes": themes,
            "total": themes.len()
        }
    }))
}

/// 重新加载指定主题的规则
pub async fn reload_theme(Path(theme_id): Path<String>) -> impl IntoResponse {
    tracing::info!("重新加载主题规则: {}", theme_id);
    let result = crate::util::zen::reload_theme_rule(&theme_id).await;
    result.into_json()
}

/// 获取前端配置
pub async fn get_frontend_config() -> impl IntoResponse {
    tracing::info!("获取前端配置");

    // 获取主题配置
    let themes = crate::util::zen::get_available_themes();

    // 测试模式已移除，确保生产环境安全

    // 构建前端需要的配置数据
    let frontend_config = serde_json::json!({
        "themes": themes,
        "features": {
            "theme_selection": true,
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
            "mock_login": CONFIG.debug.enable_mock_login
        }
    });

    Json(serde_json::json!({
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
            "enable_mock_login": CONFIG.debug.enable_mock_login,
            "mock_login_warning": CONFIG.debug.mock_login_warning,
            "tools": {
                "api_test": CONFIG.debug.tools_enabled.api_test,
                "mock_login": CONFIG.debug.tools_enabled.mock_login,
                "preview_demo": CONFIG.debug.tools_enabled.preview_demo,
                "flow_test": CONFIG.debug.tools_enabled.flow_test,
                "system_monitor": CONFIG.debug.tools_enabled.system_monitor,
                "data_manager": CONFIG.debug.tools_enabled.data_manager
            }
        }
    }))
}

/// 根路由重定向到登录页面
pub async fn root_redirect() -> impl IntoResponse {
    tracing::info!("根路由访问，重定向到登录页面");
    axum::response::Redirect::to("/static/login.html")
}