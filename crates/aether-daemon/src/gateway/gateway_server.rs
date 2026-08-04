use crate::gateway::inbound;
use crate::gateway::telegram;
use crate::gateway::GatewayChannelType;
use crate::DaemonState;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub async fn serve_gateway_webhooks(
    addr: String,
    state: Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("gateway webhook server listening on {}", addr);

    loop {
        let (mut socket, peer) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_gateway_connection(&mut socket, &state).await {
                tracing::warn!("gateway webhook session from {} ended: {}", peer, e);
            }
        });
    }
}

async fn handle_gateway_connection(
    socket: &mut tokio::net::TcpStream,
    state: &Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (method, path, headers, body) = read_http_request(socket).await?;

    if method != "POST" {
        write_http_response(socket, 405, "text/plain", "Method Not Allowed").await?;
        return Ok(());
    }

    let Some((channel_type, channel_id)) = parse_gateway_path(&path) else {
        write_http_response(socket, 400, "text/plain", "Bad gateway path").await?;
        return Ok(());
    };

    if channel_type == GatewayChannelType::Telegram {
        if let Err(reason) = telegram::verify_webhook_secret(
            headers
                .get("x-telegram-bot-api-secret-token")
                .map(String::as_str),
            channel_id,
        ) {
            write_http_response(socket, 403, "text/plain", &reason).await?;
            return Ok(());
        }
    }

    if channel_type == GatewayChannelType::Discord {
        crate::gateway::discord::log_webhook_ready(channel_id);
    }

    match inbound::handle_inbound_and_run(state, channel_type, channel_id, &body) {
        Ok(()) if channel_type == GatewayChannelType::Telegram => {
            write_http_response(socket, 200, "application/json", r#"{"ok":true}"#).await?;
        }
        Ok(()) if channel_type == GatewayChannelType::Discord => {
            write_http_response(socket, 200, "application/json", r#"{"type":1}"#).await?;
        }
        Ok(()) => write_http_response(socket, 200, "text/plain", "ok").await?,
        Err(msg) if msg.starts_with("denied:") => {
            write_http_response(socket, 403, "text/plain", &msg).await?;
        }
        Err(msg) => write_http_response(socket, 400, "text/plain", &msg).await?,
    }
    Ok(())
}

fn parse_gateway_path(path: &str) -> Option<(GatewayChannelType, &str)> {
    let trimmed = path.trim_start_matches("/gateway/").trim_matches('/');
    let (ty, channel_id) = trimmed.split_once('/')?;
    if channel_id.is_empty() {
        return None;
    }
    let channel_type = GatewayChannelType::parse(ty)?;
    Some((channel_type, channel_id))
}

async fn read_http_request(
    socket: &mut tokio::net::TcpStream,
) -> Result<(String, String, HashMap<String, String>, String), Box<dyn std::error::Error + Send + Sync>>
{
    let mut buf = vec![0u8; 8192];
    let n = socket.read(&mut buf).await?;
    if n == 0 {
        return Err("empty request".into());
    }
    let request = String::from_utf8_lossy(&buf[..n]).into_owned();

    let mut lines = request.lines();
    let request_line = lines.next().unwrap_or("");
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    let method = parts.first().unwrap_or(&"").to_string();
    let path = parts.get(1).unwrap_or(&"/").to_string();

    let mut content_length = 0usize;
    let mut headers = HashMap::new();
    for line in lines.by_ref() {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            let key = name.trim().to_ascii_lowercase();
            let val = value.trim().to_string();
            if key == "content-length" {
                content_length = val.parse().unwrap_or(0);
            }
            headers.insert(key, val);
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

    Ok((method, path, headers, body))
}

async fn write_http_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        405 => "Method Not Allowed",
        _ => "Bad Request",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        reason,
        content_type,
        body.len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_production_gateway_path() {
        let (ty, id) = parse_gateway_path("/gateway/telegram/gate02-telegram-frozen").unwrap();
        assert_eq!(ty, GatewayChannelType::Telegram);
        assert_eq!(id, "gate02-telegram-frozen");
    }
}
