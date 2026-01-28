use crate::model::{ThirdResult, Ticket, TicketId, Token};
use crate::util::WebResult;
#[cfg(feature = "reqwest")]
use crate::CLIENT;
use crate::CONFIG;
use base64::prelude::BASE64_STANDARD;
use base64::Engine;
#[cfg(feature = "reqwest")]
use reqwest::header;
use ring::hmac::{Key, HMAC_SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_sessions::Session;
use tracing::info;
use url::Url;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct User;

impl User {
    pub async fn user_save(session: Session, ticket_id: TicketId) -> anyhow::Result<WebResult> {
        session
            .insert_value(&ticket_id.ticket_id, Value::Null)
            .await?;
        info!("Save user {} to session", ticket_id.ticket_id);
        Ok(WebResult::ok(()))
    }

    pub async fn get_token_by_ticket(ticket: Ticket) -> anyhow::Result<WebResult> {
        let url = Url::parse(&CONFIG.login.access_token_url)?;
        let result = Self::sign(url, json!(ticket).to_string()).await?;
        if let Value::Object(map) = result.data {
            if let Some(token) = map.get("accessToken") {
                info!("Get token from session: {}", token.to_string());
                return Ok(WebResult::ok(token));
            }
        };
        info!("Get token fail");
        Ok(WebResult::ok(Value::Null))
    }

    pub async fn get_user_by_token(session: Session, token: Token) -> anyhow::Result<WebResult> {
        if let Ok(Some(user_info)) = session.get_value(&token.token).await {
            info!(
                "Get user info from token: {}, user: {}",
                token.token, user_info
            );
            return Ok(WebResult::ok(user_info));
        }
        let url = Url::parse(&CONFIG.login.get_user_info_url)?;
        let result = Self::sign(url, json!(token).to_string()).await?;
        session.insert(&token.token, &result.data).await?;
        info!(
            "Get user info from token: {}, user: {}",
            token.token, result.data
        );
        Ok(WebResult::ok(result.data))
    }

    async fn sign(url: Url, body: String) -> anyhow::Result<ThirdResult> {
        const X_BG_HMAC_ACCESS_KEY: &str = "X-BG-HMAC-ACCESS-KEY";
        const X_BG_HMAC_SIGNATURE: &str = "X-BG-HMAC-SIGNATURE";
        const X_BG_HMAC_ALGORITHM: &str = "X-BG-HMAC-ALGORITHM";
        const X_BG_DATE_TIME: &str = "X-BG-DATE-TIME";
        const DEFAULT_HMAC_SIGNATURE: &str = "hmac-sha256";

        let mut query = url
            .query_pairs()
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>();
        query.sort();
        let query = query.join("&");

        let date_time = chrono::Utc::now()
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        let login = &CONFIG.login;
        let sign_str = format!(
            "POST\n{}\n{}\n{}\n{}\n",
            url.path(),
            query,
            login.access_key,
            date_time
        );
        let sign = ring::hmac::sign(
            &Key::new(HMAC_SHA256, login.secret_key.as_bytes()),
            sign_str.as_bytes(),
        );
        let sign = BASE64_STANDARD.encode(sign);

        info!("Sign: date: {}, sign: {}", date_time, sign);

        #[cfg(feature = "reqwest")]
        {
            // 检查HTTP客户端是否可用
            let client = match CLIENT.as_ref() {
                Some(client) => client,
                None => {
                    return Err(anyhow::anyhow!("HTTP客户端不可用，无法发送请求"));
                }
            };

            let response = client
                .post(url)
                .header(header::CONTENT_TYPE, "application/json")
                .header(X_BG_HMAC_ACCESS_KEY, &login.access_key)
                .header(X_BG_HMAC_ALGORITHM, DEFAULT_HMAC_SIGNATURE)
                .header(X_BG_HMAC_SIGNATURE, sign)
                .header(X_BG_DATE_TIME, date_time)
                .body(body)
                .send()
                .await?;

            let result = response.json::<ThirdResult>().await?;
            Ok(result)
        }

        #[cfg(not(feature = "reqwest"))]
        {
            // MUSL环境下不支持HTTP请求，返回错误
            tracing::warn!("HTTP请求功能在MUSL环境下未启用");
            Err(anyhow::anyhow!("HTTP请求功能未启用"))
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerifyParam {
    pub goto: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VerifyTicket {
    pub ticket: String,
    #[serde(rename = "userid")]
    pub user_id: String,
}
