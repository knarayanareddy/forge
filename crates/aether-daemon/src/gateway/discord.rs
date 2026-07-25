use serde_json::Value;

pub fn parse_discord_payload(body: &str) -> Result<String, String> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| format!("invalid discord json: {}", e))?;
    value
        .get("content")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "discord payload missing content".into())
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
