use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

use crate::gateway::inbound;
use crate::gateway::GatewayChannelType;
use crate::DaemonState;

const TELEGRAM_API: &str = "https://api.telegram.org";

pub fn parse_telegram_payload(body: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid telegram json: {}", e))?;
    value
        .pointer("/message/text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "telegram payload missing message.text".into())
}

pub fn extract_chat_id(body: &str) -> Result<i64, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid telegram json: {}", e))?;
    value
        .pointer("/message/chat/id")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "telegram payload missing message.chat.id".into())
}

pub fn normalize_message(task_prompt: &str, user_text: &str) -> String {
    serde_json::json!({
        "gateway": {
            "channel": "telegram",
            "user_text": user_text,
        },
        "loop_plan": serde_json::from_str::<Value>(task_prompt)
            .ok()
            .and_then(|v| v.get("loop").cloned())
            .unwrap_or_else(|| serde_json::json!([])),
    })
    .to_string()
}

fn gateway_token_env_key(channel_id: &str) -> String {
    format!(
        "AETHER_GATEWAY_TOKEN_{}",
        channel_id.to_ascii_uppercase().replace('-', "_")
    )
}

pub fn resolve_bot_token(channel_id: &str) -> Result<Option<String>, String> {
    let per_channel = gateway_token_env_key(channel_id);
    if let Ok(token) = std::env::var(&per_channel) {
        if !token.is_empty() {
            return Ok(Some(token));
        }
    }
    if let Ok(token) = std::env::var("AETHER_TELEGRAM_BOT_TOKEN") {
        if !token.is_empty() {
            return Ok(Some(token));
        }
    }
    aether_core::load_gateway_token(channel_id)
        .map_err(|e| format!("keychain load gateway token: {}", e))
}

fn webhook_secret_env_key(channel_id: &str) -> String {
    format!(
        "AETHER_GATEWAY_WEBHOOK_SECRET_{}",
        channel_id.to_ascii_uppercase().replace('-', "_")
    )
}

pub fn verify_webhook_secret(provided: Option<&str>, channel_id: &str) -> Result<(), String> {
    let expected = std::env::var("AETHER_TELEGRAM_WEBHOOK_SECRET")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var(webhook_secret_env_key(channel_id))
                .ok()
                .filter(|s| !s.is_empty())
        });

    let Some(expected) = expected else {
        return Ok(());
    };

    if provided == Some(expected.as_str()) {
        Ok(())
    } else {
        Err("invalid telegram webhook secret".into())
    }
}

pub async fn send_message(bot_token: &str, chat_id: i64, text: &str) -> Result<(), String> {
    let url = format!("{}/bot{}/sendMessage", TELEGRAM_API, bot_token);
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&serde_json::json!({ "chat_id": chat_id, "text": text }))
        .send()
        .await
        .map_err(|e| format!("telegram sendMessage: {}", e))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "telegram sendMessage HTTP {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ))
    }
}

pub async fn run_long_poll(state: Arc<DaemonState>, channel_id: String) {
    let token = match resolve_bot_token(&channel_id) {
        Ok(Some(t)) => t,
        Ok(None) => {
            warn!(channel_id = %channel_id, "telegram long poll: bot token not configured");
            return;
        }
        Err(e) => {
            warn!(channel_id = %channel_id, error = %e, "telegram long poll: token error");
            return;
        }
    };

    info!(channel_id = %channel_id, "telegram long poll started");
    let client = reqwest::Client::new();
    let mut offset: i64 = 0;

    loop {
        let url = format!(
            "{}/bot{}/getUpdates?timeout=30&offset={}",
            TELEGRAM_API, token, offset
        );
        let resp = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                warn!(channel_id = %channel_id, error = %e, "telegram getUpdates failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let body: Value = match resp.json().await {
            Ok(v) => v,
            Err(e) => {
                warn!(channel_id = %channel_id, error = %e, "telegram getUpdates json failed");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        for update in body
            .get("result")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
        {
            if let Some(update_id) = update.get("update_id").and_then(|v| v.as_i64()) {
                offset = update_id + 1;
            }
            let update_json = update.to_string();
            match inbound::handle_inbound_and_run(
                &state,
                GatewayChannelType::Telegram,
                &channel_id,
                &update_json,
            ) {
                Ok(()) => {
                    debug!(channel_id = %channel_id, "telegram update processed");
                    if let (Ok(chat_id), Ok(text)) = (
                        extract_chat_id(&update_json),
                        parse_telegram_payload(&update_json),
                    ) {
                        let ack = format!("AetherForge received: {}", text);
                        if let Err(e) = send_message(&token, chat_id, &ack).await {
                            warn!(channel_id = %channel_id, error = %e, "telegram reply failed");
                        }
                    }
                }
                Err(msg) if msg.starts_with("denied:") => {
                    debug!(channel_id = %channel_id, reason = %msg, "telegram update denied");
                }
                Err(e) => {
                    warn!(channel_id = %channel_id, error = %e, "telegram update failed");
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mock_telegram_update() {
        let body = r#"{"update_id":1,"message":{"chat":{"id":42},"text":"ping from telegram"}}"#;
        assert_eq!(parse_telegram_payload(body).unwrap(), "ping from telegram");
        assert_eq!(extract_chat_id(body).unwrap(), 42);
    }
}
