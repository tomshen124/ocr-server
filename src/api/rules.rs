use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use serde::Serialize;
use tracing::error;

use crate::{util::rules::RuleRepository, AppState};

#[derive(Debug, Serialize)]
struct MatterRuleSummary {
    matter_id: String,
    matter_name: Option<String>,
    spec_version: String,
    mode: String,
    status: String,
    description: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
struct MatterRuleDetail {
    matter_id: String,
    matter_name: Option<String>,
    spec_version: String,
    mode: String,
    status: String,
    description: Option<String>,
    updated_at: String,
    definition: serde_json::Value,
}

pub async fn list_matter_rules(State(state): State<AppState>) -> impl IntoResponse {
    let repo = RuleRepository::new(state.database.clone());
    match repo.list(None).await {
        Ok(configs) => {
            let summaries: Vec<MatterRuleSummary> = configs
                .into_iter()
                .map(|config| MatterRuleSummary {
                    matter_id: config.record.matter_id.clone(),
                    matter_name: config.record.matter_name.clone(),
                    spec_version: config.record.spec_version.clone(),
                    mode: config.mode.as_str().to_string(),
                    status: config.record.status.clone(),
                    description: config.record.description.clone(),
                    updated_at: config.record.updated_at.to_rfc3339(),
                })
                .collect();
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": summaries
            }))
        }
        Err(err) => {
            error!("查询事项规则配置失败: {}", err);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("无法获取事项规则配置: {}", err),
                "data": serde_json::Value::Null
            }))
        }
    }
}

pub async fn get_matter_rule(
    State(state): State<AppState>,
    Path(matter_id): Path<String>,
) -> impl IntoResponse {
    let repo = RuleRepository::new(state.database.clone());
    match repo.fetch(&matter_id).await {
        Ok(Some(config)) => {
            let definition = serde_json::to_value(&config.definition).unwrap_or_default();
            Json(serde_json::json!({
                "success": true,
                "errorCode": 200,
                "errorMsg": "",
                "data": MatterRuleDetail {
                    matter_id: config.record.matter_id.clone(),
                    matter_name: config.record.matter_name.clone(),
                    spec_version: config.record.spec_version.clone(),
                    mode: config.mode.as_str().to_string(),
                    status: config.record.status.clone(),
                    description: config.record.description.clone(),
                    updated_at: config.record.updated_at.to_rfc3339(),
                    definition,
                }
            }))
        }
        Ok(None) => Json(serde_json::json!({
            "success": false,
            "errorCode": 404,
            "errorMsg": format!("未找到事项 {} 的规则配置", matter_id),
            "data": serde_json::Value::Null
        })),
        Err(err) => {
            error!("获取事项规则配置失败 (matter_id={}): {}", matter_id, err);
            Json(serde_json::json!({
                "success": false,
                "errorCode": 500,
                "errorMsg": format!("无法获取事项规则配置: {}", err),
                "data": serde_json::Value::Null
            }))
        }
    }
}

pub async fn reload_matter_rule(Path(matter_id): Path<String>) -> impl IntoResponse {
    // 目前规则配置直接从数据库读取，不需要缓存刷新
    Json(serde_json::json!({
        "success": true,
        "errorCode": 200,
        "errorMsg": "",
        "data": {
            "matterId": matter_id,
            "message": "规则配置为实时读取，无需刷新缓存"
        }
    }))
}
