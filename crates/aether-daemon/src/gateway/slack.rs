use serde_json::Value;

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
