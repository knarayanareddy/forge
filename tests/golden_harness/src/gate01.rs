use aether_core::ModelRouter;
use aether_daemon::gateway::mock_server::accept_one_mock_request;
use aether_daemon::gateway::{GatewayChannel, GatewayChannelType, GatewayOutcome, GatewayRouter};
use aether_daemon::task_runner::run_gateway_inbound;
use aether_daemon::DaemonState;
use aether_db::Database;
use aether_permissions::GatewayGrant;
use std::fs;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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

pub async fn test_gate01_impl(db: &Database) -> Result<(), String> {
    let raw = include_str!("../fixtures/gate01_slack.json");
    let fixture: Gate01Fixture = serde_json::from_str(raw)
        .map_err(|e| format!("fixture parse: {}", e))?;

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

    let payload = serde_json::to_string(&fixture.slack_payload).map_err(|e| e.to_string())?;

    {
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES (?1, 'GATE-01', 'active')",
            rusqlite::params![fixture.session_id],
        )
        .map_err(|e| e.to_string())?;

        GatewayRouter::register_channel(&conn, &channel).map_err(|e| e.to_string())?;

        // Forbidden: inbound without GatewayGrant must deny + audit (direct grant gate).
        let denied = aether_daemon::gateway::mock_server::handle_mock_slack_post(
            &conn,
            &fixture.channel_id,
            &payload,
        )?;
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
    }

    // Localhost mock Slack server must also deny before grant (TCP round-trip).
    let denied_status = post_mock_slack_tcp(db, &fixture.channel_id, &payload).await?;
    if denied_status != 403 {
        return Err(format!(
            "Expected mock server deny without grant, got HTTP {}",
            denied_status
        ));
    }

    {
        let conn = db.conn();
        GatewayGrant::grant(
            &conn,
            &fixture.channel_id,
            &fixture.session_id,
            channel_type.as_str(),
        )
        .map_err(|e| e.to_string())?;

        let accepted = aether_daemon::gateway::mock_server::handle_mock_slack_post(
            &conn,
            &fixture.channel_id,
            &payload,
        )?;
        let normalized = match accepted {
            GatewayOutcome::Accepted {
                normalized_prompt, ..
            } => normalized_prompt,
            GatewayOutcome::Denied { reason, .. } => {
                return Err(format!("Expected granted inbound to pass, got denied: {}", reason));
            }
        };

        run_gateway_inbound(&conn, &channel, &normalized).map_err(|e| e.to_string())?;
    }

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

    // Granted inbound via localhost mock server → run_task → response artifact.
    let ok_status = post_mock_slack_tcp(db, &fixture.channel_id, &payload).await?;
    if ok_status != 200 {
        return Err(format!(
            "Expected mock server accept with grant, got HTTP {}",
            ok_status
        ));
    }
    if !response.exists() {
        return Err("GATE-01 response artifact missing after mock server round-trip".into());
    }

    {
        let conn = db.conn();
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
    }

    Ok(())
}

async fn post_mock_slack_tcp(
    db: &Database,
    channel_id: &str,
    body: &str,
) -> Result<u16, String> {
    let state = Arc::new(DaemonState {
        db: db.clone(),
        router: ModelRouter::new(
            aether_core::ModelBackend::OllamaMlx {
                endpoint: "http://127.0.0.1:11434".into(),
                model: "qwen2.5:3b".into(),
            },
            None,
        ),
        auth_token: String::new(),
    });

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| e.to_string())?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let state_for_handler = Arc::clone(&state);
    let accept_task = tokio::spawn(async move {
        accept_one_mock_request(listener, state_for_handler)
            .await
            .map_err(|e| e.to_string())
    });

    let path = format!("/gateway/slack/{}", channel_id);
    let req = format!(
        "POST {} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path,
        body.len(),
        body
    );

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .map_err(|e| e.to_string())?;
    stream
        .write_all(req.as_bytes())
        .await
        .map_err(|e| e.to_string())?;

    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
    accept_task
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;

    let response = String::from_utf8_lossy(&buf[..n]);
    response
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("invalid HTTP response: {}", response))
}
