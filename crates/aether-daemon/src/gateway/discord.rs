use serde_json::Value;

pub fn parse_discord_payload(body: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid discord json: {}", e))?;
    value
        .get("content")
        .or_else(|| value.pointer("/message/content"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "discord payload missing content".into())
}

pub fn extract_channel_id(body: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid discord json: {}", e))?;
    value
        .get("channel_id")
        .or_else(|| value.pointer("/message/channel_id"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else if let Some(n) = v.as_i64() {
                n.to_string()
            } else {
                v.to_string()
            }
        })
        .ok_or_else(|| "discord payload missing channel_id".into())
}

pub fn normalize_message(task_prompt: &str, user_text: &str) -> String {
    serde_json::json!({
        "gateway": {
            "channel": "discord",
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
    if let Ok(token) = std::env::var("AETHER_DISCORD_BOT_TOKEN") {
        if !token.is_empty() {
            return Ok(Some(token));
        }
    }
    aether_core::load_gateway_token(channel_id)
        .map_err(|e| format!("keychain load gateway token: {}", e))
}

pub async fn send_message(
    bot_token: &str,
    channel_id: &str,
    text: &str,
) -> Result<(), String> {
    let url = format!(
        "https://discord.com/api/v10/channels/{}/messages",
        channel_id
    );
    let resp = reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Bot {}", bot_token))
        .json(&serde_json::json!({ "content": text }))
        .send()
        .await
        .map_err(|e| format!("discord send message: {}", e))?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!(
            "discord send message HTTP {}: {}",
            resp.status(),
            resp.text().await.unwrap_or_default()
        ))
    }
}

pub fn log_webhook_ready(channel_id: &str) {
    tracing::info!(
        channel_id = %channel_id,
        "discord gateway webhook endpoint ready (POST /gateway/discord/{channel_id})"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_content_or_nested_message() {
        assert_eq!(
            parse_discord_payload(r#"{"content":"hello"}"#).unwrap(),
            "hello"
        );
        assert_eq!(
            parse_discord_payload(r#"{"message":{"content":"nested"}}"#).unwrap(),
            "nested"
        );
    }
}
