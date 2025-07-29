//! 文件管理模块
//! 处理文件上传、下载、预审结果展示等功能

use crate::model::preview::PreviewBody;
use crate::model::Goto;
use crate::util::{IntoJson, ServerError};
use crate::AppState;
use chrono::Utc;
use axum::extract::{Multipart, Query, State, Path};
use axum::response::IntoResponse;
use axum::Json;
use axum::http::StatusCode;
use std::collections::HashMap;

/// 文件上传接口
pub async fn upload(multipart: Multipart) -> impl IntoResponse {
    let result = crate::model::ocr::upload(multipart).await;
    result.into_json()
}

/// 文件下载接口
pub async fn download(Query(goto): Query<Goto>) -> impl IntoResponse {
    let result = PreviewBody::download(goto).await;
    result.map_err(|err| ServerError::Custom(err.to_string()))
}

/// 第三方系统回调处理 (POST方式，用于预审完成通知)
pub async fn third_party_callback(Json(callback_data): Json<serde_json::Value>) -> impl IntoResponse {
    tracing::info!("=== 第三方系统回调接收 ===");
    tracing::info!("回调数据: {}", serde_json::to_string_pretty(&callback_data).unwrap_or_default());
    
    // 在模拟环境中，我们只需要记录回调，不需要实际处理
    if let Some(preview_id) = callback_data.get("previewId").and_then(|v| v.as_str()) {
        tracing::info!("✅ 模拟第三方系统收到预审完成通知: {}", preview_id);
        
        if let Some(status) = callback_data.get("status").and_then(|v| v.as_str()) {
            tracing::info!("预审状态: {}", status);
        }
        
        if let Some(third_party_id) = callback_data.get("thirdPartyRequestId").and_then(|v| v.as_str()) {
            tracing::info!("第三方请求ID: {}", third_party_id);
        }
    }
    
    tracing::info!("=== 第三方系统回调处理完成 ===");
    
    // 返回成功响应（模拟第三方系统接收成功）
    Json(serde_json::json!({
        "success": true,
        "message": "回调接收成功",
        "timestamp": Utc::now().to_rfc3339()
    }))
}

/// 获取预审结果详情（用于政务风格展示页面）
pub async fn get_preview_result(
    Path(preview_id): Path<String>,
    State(state): State<AppState>
) -> impl IntoResponse {
    tracing::info!("获取预审结果详情: {}", preview_id);

    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            // 解析评估结果（如果存在）
            let evaluation_data = if let Some(eval_result) = &preview.evaluation_result {
                serde_json::from_str::<serde_json::Value>(eval_result).unwrap_or_default()
            } else {
                serde_json::json!({})
            };

            // 构建政务风格的预审结果数据
            let result_data = serde_json::json!({
                "preview_id": preview_id,
                "applicant": evaluation_data.get("applicant").and_then(|v| v.as_str()).unwrap_or("申请人"),
                "applicant_name": evaluation_data.get("applicant").and_then(|v| v.as_str()).unwrap_or("申请人"),
                "matter_name": evaluation_data.get("matter_name").and_then(|v| v.as_str()).unwrap_or(&preview.file_name),
                "theme_name": crate::util::zen::get_theme_name(preview.theme_id.as_deref()).unwrap_or_else(|| "未知主题".to_string()),
                "status": preview.status,
                "created_at": preview.created_at,
                "materials": evaluation_data.get("materials").and_then(|v| v.as_array()).map(|materials| {
                    materials.iter().map(|material| {
                        let material_name = material.get("name").and_then(|v| v.as_str()).unwrap_or("未知材料");
                        let material_status = material.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
                        let image_path = crate::api::utils::get_material_image_path(material_name, material_status);

                        serde_json::json!({
                            "id": material.get("id").and_then(|v| v.as_u64()).unwrap_or(1),
                            "name": material_name,
                            "status": material_status,
                            "pages": material.get("pages").and_then(|v| v.as_u64()).unwrap_or(1),
                            "count": material.get("pages").and_then(|v| v.as_u64()).unwrap_or(1),
                            "image": image_path,
                            "preview_url": image_path,
                            "review_points": material.get("review_points").cloned().unwrap_or_default(),
                            "review_notes": material.get("review_notes").and_then(|v| v.as_str())
                        })
                    }).collect::<Vec<_>>()
                }).unwrap_or_else(|| {
                    // 如果没有材料数据，创建一个默认的材料项
                    vec![serde_json::json!({
                        "id": 1,
                        "name": preview.file_name,
                        "status": "pending",
                        "pages": 1,
                        "count": 1,
                        "image": crate::api::utils::get_material_image_path(&preview.file_name, "pending"),
                        "preview_url": crate::api::utils::get_material_image_path(&preview.file_name, "pending"),
                        "review_points": [],
                        "review_notes": null
                    })]
                })
            });

            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": result_data
            }))
        }
        Ok(None) => {
            Json(serde_json::json!({
                "success": false,
                "errorCode": 404,
                "errorMsg": "预审记录不存在",
                "data": null
            }))
        }
        Err(e) => {
            tracing::error!("获取预审结果失败: {}", e);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": "获取预审结果失败",
                "data": null
            }))
        }
    }
}

/// 下载预审报告
pub async fn download_preview_report(
    Path(preview_id): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    State(state): State<AppState>
) -> impl IntoResponse {
    tracing::info!("下载预审报告: {}, 格式: {:?}", preview_id, params.get("format"));

    let format = params.get("format").unwrap_or(&"pdf".to_string()).clone();

    match state.database.get_preview_record(&preview_id).await {
        Ok(Some(preview)) => {
            match format.as_str() {
                "pdf" => {
                    // PDF生成暂时不支持，返回HTML格式
                    tracing::warn!("PDF生成功能暂未实现，返回HTML格式");

                    // 从评估结果中提取材料名称，如果没有则使用文件名
                    let material_names = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("materials")
                                .and_then(|v| v.as_array())
                                .map(|materials| {
                                    materials.iter()
                                        .filter_map(|m| m.get("name").and_then(|v| v.as_str()))
                                        .map(|s| s.to_string())
                                        .collect()
                                })
                                .unwrap_or_else(|| vec![preview.file_name.clone()])
                        } else {
                            vec![preview.file_name.clone()]
                        }
                    } else {
                        vec![preview.file_name.clone()]
                    };

                    let matter_name = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("matter_name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| preview.file_name.clone())
                        } else {
                            preview.file_name.clone()
                        }
                    } else {
                        preview.file_name.clone()
                    };

                    let html_content = crate::util::report::PreviewReportGenerator::generate_simple_html(
                        &matter_name,
                        &preview_id,
                        &material_names
                    );

                    let headers = [
                        ("Content-Type", "text/html; charset=utf-8"),
                        ("Content-Disposition", &format!("attachment; filename=\"预审报告_{}.html\"", preview_id)),
                    ];
                    (headers, html_content).into_response()
                }
                "html" => {
                    // 生成简化HTML报告
                    let material_names = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("materials")
                                .and_then(|v| v.as_array())
                                .map(|materials| {
                                    materials.iter()
                                        .filter_map(|m| m.get("name").and_then(|v| v.as_str()))
                                        .map(|s| s.to_string())
                                        .collect()
                                })
                                .unwrap_or_else(|| vec![preview.file_name.clone()])
                        } else {
                            vec![preview.file_name.clone()]
                        }
                    } else {
                        vec![preview.file_name.clone()]
                    };

                    let matter_name = if let Some(eval_result) = &preview.evaluation_result {
                        if let Ok(eval_data) = serde_json::from_str::<serde_json::Value>(eval_result) {
                            eval_data.get("matter_name")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| preview.file_name.clone())
                        } else {
                            preview.file_name.clone()
                        }
                    } else {
                        preview.file_name.clone()
                    };

                    let html_content = crate::util::report::PreviewReportGenerator::generate_simple_html(
                        &matter_name,
                        &preview_id,
                        &material_names
                    );

                    let headers = [
                        ("Content-Type", "text/html; charset=utf-8"),
                        ("Content-Disposition", &format!("attachment; filename=\"预审报告_{}.html\"", preview_id)),
                    ];
                    (headers, html_content).into_response()
                }
                _ => {
                    (StatusCode::BAD_REQUEST, "不支持的格式").into_response()
                }
            }
        }
        Ok(None) => {
            (StatusCode::NOT_FOUND, "预审记录不存在").into_response()
        }
        Err(e) => {
            tracing::error!("获取预审记录失败: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, "获取预审记录失败").into_response()
        }
    }
}