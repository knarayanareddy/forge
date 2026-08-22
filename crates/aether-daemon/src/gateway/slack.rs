use crate::webhook_auth::{constant_time_eq, env_channel_suffix, hmac_sha256_hex};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

const SLACK_SIGNATURE_MAX_AGE_SECS: i64 = 5 * 60;

/// Parse a mock Slack Events API payload into user text.
pub fn parse_slack_payload(body: &str) -> Result<String, String> {
    let value: Value = serde_json::from_str(body).map_err(|e| format!("invalid slack json: {}", e))?;
    value
        .pointer("/event/text")
        .or_else(|| value.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "slack payload missing event.text".into())
}

fn signing_secret(channel_id: &str) -> Option<String> {
    let per_channel = format!(
        "AETHER_SLACK_SIGNING_SECRET_{}",
        env_channel_suffix(channel_id)
    );
    std::env::var(per_channel)
        .ok()
        .filter(|secret| !secret.is_empty())
        .or_else(|| {
            std::env::var("AETHER_SLACK_SIGNING_SECRET")
                .ok()
                .filter(|secret| !secret.is_empty())
        })
}

pub fn verify_slack_signature(
    channel_id: &str,
    timestamp: Option<&str>,
    signature: Option<&str>,
    body: &str,
) -> Result<(), String> {
    let secret = signing_secret(channel_id)
        .ok_or_else(|| "slack signing secret is not configured; webhook disabled".to_string())?;
    let timestamp = timestamp
        .ok_or_else(|| "missing X-Slack-Request-Timestamp".to_string())?
        .parse::<i64>()
        .map_err(|_| "invalid X-Slack-Request-Timestamp".to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_secs() as i64;
    if now.saturating_sub(timestamp).abs() > SLACK_SIGNATURE_MAX_AGE_SECS {
        return Err("stale Slack webhook timestamp".into());
    }
    let base = format!("v0:{timestamp}:{body}");
    let expected = format!("v0={}", hmac_sha256_hex(secret.as_bytes(), base.as_bytes()));
    if signature.is_some_and(|provided| constant_time_eq(provided, &expected)) {
        Ok(())
    } else {
        Err("invalid Slack webhook signature".into())
    }
}

/// Normalize inbound Slack text into a structured loop prompt for run_task.
pub fn normalize_message(task_prompt: &str, user_text: &str) -> String {
    serde_json::json!({
        "gateway": {
            "channel": "slack",
            "user_text": user_text,
        },
        "loop_plan": serde_json::from_str::<Value>(task_prompt)
            .ok()
            .and_then(|v| v.get("loop").cloned())
            .unwrap_or_else(|| serde_json::json!([])),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mock_slack_event() {
        let body = r#"{"event":{"type":"message","text":"ping from slack"}}"#;
        assert_eq!(parse_slack_payload(body).unwrap(), "ping from slack");
    }
}
