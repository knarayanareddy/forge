use aether_daemon::gateway::mock_server::handle_mock_slack_post;
use aether_daemon::gateway::{GatewayChannel, GatewayChannelType, GatewayOutcome, GatewayRouter};
use aether_daemon::task_runner::run_gateway_inbound;
use aether_permissions::GatewayGrant;
use std::fs;
use tempfile::tempdir;

#[derive(serde::Deserialize)]
struct Gate01Fixture {
    channel_id: String,
    session_id: String,
    channel_type: String,
    user_text: String,
    slack_payload: serde_json::Value,
    loop_plan: Vec<serde_json::Value>,
}

pub fn gate01_fixture_ready() -> Result<(), String> {
    let raw = include_str!("../fixtures/gate01_slack.json");
    let fixture: Gate01Fixture = serde_json::from_str(raw)
        .map_err(|e| format!("GATE-01 fixture parse failed: {}", e))?;
    if fixture.channel_id.is_empty() || fixture.loop_plan.is_empty() {
        return Err("GATE-01 fixture missing channel_id or loop_plan".into());
    }
    Ok(())
}

pub async fn test_gate01_impl(conn: &rusqlite::Connection) -> Result<(), String> {
    let raw = include_str!("../fixtures/gate01_slack.json");
    let fixture: Gate01Fixture = serde_json::from_str(raw)
        .map_err(|e| format!("fixture parse: {}", e))?;

    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'GATE-01', 'active')",
        rusqlite::params![fixture.session_id],
    )
    .map_err(|e| e.to_string())?;

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let workspace_str = workspace.to_string_lossy().to_string();

    let task_prompt = serde_json::json!({ "loop": fixture.loop_plan }).to_string();
    let channel_type = GatewayChannelType::parse(&fixture.channel_type)
        .ok_or_else(|| format!("unknown channel_type: {}", fixture.channel_type))?;

    let channel = GatewayChannel {
        channel_id: fixture.channel_id.clone(),
        channel_type,
        session_id: fixture.session_id.clone(),
        task_prompt: task_prompt.clone(),
        workspace_path: Some(workspace_str.clone()),
        enabled: true,
    };

    GatewayRouter::register_channel(conn, &channel).map_err(|e| e.to_string())?;

    let payload = serde_json::to_string(&fixture.slack_payload).map_err(|e| e.to_string())?;

    // Forbidden: inbound without GatewayGrant must deny + audit.
    let denied = handle_mock_slack_post(conn, &fixture.channel_id, &payload)?;
    if !matches!(denied, GatewayOutcome::Denied { .. }) {
        return Err("Expected inbound without grant to be denied".into());
    }

    let denied_audit: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log
             WHERE tool_name = 'gateway_inbound' AND decision = 'denied'
               AND arguments_json LIKE ?1",
            rusqlite::params![format!("%{}%", fixture.channel_id)],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if denied_audit < 1 {
        return Err("Expected denied gateway_inbound audit entry".into());
    }

    GatewayGrant::grant(
        conn,
        &fixture.channel_id,
        &fixture.session_id,
        channel_type.as_str(),
    )
    .map_err(|e| e.to_string())?;

    let accepted = handle_mock_slack_post(conn, &fixture.channel_id, &payload)?;
    let normalized = match accepted {
        GatewayOutcome::Accepted {
            normalized_prompt, ..
        } => normalized_prompt,
        GatewayOutcome::Denied { reason, .. } => {
            return Err(format!("Expected granted inbound to pass, got denied: {}", reason));
        }
    };

    run_gateway_inbound(conn, &channel, &normalized).map_err(|e| e.to_string())?;

    let marker = workspace.join("gate_marker.txt");
    if !marker.exists() {
        return Err("GATE-01 marker file not created by loop plan".into());
    }
    let content = fs::read_to_string(&marker).map_err(|e| e.to_string())?;
    if !content.contains("GATE-01-verified") {
        return Err(format!("Unexpected marker content: {}", content));
    }

    let response = workspace.join("gate_response.txt");
    if !response.exists() {
        return Err("GATE-01 response artifact missing".into());
    }
    if !fs::read_to_string(&response)
        .map_err(|e| e.to_string())?
        .contains(&fixture.user_text)
    {
        return Err("Response artifact missing normalized user text".into());
    }

    let approved_audit: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log
             WHERE tool_name = 'gateway_inbound' AND decision = 'approved'
               AND arguments_json LIKE ?1",
            rusqlite::params![format!("%{}%", fixture.channel_id)],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if approved_audit < 1 {
        return Err(format!(
            "Expected approved gateway_inbound audit for {}; found {}",
            fixture.channel_id, approved_audit
        ));
    }

    Ok(())
}
