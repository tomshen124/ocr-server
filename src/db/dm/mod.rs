//! 达梦数据库模块
//! 通过Go网关连接达梦数据库（HTTP API，X-API-Key认证）

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::str::FromStr;
use uuid::Uuid;

use super::factory::DmConfig;
use super::traits::*;

use crate::api::monitor_auth::DEFAULT_MONITOR_ADMIN_PASSWORD;
#[cfg(feature = "dm_go")]
use crate::db::models::{MonitorSession, MonitorUser};
#[cfg(feature = "dm_go")]
use crate::util::config::types::GoGatewayConfig;

/// 达梦数据库连接类型（Go网关）
pub enum DmConnectionType {
    /// Go网关连接（HTTP + X-API-Key）
    #[cfg(feature = "dm_go")]
    Go(DmGoConnection),
}

/// 达梦数据库实现
pub struct DmDatabase {
    connection: DmConnectionType,
}

impl DmDatabase {
    /// 创建新的达梦数据库实例 - 使用Go网关
    pub async fn new(config: &super::factory::DmConfig) -> Result<Self> {
        let connection = Self::create_connection(config).await?;
        Ok(Self { connection })
    }

    /// 创建Go网关连接
    async fn create_connection(config: &super::factory::DmConfig) -> Result<DmConnectionType> {
        #[cfg(feature = "dm_go")]
        {
            tracing::info!(" 连接达梦数据库Go网关...");
            let go_conn = DmGoConnection::new(config).await?;
            tracing::info!("[ok] 达梦数据库Go网关连接成功");
            return Ok(DmConnectionType::Go(go_conn));
        }

        #[cfg(not(feature = "dm_go"))]
        {
            anyhow::bail!(
                "dm_go特性未启用，无法连接达梦数据库。\n\
                请使用: cargo build --features dm_go"
            );
        }
    }
}

// ================= Go网关连接实现 =================

#[derive(Clone)]
struct DmGoConnection {
    base_url: String,
    api_key: String,
    client: Client,
}

impl DmGoConnection {
    fn summarize_sql(sql: &str) -> String {
        let normalized = sql.split_whitespace().collect::<Vec<_>>().join(" ");
        if normalized.len() > 160 {
            let mut truncated = normalized[..160].to_string();
            truncated.push('…');
            truncated
        } else {
            normalized
        }
    }

    fn extract_timeout(options: Option<&DbOptions>) -> Option<u64> {
        options.and_then(|o| o.timeout_ms)
    }

    fn extract_limit(options: Option<&DbOptions>) -> Option<u64> {
        options.and_then(|o| o.limit)
    }
}

// ===== 下载重试持久化（通过 Go 网关） =====
/// 记录下载重试
#[cfg(feature = "dm_go")]
pub async fn record_download_retry(url: &str, reason: &str) -> Result<()> {
    let cfg = crate::CONFIG.database.as_ref().and_then(|db| {
        db.go_gateway
            .as_ref()
            .or_else(|| db.dm.as_ref().and_then(|dm| dm.go_gateway.as_ref()))
    });
    let Some(gw) = cfg else { return Ok(()) };
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(gw.timeout.max(1)))
        .build()?;
    ensure_retry_table(&client, gw).await.ok();
    let sql = "INSERT INTO DOWNLOAD_RETRY (ID, URL, REASON, STATUS, ATTEMPTS, LAST_ERROR_AT, CREATED_AT, UPDATED_AT) \
               VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)";
    let payload = serde_json::json!({
        "sql": sql,
        "params": [
            Uuid::new_v4().to_string(),
            url,
            reason,
            "pending",
            "1"
        ],
        "options": { "timeout_ms": 10_000u64 }
    });
    let _ = client
        .post(format!("{}/db", gw.url.trim_end_matches('/')))
        .header("X-API-Key", &gw.api_key)
        .json(&payload)
        .send()
        .await;
    Ok(())
}

/// 标记下载重试完成
#[cfg(feature = "dm_go")]
pub async fn mark_download_retry_done(url: &str) -> Result<()> {
    let cfg = crate::CONFIG.database.as_ref().and_then(|db| {
        db.go_gateway
            .as_ref()
            .or_else(|| db.dm.as_ref().and_then(|dm| dm.go_gateway.as_ref()))
    });
    let Some(gw) = cfg else { return Ok(()) };
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(gw.timeout.max(1)))
        .build()?;
    ensure_retry_table(&client, gw).await.ok();
    let sql = "UPDATE DOWNLOAD_RETRY SET STATUS='done', UPDATED_AT=CURRENT_TIMESTAMP WHERE URL = ?";
    let payload = serde_json::json!({
        "sql": sql,
        "params": [url],
        "options": { "timeout_ms": 5_000u64 }
    });
    let _ = client
        .post(format!("{}/db", gw.url.trim_end_matches('/')))
        .header("X-API-Key", &gw.api_key)
        .json(&payload)
        .send()
        .await;
    Ok(())
}

#[cfg(not(feature = "dm_go"))]
pub async fn record_download_retry(_url: &str, _reason: &str) -> Result<()> {
    Ok(())
}

#[cfg(not(feature = "dm_go"))]
pub async fn mark_download_retry_done(_url: &str) -> Result<()> {
    Ok(())
}

#[cfg(feature = "dm_go")]
async fn ensure_retry_table(client: &Client, gw: &GoGatewayConfig) -> Result<()> {
    static INIT: std::sync::Once = std::sync::Once::new();
    let mut res = Ok(());
    INIT.call_once(|| {
        let sql = "CREATE TABLE IF NOT EXISTS DOWNLOAD_RETRY (
            ID VARCHAR(64) PRIMARY KEY,
            URL VARCHAR(1024) NOT NULL,
            REASON VARCHAR(128),
            STATUS VARCHAR(16),
            ATTEMPTS INT,
            LAST_ERROR_AT TIMESTAMP,
            CREATED_AT TIMESTAMP,
            UPDATED_AT TIMESTAMP
        )";
        let payload = serde_json::json!({
            "sql": sql,
            "params": [],
            "options": { "timeout_ms": 10_000u64 }
        });
        res = futures::executor::block_on(async {
            client
                .post(format!("{}/db", gw.url.trim_end_matches('/')))
                .header("X-API-Key", &gw.api_key)
                .json(&payload)
                .send()
                .await
                .map(|_| ())
                .map_err(|e| anyhow::anyhow!(e))
        });
    });
    res
}

#[derive(Serialize)]
struct DbOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<u64>,
}

#[derive(Serialize)]
struct DbRequest {
    sql: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<DbOptions>,
}

#[derive(Deserialize)]
struct DbResponse {
    success: bool,
    #[serde(default)]
    message: String,
    #[serde(default)]
    count: Option<u64>,
    #[serde(default)]
    affected: Option<i64>,
    #[serde(default)]
    data: Option<Vec<HashMap<String, Value>>>,
    #[serde(default)]
    error_detail: Option<String>,
}

impl DmGoConnection {
    async fn new(config: &DmConfig) -> Result<Self> {
        let gw = config
            .go_gateway
            .as_ref()
            .ok_or_else(|| anyhow!("缺少达梦Go网关配置: dm.go_gateway"))?;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(gw.timeout.max(1)))
            .build()?;

        Ok(Self {
            base_url: gw.url.trim_end_matches('/').to_string(),
            api_key: gw.api_key.clone(),
            client,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        let resp = self
            .client
            .post(url)
            .header("X-API-Key", &self.api_key)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    target: "dm_go::health",
                    event = "dm.health_error",
                    gateway_url = %self.base_url,
                    error = %e
                );
                e
            })?;
        let val: Value = resp.json().await.unwrap_or(Value::Null);
        let healthy = val.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        if healthy {
            tracing::info!(
                target: "dm_go::health",
                event = "dm.health_ok",
                gateway_url = %self.base_url
            );
        } else {
            tracing::warn!(
                target: "dm_go::health",
                event = "dm.health_warning",
                gateway_url = %self.base_url,
                response = ?val
            );
        }
        Ok(healthy)
    }

    async fn db_call(
        &self,
        sql: &str,
        params: Option<Vec<Value>>,
        options: Option<DbOptions>,
    ) -> Result<DbResponse> {
        let url = format!("{}/db", self.base_url);
        let operation = sql
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_ascii_uppercase();
        let params_len = params.as_ref().map(|p| p.len()).unwrap_or(0);
        let sql_summary = DmGoConnection::summarize_sql(sql);
        let timeout_ms = DmGoConnection::extract_timeout(options.as_ref());
        let limit = DmGoConnection::extract_limit(options.as_ref());
        let started_at = std::time::Instant::now();
        let req = DbRequest {
            sql: sql.to_string(),
            params,
            options,
        };
        let resp = match self
            .client
            .post(url)
            .header("X-API-Key", &self.api_key)
            .json(&req)
            .send()
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                let elapsed_ms = started_at.elapsed().as_millis() as u128;
                tracing::error!(
                    target: "dm_go::db",
                    event = "sql.exec_error",
                    elapsed_ms = elapsed_ms,
                    operation = %operation,
                    sql_summary = %sql_summary,
                    param_count = params_len,
                    timeout_ms = timeout_ms.unwrap_or(0),
                    limit = limit.unwrap_or(0),
                    error = %e
                );
                return Err(anyhow!("Go网关请求失败: {}", e));
            }
        };
        let status = resp.status();
        let elapsed_ms = started_at.elapsed().as_millis() as u128;
        let body = match resp.text().await {
            Ok(body) => body,
            Err(e) => {
                tracing::error!(
                    target: "dm_go::db",
                    event = "sql.exec_error",
                    http_status = status.as_u16(),
                    elapsed_ms = elapsed_ms,
                    operation = %operation,
                    sql_summary = %sql_summary,
                    param_count = params_len,
                    timeout_ms = timeout_ms.unwrap_or(0),
                    limit = limit.unwrap_or(0),
                    error = %e
                );
                return Err(anyhow!("Go网关响应读取失败: {}", e));
            }
        };
        let parsed: DbResponse = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(e) => {
                tracing::error!(
                    target: "dm_go::db",
                    event = "sql.exec_error",
                    http_status = status.as_u16(),
                    elapsed_ms = elapsed_ms,
                    operation = %operation,
                    sql_summary = %sql_summary,
                    param_count = params_len,
                    timeout_ms = timeout_ms.unwrap_or(0),
                    limit = limit.unwrap_or(0),
                    response_body = %body,
                    error = %e
                );
                return Err(anyhow!("Go网关响应解析失败: {} | 原始: {}", e, body));
            }
        };
        if !status.is_success() || !parsed.success {
            tracing::error!(
                target: "dm_go::db",
                event = "sql.exec_error",
                http_status = status.as_u16(),
                elapsed_ms = elapsed_ms,
                operation = %operation,
                sql_summary = %sql_summary,
                param_count = params_len,
                timeout_ms = timeout_ms.unwrap_or(0),
                limit = limit.unwrap_or(0),
                message = %parsed.message,
                error_detail = %parsed.error_detail.as_deref().unwrap_or(""),
                response_body = %body
            );
            return Err(anyhow!(
                "Go网关执行失败: {} (HTTP {})",
                parsed
                    .error_detail
                    .clone()
                    .unwrap_or(parsed.message.clone()),
                status
            ));
        }
        let rows = parsed.data.as_ref().map(|d| d.len()).unwrap_or(0);
        tracing::info!(
            target: "dm_go::db",
            event = "sql.exec",
            http_status = status.as_u16(),
            elapsed_ms = elapsed_ms,
            operation = %operation,
            sql_summary = %sql_summary,
            param_count = params_len,
            timeout_ms = timeout_ms.unwrap_or(0),
            limit = limit.unwrap_or(0),
            affected = parsed.affected.unwrap_or(0),
            count = parsed.count.unwrap_or(0),
            rows = rows
        );
        Ok(parsed)
    }

    async fn query_rows(
        &self,
        sql: &str,
        str_params: Option<Vec<String>>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let params = str_params.map(|v| v.into_iter().map(|s| Value::String(s)).collect());
        let resp = self
            .db_call(
                sql,
                params,
                Some(DbOptions {
                    timeout_ms: Some(30_000),
                    limit: None,
                }),
            )
            .await?;
        Ok(resp.data.unwrap_or_default())
    }

    // 新增：参数化查询方法
    async fn query_with_params(
        &self,
        sql: &str,
        params: Vec<String>,
    ) -> Result<Vec<HashMap<String, Value>>> {
        let json_params: Vec<Value> = params.into_iter().map(|s| Value::String(s)).collect();
        let resp = self
            .db_call(
                sql,
                Some(json_params),
                Some(DbOptions {
                    timeout_ms: Some(30_000),
                    limit: None,
                }),
            )
            .await?;
        Ok(resp.data.unwrap_or_default())
    }

    async fn execute_update(&self, sql: &str, str_params: Option<Vec<String>>) -> Result<u64> {
        let params = str_params.map(|v| v.into_iter().map(|s| Value::String(s)).collect());
        let resp = self.db_call(sql, params, None).await?;
        Ok(resp.affected.unwrap_or(0) as u64)
    }

    async fn execute_update_values(&self, sql: &str, params: Vec<Value>) -> Result<u64> {
        let resp = self.db_call(sql, Some(params), None).await?;
        Ok(resp.affected.unwrap_or(0) as u64)
    }

    // 新增：执行参数化查询（支持多种类型参数）
    async fn execute_with_params(&self, sql: &str, params: Vec<String>) -> Result<u64> {
        let json_params: Vec<Value> = params.into_iter().map(|s| Value::String(s)).collect();
        let resp = self.db_call(sql, Some(json_params), None).await?;
        Ok(resp.affected.unwrap_or(0) as u64)
    }

    async fn table_exists(&self, table: &str) -> Result<bool> {
        let sql = "SELECT COUNT(*) AS COUNT FROM USER_TABLES WHERE TABLE_NAME = ?";
        let rows = self
            .query_rows(sql, Some(vec![table.to_uppercase()]))
            .await?;
        let count = rows
            .get(0)
            .and_then(|r| r.get("COUNT").or_else(|| r.get("count")))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(count > 0)
    }

    async fn column_exists(&self, table: &str, column: &str) -> Result<bool> {
        let sql = "SELECT COUNT(*) AS COUNT FROM USER_TAB_COLUMNS WHERE TABLE_NAME = ? AND COLUMN_NAME = ?";
        let rows = self
            .query_rows(sql, Some(vec![table.to_uppercase(), column.to_uppercase()]))
            .await?;
        let count = rows
            .get(0)
            .and_then(|r| r.get("COUNT").or_else(|| r.get("count")))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        Ok(count > 0)
    }

    async fn ensure_column(&self, table: &str, column: &str, definition: &str) -> Result<()> {
        if !self.column_exists(table, column).await? {
            let sql = format!("ALTER TABLE {} ADD ({})", table, definition);
            self.execute_update(&sql, None).await?;
        }
        Ok(())
    }

    fn default_monitor_admin_password_hash() -> String {
        #[cfg(feature = "monitoring")]
        {
            use bcrypt::{hash, DEFAULT_COST};
            hash(DEFAULT_MONITOR_ADMIN_PASSWORD, DEFAULT_COST)
                .unwrap_or_else(|_| DEFAULT_MONITOR_ADMIN_PASSWORD.to_string())
        }
        #[cfg(not(feature = "monitoring"))]
        {
            DEFAULT_MONITOR_ADMIN_PASSWORD.to_string()
        }
    }

    async fn ensure_default_monitor_admin(&self) -> Result<()> {
        let sql = "SELECT COUNT(*) AS COUNT FROM MONITOR_USERS WHERE ROLE = 'admin'";
        let rows = self.query_rows(sql, None).await?;
        let count = rows
            .get(0)
            .and_then(|row| row.get("COUNT").or_else(|| row.get("count")))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);

        if count == 0 {
            let admin_id = Uuid::new_v4().to_string();
            let password_hash = Self::default_monitor_admin_password_hash();
            let insert_sql = "INSERT INTO MONITOR_USERS \
                (ID, USERNAME, PASSWORD_HASH, ROLE, LOGIN_COUNT, CREATED_AT, UPDATED_AT, IS_ACTIVE) \
                VALUES (?, ?, ?, ?, 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, 1)";
            self.execute_update(
                insert_sql,
                Some(vec![
                    admin_id,
                    "admin".to_string(),
                    password_hash,
                    "admin".to_string(),
                ]),
            )
            .await?;

            tracing::info!(
                "[ok] 默认监控管理员账户已创建: admin/{}",
                DEFAULT_MONITOR_ADMIN_PASSWORD
            );
            tracing::warn!("[warn]  请在生产环境中修改默认密码！");
        }

        Ok(())
    }

    async fn initialize_schema(&self) -> Result<()> {
        // 预审请求表
        if !self.table_exists("PREVIEW_REQUESTS").await? {
            let create = r#"
                CREATE TABLE PREVIEW_REQUESTS (
                    ID VARCHAR(100) PRIMARY KEY,
                    THIRD_PARTY_REQUEST_ID VARCHAR(200),
                    USER_ID VARCHAR(100) NOT NULL,
                    USER_INFO_JSON CLOB,
                    MATTER_ID VARCHAR(100) NOT NULL,
                    MATTER_TYPE VARCHAR(100) NOT NULL,
                    MATTER_NAME VARCHAR(500) NOT NULL,
                    CHANNEL VARCHAR(100) NOT NULL,
                    SEQUENCE_NO VARCHAR(100) NOT NULL,
                    AGENT_INFO_JSON CLOB,
                    SUBJECT_INFO_JSON CLOB,
                    FORM_DATA_JSON CLOB,
                    SCENE_DATA_JSON CLOB,
                    MATERIAL_DATA_JSON CLOB,
                    LATEST_PREVIEW_ID VARCHAR(100),
                    LATEST_STATUS VARCHAR(50),
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_REQ_USER ON PREVIEW_REQUESTS(USER_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_REQ_MATTER ON PREVIEW_REQUESTS(MATTER_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_REQ_CREATED ON PREVIEW_REQUESTS(CREATED_AT)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE UNIQUE INDEX IDX_PREVIEW_REQ_TP ON PREVIEW_REQUESTS(THIRD_PARTY_REQUEST_ID)",
                    None,
                )
                .await
                .ok();
        }

        // 尝试补充新增列
        self.ensure_column(
            "PREVIEW_REQUESTS",
            "THIRD_PARTY_REQUEST_ID",
            "THIRD_PARTY_REQUEST_ID VARCHAR(200)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_REQUESTS",
            "LATEST_PREVIEW_ID",
            "LATEST_PREVIEW_ID VARCHAR(100)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_REQUESTS",
            "LATEST_STATUS",
            "LATEST_STATUS VARCHAR(50)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_REQUESTS",
            "AGENT_INFO_JSON",
            "AGENT_INFO_JSON CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_REQUESTS",
            "SUBJECT_INFO_JSON",
            "SUBJECT_INFO_JSON CLOB",
        )
        .await?;
        self.ensure_column("PREVIEW_REQUESTS", "FORM_DATA_JSON", "FORM_DATA_JSON CLOB")
            .await?;
        self.ensure_column(
            "PREVIEW_REQUESTS",
            "SCENE_DATA_JSON",
            "SCENE_DATA_JSON CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_REQUESTS",
            "MATERIAL_DATA_JSON",
            "MATERIAL_DATA_JSON CLOB",
        )
        .await?;
        self.ensure_column("PREVIEW_REQUESTS", "USER_INFO_JSON", "USER_INFO_JSON CLOB")
            .await?;

        // 预审记录表
        if !self.table_exists("PREVIEW_RECORDS").await? {
            let create = r#"
                CREATE TABLE PREVIEW_RECORDS (
                    ID VARCHAR(100) PRIMARY KEY,
                    USER_ID VARCHAR(100) NOT NULL,
                    USER_INFO_JSON CLOB,
                    FILE_NAME VARCHAR(500) NOT NULL,
                    OCR_TEXT CLOB,
                    THEME_ID VARCHAR(100),
                    EVALUATION_RESULT CLOB,
                    PREVIEW_URL VARCHAR(1000),
                    PREVIEW_VIEW_URL VARCHAR(1000),
                    PREVIEW_DOWNLOAD_URL VARCHAR(1000),
                    STATUS VARCHAR(50) DEFAULT 'processing',
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    THIRD_PARTY_REQUEST_ID VARCHAR(200),
                    QUEUED_AT TIMESTAMP,
                    PROCESSING_STARTED_AT TIMESTAMP,
                    RETRY_COUNT INTEGER DEFAULT 0,
                    LAST_WORKER_ID VARCHAR(100),
                    LAST_ATTEMPT_ID VARCHAR(100),
                    FAILURE_REASON CLOB,
                    OCR_STDERR_SUMMARY CLOB,
                    FAILURE_CONTEXT CLOB,
                    LAST_ERROR_CODE VARCHAR(100),
                    SLOW_ATTACHMENT_INFO_JSON CLOB,
                    CALLBACK_URL VARCHAR(1000),
                    CALLBACK_STATUS VARCHAR(50),
                    CALLBACK_ATTEMPTS INTEGER DEFAULT 0,
                    CALLBACK_SUCCESSES INTEGER DEFAULT 0,
                    CALLBACK_FAILURES INTEGER DEFAULT 0,
                    LAST_CALLBACK_AT TIMESTAMP,
                    LAST_CALLBACK_STATUS_CODE INTEGER,
                    LAST_CALLBACK_RESPONSE CLOB,
                    LAST_CALLBACK_ERROR CLOB,
                    CALLBACK_PAYLOAD CLOB,
                    NEXT_CALLBACK_AFTER TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_USER_ID ON PREVIEW_RECORDS(USER_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_STATUS ON PREVIEW_RECORDS(STATUS)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_CREATED_AT ON PREVIEW_RECORDS(CREATED_AT)",
                    None,
                )
                .await
                .ok();
            let _ = self.execute_update("CREATE INDEX IDX_PREVIEW_THIRD_PARTY ON PREVIEW_RECORDS(THIRD_PARTY_REQUEST_ID)", None).await.ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_PROCESSING_AT ON PREVIEW_RECORDS(PROCESSING_STARTED_AT)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_STATUS_RETRY ON PREVIEW_RECORDS(STATUS, RETRY_COUNT)",
                    None,
                )
                .await
                .ok();
        }

        self.ensure_column("PREVIEW_RECORDS", "QUEUED_AT", "QUEUED_AT TIMESTAMP")
            .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "PROCESSING_STARTED_AT",
            "PROCESSING_STARTED_AT TIMESTAMP",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "RETRY_COUNT",
            "RETRY_COUNT INTEGER DEFAULT 0",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "LAST_WORKER_ID",
            "LAST_WORKER_ID VARCHAR(100)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "LAST_ATTEMPT_ID",
            "LAST_ATTEMPT_ID VARCHAR(100)",
        )
        .await?;
        self.ensure_column("PREVIEW_RECORDS", "USER_INFO_JSON", "USER_INFO_JSON CLOB")
            .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "PREVIEW_VIEW_URL",
            "PREVIEW_VIEW_URL VARCHAR(1000)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "PREVIEW_DOWNLOAD_URL",
            "PREVIEW_DOWNLOAD_URL VARCHAR(1000)",
        )
        .await?;
        self.ensure_column("PREVIEW_RECORDS", "FAILURE_REASON", "FAILURE_REASON CLOB")
            .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "OCR_STDERR_SUMMARY",
            "OCR_STDERR_SUMMARY CLOB",
        )
        .await?;
        self.ensure_column("PREVIEW_RECORDS", "FAILURE_CONTEXT", "FAILURE_CONTEXT CLOB")
            .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "LAST_ERROR_CODE",
            "LAST_ERROR_CODE VARCHAR(100)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "SLOW_ATTACHMENT_INFO_JSON",
            "SLOW_ATTACHMENT_INFO_JSON CLOB",
        )
        .await?;

        // 下载重试表
        if !self.table_exists("DOWNLOAD_RETRY").await? {
            let create = r#"
                CREATE TABLE DOWNLOAD_RETRY (
                    ID VARCHAR(64) PRIMARY KEY,
                    URL VARCHAR(1024) NOT NULL,
                    REASON VARCHAR(128),
                    STATUS VARCHAR(16),
                    ATTEMPTS INT,
                    LAST_ERROR_AT TIMESTAMP,
                    CREATED_AT TIMESTAMP,
                    UPDATED_AT TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            tracing::info!("[init] Created DOWNLOAD_RETRY table");
        }

        // Worker结果异步处理队列表
        if !self.table_exists("WORKER_RESULTS_QUEUE").await? {
            let create = r#"
                CREATE TABLE WORKER_RESULTS_QUEUE (
                    ID VARCHAR(100) PRIMARY KEY,
                    PREVIEW_ID VARCHAR(100) NOT NULL,
                    PAYLOAD CLOB NOT NULL,
                    STATUS VARCHAR(50) DEFAULT 'pending',
                    ATTEMPTS INTEGER DEFAULT 0,
                    LAST_ERROR CLOB,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_WRQ_STATUS ON WORKER_RESULTS_QUEUE(STATUS, CREATED_AT)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_WRQ_PREVIEW ON WORKER_RESULTS_QUEUE(PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
            // 保证 PREVIEW_ID 唯一，避免重复入队
            let _ = self
                .execute_update(
                    "CREATE UNIQUE INDEX IDX_WRQ_PREVIEW_UNIQ ON WORKER_RESULTS_QUEUE(PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
        }

        self.ensure_column(
            "PREVIEW_RECORDS",
            "CALLBACK_URL",
            "CALLBACK_URL VARCHAR(1000)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "CALLBACK_STATUS",
            "CALLBACK_STATUS VARCHAR(50)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "CALLBACK_ATTEMPTS",
            "CALLBACK_ATTEMPTS INTEGER DEFAULT 0",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "CALLBACK_SUCCESSES",
            "CALLBACK_SUCCESSES INTEGER DEFAULT 0",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "CALLBACK_FAILURES",
            "CALLBACK_FAILURES INTEGER DEFAULT 0",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "LAST_CALLBACK_AT",
            "LAST_CALLBACK_AT TIMESTAMP",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "LAST_CALLBACK_STATUS_CODE",
            "LAST_CALLBACK_STATUS_CODE INTEGER",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "LAST_CALLBACK_RESPONSE",
            "LAST_CALLBACK_RESPONSE CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "LAST_CALLBACK_ERROR",
            "LAST_CALLBACK_ERROR CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "CALLBACK_PAYLOAD",
            "CALLBACK_PAYLOAD CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RECORDS",
            "NEXT_CALLBACK_AFTER",
            "NEXT_CALLBACK_AFTER TIMESTAMP",
        )
        .await?;

        if !self.table_exists("PREVIEW_MATERIAL_RESULTS").await? {
            let create = r#"
                CREATE TABLE PREVIEW_MATERIAL_RESULTS (
                    ID VARCHAR(100) PRIMARY KEY,
                    PREVIEW_ID VARCHAR(100) NOT NULL,
                    MATERIAL_CODE VARCHAR(200) NOT NULL,
                    MATERIAL_NAME VARCHAR(500),
                    STATUS VARCHAR(50) NOT NULL,
                    STATUS_CODE INTEGER NOT NULL,
                    PROCESSING_STATUS VARCHAR(50),
                    ISSUES_COUNT INTEGER DEFAULT 0,
                    WARNINGS_COUNT INTEGER DEFAULT 0,
                    ATTACHMENTS_JSON CLOB,
                    SUMMARY_JSON CLOB,
                    SCHEMA_VERSION INTEGER DEFAULT 1,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATERIAL_RESULTS_PREVIEW ON PREVIEW_MATERIAL_RESULTS(PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATERIAL_RESULTS_CODE ON PREVIEW_MATERIAL_RESULTS(PREVIEW_ID, MATERIAL_CODE)",
                    None,
                )
                .await
                .ok();
        }

        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "MATERIAL_NAME",
            "MATERIAL_NAME VARCHAR(500)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "PROCESSING_STATUS",
            "PROCESSING_STATUS VARCHAR(50)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "ISSUES_COUNT",
            "ISSUES_COUNT INTEGER DEFAULT 0",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "WARNINGS_COUNT",
            "WARNINGS_COUNT INTEGER DEFAULT 0",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "ATTACHMENTS_JSON",
            "ATTACHMENTS_JSON CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "SUMMARY_JSON",
            "SUMMARY_JSON CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "SCHEMA_VERSION",
            "SCHEMA_VERSION INTEGER DEFAULT 1",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_MATERIAL_RESULTS",
            "UPDATED_AT",
            "UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP",
        )
        .await?;

        if !self.table_exists("PREVIEW_RULE_RESULTS").await? {
            let create = r#"
                CREATE TABLE PREVIEW_RULE_RESULTS (
                    ID VARCHAR(100) PRIMARY KEY,
                    PREVIEW_ID VARCHAR(100) NOT NULL,
                    MATERIAL_RESULT_ID VARCHAR(100),
                    MATERIAL_CODE VARCHAR(200),
                    RULE_ID VARCHAR(200),
                    RULE_CODE VARCHAR(200),
                    RULE_NAME VARCHAR(500),
                    ENGINE VARCHAR(100),
                    SEVERITY VARCHAR(50),
                    STATUS VARCHAR(50),
                    MESSAGE CLOB,
                    SUGGESTIONS_JSON CLOB,
                    EVIDENCE_JSON CLOB,
                    EXTRA_JSON CLOB,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_RULE_RESULTS_PREVIEW ON PREVIEW_RULE_RESULTS(PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_RULE_RESULTS_MATERIAL ON PREVIEW_RULE_RESULTS(MATERIAL_RESULT_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_RULE_RESULTS_CODE ON PREVIEW_RULE_RESULTS(RULE_CODE)",
                    None,
                )
                .await
                .ok();
        }

        self.ensure_column(
            "PREVIEW_RULE_RESULTS",
            "MATERIAL_CODE",
            "MATERIAL_CODE VARCHAR(200)",
        )
        .await?;
        self.ensure_column("PREVIEW_RULE_RESULTS", "RULE_ID", "RULE_ID VARCHAR(200)")
            .await?;
        self.ensure_column(
            "PREVIEW_RULE_RESULTS",
            "RULE_CODE",
            "RULE_CODE VARCHAR(200)",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RULE_RESULTS",
            "RULE_NAME",
            "RULE_NAME VARCHAR(500)",
        )
        .await?;
        self.ensure_column("PREVIEW_RULE_RESULTS", "ENGINE", "ENGINE VARCHAR(100)")
            .await?;
        self.ensure_column("PREVIEW_RULE_RESULTS", "SEVERITY", "SEVERITY VARCHAR(50)")
            .await?;
        self.ensure_column("PREVIEW_RULE_RESULTS", "STATUS", "STATUS VARCHAR(50)")
            .await?;
        self.ensure_column("PREVIEW_RULE_RESULTS", "MESSAGE", "MESSAGE CLOB")
            .await?;
        self.ensure_column(
            "PREVIEW_RULE_RESULTS",
            "SUGGESTIONS_JSON",
            "SUGGESTIONS_JSON CLOB",
        )
        .await?;
        self.ensure_column(
            "PREVIEW_RULE_RESULTS",
            "EVIDENCE_JSON",
            "EVIDENCE_JSON CLOB",
        )
        .await?;
        self.ensure_column("PREVIEW_RULE_RESULTS", "EXTRA_JSON", "EXTRA_JSON CLOB")
            .await?;
        self.ensure_column(
            "PREVIEW_RULE_RESULTS",
            "UPDATED_AT",
            "UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP",
        )
        .await?;

        if !self.table_exists("PREVIEW_TASK_PAYLOADS").await? {
            let create = r#"
                CREATE TABLE PREVIEW_TASK_PAYLOADS (
                    PREVIEW_ID VARCHAR(100) PRIMARY KEY,
                    PAYLOAD CLOB NOT NULL,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_TASK_PAYLOADS_UPDATED_AT ON PREVIEW_TASK_PAYLOADS(UPDATED_AT)",
                    None,
                )
                .await
                .ok();
        }

        let _ = self
            .execute_update(
                "ALTER TABLE PREVIEW_TASK_PAYLOADS ADD (UPDATED_AT TIMESTAMP)",
                None,
            )
            .await;

        if !self.table_exists("MATTER_RULE_CONFIGS").await? {
            let create = r#"
                CREATE TABLE MATTER_RULE_CONFIGS (
                    ID VARCHAR(100) PRIMARY KEY,
                    MATTER_ID VARCHAR(100) NOT NULL,
                    MATTER_NAME VARCHAR(500),
                    SPEC_VERSION VARCHAR(20) NOT NULL,
                    MODE VARCHAR(50) DEFAULT 'presentOnly',
                    RULE_PAYLOAD CLOB NOT NULL,
                    STATUS VARCHAR(20) DEFAULT 'active',
                    DESCRIPTION CLOB,
                    CHECKSUM VARCHAR(128),
                    UPDATED_BY VARCHAR(100),
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE UNIQUE INDEX IDX_MATTER_RULE_CFG_MATTER ON MATTER_RULE_CONFIGS(MATTER_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATTER_RULE_CFG_STATUS ON MATTER_RULE_CONFIGS(STATUS)",
                    None,
                )
                .await
                .ok();
        }

        // API统计表
        if !self.table_exists("API_STATS").await? {
            let create = r#"
                CREATE TABLE API_STATS (
                    ID VARCHAR(100) PRIMARY KEY,
                    ENDPOINT VARCHAR(200) NOT NULL,
                    METHOD VARCHAR(10) NOT NULL,
                    CLIENT_ID VARCHAR(100),
                    USER_ID VARCHAR(100),
                    STATUS_CODE INTEGER NOT NULL,
                    RESPONSE_TIME_MS INTEGER NOT NULL,
                    REQUEST_SIZE INTEGER DEFAULT 0,
                    RESPONSE_SIZE INTEGER DEFAULT 0,
                    ERROR_MESSAGE VARCHAR(1000),
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_API_STATS_ENDPOINT ON API_STATS(ENDPOINT)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_API_STATS_USER_ID ON API_STATS(USER_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_API_STATS_CREATED_AT ON API_STATS(CREATED_AT)",
                    None,
                )
                .await
                .ok();
        }

        // 监控用户表
        if !self.table_exists("MONITOR_USERS").await? {
            let create = r#"
                CREATE TABLE MONITOR_USERS (
                    ID VARCHAR(64) PRIMARY KEY,
                    USERNAME VARCHAR(100) UNIQUE NOT NULL,
                    PASSWORD_HASH VARCHAR(200) NOT NULL,
                    ROLE VARCHAR(50) DEFAULT 'readonly' NOT NULL,
                    LAST_LOGIN_AT TIMESTAMP,
                    LOGIN_COUNT INTEGER DEFAULT 0,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    IS_ACTIVE INTEGER DEFAULT 1
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MONITOR_USERS_USERNAME ON MONITOR_USERS(USERNAME)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MONITOR_USERS_ROLE ON MONITOR_USERS(ROLE)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MONITOR_USERS_CREATED_AT ON MONITOR_USERS(CREATED_AT)",
                    None,
                )
                .await
                .ok();
        }

        self.ensure_default_monitor_admin().await?;

        // 监控会话表
        if !self.table_exists("MONITOR_SESSIONS").await? {
            let create = r#"
                CREATE TABLE MONITOR_SESSIONS (
                    ID VARCHAR(64) PRIMARY KEY,
                    USER_ID VARCHAR(64) NOT NULL,
                    IP_ADDRESS VARCHAR(64),
                    USER_AGENT VARCHAR(500),
                    CREATED_AT TIMESTAMP NOT NULL,
                    EXPIRES_AT TIMESTAMP NOT NULL,
                    LAST_ACTIVITY TIMESTAMP NOT NULL,
                    IS_ACTIVE INTEGER DEFAULT 1
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MONITOR_SESS_USER_ID ON MONITOR_SESSIONS(USER_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MONITOR_SESS_EXPIRES ON MONITOR_SESSIONS(EXPIRES_AT)",
                    None,
                )
                .await
                .ok();
        }

        // 用户登录记录表
        if !self.table_exists("USER_LOGIN_RECORDS").await? {
            let create = r#"
                CREATE TABLE USER_LOGIN_RECORDS (
                    ID NUMBER(20) PRIMARY KEY,
                    USER_ID VARCHAR(100) NOT NULL,
                    USER_NAME VARCHAR(100),
                    CERTIFICATE_TYPE VARCHAR(50),
                    CERTIFICATE_NUMBER VARCHAR(100),
                    PHONE_NUMBER VARCHAR(50),
                    EMAIL VARCHAR(100),
                    ORGANIZATION_NAME VARCHAR(200),
                    ORGANIZATION_CODE VARCHAR(100),
                    LOGIN_TYPE VARCHAR(50) NOT NULL,
                    LOGIN_TIME TIMESTAMP NOT NULL,
                    CLIENT_IP VARCHAR(64) NOT NULL,
                    USER_AGENT VARCHAR(500) NOT NULL,
                    REFERER VARCHAR(1000),
                    COOKIE_INFO CLOB,
                    RAW_DATA CLOB,
                    CREATED_AT TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_USER_LOGIN_USER_ID ON USER_LOGIN_RECORDS(USER_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_USER_LOGIN_TYPE ON USER_LOGIN_RECORDS(LOGIN_TYPE)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_USER_LOGIN_CREATED_AT ON USER_LOGIN_RECORDS(CREATED_AT)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_USER_LOGIN_CLIENT_IP ON USER_LOGIN_RECORDS(CLIENT_IP)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE SEQUENCE USER_LOGIN_RECORDS_SEQ START WITH 1 INCREMENT BY 1 CACHE 50",
                    None,
                )
                .await;
        }

        // 回灌Outbox事件表
        if !self.table_exists("DB_OUTBOX").await? {
            let create = r#"
                CREATE TABLE DB_OUTBOX (
                    ID NUMBER(20) PRIMARY KEY,
                    TABLE_NAME VARCHAR(64) NOT NULL,
                    OP_TYPE VARCHAR(20) NOT NULL,
                    PK_VALUE VARCHAR(200) NOT NULL,
                    IDEMPOTENCY_KEY VARCHAR(200) UNIQUE NOT NULL,
                    PAYLOAD CLOB NOT NULL,
                    CREATED_AT TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    APPLIED_AT TIMESTAMP,
                    RETRIES INTEGER DEFAULT 0,
                    LAST_ERROR VARCHAR(1000)
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_DB_OUTBOX_TABLE_NAME ON DB_OUTBOX(TABLE_NAME)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_DB_OUTBOX_CREATED_AT ON DB_OUTBOX(CREATED_AT)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_DB_OUTBOX_APPLIED_AT ON DB_OUTBOX(APPLIED_AT)",
                    None,
                )
                .await
                .ok();
        }
        let _ = self
            .execute_update(
                "CREATE SEQUENCE DB_OUTBOX_SEQ START WITH 1 INCREMENT BY 1 CACHE 50",
                None,
            )
            .await;

        // 材料文件记录表
        if !self.table_exists("PREVIEW_MATERIAL_FILES").await? {
            let create = r#"
                CREATE TABLE PREVIEW_MATERIAL_FILES (
                    ID VARCHAR(100) PRIMARY KEY,
                    PREVIEW_ID VARCHAR(100) NOT NULL,
                    MATERIAL_CODE VARCHAR(100) NOT NULL,
                    ATTACHMENT_NAME VARCHAR(500),
                    SOURCE_URL VARCHAR(1000),
                    STORED_ORIGINAL_KEY VARCHAR(1000) NOT NULL,
                    STORED_PROCESSED_KEYS CLOB,
                    MIME_TYPE VARCHAR(100),
                    SIZE_BYTES NUMBER(20),
                    CHECKSUM_SHA256 VARCHAR(100),
                    OCR_TEXT_KEY VARCHAR(1000),
                    OCR_TEXT_LENGTH NUMBER(20),
                    STATUS VARCHAR(50) DEFAULT 'pending',
                    ERROR_MESSAGE VARCHAR(2000),
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATERIAL_PREVIEW_ID ON PREVIEW_MATERIAL_FILES(PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATERIAL_CODE ON PREVIEW_MATERIAL_FILES(MATERIAL_CODE)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATERIAL_STATUS ON PREVIEW_MATERIAL_FILES(STATUS)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATERIAL_CHECKSUM ON PREVIEW_MATERIAL_FILES(CHECKSUM_SHA256)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MATERIAL_CREATED_AT ON PREVIEW_MATERIAL_FILES(CREATED_AT)",
                    None,
                )
                .await
                .ok();
            tracing::info!("[ok] 创建材料文件记录表 PREVIEW_MATERIAL_FILES 成功");
        }

        if !self.table_exists("CACHED_MATERIALS").await? {
            let create = r#"
                CREATE TABLE CACHED_MATERIALS (
                    ID VARCHAR(100) PRIMARY KEY,
                    PREVIEW_ID VARCHAR(100) NOT NULL,
                    MATERIAL_CODE VARCHAR(100) NOT NULL,
                    ATTACHMENT_INDEX NUMBER(10) NOT NULL,
                    TOKEN VARCHAR(200) NOT NULL,
                    LOCAL_PATH VARCHAR(1000) NOT NULL,
                    UPLOAD_STATUS VARCHAR(20) NOT NULL,
                    OSS_KEY VARCHAR(1000),
                    LAST_ERROR CLOB,
                    FILE_SIZE NUMBER(20),
                    CHECKSUM_SHA256 VARCHAR(128),
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_CACHED_MATERIALS_PREVIEW ON CACHED_MATERIALS(PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_CACHED_MATERIALS_STATUS ON CACHED_MATERIALS(UPLOAD_STATUS, PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
        }

        // 材料下载队列表
        if !self.table_exists("MATERIAL_DOWNLOAD_QUEUE").await? {
            let create = r#"
                CREATE TABLE MATERIAL_DOWNLOAD_QUEUE (
                    ID VARCHAR(50) PRIMARY KEY,
                    PREVIEW_ID VARCHAR(50) NOT NULL,
                    PAYLOAD CLOB,
                    STATUS VARCHAR(20) DEFAULT 'pending',
                    ATTEMPTS INTEGER DEFAULT 0,
                    LAST_ERROR CLOB,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MDQ_STATUS ON MATERIAL_DOWNLOAD_QUEUE(STATUS)",
                    None,
                )
                .await
                .ok();
        }

        // 跨请求下载去重缓存表：URL → TOKEN，带过期时间
        if !self.table_exists("MATERIAL_DOWNLOAD_CACHE").await? {
            let create = r#"
                CREATE TABLE MATERIAL_DOWNLOAD_CACHE (
                    URL VARCHAR(2000) PRIMARY KEY,
                    TOKEN VARCHAR(200) NOT NULL,
                    EXPIRES_AT TIMESTAMP NOT NULL,
                    UPDATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_MDC_EXPIRES_AT ON MATERIAL_DOWNLOAD_CACHE(EXPIRES_AT)",
                    None,
                )
                .await
                .ok();
        }

        // 外部分享一次性访问token表：TOKEN → PREVIEW_ID/FORMAT，带过期与使用标记
        if !self.table_exists("PREVIEW_SHARE_TOKENS").await? {
            let create = r#"
                CREATE TABLE PREVIEW_SHARE_TOKENS (
                    TOKEN VARCHAR(64) PRIMARY KEY,
                    PREVIEW_ID VARCHAR(50) NOT NULL,
                    FORMAT VARCHAR(10) DEFAULT 'pdf',
                    EXPIRES_AT TIMESTAMP NOT NULL,
                    USED_AT TIMESTAMP,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PST_PREVIEW_ID ON PREVIEW_SHARE_TOKENS(PREVIEW_ID)",
                    None,
                )
                .await
                .ok();
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PST_EXPIRES_AT ON PREVIEW_SHARE_TOKENS(EXPIRES_AT)",
                    None,
                )
                .await
                .ok();
        }

        // 预审去重指纹表
        if !self.table_exists("PREVIEW_DEDUP").await? {
            let create = r#"
                CREATE TABLE PREVIEW_DEDUP (
                    FINGERPRINT VARCHAR(512) PRIMARY KEY,
                    FIRST_PREVIEW_ID VARCHAR(50),
                    LAST_PREVIEW_ID VARCHAR(50),
                    USER_ID VARCHAR(200),
                    MATTER_ID VARCHAR(200),
                    THIRD_PARTY_REQUEST_ID VARCHAR(200),
                    PAYLOAD_HASH VARCHAR(128),
                    REPEAT_COUNT INTEGER DEFAULT 1,
                    LAST_SEEN_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
                    CREATED_AT TIMESTAMP DEFAULT CURRENT_TIMESTAMP
                )
            "#;
            let _ = self.execute_update(create, None).await?;
            let _ = self
                .execute_update(
                    "CREATE INDEX IDX_PREVIEW_DEDUP_LAST_SEEN ON PREVIEW_DEDUP(LAST_SEEN_AT)",
                    None,
                )
                .await
                .ok();
        }

        Ok(())
    }
}

// Helper mappers and parsers (module-level, not trait methods)
#[cfg(feature = "dm_go")]
fn str_ref_option_to_value(opt: Option<&str>) -> Value {
    match opt {
        Some(v) if !v.is_empty() => Value::String(v.to_string()),
        _ => Value::Null,
    }
}

#[cfg(feature = "dm_go")]
fn str_option_to_value(opt: &Option<String>) -> Value {
    match opt {
        Some(v) => Value::String(v.clone()),
        None => Value::Null,
    }
}

#[cfg(feature = "dm_go")]
fn status_option_to_value(status: Option<&str>) -> Value {
    match status {
        Some(s) => Value::String(s.to_string()),
        None => Value::Null,
    }
}

#[cfg(feature = "dm_go")]
fn i64_option_to_value(opt: Option<i64>) -> Value {
    match opt {
        Some(v) => Value::from(v),
        None => Value::Null,
    }
}

#[cfg(feature = "dm_go")]
fn parse_status_opt(value: Option<&serde_json::Value>) -> Option<PreviewStatus> {
    as_str(value).and_then(|s| match s.as_str() {
        "pending" => Some(PreviewStatus::Pending),
        "queued" => Some(PreviewStatus::Queued),
        "processing" => Some(PreviewStatus::Processing),
        "completed" => Some(PreviewStatus::Completed),
        "failed" => Some(PreviewStatus::Failed),
        _ => None,
    })
}

#[cfg(feature = "dm_go")]
fn map_preview_request_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<PreviewRequestRecord> {
    Ok(PreviewRequestRecord {
        id: as_str(row.get("ID")).unwrap_or_default(),
        third_party_request_id: opt_str(row.get("THIRD_PARTY_REQUEST_ID")),
        user_id: as_str(row.get("USER_ID")).unwrap_or_default(),
        user_info_json: opt_str(row.get("USER_INFO_JSON")),
        matter_id: as_str(row.get("MATTER_ID")).unwrap_or_default(),
        matter_type: as_str(row.get("MATTER_TYPE")).unwrap_or_default(),
        matter_name: as_str(row.get("MATTER_NAME")).unwrap_or_default(),
        channel: as_str(row.get("CHANNEL")).unwrap_or_default(),
        sequence_no: as_str(row.get("SEQUENCE_NO")).unwrap_or_default(),
        agent_info_json: opt_str(row.get("AGENT_INFO_JSON")),
        subject_info_json: opt_str(row.get("SUBJECT_INFO_JSON")),
        form_data_json: opt_str(row.get("FORM_DATA_JSON")),
        scene_data_json: opt_str(row.get("SCENE_DATA_JSON")),
        material_data_json: opt_str(row.get("MATERIAL_DATA_JSON")),
        latest_preview_id: opt_str(row.get("LATEST_PREVIEW_ID")),
        latest_status: parse_status_opt(row.get("LATEST_STATUS")),
        created_at: parse_dt(row.get("CREATED_AT")),
        updated_at: parse_dt(row.get("UPDATED_AT")),
    })
}

#[cfg(feature = "dm_go")]
fn map_preview_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<PreviewRecord> {
    let id = as_str(row.get("ID")).unwrap_or_default();
    let raw_evaluation_value = row.get("EVALUATION_RESULT");
    let evaluation_result = opt_str(raw_evaluation_value);

    if evaluation_result.is_none() {
        if let Some(raw) = raw_evaluation_value {
            if !raw.is_null() {
                if let Ok(serialized) = serde_json::to_string(raw) {
                    tracing::debug!(
                        target: "dm_go::clob",
                        preview_id = %id,
                        raw_value = %serialized,
                        "无法直接解析达梦CLOB字段，记录原始结构"
                    );
                }
            }
        }
    }

    Ok(PreviewRecord {
        id,
        user_id: as_str(row.get("USER_ID")).unwrap_or_default(),
        user_info_json: opt_str(row.get("USER_INFO_JSON")),
        file_name: as_str(row.get("FILE_NAME")).unwrap_or_default(),
        ocr_text: as_str(row.get("OCR_TEXT")).unwrap_or_default(),
        theme_id: opt_str(row.get("THEME_ID")),
        evaluation_result,
        preview_url: as_str(row.get("PREVIEW_URL")).unwrap_or_default(),
        preview_view_url: opt_str(row.get("PREVIEW_VIEW_URL")),
        preview_download_url: opt_str(row.get("PREVIEW_DOWNLOAD_URL")),
        status: match as_str(row.get("STATUS"))
            .unwrap_or_else(|| "processing".to_string())
            .as_str()
        {
            "pending" => PreviewStatus::Pending,
            "queued" => PreviewStatus::Queued,
            "completed" => PreviewStatus::Completed,
            "failed" => PreviewStatus::Failed,
            _ => PreviewStatus::Processing,
        },
        created_at: parse_dt(row.get("CREATED_AT")),
        updated_at: parse_dt(row.get("UPDATED_AT")),
        third_party_request_id: opt_str(row.get("THIRD_PARTY_REQUEST_ID")),
        queued_at: parse_dt_opt(row.get("QUEUED_AT")),
        processing_started_at: parse_dt_opt(row.get("PROCESSING_STARTED_AT")),
        retry_count: as_i64(row.get("RETRY_COUNT")).unwrap_or(0) as i32,
        last_worker_id: opt_str(row.get("LAST_WORKER_ID")),
        last_attempt_id: opt_str(row.get("LAST_ATTEMPT_ID")),
        failure_reason: opt_str(row.get("FAILURE_REASON")),
        ocr_stderr_summary: opt_str(row.get("OCR_STDERR_SUMMARY")),
        failure_context: opt_str(row.get("FAILURE_CONTEXT")),
        last_error_code: opt_str(row.get("LAST_ERROR_CODE")),
        slow_attachment_info_json: opt_str(row.get("SLOW_ATTACHMENT_INFO_JSON")),
        callback_url: opt_str(row.get("CALLBACK_URL")),
        callback_status: opt_str(row.get("CALLBACK_STATUS")),
        callback_attempts: as_i64(row.get("CALLBACK_ATTEMPTS")).unwrap_or(0) as i32,
        callback_successes: as_i64(row.get("CALLBACK_SUCCESSES")).unwrap_or(0) as i32,
        callback_failures: as_i64(row.get("CALLBACK_FAILURES")).unwrap_or(0) as i32,
        last_callback_at: parse_dt_opt(row.get("LAST_CALLBACK_AT")),
        last_callback_status_code: as_i64(row.get("LAST_CALLBACK_STATUS_CODE")).map(|v| v as i32),
        last_callback_response: opt_str(row.get("LAST_CALLBACK_RESPONSE")),
        last_callback_error: opt_str(row.get("LAST_CALLBACK_ERROR")),
        callback_payload: opt_str(row.get("CALLBACK_PAYLOAD")),
        next_callback_after: parse_dt_opt(row.get("NEXT_CALLBACK_AFTER")),
    })
}

#[cfg(feature = "dm_go")]
fn map_matter_rule_config_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<MatterRuleConfigRecord> {
    Ok(MatterRuleConfigRecord {
        id: as_str(row.get("ID")).unwrap_or_default(),
        matter_id: as_str(row.get("MATTER_ID")).unwrap_or_default(),
        matter_name: opt_str(row.get("MATTER_NAME")),
        spec_version: as_str(row.get("SPEC_VERSION")).unwrap_or_else(|| "1.0".to_string()),
        mode: as_str(row.get("MODE")).unwrap_or_else(|| "presentOnly".to_string()),
        rule_payload: as_str(row.get("RULE_PAYLOAD")).unwrap_or_default(),
        status: as_str(row.get("STATUS")).unwrap_or_else(|| "active".to_string()),
        description: opt_str(row.get("DESCRIPTION")),
        checksum: opt_str(row.get("CHECKSUM")),
        updated_by: opt_str(row.get("UPDATED_BY")),
        created_at: parse_dt(row.get("CREATED_AT")),
        updated_at: parse_dt(row.get("UPDATED_AT")),
    })
}

#[cfg(feature = "dm_go")]
fn map_api_stats_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<ApiStats> {
    Ok(ApiStats {
        id: as_str(row.get("ID")).unwrap_or_default(),
        endpoint: as_str(row.get("ENDPOINT")).unwrap_or_default(),
        method: as_str(row.get("METHOD")).unwrap_or_default(),
        client_id: opt_str(row.get("CLIENT_ID")),
        user_id: opt_str(row.get("USER_ID")),
        status_code: as_i64(row.get("STATUS_CODE")).unwrap_or(0) as u16,
        response_time_ms: as_i64(row.get("RESPONSE_TIME_MS")).unwrap_or(0) as u32,
        request_size: as_i64(row.get("REQUEST_SIZE")).unwrap_or(0) as u32,
        response_size: as_i64(row.get("RESPONSE_SIZE")).unwrap_or(0) as u32,
        error_message: opt_str(row.get("ERROR_MESSAGE")),
        created_at: parse_dt(row.get("CREATED_AT")),
    })
}

#[cfg(feature = "dm_go")]
fn extract_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(num) => Some(num.to_string()),
        serde_json::Value::Bool(flag) => Some(flag.to_string()),
        serde_json::Value::Array(items) => {
            if let Some(found) = items.iter().find_map(|item| extract_string(item)) {
                Some(found)
            } else {
                Some(value.to_string())
            }
        }
        serde_json::Value::Object(map) => {
            // 优先匹配常见字段名称
            let preferred_keys = [
                "String",
                "string",
                "Value",
                "value",
                "Text",
                "text",
                "data",
                "content",
                "DmDbNClob",
                "dmdbnclob",
            ];

            for key in preferred_keys {
                if let Some(inner) = map.get(key) {
                    if let Some(result) = extract_string(inner) {
                        return Some(result);
                    }
                }
            }

            // 回退：遍历第一个可转换的字段
            if let Some(found) = map.values().find_map(|inner| extract_string(inner)) {
                Some(found)
            } else {
                Some(value.to_string())
            }
        }
        serde_json::Value::Null => None,
    }
}

#[cfg(feature = "dm_go")]
fn as_str(v: Option<&serde_json::Value>) -> Option<String> {
    v.and_then(extract_string)
}

#[cfg(feature = "dm_go")]
fn opt_str(v: Option<&serde_json::Value>) -> Option<String> {
    as_str(v)
}

#[cfg(feature = "dm_go")]
fn as_bool_flag(v: Option<&serde_json::Value>) -> bool {
    if let Some(val) = v {
        if let Some(b) = val.as_bool() {
            b
        } else if let Some(i) = val.as_i64() {
            i != 0
        } else if let Some(s) = val.as_str() {
            matches!(s, "1" | "true" | "TRUE")
        } else {
            false
        }
    } else {
        false
    }
}

#[cfg(feature = "dm_go")]
fn map_monitor_user_row(row: &std::collections::HashMap<String, serde_json::Value>) -> MonitorUser {
    MonitorUser {
        id: as_str(row.get("ID")).unwrap_or_default(),
        username: as_str(row.get("USERNAME")).unwrap_or_default(),
        role: as_str(row.get("ROLE")).unwrap_or_else(|| "ops_admin".to_string()),
        last_login_at: opt_str(row.get("LAST_LOGIN_AT")),
        login_count: as_i64(row.get("LOGIN_COUNT")).unwrap_or(0),
        is_active: as_bool_flag(row.get("IS_ACTIVE")),
    }
}

#[cfg(feature = "dm_go")]
fn map_monitor_user_with_prefix(
    row: &std::collections::HashMap<String, serde_json::Value>,
    prefix: &str,
) -> MonitorUser {
    let key = |suffix: &str| format!("{}{}", prefix, suffix);
    MonitorUser {
        id: as_str(row.get(&key("ID"))).unwrap_or_default(),
        username: as_str(row.get(&key("USERNAME"))).unwrap_or_default(),
        role: as_str(row.get(&key("ROLE"))).unwrap_or_else(|| "ops_admin".to_string()),
        last_login_at: opt_str(row.get(&key("LAST_LOGIN_AT"))),
        login_count: as_i64(row.get(&key("LOGIN_COUNT"))).unwrap_or(0),
        is_active: as_bool_flag(row.get(&key("IS_ACTIVE"))),
    }
}

#[cfg(feature = "dm_go")]
fn map_monitor_session_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<MonitorSession> {
    Ok(MonitorSession {
        id: as_str(row.get("SESSION_ID")).unwrap_or_default(),
        user_id: as_str(row.get("SESSION_USER_ID")).unwrap_or_default(),
        user: map_monitor_user_with_prefix(row, "USER_"),
        expires_at: as_str(row.get("SESSION_EXPIRES_AT")).unwrap_or_default(),
        ip_address: opt_str(row.get("SESSION_IP_ADDRESS")),
    })
}

#[cfg(feature = "dm_go")]
fn as_i64(v: Option<&serde_json::Value>) -> Option<i64> {
    v.and_then(|x| x.as_i64().or_else(|| x.as_str()?.parse().ok()))
}

#[cfg(feature = "dm_go")]
fn as_f64(v: Option<&serde_json::Value>) -> Option<f64> {
    v.and_then(|x| x.as_f64().or_else(|| x.as_str()?.parse().ok()))
}

#[cfg(feature = "dm_go")]
fn parse_dt(v: Option<&serde_json::Value>) -> chrono::DateTime<chrono::Utc> {
    if let Some(s) = v.and_then(|x| x.as_str()) {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return dt.with_timezone(&chrono::Utc);
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc);
        }
    }
    chrono::Utc::now()
}

#[cfg(feature = "dm_go")]
fn format_dm_datetime(dt: &chrono::DateTime<chrono::Utc>) -> String {
    let local = dt.with_timezone(&chrono::Local);
    local.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(feature = "dm_go")]
fn parse_dt_opt(v: Option<&serde_json::Value>) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Some(s) = v.and_then(|x| x.as_str()) {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.with_timezone(&chrono::Utc));
        }
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            return Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                dt,
                chrono::Utc,
            ));
        }
    }
    None
}

#[async_trait]
impl Database for DmDatabase {
    async fn initialize(&self) -> Result<()> {
        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => conn.initialize_schema().await,
        }
    }

    // Re-open the trait impl for the remaining Database methods (kept in same impl now)
    async fn health_check(&self) -> Result<bool> {
        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => conn.health_check().await,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    // === 预审记录与统计 ===
    async fn save_preview_request(&self, request: &PreviewRequestRecord) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let update_sql = r#"
                UPDATE PREVIEW_REQUESTS SET
                    THIRD_PARTY_REQUEST_ID = ?,
                    USER_ID = ?,
                    USER_INFO_JSON = ?,
                    MATTER_ID = ?,
                    MATTER_TYPE = ?,
                    MATTER_NAME = ?,
                    CHANNEL = ?,
                    SEQUENCE_NO = ?,
                    AGENT_INFO_JSON = ?,
                    SUBJECT_INFO_JSON = ?,
                    FORM_DATA_JSON = ?,
                    SCENE_DATA_JSON = ?,
                    MATERIAL_DATA_JSON = ?,
                    LATEST_PREVIEW_ID = ?,
                    LATEST_STATUS = ?,
                    UPDATED_AT = CURRENT_TIMESTAMP
                WHERE ID = ?
            "#;

            let latest_status_str = request
                .latest_status
                .as_ref()
                .map(|status| status.as_str().to_string());

            let update_params = vec![
                str_option_to_value(&request.third_party_request_id),
                Value::String(request.user_id.clone()),
                str_option_to_value(&request.user_info_json),
                Value::String(request.matter_id.clone()),
                Value::String(request.matter_type.clone()),
                Value::String(request.matter_name.clone()),
                Value::String(request.channel.clone()),
                Value::String(request.sequence_no.clone()),
                str_option_to_value(&request.agent_info_json),
                str_option_to_value(&request.subject_info_json),
                str_option_to_value(&request.form_data_json),
                str_option_to_value(&request.scene_data_json),
                str_option_to_value(&request.material_data_json),
                str_option_to_value(&request.latest_preview_id),
                status_option_to_value(latest_status_str.as_ref().map(|s| s.as_str())),
                Value::String(request.id.clone()),
            ];

            let affected = conn
                .execute_update_values(update_sql, update_params)
                .await?;

            if affected == 0 {
                let insert_sql = r#"
                    INSERT INTO PREVIEW_REQUESTS (
                        ID, THIRD_PARTY_REQUEST_ID, USER_ID, MATTER_ID, MATTER_TYPE,
                        MATTER_NAME, CHANNEL, SEQUENCE_NO, AGENT_INFO_JSON, SUBJECT_INFO_JSON,
                        FORM_DATA_JSON, SCENE_DATA_JSON, MATERIAL_DATA_JSON, USER_INFO_JSON,
                        LATEST_PREVIEW_ID, LATEST_STATUS, CREATED_AT, UPDATED_AT
                    ) VALUES (
                        ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
                    )
                "#;

                let insert_params = vec![
                    Value::String(request.id.clone()),
                    str_option_to_value(&request.third_party_request_id),
                    Value::String(request.user_id.clone()),
                    Value::String(request.matter_id.clone()),
                    Value::String(request.matter_type.clone()),
                    Value::String(request.matter_name.clone()),
                    Value::String(request.channel.clone()),
                    Value::String(request.sequence_no.clone()),
                    str_option_to_value(&request.agent_info_json),
                    str_option_to_value(&request.subject_info_json),
                    str_option_to_value(&request.form_data_json),
                    str_option_to_value(&request.scene_data_json),
                    str_option_to_value(&request.material_data_json),
                    str_option_to_value(&request.user_info_json),
                    str_option_to_value(&request.latest_preview_id),
                    status_option_to_value(latest_status_str.as_ref().map(|s| s.as_str())),
                ];

                conn.execute_update_values(insert_sql, insert_params)
                    .await?;
            }

            return Ok(());
        }

        Err(anyhow!("DM-Go save_preview_request not implemented"))
    }

    async fn get_preview_request(&self, id: &str) -> Result<Option<PreviewRequestRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                SELECT ID, THIRD_PARTY_REQUEST_ID, USER_ID, MATTER_ID, MATTER_TYPE,
                       MATTER_NAME, CHANNEL, SEQUENCE_NO, AGENT_INFO_JSON, SUBJECT_INFO_JSON,
                       FORM_DATA_JSON, SCENE_DATA_JSON, MATERIAL_DATA_JSON, LATEST_PREVIEW_ID,
                       LATEST_STATUS, CREATED_AT, UPDATED_AT
                FROM PREVIEW_REQUESTS
                WHERE ID = ?
            "#;
            let rows = conn.query_rows(sql, Some(vec![id.to_string()])).await?;
            if let Some(row) = rows.get(0) {
                return Ok(Some(map_preview_request_row(row)?));
            }
            return Ok(None);
        }

        Ok(None)
    }

    async fn find_preview_request_by_third_party(
        &self,
        third_party_request_id: &str,
    ) -> Result<Option<PreviewRequestRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                SELECT ID, THIRD_PARTY_REQUEST_ID, USER_ID, MATTER_ID, MATTER_TYPE,
                       MATTER_NAME, CHANNEL, SEQUENCE_NO, AGENT_INFO_JSON, SUBJECT_INFO_JSON,
                       FORM_DATA_JSON, SCENE_DATA_JSON, MATERIAL_DATA_JSON, LATEST_PREVIEW_ID,
                       LATEST_STATUS, CREATED_AT, UPDATED_AT
                FROM PREVIEW_REQUESTS
                WHERE THIRD_PARTY_REQUEST_ID = ?
            "#;
            let rows = conn
                .query_rows(sql, Some(vec![third_party_request_id.to_string()]))
                .await?;
            if let Some(row) = rows.get(0) {
                return Ok(Some(map_preview_request_row(row)?));
            }
            return Ok(None);
        }

        Ok(None)
    }

    async fn update_preview_request_latest(
        &self,
        request_id: &str,
        latest_preview_id: Option<&str>,
        latest_status: Option<PreviewStatus>,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                UPDATE PREVIEW_REQUESTS
                SET LATEST_PREVIEW_ID = ?,
                    LATEST_STATUS = ?,
                    UPDATED_AT = CURRENT_TIMESTAMP
                WHERE ID = ?
            "#;

            let status_str = latest_status.map(|s| s.as_str().to_string());
            let params = vec![
                latest_preview_id
                    .map(|s| Value::String(s.to_string()))
                    .unwrap_or(Value::Null),
                status_option_to_value(status_str.as_ref().map(|s| s.as_str())),
                Value::String(request_id.to_string()),
            ];

            conn.execute_update_values(sql, params).await?;
            return Ok(());
        }

        Err(anyhow!(
            "DM-Go update_preview_request_latest not implemented"
        ))
    }

    async fn list_preview_requests(
        &self,
        filter: &PreviewRequestFilter,
    ) -> Result<Vec<PreviewRequestRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sql = String::from(
                "SELECT ID, THIRD_PARTY_REQUEST_ID, USER_ID, MATTER_ID, MATTER_TYPE, MATTER_NAME, \
                        CHANNEL, SEQUENCE_NO, AGENT_INFO_JSON, SUBJECT_INFO_JSON, FORM_DATA_JSON, \
                        SCENE_DATA_JSON, MATERIAL_DATA_JSON, LATEST_PREVIEW_ID, LATEST_STATUS, \
                        CREATED_AT, UPDATED_AT FROM PREVIEW_REQUESTS WHERE 1=1",
            );
            let mut params: Vec<String> = Vec::new();

            if let Some(user_id) = &filter.user_id {
                sql.push_str(" AND USER_ID = ?");
                params.push(user_id.clone());
            }

            if let Some(matter_id) = &filter.matter_id {
                sql.push_str(" AND MATTER_ID = ?");
                params.push(matter_id.clone());
            }

            if let Some(channel) = &filter.channel {
                sql.push_str(" AND CHANNEL = ?");
                params.push(channel.clone());
            }

            if let Some(sequence_no) = &filter.sequence_no {
                sql.push_str(" AND SEQUENCE_NO = ?");
                params.push(sequence_no.clone());
            }

            if let Some(third_party) = &filter.third_party_request_id {
                sql.push_str(" AND THIRD_PARTY_REQUEST_ID = ?");
                params.push(third_party.clone());
            }

            if let Some(status) = &filter.latest_status {
                sql.push_str(" AND LATEST_STATUS = ?");
                params.push(status.as_str().to_string());
            }

            if let Some(created_from) = filter.created_from {
                sql.push_str(" AND CREATED_AT >= ?");
                params.push(format_dm_datetime(&created_from));
            }

            if let Some(created_to) = filter.created_to {
                sql.push_str(" AND CREATED_AT <= ?");
                params.push(format_dm_datetime(&created_to));
            }

            if let Some(search) = &filter.search {
                let pattern = format!("%{}%", search);
                sql.push_str(" AND (ID LIKE ? OR THIRD_PARTY_REQUEST_ID LIKE ?)");
                params.push(pattern.clone());
                params.push(pattern);
            }

            sql.push_str(" ORDER BY UPDATED_AT DESC");

            let rows = conn.query_rows(&sql, Some(params)).await?;

            let mut records: Vec<PreviewRequestRecord> = rows
                .iter()
                .filter_map(|row| map_preview_request_row(row).ok())
                .collect();

            if let Some(offset) = filter.offset {
                let skip = offset as usize;
                if skip >= records.len() {
                    records.clear();
                } else {
                    records.drain(0..skip);
                }
            }

            if let Some(limit) = filter.limit {
                if records.len() > limit as usize {
                    records.truncate(limit as usize);
                }
            }

            return Ok(records);
        }

        Ok(Vec::new())
    }

    async fn save_preview_record(&self, record: &PreviewRecord) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "INSERT INTO PREVIEW_RECORDS (ID, USER_ID, USER_INFO_JSON, FILE_NAME, OCR_TEXT, THEME_ID, EVALUATION_RESULT, PREVIEW_URL, PREVIEW_VIEW_URL, PREVIEW_DOWNLOAD_URL, STATUS, CREATED_AT, UPDATED_AT, THIRD_PARTY_REQUEST_ID, QUEUED_AT, PROCESSING_STARTED_AT, RETRY_COUNT, LAST_WORKER_ID, LAST_ATTEMPT_ID, FAILURE_REASON, OCR_STDERR_SUMMARY, FAILURE_CONTEXT, LAST_ERROR_CODE, SLOW_ATTACHMENT_INFO_JSON, CALLBACK_URL, CALLBACK_STATUS, CALLBACK_ATTEMPTS, CALLBACK_SUCCESSES, CALLBACK_FAILURES, LAST_CALLBACK_AT, LAST_CALLBACK_STATUS_CODE, LAST_CALLBACK_RESPONSE, LAST_CALLBACK_ERROR, CALLBACK_PAYLOAD, NEXT_CALLBACK_AFTER) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";

            let queued_at = record.queued_at.as_ref().map(format_dm_datetime);
            let processing_started_at = record
                .processing_started_at
                .as_ref()
                .map(format_dm_datetime);
            let last_callback_at = record.last_callback_at.as_ref().map(format_dm_datetime);
            let next_callback_after = record.next_callback_after.as_ref().map(format_dm_datetime);

            let mut params: Vec<Value> = vec![
                Value::String(record.id.clone()),
                Value::String(record.user_id.clone()),
                str_option_to_value(&record.user_info_json),
                Value::String(record.file_name.clone()),
                Value::String(record.ocr_text.clone()),
                str_option_to_value(&record.theme_id),
                str_option_to_value(&record.evaluation_result),
                Value::String(record.preview_url.clone()),
                str_option_to_value(&record.preview_view_url),
                str_option_to_value(&record.preview_download_url),
                Value::String(record.status.to_string()),
                str_option_to_value(&record.third_party_request_id),
                str_option_to_value(&queued_at),
                str_option_to_value(&processing_started_at),
                Value::Number(serde_json::Number::from(record.retry_count)),
                str_option_to_value(&record.last_worker_id),
                str_option_to_value(&record.last_attempt_id),
                str_option_to_value(&record.failure_reason),
                str_option_to_value(&record.ocr_stderr_summary),
                str_option_to_value(&record.failure_context),
                str_option_to_value(&record.last_error_code),
                str_option_to_value(&record.slow_attachment_info_json),
                str_option_to_value(&record.callback_url),
                str_option_to_value(&record.callback_status),
                Value::Number(serde_json::Number::from(record.callback_attempts)),
                Value::Number(serde_json::Number::from(record.callback_successes)),
                Value::Number(serde_json::Number::from(record.callback_failures)),
                str_option_to_value(&last_callback_at),
                match record.last_callback_status_code {
                    Some(code) => Value::Number(serde_json::Number::from(code)),
                    None => Value::Null,
                },
                str_option_to_value(&record.last_callback_response),
                str_option_to_value(&record.last_callback_error),
                str_option_to_value(&record.callback_payload),
                str_option_to_value(&next_callback_after),
            ];

            conn.execute_update_values(sql, params).await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go save_preview_record not implemented"))
    }

    async fn get_preview_record(&self, id: &str) -> Result<Option<PreviewRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "SELECT ID, USER_ID, USER_INFO_JSON, FILE_NAME, OCR_TEXT, THEME_ID, EVALUATION_RESULT, PREVIEW_URL, PREVIEW_VIEW_URL, PREVIEW_DOWNLOAD_URL, STATUS, CREATED_AT, UPDATED_AT, THIRD_PARTY_REQUEST_ID, QUEUED_AT, PROCESSING_STARTED_AT, RETRY_COUNT, LAST_WORKER_ID, LAST_ATTEMPT_ID, FAILURE_REASON, OCR_STDERR_SUMMARY, FAILURE_CONTEXT, LAST_ERROR_CODE, SLOW_ATTACHMENT_INFO_JSON, CALLBACK_URL, CALLBACK_STATUS, CALLBACK_ATTEMPTS, CALLBACK_SUCCESSES, CALLBACK_FAILURES, LAST_CALLBACK_AT, LAST_CALLBACK_STATUS_CODE, LAST_CALLBACK_RESPONSE, LAST_CALLBACK_ERROR, CALLBACK_PAYLOAD, NEXT_CALLBACK_AFTER FROM PREVIEW_RECORDS WHERE ID = ?";
            let rows = conn.query_rows(sql, Some(vec![id.to_string()])).await?;
            if let Some(row) = rows.get(0) {
                return Ok(Some(map_preview_row(row)?));
            }
            return Ok(None);
        }
        Ok(None)
    }

    async fn update_preview_status(&self, id: &str, status: PreviewStatus) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let status_str = status.to_string();
            let params: Vec<String>;
            let sql = match status {
                PreviewStatus::Pending => {
                    params = vec![status_str, id.to_string()];
                    "UPDATE PREVIEW_RECORDS SET STATUS = ?, UPDATED_AT = CURRENT_TIMESTAMP, QUEUED_AT = NULL, PROCESSING_STARTED_AT = NULL, RETRY_COUNT = 0, LAST_WORKER_ID = NULL, LAST_ATTEMPT_ID = NULL WHERE ID = ?"
                }
                PreviewStatus::Queued => {
                    params = vec![status_str, id.to_string()];
                    "UPDATE PREVIEW_RECORDS SET STATUS = ?, UPDATED_AT = CURRENT_TIMESTAMP, QUEUED_AT = CURRENT_TIMESTAMP, PROCESSING_STARTED_AT = NULL WHERE ID = ?"
                }
                PreviewStatus::Processing => {
                    params = vec![status_str, id.to_string()];
                    "UPDATE PREVIEW_RECORDS SET STATUS = ?, UPDATED_AT = CURRENT_TIMESTAMP, PROCESSING_STARTED_AT = CURRENT_TIMESTAMP, RETRY_COUNT = RETRY_COUNT + 1 WHERE ID = ?"
                }
                PreviewStatus::Completed | PreviewStatus::Failed => {
                    params = vec![status_str, id.to_string()];
                    "UPDATE PREVIEW_RECORDS SET STATUS = ?, UPDATED_AT = CURRENT_TIMESTAMP WHERE ID = ?"
                }
            };
            conn.execute_update(sql, Some(params)).await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go update_preview_status not implemented"))
    }

    async fn update_preview_evaluation_result(
        &self,
        id: &str,
        evaluation_result: &str,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "UPDATE PREVIEW_RECORDS SET EVALUATION_RESULT = ?, UPDATED_AT = CURRENT_TIMESTAMP WHERE ID = ?";
            let params = vec![
                Value::String(evaluation_result.to_string()),
                Value::String(id.to_string()),
            ];
            conn.execute_update_values(sql, params).await?;
            return Ok(());
        }
        Err(anyhow!(
            "DM-Go update_preview_evaluation_result not implemented"
        ))
    }

    async fn mark_preview_processing(
        &self,
        id: &str,
        worker_id: &str,
        attempt_id: &str,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "UPDATE PREVIEW_RECORDS SET STATUS = 'processing', UPDATED_AT = CURRENT_TIMESTAMP, PROCESSING_STARTED_AT = CURRENT_TIMESTAMP, RETRY_COUNT = RETRY_COUNT + 1, LAST_WORKER_ID = ?, LAST_ATTEMPT_ID = ? WHERE ID = ?";
            let params: Vec<String> = vec![
                worker_id.to_string(),
                attempt_id.to_string(),
                id.to_string(),
            ];
            conn.execute_update(sql, Some(params)).await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go mark_preview_processing not implemented"))
    }

    async fn update_preview_artifacts(
        &self,
        id: &str,
        file_name: &str,
        preview_url: &str,
        _preview_view_url: Option<&str>,
        _preview_download_url: Option<&str>,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "UPDATE PREVIEW_RECORDS SET FILE_NAME = ?, PREVIEW_URL = ?, UPDATED_AT = CURRENT_TIMESTAMP WHERE ID = ?";
            let params: Vec<String> = vec![
                file_name.to_string(),
                preview_url.to_string(),
                id.to_string(),
            ];
            conn.execute_update(sql, Some(params)).await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go update_preview_artifacts not implemented"))
    }

    async fn replace_preview_material_results(
        &self,
        preview_id: &str,
        records: &[PreviewMaterialResultRecord],
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let delete_sql = "DELETE FROM PREVIEW_MATERIAL_RESULTS WHERE PREVIEW_ID = ?";
            conn.execute_update(delete_sql, Some(vec![preview_id.to_string()]))
                .await?;

            for record in records {
                let insert_sql = "INSERT INTO PREVIEW_MATERIAL_RESULTS (ID, PREVIEW_ID, MATERIAL_CODE, MATERIAL_NAME, STATUS, STATUS_CODE, PROCESSING_STATUS, ISSUES_COUNT, WARNINGS_COUNT, ATTACHMENTS_JSON, SUMMARY_JSON, SCHEMA_VERSION, CREATED_AT, UPDATED_AT) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";

                let params: Vec<Value> = vec![
                    Value::String(record.id.clone()),
                    Value::String(record.preview_id.clone()),
                    Value::String(record.material_code.clone()),
                    str_option_to_value(&record.material_name),
                    Value::String(record.status.clone()),
                    Value::Number(serde_json::Number::from(record.status_code as i64)),
                    str_option_to_value(&record.processing_status),
                    Value::Number(serde_json::Number::from(record.issues_count as i64)),
                    Value::Number(serde_json::Number::from(record.warnings_count as i64)),
                    str_option_to_value(&record.attachments_json),
                    str_option_to_value(&record.summary_json),
                    Value::Number(serde_json::Number::from(1i64)),
                    Value::String(format_dm_datetime(&record.created_at)),
                    Value::String(format_dm_datetime(&record.updated_at)),
                ];

                conn.execute_update_values(insert_sql, params).await?;
            }
            return Ok(());
        }
        Err(anyhow!(
            "DM-Go replace_preview_material_results not implemented"
        ))
    }

    async fn replace_preview_rule_results(
        &self,
        preview_id: &str,
        records: &[PreviewRuleResultRecord],
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let delete_sql = "DELETE FROM PREVIEW_RULE_RESULTS WHERE PREVIEW_ID = ?";
            conn.execute_update(delete_sql, Some(vec![preview_id.to_string()]))
                .await?;

            for record in records {
                let insert_sql = "INSERT INTO PREVIEW_RULE_RESULTS (ID, PREVIEW_ID, MATERIAL_RESULT_ID, MATERIAL_CODE, RULE_ID, RULE_CODE, RULE_NAME, ENGINE, SEVERITY, STATUS, MESSAGE, SUGGESTIONS_JSON, EVIDENCE_JSON, EXTRA_JSON, CREATED_AT, UPDATED_AT) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)";
                let params: Vec<Value> = vec![
                    Value::String(record.id.clone()),
                    Value::String(record.preview_id.clone()),
                    str_option_to_value(&record.material_result_id),
                    str_option_to_value(&record.material_code),
                    str_option_to_value(&record.rule_id),
                    str_option_to_value(&record.rule_code),
                    str_option_to_value(&record.rule_name),
                    str_option_to_value(&record.engine),
                    str_option_to_value(&record.severity),
                    str_option_to_value(&record.status),
                    str_option_to_value(&record.message),
                    str_option_to_value(&record.suggestions_json),
                    str_option_to_value(&record.evidence_json),
                    str_option_to_value(&record.extra_json),
                    Value::String(format_dm_datetime(&record.created_at)),
                    Value::String(format_dm_datetime(&record.updated_at)),
                ];
                conn.execute_update_values(insert_sql, params).await?;
            }
            return Ok(());
        }
        Err(anyhow!(
            "DM-Go replace_preview_rule_results not implemented"
        ))
    }

    async fn update_preview_failure_context(&self, update: &PreviewFailureUpdate) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sets = vec!["UPDATED_AT = CURRENT_TIMESTAMP".to_string()];
            let mut params: Vec<String> = Vec::new();

            if let Some(reason_opt) = &update.failure_reason {
                match reason_opt {
                    Some(reason) => {
                        sets.push("FAILURE_REASON = ?".to_string());
                        params.push(reason.clone());
                    }
                    None => sets.push("FAILURE_REASON = NULL".to_string()),
                }
            }

            if let Some(context_opt) = &update.failure_context {
                match context_opt {
                    Some(context) => {
                        sets.push("FAILURE_CONTEXT = ?".to_string());
                        params.push(context.clone());
                    }
                    None => sets.push("FAILURE_CONTEXT = NULL".to_string()),
                }
            }

            if let Some(code_opt) = &update.last_error_code {
                match code_opt {
                    Some(code) => {
                        sets.push("LAST_ERROR_CODE = ?".to_string());
                        params.push(code.clone());
                    }
                    None => sets.push("LAST_ERROR_CODE = NULL".to_string()),
                }
            }

            if let Some(slow_opt) = &update.slow_attachment_info_json {
                match slow_opt {
                    Some(json) => {
                        sets.push("SLOW_ATTACHMENT_INFO_JSON = ?".to_string());
                        params.push(json.clone());
                    }
                    None => sets.push("SLOW_ATTACHMENT_INFO_JSON = NULL".to_string()),
                }
            }

            if let Some(ocr_opt) = &update.ocr_stderr_summary {
                match ocr_opt {
                    Some(summary) => {
                        sets.push("OCR_STDERR_SUMMARY = ?".to_string());
                        params.push(summary.clone());
                    }
                    None => sets.push("OCR_STDERR_SUMMARY = NULL".to_string()),
                }
            }

            let sql = format!(
                "UPDATE PREVIEW_RECORDS SET {} WHERE ID = ?",
                sets.join(", ")
            );
            params.push(update.preview_id.clone());
            conn.execute_update(&sql, Some(params)).await?;
            return Ok(());
        }
        Err(anyhow!(
            "DM-Go update_preview_failure_context not implemented"
        ))
    }

    async fn list_preview_records(&self, filter: &PreviewFilter) -> Result<Vec<PreviewRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sql = String::from("SELECT ID, USER_ID, USER_INFO_JSON, FILE_NAME, OCR_TEXT, THEME_ID, EVALUATION_RESULT, PREVIEW_URL, PREVIEW_VIEW_URL, PREVIEW_DOWNLOAD_URL, STATUS, CREATED_AT, UPDATED_AT, THIRD_PARTY_REQUEST_ID, QUEUED_AT, PROCESSING_STARTED_AT, RETRY_COUNT, LAST_WORKER_ID, LAST_ATTEMPT_ID, FAILURE_REASON, OCR_STDERR_SUMMARY, FAILURE_CONTEXT, LAST_ERROR_CODE, SLOW_ATTACHMENT_INFO_JSON, CALLBACK_URL, CALLBACK_STATUS, CALLBACK_ATTEMPTS, CALLBACK_SUCCESSES, CALLBACK_FAILURES, LAST_CALLBACK_AT, LAST_CALLBACK_STATUS_CODE, LAST_CALLBACK_RESPONSE, LAST_CALLBACK_ERROR, CALLBACK_PAYLOAD, NEXT_CALLBACK_AFTER FROM PREVIEW_RECORDS WHERE 1=1");
            let mut params: Vec<String> = Vec::new();
            if let Some(user_id) = &filter.user_id {
                sql.push_str(" AND USER_ID = ?");
                params.push(user_id.clone());
            }
            if let Some(status) = &filter.status {
                sql.push_str(" AND STATUS = ?");
                params.push(status.to_string());
            }
            if let Some(theme_id) = &filter.theme_id {
                sql.push_str(" AND THEME_ID = ?");
                params.push(theme_id.clone());
            }
            if let Some(tp_id) = &filter.third_party_request_id {
                sql.push_str(" AND THIRD_PARTY_REQUEST_ID = ?");
                params.push(tp_id.clone());
            }
            if let Some(start) = &filter.start_date {
                sql.push_str(" AND CREATED_AT >= ?");
                params.push(format_dm_datetime(start));
            }
            if let Some(end) = &filter.end_date {
                sql.push_str(" AND CREATED_AT <= ?");
                params.push(format_dm_datetime(end));
            }
            sql.push_str(" ORDER BY CREATED_AT DESC");
            if let Some(limit) = filter.limit {
                sql.push_str(" LIMIT ?");
                params.push(limit.to_string());
            }
            if let Some(offset) = filter.offset {
                sql.push_str(" OFFSET ?");
                params.push(offset.to_string());
            }
            let rows = conn.query_rows(&sql, Some(params)).await?;
            let mut out = Vec::new();
            for r in rows.iter() {
                if let Ok(rec) = map_preview_row(r) {
                    out.push(rec);
                }
            }
            return Ok(out);
        }
        Ok(vec![])
    }

    async fn check_and_update_preview_dedup(
        &self,
        fingerprint: &str,
        preview_id: &str,
        meta: &PreviewDedupMeta,
        limit: i32,
    ) -> Result<PreviewDedupDecision> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let select_sql =
                "SELECT LAST_PREVIEW_ID, REPEAT_COUNT FROM PREVIEW_DEDUP WHERE FINGERPRINT = ?";
            let rows = conn
                .query_rows(select_sql, Some(vec![fingerprint.to_string()]))
                .await?;

            if let Some(row) = rows.first() {
                let last_preview_id = as_str(row.get("LAST_PREVIEW_ID")).unwrap_or_default();
                let repeat_count = row
                    .get("REPEAT_COUNT")
                    .or_else(|| row.get("repeat_count"))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32;

                let new_count = repeat_count.saturating_add(1);
                let reuse = new_count >= limit && !last_preview_id.is_empty();
                let target_preview_id = if reuse {
                    last_preview_id.clone()
                } else {
                    preview_id.to_string()
                };

                let update_sql = "UPDATE PREVIEW_DEDUP SET LAST_PREVIEW_ID = ?, REPEAT_COUNT = ?, LAST_SEEN_AT = CURRENT_TIMESTAMP, PAYLOAD_HASH = ?, THIRD_PARTY_REQUEST_ID = ?, USER_ID = ?, MATTER_ID = ? WHERE FINGERPRINT = ?";
                conn.execute_with_params(
                    update_sql,
                    vec![
                        target_preview_id.clone(),
                        new_count.to_string(),
                        meta.payload_hash.clone(),
                        meta.third_party_request_id.clone().unwrap_or_default(),
                        meta.user_id.clone(),
                        meta.matter_id.clone(),
                        fingerprint.to_string(),
                    ],
                )
                .await?;

                return if reuse {
                    Ok(PreviewDedupDecision::ReuseExisting {
                        preview_id: target_preview_id,
                        repeat_count: new_count,
                    })
                } else {
                    Ok(PreviewDedupDecision::Allowed {
                        repeat_count: new_count,
                    })
                };
            }

            let insert_sql = "INSERT INTO PREVIEW_DEDUP (FINGERPRINT, FIRST_PREVIEW_ID, LAST_PREVIEW_ID, USER_ID, MATTER_ID, THIRD_PARTY_REQUEST_ID, PAYLOAD_HASH, REPEAT_COUNT, LAST_SEEN_AT, CREATED_AT) VALUES (?, ?, ?, ?, ?, ?, ?, 1, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)";
            conn.execute_with_params(
                insert_sql,
                vec![
                    fingerprint.to_string(),
                    preview_id.to_string(),
                    preview_id.to_string(),
                    meta.user_id.clone(),
                    meta.matter_id.clone(),
                    meta.third_party_request_id.clone().unwrap_or_default(),
                    meta.payload_hash.clone(),
                ],
            )
            .await?;
        }

        Ok(PreviewDedupDecision::Allowed { repeat_count: 1 })
    }

    async fn get_preview_status_counts(&self) -> Result<PreviewStatusCounts> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "SELECT STATUS, COUNT(*) AS CNT FROM PREVIEW_RECORDS GROUP BY STATUS";
            let rows = conn.query_rows(sql, None).await?;
            let mut counts = PreviewStatusCounts::default();
            for row in rows.iter() {
                let status_raw = as_str(row.get("STATUS")).unwrap_or_else(|| "pending".to_string());
                let cnt = as_i64(row.get("CNT")).unwrap_or(0).max(0) as u64;
                let status = PreviewStatus::from_str(&status_raw).unwrap_or(PreviewStatus::Pending);
                counts.total += cnt;
                match status {
                    PreviewStatus::Completed => counts.completed += cnt,
                    PreviewStatus::Processing => counts.processing += cnt,
                    PreviewStatus::Failed => counts.failed += cnt,
                    PreviewStatus::Pending => counts.pending += cnt,
                    PreviewStatus::Queued => counts.queued += cnt,
                }
            }
            return Ok(counts);
        }
        Ok(PreviewStatusCounts::default())
    }

    async fn find_preview_by_third_party_id(
        &self,
        third_party_id: &str,
        user_id: &str,
    ) -> Result<Option<PreviewRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "SELECT ID, USER_ID, USER_INFO_JSON, FILE_NAME, OCR_TEXT, THEME_ID, EVALUATION_RESULT, PREVIEW_URL, PREVIEW_VIEW_URL, PREVIEW_DOWNLOAD_URL, STATUS, CREATED_AT, UPDATED_AT, THIRD_PARTY_REQUEST_ID, QUEUED_AT, PROCESSING_STARTED_AT, RETRY_COUNT, LAST_WORKER_ID, LAST_ATTEMPT_ID, FAILURE_REASON, OCR_STDERR_SUMMARY, FAILURE_CONTEXT, LAST_ERROR_CODE, SLOW_ATTACHMENT_INFO_JSON, CALLBACK_URL, CALLBACK_STATUS, CALLBACK_ATTEMPTS, CALLBACK_SUCCESSES, CALLBACK_FAILURES, LAST_CALLBACK_AT, LAST_CALLBACK_STATUS_CODE, LAST_CALLBACK_RESPONSE, LAST_CALLBACK_ERROR, CALLBACK_PAYLOAD, NEXT_CALLBACK_AFTER FROM PREVIEW_RECORDS WHERE THIRD_PARTY_REQUEST_ID = ? AND USER_ID = ?";
            let rows = conn
                .query_rows(
                    sql,
                    Some(vec![third_party_id.to_string(), user_id.to_string()]),
                )
                .await?;
            if let Some(row) = rows.get(0) {
                return Ok(Some(map_preview_row(row)?));
            }
            return Ok(None);
        }
        Ok(None)
    }

    async fn save_api_stats(&self, stats: &ApiStats) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "INSERT INTO API_STATS (ID, ENDPOINT, METHOD, CLIENT_ID, USER_ID, STATUS_CODE, RESPONSE_TIME_MS, REQUEST_SIZE, RESPONSE_SIZE, ERROR_MESSAGE, CREATED_AT) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)";
            let params: Vec<String> = vec![
                stats.id.clone(),
                stats.endpoint.clone(),
                stats.method.clone(),
                stats.client_id.clone().unwrap_or_default(),
                stats.user_id.clone().unwrap_or_default(),
                (stats.status_code as i64).to_string(),
                (stats.response_time_ms as i64).to_string(),
                (stats.request_size as i64).to_string(),
                (stats.response_size as i64).to_string(),
                stats.error_message.clone().unwrap_or_default(),
            ];
            conn.execute_update(sql, Some(params)).await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go save_api_stats not implemented"))
    }

    async fn get_api_stats(&self, filter: &StatsFilter) -> Result<Vec<ApiStats>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sql = String::from("SELECT ID, ENDPOINT, METHOD, CLIENT_ID, USER_ID, STATUS_CODE, RESPONSE_TIME_MS, REQUEST_SIZE, RESPONSE_SIZE, ERROR_MESSAGE, CREATED_AT FROM API_STATS WHERE 1=1");
            let mut params: Vec<String> = Vec::new();
            if let Some(endpoint) = &filter.endpoint {
                sql.push_str(" AND ENDPOINT = ?");
                params.push(endpoint.clone());
            }
            if let Some(client_id) = &filter.client_id {
                sql.push_str(" AND CLIENT_ID = ?");
                params.push(client_id.clone());
            }
            if let Some(user_id) = &filter.user_id {
                sql.push_str(" AND USER_ID = ?");
                params.push(user_id.clone());
            }
            if let Some(start) = &filter.start_date {
                sql.push_str(" AND CREATED_AT >= ?");
                params.push(format_dm_datetime(start));
            }
            if let Some(end) = &filter.end_date {
                sql.push_str(" AND CREATED_AT <= ?");
                params.push(format_dm_datetime(end));
            }
            sql.push_str(" ORDER BY CREATED_AT DESC");
            let rows = conn.query_rows(&sql, Some(params)).await?;
            let mut out = Vec::new();
            for r in rows.iter() {
                if let Ok(x) = map_api_stats_row(r) {
                    out.push(x);
                }
            }
            return Ok(out);
        }
        Ok(vec![])
    }

    async fn get_api_summary(&self, filter: &StatsFilter) -> Result<ApiSummary> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sql = String::from("SELECT COUNT(*) AS TOTAL, SUM(CASE WHEN STATUS_CODE BETWEEN 200 AND 299 THEN 1 ELSE 0 END) AS SUCCESS, SUM(CASE WHEN STATUS_CODE >= 400 THEN 1 ELSE 0 END) AS FAILED, AVG(RESPONSE_TIME_MS) AS AVG_RT, SUM(REQUEST_SIZE) AS REQ_SZ, SUM(RESPONSE_SIZE) AS RESP_SZ FROM API_STATS WHERE 1=1");
            let mut params: Vec<String> = Vec::new();
            if let Some(endpoint) = &filter.endpoint {
                sql.push_str(" AND ENDPOINT = ?");
                params.push(endpoint.clone());
            }
            if let Some(client_id) = &filter.client_id {
                sql.push_str(" AND CLIENT_ID = ?");
                params.push(client_id.clone());
            }
            if let Some(user_id) = &filter.user_id {
                sql.push_str(" AND USER_ID = ?");
                params.push(user_id.clone());
            }
            if let Some(start) = &filter.start_date {
                sql.push_str(" AND CREATED_AT >= ?");
                params.push(format_dm_datetime(start));
            }
            if let Some(end) = &filter.end_date {
                sql.push_str(" AND CREATED_AT <= ?");
                params.push(format_dm_datetime(end));
            }
            let rows = conn.query_rows(&sql, Some(params)).await?;
            if let Some(r) = rows.get(0) {
                let total = as_i64(r.get("TOTAL")).unwrap_or(0) as u64;
                let success = as_i64(r.get("SUCCESS")).unwrap_or(0) as u64;
                let failed = as_i64(r.get("FAILED")).unwrap_or(0) as u64;
                let avg = as_f64(r.get("AVG_RT")).unwrap_or(0.0);
                let req_sz = as_i64(r.get("REQ_SZ")).unwrap_or(0) as u64;
                let resp_sz = as_i64(r.get("RESP_SZ")).unwrap_or(0) as u64;
                return Ok(ApiSummary {
                    total_calls: total,
                    success_calls: success,
                    failed_calls: failed,
                    avg_response_time_ms: avg,
                    total_request_size: req_sz,
                    total_response_size: resp_sz,
                });
            }
            return Ok(ApiSummary {
                total_calls: 0,
                success_calls: 0,
                failed_calls: 0,
                avg_response_time_ms: 0.0,
                total_request_size: 0,
                total_response_size: 0,
            });
        }
        Ok(ApiSummary {
            total_calls: 0,
            success_calls: 0,
            failed_calls: 0,
            avg_response_time_ms: 0.0,
            total_request_size: 0,
            total_response_size: 0,
        })
    }

    async fn save_user_login_record(
        &self,
        _user_id: &str,
        _user_name: Option<&str>,
        _certificate_type: &str,
        _certificate_number: Option<&str>,
        _phone_number: Option<&str>,
        _email: Option<&str>,
        _organization_name: Option<&str>,
        _organization_code: Option<&str>,
        _login_type: &str,
        _login_time: &str,
        _client_ip: &str,
        _user_agent: &str,
        _referer: &str,
        _cookie_info: &str,
        _raw_data: &str,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                INSERT INTO USER_LOGIN_RECORDS (
                    ID, USER_ID, USER_NAME, CERTIFICATE_TYPE, CERTIFICATE_NUMBER,
                    PHONE_NUMBER, EMAIL, ORGANIZATION_NAME, ORGANIZATION_CODE,
                    LOGIN_TYPE, LOGIN_TIME, CLIENT_IP, USER_AGENT, REFERER,
                    COOKIE_INFO, RAW_DATA, CREATED_AT
                ) VALUES (
                    USER_LOGIN_RECORDS_SEQ.NEXTVAL, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP
                )
            "#;

            let params = vec![
                Value::String(_user_id.to_string()),
                str_ref_option_to_value(_user_name),
                Value::String(_certificate_type.to_string()),
                str_ref_option_to_value(_certificate_number),
                str_ref_option_to_value(_phone_number),
                str_ref_option_to_value(_email),
                str_ref_option_to_value(_organization_name),
                str_ref_option_to_value(_organization_code),
                Value::String(_login_type.to_string()),
                Value::String(_login_time.to_string()),
                Value::String(_client_ip.to_string()),
                Value::String(_user_agent.to_string()),
                str_ref_option_to_value(Some(_referer)),
                Value::String(_cookie_info.to_string()),
                Value::String(_raw_data.to_string()),
            ];

            conn.execute_update_values(sql, params).await?;
            return Ok(());
        }

        Err(anyhow!("DM-Go save_user_login_record not implemented"))
    }

    async fn upsert_cached_material_record(&self, record: &CachedMaterialRecord) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let update_sql = r#"
                UPDATE CACHED_MATERIALS SET
                    PREVIEW_ID = ?,
                    MATERIAL_CODE = ?,
                    ATTACHMENT_INDEX = ?,
                    TOKEN = ?,
                    LOCAL_PATH = ?,
                    UPLOAD_STATUS = ?,
                    OSS_KEY = ?,
                    LAST_ERROR = ?,
                    FILE_SIZE = ?,
                    CHECKSUM_SHA256 = ?,
                    UPDATED_AT = CURRENT_TIMESTAMP
                WHERE ID = ?
            "#;

            let update_params = vec![
                Value::String(record.preview_id.clone()),
                Value::String(record.material_code.clone()),
                Value::from(record.attachment_index as i64),
                Value::String(record.token.clone()),
                Value::String(record.local_path.clone()),
                Value::String(record.upload_status.as_str().to_string()),
                str_option_to_value(&record.oss_key),
                str_option_to_value(&record.last_error),
                i64_option_to_value(record.file_size),
                str_option_to_value(&record.checksum_sha256),
                Value::String(record.id.clone()),
            ];

            let affected = conn
                .execute_update_values(update_sql, update_params)
                .await?;

            if affected == 0 {
                let insert_sql = r#"
                    INSERT INTO CACHED_MATERIALS (
                        ID, PREVIEW_ID, MATERIAL_CODE, ATTACHMENT_INDEX, TOKEN,
                        LOCAL_PATH, UPLOAD_STATUS, OSS_KEY, LAST_ERROR, FILE_SIZE,
                        CHECKSUM_SHA256, CREATED_AT, UPDATED_AT
                    ) VALUES (
                        ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
                    )
                "#;

                let insert_params = vec![
                    Value::String(record.id.clone()),
                    Value::String(record.preview_id.clone()),
                    Value::String(record.material_code.clone()),
                    Value::from(record.attachment_index as i64),
                    Value::String(record.token.clone()),
                    Value::String(record.local_path.clone()),
                    Value::String(record.upload_status.as_str().to_string()),
                    str_option_to_value(&record.oss_key),
                    str_option_to_value(&record.last_error),
                    i64_option_to_value(record.file_size),
                    str_option_to_value(&record.checksum_sha256),
                ];

                conn.execute_update_values(insert_sql, insert_params)
                    .await?;
            }

            return Ok(());
        }

        Err(anyhow!(
            "DM-Go upsert_cached_material_record not implemented"
        ))
    }

    async fn update_cached_material_status(
        &self,
        id: &str,
        status: CachedMaterialStatus,
        oss_key: Option<&str>,
        last_error: Option<&str>,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                UPDATE CACHED_MATERIALS
                SET UPLOAD_STATUS = ?,
                    OSS_KEY = ?,
                    LAST_ERROR = ?,
                    UPDATED_AT = CURRENT_TIMESTAMP
                WHERE ID = ?
            "#;

            let params = vec![
                Value::String(status.as_str().to_string()),
                str_ref_option_to_value(oss_key),
                str_ref_option_to_value(last_error),
                Value::String(id.to_string()),
            ];

            conn.execute_update_values(sql, params).await?;
            return Ok(());
        }

        Err(anyhow!(
            "DM-Go update_cached_material_status not implemented"
        ))
    }

    async fn list_cached_material_records(
        &self,
        filter: &CachedMaterialFilter,
    ) -> Result<Vec<CachedMaterialRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sql = String::from(
                "SELECT ID, PREVIEW_ID, MATERIAL_CODE, ATTACHMENT_INDEX, TOKEN, LOCAL_PATH, \
                        UPLOAD_STATUS, OSS_KEY, LAST_ERROR, FILE_SIZE, CHECKSUM_SHA256, CREATED_AT, UPDATED_AT \
                       FROM CACHED_MATERIALS WHERE 1=1",
            );
            let mut params: Vec<String> = Vec::new();

            if let Some(preview_id) = &filter.preview_id {
                sql.push_str(" AND PREVIEW_ID = ?");
                params.push(preview_id.clone());
            }

            if let Some(status) = &filter.status {
                sql.push_str(" AND UPLOAD_STATUS = ?");
                params.push(status.as_str().to_string());
            }

            sql.push_str(" ORDER BY UPDATED_AT DESC");

            if let Some(limit) = filter.limit {
                sql.push_str(" FETCH FIRST ");
                sql.push_str(&limit.to_string());
                sql.push_str(" ROWS ONLY");
            }

            let rows = if params.is_empty() {
                conn.query_rows(&sql, None).await?
            } else {
                conn.query_rows(&sql, Some(params)).await?
            };

            let records = rows
                .iter()
                .filter_map(|row| map_cached_material_row(row).ok())
                .collect();
            return Ok(records);
        }

        Ok(Vec::new())
    }

    async fn delete_cached_material_record(&self, id: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "DELETE FROM CACHED_MATERIALS WHERE ID = ?";
            conn.execute_update(sql, Some(vec![id.to_string()])).await?;
            return Ok(());
        }

        Err(anyhow!(
            "DM-Go delete_cached_material_record not implemented"
        ))
    }

    async fn delete_cached_materials_by_preview(&self, preview_id: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "DELETE FROM CACHED_MATERIALS WHERE PREVIEW_ID = ?";
            conn.execute_update(sql, Some(vec![preview_id.to_string()]))
                .await?;
            return Ok(());
        }

        Err(anyhow!(
            "DM-Go delete_cached_materials_by_preview not implemented"
        ))
    }

    /// 保存材料文件记录
    async fn save_material_file_record(&self, record: &MaterialFileRecord) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                INSERT INTO PREVIEW_MATERIAL_FILES (
                    ID, PREVIEW_ID, MATERIAL_CODE, ATTACHMENT_NAME, SOURCE_URL,
                    STORED_ORIGINAL_KEY, STORED_PROCESSED_KEYS, MIME_TYPE, SIZE_BYTES,
                    CHECKSUM_SHA256, OCR_TEXT_KEY, OCR_TEXT_LENGTH, STATUS, ERROR_MESSAGE,
                    CREATED_AT, UPDATED_AT
                ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
            "#;

            let params: Vec<String> = vec![
                record.id.clone(),
                record.preview_id.clone(),
                record.material_code.clone(),
                record.attachment_name.clone().unwrap_or_default(),
                record.source_url.clone().unwrap_or_default(),
                record.stored_original_key.clone(),
                record.stored_processed_keys.clone().unwrap_or_default(),
                record.mime_type.clone().unwrap_or_default(),
                record.size_bytes.map(|v| v.to_string()).unwrap_or_default(),
                record.checksum_sha256.clone().unwrap_or_default(),
                record.ocr_text_key.clone().unwrap_or_default(),
                record
                    .ocr_text_length
                    .map(|v| v.to_string())
                    .unwrap_or_default(),
                record.status.clone(),
                record.error_message.clone().unwrap_or_default(),
            ];

            conn.execute_update(sql, Some(params)).await?;
            tracing::info!("[ok] [DM-Go] 保存材料文件记录成功: {}", record.id);
            return Ok(());
        }
        Err(anyhow!("DM-Go save_material_file_record not implemented"))
    }

    /// 更新材料文件状态
    async fn update_material_file_status(
        &self,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                UPDATE PREVIEW_MATERIAL_FILES
                SET STATUS = ?, ERROR_MESSAGE = ?, UPDATED_AT = CURRENT_TIMESTAMP
                WHERE ID = ?
            "#;

            let params = vec![
                status.to_string(),
                error.unwrap_or("").to_string(),
                id.to_string(),
            ];

            conn.execute_update(sql, Some(params)).await?;
            tracing::info!("[ok] [DM-Go] 更新材料文件状态成功: {} -> {}", id, status);
            return Ok(());
        }
        Err(anyhow!("DM-Go update_material_file_status not implemented"))
    }

    /// 更新材料文件处理信息
    async fn update_material_file_processing(
        &self,
        id: &str,
        processed_keys_json: Option<&str>,
        ocr_text_key: Option<&str>,
        ocr_text_length: Option<i64>,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            // 构建动态UPDATE语句
            let mut sql_parts = vec!["UPDATE PREVIEW_MATERIAL_FILES SET"];
            let mut params = Vec::new();
            let mut updates = Vec::new();

            if let Some(keys) = processed_keys_json {
                updates.push("STORED_PROCESSED_KEYS = ?");
                params.push(keys.to_string());
            }

            if let Some(ocr_key) = ocr_text_key {
                updates.push("OCR_TEXT_KEY = ?");
                params.push(ocr_key.to_string());
            }

            if let Some(length) = ocr_text_length {
                updates.push("OCR_TEXT_LENGTH = ?");
                params.push(length.to_string());
            }

            if updates.is_empty() {
                return Ok(()); // 没有需要更新的字段
            }

            updates.push("UPDATED_AT = CURRENT_TIMESTAMP");
            let updates_str = updates.join(", ");
            sql_parts.push(&updates_str);
            sql_parts.push("WHERE ID = ?");
            params.push(id.to_string());

            let sql = sql_parts.join(" ");
            conn.execute_update(&sql, Some(params)).await?;
            tracing::info!("[ok] [DM-Go] 更新材料文件处理信息成功: {}", id);
            return Ok(());
        }
        Err(anyhow!(
            "DM-Go update_material_file_processing not implemented"
        ))
    }

    /// 查询材料文件列表
    async fn list_material_files(
        &self,
        filter: &MaterialFileFilter,
    ) -> Result<Vec<MaterialFileRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sql = String::from("SELECT * FROM PREVIEW_MATERIAL_FILES WHERE 1=1");
            let mut params = Vec::new();

            if let Some(preview_id) = &filter.preview_id {
                sql.push_str(" AND PREVIEW_ID = ?");
                params.push(preview_id.clone());
            }

            if let Some(material_code) = &filter.material_code {
                sql.push_str(" AND MATERIAL_CODE = ?");
                params.push(material_code.clone());
            }

            sql.push_str(" ORDER BY CREATED_AT ASC");

            let rows = conn
                .query_rows(
                    &sql,
                    if params.is_empty() {
                        None
                    } else {
                        Some(params)
                    },
                )
                .await?;
            let mut records = Vec::new();

            for row in rows {
                records.push(map_material_file_row(&row)?);
            }

            tracing::info!(
                "[ok] [DM-Go] 查询材料文件列表成功: {} 条记录",
                records.len()
            );
            return Ok(records);
        }
        Err(anyhow!("DM-Go list_material_files not implemented"))
    }

    async fn save_task_payload(&self, preview_id: &str, payload: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let delete_sql = "DELETE FROM PREVIEW_TASK_PAYLOADS WHERE PREVIEW_ID = ?";
            conn.execute_update(delete_sql, Some(vec![preview_id.to_string()]))
                .await
                .ok();

            let sql = "INSERT INTO PREVIEW_TASK_PAYLOADS (PREVIEW_ID, PAYLOAD, CREATED_AT, UPDATED_AT) VALUES (?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)";
            let params = vec![preview_id.to_string(), payload.to_string()];
            conn.execute_update(sql, Some(params)).await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go save_task_payload not implemented"))
    }

    async fn load_task_payload(&self, preview_id: &str) -> Result<Option<String>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "SELECT PAYLOAD FROM PREVIEW_TASK_PAYLOADS WHERE PREVIEW_ID = ?";
            let rows = conn
                .query_rows(sql, Some(vec![preview_id.to_string()]))
                .await?;
            if let Some(row) = rows.get(0) {
                return Ok(row
                    .get("PAYLOAD")
                    .and_then(|v| v.as_str().map(|s| s.to_string())));
            }
            return Ok(None);
        }
        Ok(None)
    }

    async fn delete_task_payload(&self, preview_id: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "DELETE FROM PREVIEW_TASK_PAYLOADS WHERE PREVIEW_ID = ?";
            conn.execute_update(sql, Some(vec![preview_id.to_string()]))
                .await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go delete_task_payload not implemented"))
    }

    async fn update_preview_callback_state(&self, update: &PreviewCallbackUpdate) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sets = vec!["UPDATED_AT = CURRENT_TIMESTAMP".to_string()];
            let mut params: Vec<String> = Vec::new();

            if let Some(url_opt) = &update.callback_url {
                match url_opt {
                    Some(url) => {
                        sets.push("CALLBACK_URL = ?".to_string());
                        params.push(url.clone());
                    }
                    None => sets.push("CALLBACK_URL = NULL".to_string()),
                }
            }

            if let Some(status_opt) = &update.callback_status {
                match status_opt {
                    Some(status) => {
                        sets.push("CALLBACK_STATUS = ?".to_string());
                        params.push(status.clone());
                    }
                    None => sets.push("CALLBACK_STATUS = NULL".to_string()),
                }
            }

            if let Some(attempts) = update.callback_attempts {
                sets.push("CALLBACK_ATTEMPTS = ?".to_string());
                params.push(attempts.to_string());
            }
            if let Some(successes) = update.callback_successes {
                sets.push("CALLBACK_SUCCESSES = ?".to_string());
                params.push(successes.to_string());
            }
            if let Some(failures) = update.callback_failures {
                sets.push("CALLBACK_FAILURES = ?".to_string());
                params.push(failures.to_string());
            }

            if let Some(last_at_opt) = &update.last_callback_at {
                match last_at_opt {
                    Some(dt) => {
                        sets.push("LAST_CALLBACK_AT = ?".to_string());
                        params.push(format_dm_datetime(dt));
                    }
                    None => sets.push("LAST_CALLBACK_AT = NULL".to_string()),
                }
            }

            if let Some(code_opt) = &update.last_callback_status_code {
                match code_opt {
                    Some(code) => {
                        sets.push("LAST_CALLBACK_STATUS_CODE = ?".to_string());
                        params.push(code.to_string());
                    }
                    None => sets.push("LAST_CALLBACK_STATUS_CODE = NULL".to_string()),
                }
            }

            if let Some(resp_opt) = &update.last_callback_response {
                match resp_opt {
                    Some(resp) => {
                        sets.push("LAST_CALLBACK_RESPONSE = ?".to_string());
                        params.push(resp.clone());
                    }
                    None => sets.push("LAST_CALLBACK_RESPONSE = NULL".to_string()),
                }
            }

            if let Some(err_opt) = &update.last_callback_error {
                match err_opt {
                    Some(err) => {
                        sets.push("LAST_CALLBACK_ERROR = ?".to_string());
                        params.push(err.clone());
                    }
                    None => sets.push("LAST_CALLBACK_ERROR = NULL".to_string()),
                }
            }

            if let Some(payload_opt) = &update.callback_payload {
                match payload_opt {
                    Some(payload) => {
                        sets.push("CALLBACK_PAYLOAD = ?".to_string());
                        params.push(payload.clone());
                    }
                    None => sets.push("CALLBACK_PAYLOAD = NULL".to_string()),
                }
            }

            if let Some(next_opt) = &update.next_callback_after {
                match next_opt {
                    Some(dt) => {
                        sets.push("NEXT_CALLBACK_AFTER = ?".to_string());
                        params.push(format_dm_datetime(dt));
                    }
                    None => sets.push("NEXT_CALLBACK_AFTER = NULL".to_string()),
                }
            }

            let sql = format!(
                "UPDATE PREVIEW_RECORDS SET {} WHERE ID = ?",
                sets.join(", ")
            );
            params.push(update.preview_id.clone());
            conn.execute_update(&sql, Some(params)).await?;
            return Ok(());
        }
        Err(anyhow!(
            "DM-Go update_preview_callback_state not implemented"
        ))
    }

    async fn list_due_callbacks(&self, limit: u32) -> Result<Vec<PreviewRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "SELECT ID, USER_ID, USER_INFO_JSON, FILE_NAME, OCR_TEXT, THEME_ID, EVALUATION_RESULT, PREVIEW_URL, PREVIEW_VIEW_URL, PREVIEW_DOWNLOAD_URL, STATUS, CREATED_AT, UPDATED_AT, THIRD_PARTY_REQUEST_ID, QUEUED_AT, PROCESSING_STARTED_AT, RETRY_COUNT, LAST_WORKER_ID, LAST_ATTEMPT_ID, FAILURE_REASON, OCR_STDERR_SUMMARY, FAILURE_CONTEXT, LAST_ERROR_CODE, SLOW_ATTACHMENT_INFO_JSON, CALLBACK_URL, CALLBACK_STATUS, CALLBACK_ATTEMPTS, CALLBACK_SUCCESSES, CALLBACK_FAILURES, LAST_CALLBACK_AT, LAST_CALLBACK_STATUS_CODE, LAST_CALLBACK_RESPONSE, LAST_CALLBACK_ERROR, CALLBACK_PAYLOAD, NEXT_CALLBACK_AFTER FROM PREVIEW_RECORDS WHERE CALLBACK_URL IS NOT NULL AND CALLBACK_URL <> '' AND CALLBACK_PAYLOAD IS NOT NULL AND CALLBACK_STATUS IN ('scheduled', 'retrying') AND (NEXT_CALLBACK_AFTER IS NULL OR NEXT_CALLBACK_AFTER <= CURRENT_TIMESTAMP) ORDER BY COALESCE(NEXT_CALLBACK_AFTER, UPDATED_AT) ASC LIMIT ?";
            let rows = conn.query_rows(sql, Some(vec![limit.to_string()])).await?;
            let mut records = Vec::new();
            for row in rows {
                if let Ok(record) = map_preview_row(&row) {
                    records.push(record);
                }
            }
            return Ok(records);
        }
        Ok(Vec::new())
    }

    async fn enqueue_outbox_event(&self, event: &NewOutboxEvent) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = "INSERT INTO DB_OUTBOX (ID, TABLE_NAME, OP_TYPE, PK_VALUE, IDEMPOTENCY_KEY, PAYLOAD, CREATED_AT, APPLIED_AT, RETRIES, LAST_ERROR) VALUES (DB_OUTBOX_SEQ.NEXTVAL, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, NULL, 0, NULL)";
            let params = vec![
                Value::String(event.table_name.clone()),
                Value::String(event.op_type.clone()),
                Value::String(event.pk_value.clone()),
                Value::String(event.idempotency_key.clone()),
                Value::String(event.payload.clone()),
            ];
            conn.execute_update_values(sql, params).await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go enqueue_outbox_event not implemented"))
    }

    async fn fetch_pending_outbox_events(&self, limit: u32) -> Result<Vec<OutboxEvent>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let fetch_limit = limit.max(1);
            let sql = format!(
                "SELECT ID, TABLE_NAME, OP_TYPE, PK_VALUE, IDEMPOTENCY_KEY, PAYLOAD, CREATED_AT, APPLIED_AT, RETRIES, LAST_ERROR FROM DB_OUTBOX WHERE APPLIED_AT IS NULL ORDER BY CREATED_AT ASC FETCH FIRST {} ROWS ONLY",
                fetch_limit
            );
            let rows = conn.query_rows(&sql, None).await?;
            let mut events = Vec::with_capacity(rows.len());
            for row in rows {
                events.push(map_outbox_row(&row)?);
            }
            return Ok(events);
        }
        Ok(Vec::new())
    }

    async fn mark_outbox_event_applied(&self, event_id: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql =
                "UPDATE DB_OUTBOX SET APPLIED_AT = CURRENT_TIMESTAMP, LAST_ERROR = NULL WHERE ID = ?";
            conn.execute_update(sql, Some(vec![event_id.to_string()]))
                .await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go mark_outbox_event_applied not implemented"))
    }

    async fn mark_outbox_event_failed(&self, event_id: &str, error: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql =
                "UPDATE DB_OUTBOX SET RETRIES = RETRIES + 1, LAST_ERROR = ?, APPLIED_AT = NULL WHERE ID = ?";
            conn.execute_update(sql, Some(vec![error.to_string(), event_id.to_string()]))
                .await?;
            return Ok(());
        }
        Err(anyhow!("DM-Go mark_outbox_event_failed not implemented"))
    }

    async fn get_matter_rule_config(
        &self,
        matter_id: &str,
    ) -> Result<Option<MatterRuleConfigRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let sql = r#"
                SELECT ID, MATTER_ID, MATTER_NAME, SPEC_VERSION, MODE, RULE_PAYLOAD,
                       STATUS, DESCRIPTION, CHECKSUM, UPDATED_BY, CREATED_AT, UPDATED_AT
                FROM MATTER_RULE_CONFIGS
                WHERE MATTER_ID = ?
            "#;
            let rows = conn
                .query_rows(sql, Some(vec![matter_id.to_string()]))
                .await?;
            if let Some(row) = rows.get(0) {
                return Ok(Some(map_matter_rule_config_row(row)?));
            }
            return Ok(None);
        }
        Ok(None)
    }

    async fn upsert_matter_rule_config(&self, config: &MatterRuleConfigRecord) -> Result<()> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let update_sql = r#"
                UPDATE MATTER_RULE_CONFIGS SET
                    MATTER_NAME = ?,
                    SPEC_VERSION = ?,
                    MODE = ?,
                    RULE_PAYLOAD = ?,
                    STATUS = ?,
                    DESCRIPTION = ?,
                    CHECKSUM = ?,
                    UPDATED_BY = ?,
                    UPDATED_AT = CURRENT_TIMESTAMP
                WHERE MATTER_ID = ?
            "#;

            let update_params = vec![
                str_option_to_value(&config.matter_name),
                Value::String(config.spec_version.clone()),
                Value::String(config.mode.clone()),
                Value::String(config.rule_payload.clone()),
                Value::String(config.status.clone()),
                str_option_to_value(&config.description),
                str_option_to_value(&config.checksum),
                str_option_to_value(&config.updated_by),
                Value::String(config.matter_id.clone()),
            ];

            let affected = conn
                .execute_update_values(update_sql, update_params)
                .await?;

            if affected == 0 {
                let insert_sql = r#"
                    INSERT INTO MATTER_RULE_CONFIGS (
                        ID, MATTER_ID, MATTER_NAME, SPEC_VERSION, MODE, RULE_PAYLOAD,
                        STATUS, DESCRIPTION, CHECKSUM, UPDATED_BY, CREATED_AT, UPDATED_AT
                    ) VALUES (
                        ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP
                    )
                "#;

                let insert_params = vec![
                    Value::String(config.id.clone()),
                    Value::String(config.matter_id.clone()),
                    str_option_to_value(&config.matter_name),
                    Value::String(config.spec_version.clone()),
                    Value::String(config.mode.clone()),
                    Value::String(config.rule_payload.clone()),
                    Value::String(config.status.clone()),
                    str_option_to_value(&config.description),
                    str_option_to_value(&config.checksum),
                    str_option_to_value(&config.updated_by),
                ];

                conn.execute_update_values(insert_sql, insert_params)
                    .await?;
            }

            return Ok(());
        }

        Err(anyhow!("DM-Go upsert_matter_rule_config not implemented"))
    }

    async fn list_matter_rule_configs(
        &self,
        status: Option<&str>,
    ) -> Result<Vec<MatterRuleConfigRecord>> {
        #[cfg(feature = "dm_go")]
        if let DmConnectionType::Go(conn) = &self.connection {
            let mut sql = String::from(
                "SELECT ID, MATTER_ID, MATTER_NAME, SPEC_VERSION, MODE, RULE_PAYLOAD, STATUS, DESCRIPTION, CHECKSUM, UPDATED_BY, CREATED_AT, UPDATED_AT FROM MATTER_RULE_CONFIGS WHERE 1=1",
            );
            let mut params: Vec<String> = Vec::new();

            if let Some(status) = status {
                sql.push_str(" AND STATUS = ?");
                params.push(status.to_string());
            }

            sql.push_str(" ORDER BY UPDATED_AT DESC");

            let rows = conn.query_rows(&sql, Some(params)).await?;
            let configs = rows
                .iter()
                .filter_map(|row| map_matter_rule_config_row(row).ok())
                .collect();
            return Ok(configs);
        }

        Ok(Vec::new())
    }
    // Worker结果异步处理队列相关方法

    /// 入队Worker结果
    async fn enqueue_worker_result(&self, preview_id: &str, payload: &str) -> Result<()> {
        let insert_sql = "INSERT INTO WORKER_RESULTS_QUEUE (ID, PREVIEW_ID, PAYLOAD, STATUS, ATTEMPTS, CREATED_AT, UPDATED_AT) \
                          VALUES (?, ?, ?, 'pending', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)";
        let id = Uuid::new_v4().to_string();

        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                // 优先尝试复用已有记录（按 PREVIEW_ID 覆盖），避免重复入队
                let update_sql = "UPDATE WORKER_RESULTS_QUEUE \
                                  SET PAYLOAD = ?, STATUS = 'pending', ATTEMPTS = 0, LAST_ERROR = NULL, UPDATED_AT = CURRENT_TIMESTAMP \
                                  WHERE PREVIEW_ID = ?";
                let updated = conn
                    .execute_with_params(
                        update_sql,
                        vec![payload.to_string(), preview_id.to_string()],
                    )
                    .await
                    .unwrap_or(0);

                if updated == 0 {
                    conn.execute_with_params(
                        insert_sql,
                        vec![id, preview_id.to_string(), payload.to_string()],
                    )
                    .await?;
                }
            }
        }
        Ok(())
    }

    /// 拉取待处理的Worker结果
    async fn fetch_pending_worker_results(
        &self,
        limit: u32,
    ) -> Result<Vec<WorkerResultQueueRecord>> {
        let sql = format!(
            "SELECT ID, PREVIEW_ID, PAYLOAD, STATUS, ATTEMPTS, LAST_ERROR, CREATED_AT, UPDATED_AT \
             FROM WORKER_RESULTS_QUEUE \
             WHERE STATUS = 'pending' \
             ORDER BY CREATED_AT ASC \
             LIMIT {}",
            limit
        );

        let rows = match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => conn.query_rows(&sql, None).await?,
        };

        let mut results = Vec::new();
        for row in rows {
            // 简单的辅助函数，从 Value 中提取 String
            let get_str = |key: &str| -> String {
                row.get(key)
                    .or_else(|| row.get(&key.to_lowercase()))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };

            let get_opt_str = |key: &str| -> Option<String> {
                row.get(key)
                    .or_else(|| row.get(&key.to_lowercase()))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            };

            let get_int = |key: &str| -> i32 {
                row.get(key)
                    .or_else(|| row.get(&key.to_lowercase()))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32
            };

            // 解析时间，这里简化处理，实际可能需要更严谨的解析
            let parse_time = |key: &str| -> DateTime<Utc> {
                let s = get_str(key);
                // 尝试解析常见格式，或者直接返回当前时间作为fallback
                // 注意：DM返回的时间格式可能因配置而异
                chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                    .map(|dt| DateTime::from_utc(dt, Utc))
                    .unwrap_or_else(|_| Utc::now())
            };

            results.push(WorkerResultQueueRecord {
                id: get_str("ID"),
                preview_id: get_str("PREVIEW_ID"),
                payload: get_str("PAYLOAD"),
                status: get_str("STATUS"),
                attempts: get_int("ATTEMPTS"),
                last_error: get_opt_str("LAST_ERROR"),
                created_at: parse_time("CREATED_AT"),
                updated_at: parse_time("UPDATED_AT"),
            });
        }

        Ok(results)
    }

    async fn get_worker_result_by_preview_id(
        &self,
        preview_id: &str,
    ) -> Result<Option<WorkerResultQueueRecord>> {
        let sql = "SELECT ID, PREVIEW_ID, PAYLOAD, STATUS, ATTEMPTS, LAST_ERROR, CREATED_AT, UPDATED_AT \
                   FROM WORKER_RESULTS_QUEUE WHERE PREVIEW_ID = ? LIMIT 1";

        let rows = match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => conn.query_rows(sql, Some(vec![preview_id.to_string()])).await?,
        };

        let Some(row) = rows.into_iter().next() else {
            return Ok(None);
        };

        let get_str = |key: &str| -> String {
            row.get(key)
                .or_else(|| row.get(&key.to_lowercase()))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string()
        };

        let get_opt_str = |key: &str| -> Option<String> {
            row.get(key)
                .or_else(|| row.get(&key.to_lowercase()))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        };

        let get_int = |key: &str| -> i32 {
            row.get(key)
                .or_else(|| row.get(&key.to_lowercase()))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32
        };

        let parse_time = |key: &str| -> DateTime<Utc> {
            let s = get_str(key);
            chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                .map(|dt| DateTime::from_utc(dt, Utc))
                .unwrap_or_else(|_| Utc::now())
        };

        Ok(Some(WorkerResultQueueRecord {
            id: get_str("ID"),
            preview_id: get_str("PREVIEW_ID"),
            payload: get_str("PAYLOAD"),
            status: get_str("STATUS"),
            attempts: get_int("ATTEMPTS"),
            last_error: get_opt_str("LAST_ERROR"),
            created_at: parse_time("CREATED_AT"),
            updated_at: parse_time("UPDATED_AT"),
        }))
    }

    /// 更新Worker结果处理状态
    async fn update_worker_result_status(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<()> {
        let sql = if last_error.is_some() {
            "UPDATE WORKER_RESULTS_QUEUE \
             SET STATUS = ?, LAST_ERROR = ?, ATTEMPTS = ATTEMPTS + 1, UPDATED_AT = CURRENT_TIMESTAMP \
             WHERE ID = ?"
        } else {
            "UPDATE WORKER_RESULTS_QUEUE \
             SET STATUS = ?, UPDATED_AT = CURRENT_TIMESTAMP \
             WHERE ID = ?"
        };

        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                let mut params = vec![status.to_string()];
                if let Some(err) = last_error {
                    params.push(err.to_string());
                }
                params.push(id.to_string());
                conn.execute_with_params(sql, params).await?;
            }
        }
        Ok(())
    }

    // 材料下载队列相关方法

    /// 入队材料下载任务
    async fn enqueue_material_download(&self, preview_id: &str, payload: &str) -> Result<()> {
        let sql = "INSERT INTO MATERIAL_DOWNLOAD_QUEUE (ID, PREVIEW_ID, PAYLOAD, STATUS, ATTEMPTS, CREATED_AT, UPDATED_AT) \
                   VALUES (?, ?, ?, 'pending', 0, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)";
        let id = Uuid::new_v4().to_string();

        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                conn.execute_with_params(
                    sql,
                    vec![id, preview_id.to_string(), payload.to_string()],
                )
                .await?;
            }
        }
        Ok(())
    }

    /// 拉取待处理的材料下载任务
    async fn fetch_pending_material_downloads(
        &self,
        limit: u32,
    ) -> Result<Vec<MaterialDownloadQueueRecord>> {
        let sql = format!(
            "SELECT ID, PREVIEW_ID, PAYLOAD, STATUS, ATTEMPTS, LAST_ERROR, CREATED_AT, UPDATED_AT \
             FROM MATERIAL_DOWNLOAD_QUEUE \
             WHERE STATUS = 'pending' \
             ORDER BY CREATED_AT ASC \
             LIMIT {}",
            limit
        );

        let rows = match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => conn.query_rows(&sql, None).await?,
        };

        let mut results = Vec::new();
        for row in rows {
            let get_str = |key: &str| -> String {
                row.get(key)
                    .or_else(|| row.get(&key.to_lowercase()))
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let get_opt_str = |key: &str| -> Option<String> {
                row.get(key)
                    .or_else(|| row.get(&key.to_lowercase()))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            };
            let get_int = |key: &str| -> i32 {
                row.get(key)
                    .or_else(|| row.get(&key.to_lowercase()))
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0) as i32
            };
            let parse_time = |key: &str| -> DateTime<Utc> {
                let s = get_str(key);
                if s.is_empty() {
                    return Utc::now();
                }
                chrono::NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
                    .map(|dt| DateTime::from_utc(dt, Utc))
                    .unwrap_or_else(|_| Utc::now())
            };

            results.push(MaterialDownloadQueueRecord {
                id: get_str("ID"),
                preview_id: get_str("PREVIEW_ID"),
                payload: get_str("PAYLOAD"),
                status: get_str("STATUS"),
                attempts: get_int("ATTEMPTS"),
                last_error: get_opt_str("LAST_ERROR"),
                created_at: parse_time("CREATED_AT"),
                updated_at: parse_time("UPDATED_AT"),
            });
        }
        Ok(results)
    }

    /// 更新材料下载任务状态
    async fn update_material_download_status(
        &self,
        id: &str,
        status: &str,
        last_error: Option<&str>,
    ) -> Result<()> {
        let sql = if last_error.is_some() {
            "UPDATE MATERIAL_DOWNLOAD_QUEUE \
             SET STATUS = ?, LAST_ERROR = ?, ATTEMPTS = ATTEMPTS + 1, UPDATED_AT = CURRENT_TIMESTAMP \
             WHERE ID = ?"
        } else {
            "UPDATE MATERIAL_DOWNLOAD_QUEUE \
             SET STATUS = ?, UPDATED_AT = CURRENT_TIMESTAMP \
             WHERE ID = ?"
        };

        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                let mut params = vec![status.to_string()];
                if let Some(err) = last_error {
                    params.push(err.to_string());
                }
                params.push(id.to_string());
                conn.execute_with_params(sql, params).await?;
            }
        }
        Ok(())
    }

    async fn update_material_download_payload(&self, id: &str, payload: &str) -> Result<()> {
        let sql = "UPDATE MATERIAL_DOWNLOAD_QUEUE \
                   SET PAYLOAD = ?, UPDATED_AT = CURRENT_TIMESTAMP \
                   WHERE ID = ?";

        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                conn.execute_with_params(sql, vec![payload.to_string(), id.to_string()])
                    .await?;
            }
        }
        Ok(())
    }

    async fn get_download_cache_token(
        &self,
        url: &str,
    ) -> Result<Option<crate::db::traits::MaterialDownloadCacheEntry>> {
        let sql = "SELECT URL, TOKEN, EXPIRES_AT FROM MATERIAL_DOWNLOAD_CACHE \
                   WHERE URL = ? AND EXPIRES_AT > CURRENT_TIMESTAMP";
        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                let rows = conn.query_rows(sql, Some(vec![url.to_string()])).await?;
                if let Some(row) = rows.into_iter().next() {
                    let url = as_str(row.get("URL")).unwrap_or_default();
                    let token = as_str(row.get("TOKEN")).unwrap_or_default();
                    let expires_at = parse_dt(row.get("EXPIRES_AT"));
                    return Ok(Some(crate::db::traits::MaterialDownloadCacheEntry {
                        url,
                        token,
                        expires_at,
                    }));
                }
            }
        }
        Ok(None)
    }

    async fn upsert_download_cache_token(
        &self,
        url: &str,
        token: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        let sql = "MERGE INTO MATERIAL_DOWNLOAD_CACHE t \
                   USING (SELECT ? AS URL, ? AS TOKEN, (CURRENT_TIMESTAMP + ?/86400) AS EXPIRES_AT FROM DUAL) s \
                   ON (t.URL = s.URL) \
                   WHEN MATCHED THEN UPDATE SET t.TOKEN = s.TOKEN, t.EXPIRES_AT = s.EXPIRES_AT, t.UPDATED_AT = CURRENT_TIMESTAMP \
                   WHEN NOT MATCHED THEN INSERT (URL, TOKEN, EXPIRES_AT, UPDATED_AT) VALUES (s.URL, s.TOKEN, s.EXPIRES_AT, CURRENT_TIMESTAMP)";
        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                conn.execute_with_params(
                    sql,
                    vec![url.to_string(), token.to_string(), ttl_secs.to_string()],
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn create_preview_share_token(
        &self,
        preview_id: &str,
        token: &str,
        format: &str,
        ttl_secs: i64,
    ) -> Result<()> {
        let sql = "INSERT INTO PREVIEW_SHARE_TOKENS (TOKEN, PREVIEW_ID, FORMAT, EXPIRES_AT, CREATED_AT) \
                   VALUES (?, ?, ?, (CURRENT_TIMESTAMP + ?/86400), CURRENT_TIMESTAMP)";
        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                conn.execute_with_params(
                    sql,
                    vec![
                        token.to_string(),
                        preview_id.to_string(),
                        format.to_string(),
                        ttl_secs.to_string(),
                    ],
                )
                .await?;
            }
        }
        Ok(())
    }

    async fn consume_preview_share_token(
        &self,
        token: &str,
    ) -> Result<Option<crate::db::traits::PreviewShareTokenRecord>> {
        let update_sql = "UPDATE PREVIEW_SHARE_TOKENS \
                          SET USED_AT = CURRENT_TIMESTAMP \
                          WHERE TOKEN = ? AND USED_AT IS NULL AND EXPIRES_AT > CURRENT_TIMESTAMP";

        let affected = match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                conn.execute_with_params(update_sql, vec![token.to_string()])
                    .await?
            }
        };

        if affected == 0 {
            return Ok(None);
        }

        let select_sql =
            "SELECT TOKEN, PREVIEW_ID, FORMAT, EXPIRES_AT, USED_AT FROM PREVIEW_SHARE_TOKENS WHERE TOKEN = ?";

        match &self.connection {
            #[cfg(feature = "dm_go")]
            DmConnectionType::Go(conn) => {
                let rows = conn
                    .query_rows(select_sql, Some(vec![token.to_string()]))
                    .await?;
                if let Some(row) = rows.into_iter().next() {
                    let token = as_str(row.get("TOKEN")).unwrap_or_default();
                    let preview_id = as_str(row.get("PREVIEW_ID")).unwrap_or_default();
                    let format =
                        as_str(row.get("FORMAT")).unwrap_or_else(|| "pdf".to_string());
                    let expires_at = parse_dt(row.get("EXPIRES_AT"));
                    let used_at = parse_dt_opt(row.get("USED_AT"));
                    return Ok(Some(crate::db::traits::PreviewShareTokenRecord {
                        token,
                        preview_id,
                        format,
                        expires_at,
                        used_at,
                    }));
                }
            }
        }

        Ok(None)
    }

    // 监控系统相关方法

    async fn find_monitor_user_by_username(&self, username: &str) -> Result<Option<MonitorUser>> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "SELECT ID, USERNAME, ROLE, LAST_LOGIN_AT, LOGIN_COUNT, IS_ACTIVE \
                           FROM MONITOR_USERS WHERE USERNAME = ? AND IS_ACTIVE = 1";
                let rows = conn
                    .query_rows(sql, Some(vec![username.to_string()]))
                    .await?;
                if let Some(row) = rows.get(0) {
                    return Ok(Some(map_monitor_user_row(row)));
                }
                return Ok(None);
            }
            Err(anyhow!("DM-Go monitor user lookup不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = username;
            Err(anyhow!("DM-Go monitor user lookup不可用"))
        }
    }

    async fn find_monitor_user_by_id(&self, user_id: &str) -> Result<Option<MonitorUser>> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "SELECT ID, USERNAME, ROLE, LAST_LOGIN_AT, LOGIN_COUNT, IS_ACTIVE \
                           FROM MONITOR_USERS WHERE ID = ?";
                let rows = conn
                    .query_rows(sql, Some(vec![user_id.to_string()]))
                    .await?;
                if let Some(row) = rows.get(0) {
                    return Ok(Some(map_monitor_user_row(row)));
                }
                return Ok(None);
            }
            Err(anyhow!("DM-Go monitor user lookup不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = user_id;
            Err(anyhow!("DM-Go monitor user lookup不可用"))
        }
    }

    async fn list_monitor_users(&self) -> Result<Vec<MonitorUser>> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "SELECT ID, USERNAME, ROLE, LAST_LOGIN_AT, LOGIN_COUNT, IS_ACTIVE \
                           FROM MONITOR_USERS ORDER BY CREATED_AT ASC";
                let rows = conn.query_rows(sql, None).await?;
                let mut users = Vec::with_capacity(rows.len());
                for row in rows.iter() {
                    users.push(map_monitor_user_row(row));
                }
                return Ok(users);
            }
            Err(anyhow!("DM-Go monitor user列表不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        Err(anyhow!("DM-Go monitor user列表不可用"))
    }

    async fn create_monitor_user(
        &self,
        id: &str,
        username: &str,
        password_hash: &str,
        role: &str,
        now: &str,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "INSERT INTO MONITOR_USERS \
                           (ID, USERNAME, PASSWORD_HASH, ROLE, LOGIN_COUNT, CREATED_AT, UPDATED_AT, IS_ACTIVE) \
                           VALUES (?, ?, ?, ?, 0, ?, ?, 1)";
                let params = vec![
                    id.to_string(),
                    username.to_string(),
                    password_hash.to_string(),
                    role.to_string(),
                    now.to_string(),
                    now.to_string(),
                ];
                conn.execute_update(sql, Some(params)).await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor user创建不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = (id, username, password_hash, role, now);
            Err(anyhow!("DM-Go monitor user创建不可用"))
        }
    }

    async fn update_monitor_user_role(&self, user_id: &str, role: &str, now: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "UPDATE MONITOR_USERS SET ROLE = ?, UPDATED_AT = ? WHERE ID = ?";
                let params = vec![role.to_string(), now.to_string(), user_id.to_string()];
                conn.execute_update(sql, Some(params)).await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor user角色更新不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = (user_id, role, now);
            Err(anyhow!("DM-Go monitor user角色更新不可用"))
        }
    }

    async fn update_monitor_user_password(
        &self,
        user_id: &str,
        password_hash: &str,
        now: &str,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "UPDATE MONITOR_USERS SET PASSWORD_HASH = ?, UPDATED_AT = ? \
                           WHERE ID = ?";
                let params = vec![
                    password_hash.to_string(),
                    now.to_string(),
                    user_id.to_string(),
                ];
                conn.execute_update(sql, Some(params)).await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor user密码更新不可用"))
        }

        #[cfg(not(feature = "dm_go"))]
        {
            let _ = (user_id, password_hash, now);
            Err(anyhow!("DM-Go monitor user密码更新不可用"))
        }
    }

    async fn set_monitor_user_active(
        &self,
        user_id: &str,
        is_active: bool,
        now: &str,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "UPDATE MONITOR_USERS SET IS_ACTIVE = ?, UPDATED_AT = ? WHERE ID = ?";
                let params = vec![
                    if is_active {
                        "1".to_string()
                    } else {
                        "0".to_string()
                    },
                    now.to_string(),
                    user_id.to_string(),
                ];
                conn.execute_update(sql, Some(params)).await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor user状态更新不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = (user_id, is_active, now);
            Err(anyhow!("DM-Go monitor user状态更新不可用"))
        }
    }

    async fn count_active_monitor_admins(&self) -> Result<i64> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql =
                    "SELECT COUNT(*) AS COUNT FROM MONITOR_USERS WHERE ROLE = 'admin' AND IS_ACTIVE = 1";
                let rows = conn.query_rows(sql, None).await?;
                let count = rows
                    .get(0)
                    .and_then(|row| as_i64(row.get("COUNT")))
                    .unwrap_or(0);
                return Ok(count);
            }
            Err(anyhow!("DM-Go monitor user统计不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        Err(anyhow!("DM-Go monitor user统计不可用"))
    }

    async fn get_monitor_user_password_hash(&self, user_id: &str) -> Result<String> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "SELECT PASSWORD_HASH FROM MONITOR_USERS WHERE ID = ?";
                let rows = conn
                    .query_rows(sql, Some(vec![user_id.to_string()]))
                    .await?;
                if let Some(row) = rows.get(0) {
                    return Ok(as_str(row.get("PASSWORD_HASH")).unwrap_or_default());
                }
                return Err(anyhow!("用户不存在"));
            }
            Err(anyhow!("DM-Go monitor user密码查询不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = user_id;
            Err(anyhow!("DM-Go monitor user密码查询不可用"))
        }
    }

    async fn create_monitor_session(
        &self,
        session_id: &str,
        user_id: &str,
        ip: &str,
        user_agent: &str,
        created_at: &str,
        expires_at: &str,
    ) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "INSERT INTO MONITOR_SESSIONS \
                           (ID, USER_ID, IP_ADDRESS, USER_AGENT, CREATED_AT, EXPIRES_AT, LAST_ACTIVITY, IS_ACTIVE) \
                           VALUES (?, ?, ?, ?, ?, ?, ?, 1)";
                let params = vec![
                    session_id.to_string(),
                    user_id.to_string(),
                    ip.to_string(),
                    user_agent.to_string(),
                    created_at.to_string(),
                    expires_at.to_string(),
                    created_at.to_string(),
                ];
                conn.execute_update(sql, Some(params)).await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor session创建不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = (session_id, user_id, ip, user_agent, created_at, expires_at);
            Err(anyhow!("DM-Go monitor session创建不可用"))
        }
    }

    async fn find_monitor_session_by_id(&self, session_id: &str) -> Result<Option<MonitorSession>> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "
                    SELECT 
                        s.ID AS SESSION_ID,
                        s.USER_ID AS SESSION_USER_ID,
                        s.EXPIRES_AT AS SESSION_EXPIRES_AT,
                        s.IP_ADDRESS AS SESSION_IP_ADDRESS,
                        u.ID AS USER_ID,
                        u.USERNAME AS USER_USERNAME,
                        u.ROLE AS USER_ROLE,
                        u.LAST_LOGIN_AT AS USER_LAST_LOGIN_AT,
                        u.LOGIN_COUNT AS USER_LOGIN_COUNT,
                        u.IS_ACTIVE AS USER_IS_ACTIVE
                    FROM MONITOR_SESSIONS s
                    JOIN MONITOR_USERS u ON s.USER_ID = u.ID
                    WHERE s.ID = ? AND s.IS_ACTIVE = 1
                ";
                let rows = conn
                    .query_rows(sql, Some(vec![session_id.to_string()]))
                    .await?;
                if let Some(row) = rows.get(0) {
                    return Ok(Some(map_monitor_session_row(row)?));
                }
                return Ok(None);
            }
            Err(anyhow!("DM-Go monitor session查询不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = session_id;
            Err(anyhow!("DM-Go monitor session查询不可用"))
        }
    }

    async fn update_monitor_login_info(&self, user_id: &str, now: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "UPDATE MONITOR_USERS \
                           SET LAST_LOGIN_AT = ?, LOGIN_COUNT = LOGIN_COUNT + 1, UPDATED_AT = ? \
                           WHERE ID = ?";
                let params = vec![now.to_string(), now.to_string(), user_id.to_string()];
                conn.execute_update(sql, Some(params)).await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor user登录信息更新不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = (user_id, now);
            Err(anyhow!("DM-Go monitor user登录信息更新不可用"))
        }
    }

    async fn update_monitor_session_activity(&self, session_id: &str, now: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "UPDATE MONITOR_SESSIONS SET LAST_ACTIVITY = ? WHERE ID = ?";
                let params = vec![now.to_string(), session_id.to_string()];
                conn.execute_update(sql, Some(params)).await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor session活动更新不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = (session_id, now);
            Err(anyhow!("DM-Go monitor session活动更新不可用"))
        }
    }

    async fn delete_monitor_session(&self, session_id: &str) -> Result<()> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql = "UPDATE MONITOR_SESSIONS SET IS_ACTIVE = 0 WHERE ID = ?";
                conn.execute_update(sql, Some(vec![session_id.to_string()]))
                    .await?;
                return Ok(());
            }
            Err(anyhow!("DM-Go monitor session删除不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = session_id;
            Err(anyhow!("DM-Go monitor session删除不可用"))
        }
    }

    async fn cleanup_expired_monitor_sessions(&self, now: &str) -> Result<u64> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql =
                    "UPDATE MONITOR_SESSIONS SET IS_ACTIVE = 0 WHERE EXPIRES_AT < ? AND IS_ACTIVE = 1";
                let affected = conn
                    .execute_update(sql, Some(vec![now.to_string()]))
                    .await?;
                return Ok(affected);
            }
            Err(anyhow!("DM-Go monitor session清理不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = now;
            Err(anyhow!("DM-Go monitor session清理不可用"))
        }
    }

    async fn get_active_monitor_sessions_count(&self, now: &str) -> Result<i64> {
        #[cfg(feature = "dm_go")]
        {
            if let DmConnectionType::Go(conn) = &self.connection {
                let sql =
                    "SELECT COUNT(*) AS COUNT FROM MONITOR_SESSIONS WHERE EXPIRES_AT > ? AND IS_ACTIVE = 1";
                let rows = conn.query_rows(sql, Some(vec![now.to_string()])).await?;
                let count = rows
                    .get(0)
                    .and_then(|row| as_i64(row.get("COUNT")))
                    .unwrap_or(0);
                return Ok(count);
            }
            Err(anyhow!("DM-Go monitor session统计不可用"))
        }
        #[cfg(not(feature = "dm_go"))]
        {
            let _ = now;
            Err(anyhow!("DM-Go monitor session统计不可用"))
        }
    }
}

/// 映射材料文件行数据
#[cfg(feature = "dm_go")]
fn map_material_file_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<MaterialFileRecord> {
    Ok(MaterialFileRecord {
        id: as_str(row.get("ID")).unwrap_or_default(),
        preview_id: as_str(row.get("PREVIEW_ID")).unwrap_or_default(),
        material_code: as_str(row.get("MATERIAL_CODE")).unwrap_or_default(),
        attachment_name: opt_str(row.get("ATTACHMENT_NAME")),
        source_url: opt_str(row.get("SOURCE_URL")),
        stored_original_key: as_str(row.get("STORED_ORIGINAL_KEY")).unwrap_or_default(),
        stored_processed_keys: opt_str(row.get("STORED_PROCESSED_KEYS")),
        mime_type: opt_str(row.get("MIME_TYPE")),
        size_bytes: as_i64(row.get("SIZE_BYTES")),
        checksum_sha256: opt_str(row.get("CHECKSUM_SHA256")),
        ocr_text_key: opt_str(row.get("OCR_TEXT_KEY")),
        ocr_text_length: as_i64(row.get("OCR_TEXT_LENGTH")),
        status: as_str(row.get("STATUS")).unwrap_or_else(|| "pending".to_string()),
        error_message: opt_str(row.get("ERROR_MESSAGE")),
        created_at: parse_dt(row.get("CREATED_AT")),
        updated_at: parse_dt(row.get("UPDATED_AT")),
    })
}

#[cfg(feature = "dm_go")]
fn map_cached_material_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<CachedMaterialRecord> {
    let status = as_str(row.get("UPLOAD_STATUS")).unwrap_or_else(|| "downloaded".to_string());
    Ok(CachedMaterialRecord {
        id: as_str(row.get("ID")).unwrap_or_default(),
        preview_id: as_str(row.get("PREVIEW_ID")).unwrap_or_default(),
        material_code: as_str(row.get("MATERIAL_CODE")).unwrap_or_default(),
        attachment_index: as_i64(row.get("ATTACHMENT_INDEX")).unwrap_or(0) as i32,
        token: as_str(row.get("TOKEN")).unwrap_or_default(),
        local_path: as_str(row.get("LOCAL_PATH")).unwrap_or_default(),
        upload_status: CachedMaterialStatus::from_str(&status),
        oss_key: opt_str(row.get("OSS_KEY")),
        last_error: opt_str(row.get("LAST_ERROR")),
        file_size: as_i64(row.get("FILE_SIZE")),
        checksum_sha256: opt_str(row.get("CHECKSUM_SHA256")),
        created_at: parse_dt(row.get("CREATED_AT")),
        updated_at: parse_dt(row.get("UPDATED_AT")),
    })
}

#[cfg(feature = "dm_go")]
fn map_outbox_row(
    row: &std::collections::HashMap<String, serde_json::Value>,
) -> Result<OutboxEvent> {
    Ok(OutboxEvent {
        id: as_str(row.get("ID")).unwrap_or_default(),
        table_name: as_str(row.get("TABLE_NAME")).unwrap_or_default(),
        op_type: as_str(row.get("OP_TYPE")).unwrap_or_default(),
        pk_value: as_str(row.get("PK_VALUE")).unwrap_or_default(),
        idempotency_key: as_str(row.get("IDEMPOTENCY_KEY")).unwrap_or_default(),
        payload: as_str(row.get("PAYLOAD")).unwrap_or_default(),
        created_at: parse_dt(row.get("CREATED_AT")),
        applied_at: parse_dt_opt(row.get("APPLIED_AT")),
        retries: as_i64(row.get("RETRIES")).unwrap_or(0) as i32,
        last_error: opt_str(row.get("LAST_ERROR")),
    })
}
