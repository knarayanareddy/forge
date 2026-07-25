use crate::gateway::{GatewayChannelType, GatewayOutcome, GatewayRouter};
use crate::task_runner::run_gateway_inbound;
use crate::DaemonState;
use aether_permissions::GatewayGrant;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Localhost mock Slack inbound server for GATE-01 (no real network).
pub async fn serve_mock_gateway(
    addr: String,
    state: Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("gateway mock server listening on {}", addr);

    loop {
        let (mut socket, peer) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_mock_connection(&mut socket, &state).await {
                tracing::warn!("gateway mock session from {} ended: {}", peer, e);
            }
        });
    }
}

/// Accept one inbound POST on a pre-bound listener (harness / unit tests).
pub async fn accept_one_mock_request(
    listener: TcpListener,
    state: Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut socket, _) = listener.accept().await?;
    handle_mock_connection(&mut socket, &state).await
}

async fn handle_mock_connection(
    socket: &mut tokio::net::TcpStream,
    state: &Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buf = vec![0u8; 8192];
    let n = socket.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buf[..n]).into_owned();

    let mut lines = request.lines();
    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "POST" {
        write_http_response(socket, 405, "Method Not Allowed").await?;
        return Ok(());
    }

    let path = parts[1];
    let channel_id = path
        .trim_start_matches("/gateway/slack/")
        .trim_matches('/');

    if channel_id.is_empty() {
        write_http_response(socket, 400, "Missing channel_id").await?;
        return Ok(());
    }

    let mut content_length = 0usize;
    for line in lines.by_ref() {
        if line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("content-length:") {
            content_length = lower
                .trim_start_matches("content-length:")
                .trim()
                .parse()
                .unwrap_or(0);
        }
    }

    let body_start = request
        .find("\r\n\r\n")
        .or_else(|| request.find("\n\n"))
        .map(|i| i + if request.contains("\r\n\r\n") { 4 } else { 2 })
        .unwrap_or(n);
    let mut body = request[body_start..].to_string();
    while body.len() < content_length && body.len() < 65536 {
        let extra = socket.read(&mut buf).await?;
        if extra == 0 {
            break;
        }
        body.push_str(&String::from_utf8_lossy(&buf[..extra]));
    }
    if content_length > 0 && body.len() > content_length {
        body.truncate(content_length);
    }

    let response = {
        let conn = state.db.conn();
        let channel = GatewayRouter::load_channel(&conn, channel_id)?
            .ok_or_else(|| format!("unknown channel {}", channel_id))?;
        let user_text = crate::gateway::slack::parse_slack_payload(body.trim())?;
        let inbound = GatewayRouter::normalize_inbound(&channel, &user_text);
        match GatewayRouter::handle_inbound(&conn, &inbound)? {
            GatewayOutcome::Denied { reason, .. } => Err(format!("denied: {}", reason)),
            GatewayOutcome::Accepted {
                normalized_prompt, ..
            } => {
                run_gateway_inbound(&conn, &channel, &normalized_prompt)?;
                GatewayGrant::audit_event(
                    &conn,
                    &channel.session_id,
                    channel_id,
                    "response",
                    &aether_permissions::PermissionDecision::Approved,
                    &serde_json::json!({"artifact": "gate_response.txt"}),
                )
                .map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    };

    match response {
        Ok(()) => write_http_response(socket, 200, "ok").await?,
        Err(msg) if msg.starts_with("denied:") => write_http_response(socket, 403, &msg).await?,
        Err(msg) => write_http_response(socket, 400, &msg).await?,
    }
    Ok(())
}

async fn write_http_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    body: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        reason,
        body.len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await?;
    Ok(())
}

pub fn handle_mock_slack_post(
    conn: &rusqlite::Connection,
    channel_id: &str,
    body: &str,
) -> Result<GatewayOutcome, String> {
    let channel = GatewayRouter::load_channel(conn, channel_id)?
        .ok_or_else(|| format!("unknown channel {}", channel_id))?;
    if channel.channel_type != GatewayChannelType::Slack {
        return Err("mock server only supports slack in GATE-01".into());
    }
    let user_text = crate::gateway::slack::parse_slack_payload(body)?;
    let inbound = GatewayRouter::normalize_inbound(&channel, &user_text);
    GatewayRouter::handle_inbound(conn, &inbound)
}
