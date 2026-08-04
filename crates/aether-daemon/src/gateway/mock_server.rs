use crate::gateway::inbound;
use crate::gateway::telegram;
use crate::gateway::{GatewayChannelType, GatewayOutcome};
use crate::DaemonState;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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
    let Some((channel_type, channel_id)) = parse_gateway_path(path) else {
        write_http_response(socket, 400, "Missing channel type or channel_id").await?;
        return Ok(());
    };

    let mut content_length = 0usize;
    let mut webhook_secret = None;
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
        } else if lower.starts_with("x-telegram-bot-api-secret-token:") {
            webhook_secret = Some(
                line.split_once(':')
                    .map(|(_, v)| v.trim().to_string())
                    .unwrap_or_default(),
            );
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

    if channel_type == GatewayChannelType::Telegram {
        if let Err(reason) = telegram::verify_webhook_secret(webhook_secret.as_deref(), channel_id)
        {
            write_http_response(socket, 403, &reason).await?;
            return Ok(());
        }
    }

    let response =
        inbound::handle_inbound_and_run(state, channel_type, channel_id, body.trim());

    match response {
        Ok(()) => write_http_response(socket, 200, "ok").await?,
        Err(msg) if msg.starts_with("denied:") => write_http_response(socket, 403, &msg).await?,
        Err(msg) => write_http_response(socket, 400, &msg).await?,
    }
    Ok(())
}

fn parse_gateway_path(path: &str) -> Option<(GatewayChannelType, &str)> {
    let trimmed = path.trim_start_matches('/').trim_matches('/');
    let mut parts = trimmed.split('/');
    if parts.next()? != "gateway" {
        return None;
    }
    let ty = parts.next()?;
    let channel_id = parts.next()?;
    if channel_id.is_empty() {
        return None;
    }
    let channel_type = GatewayChannelType::parse(ty)?;
    Some((channel_type, channel_id))
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
    inbound::handle_inbound_post(conn, GatewayChannelType::Slack, channel_id, body)
}

pub fn handle_mock_telegram_post(
    conn: &rusqlite::Connection,
    channel_id: &str,
    body: &str,
) -> Result<GatewayOutcome, String> {
    inbound::handle_inbound_post(conn, GatewayChannelType::Telegram, channel_id, body)
}

pub fn handle_mock_discord_post(
    conn: &rusqlite::Connection,
    channel_id: &str,
    body: &str,
) -> Result<GatewayOutcome, String> {
    inbound::handle_inbound_post(conn, GatewayChannelType::Discord, channel_id, body)
}
