use crate::automation::AutomationScheduler;
use crate::DaemonState;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Minimal HTTP POST stub for PR webhook triggers (Slice 7.2).
pub async fn serve_webhook(
    addr: String,
    state: Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("automation webhook stub listening on {}", addr);

    loop {
        let (mut socket, peer) = listener.accept().await?;
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_webhook_connection(&mut socket, &state).await {
                tracing::warn!("webhook session from {} ended: {}", peer, e);
            }
        });
    }
}

async fn handle_webhook_connection(
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
    let trigger_id = path
        .trim_start_matches("/automation/webhook/")
        .trim_matches('/');

    if trigger_id.is_empty() {
        write_http_response(socket, 400, "Missing trigger_id").await?;
        return Ok(());
    }

    let mut content_length = 0usize;
    let mut secret = None;
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
        } else if lower.starts_with("x-aether-webhook-secret:") {
            secret = Some(
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

    let status = {
        let conn = state.db.conn();
        let mut scheduler = AutomationScheduler::new();
        let outcome = scheduler.handle_pr_webhook(
            &conn,
            trigger_id,
            body.trim(),
            secret.as_deref(),
        )?;
        match outcome {
            crate::automation::AutomationOutcome::Enqueued { .. } => (202, "Accepted"),
            crate::automation::AutomationOutcome::Denied { .. } => (403, "Forbidden"),
            crate::automation::AutomationOutcome::Skipped { .. } => (200, "Skipped"),
            crate::automation::AutomationOutcome::Audited { .. } => (200, "OK"),
        }
    };
    write_http_response(socket, status.0, status.1).await
}

async fn write_http_response(
    socket: &mut tokio::net::TcpStream,
    status: u16,
    message: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let body = format!("{{\"status\":\"{}\"}}", message);
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        message,
        body.len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    socket.shutdown().await?;
    Ok(())
}

/// Background cron/file-watch polling loop.
pub async fn run_automation_scheduler(state: Arc<DaemonState>) {
    let interval_secs = std::env::var("AETHER_AUTOMATION_POLL_SECS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let mut scheduler = AutomationScheduler::new();

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(interval_secs)).await;
        let conn = state.db.conn();
        if let Err(e) = scheduler.tick_cron(&conn, std::time::SystemTime::now()) {
            tracing::warn!("automation cron tick failed: {}", e);
        }
        if let Err(e) = scheduler.poll_file_watchers(&conn) {
            tracing::warn!("automation file watch poll failed: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_core::ModelRouter;
    use crate::automation::{AutomationScheduler, AutomationTrigger, TriggerConfig, TriggerType};
    use aether_permissions::AutomationGrant;
    use aether_db::Database;
    use std::sync::Arc;

    #[tokio::test]
    async fn webhook_enqueues_with_grant() {
        let db = Database::open_in_memory().unwrap();
        {
            let conn = db.conn();
            conn.execute(
                "INSERT INTO sessions (id, title, status) VALUES ('sess-auto', 'Auto', 'active')",
                [],
            )
            .unwrap();
            AutomationGrant::grant(&conn, "trg-pr", "sess-auto").unwrap();
            let trigger = AutomationTrigger {
                trigger_id: "trg-pr".into(),
                trigger_type: TriggerType::PrWebhook,
                session_id: "sess-auto".into(),
                config: TriggerConfig {
                    webhook_secret: Some("secret".into()),
                    ..Default::default()
                },
                task_prompt: "plan:[]".into(),
                workspace_path: None,
                enabled: true,
                last_fired_at: None,
            };
            AutomationScheduler::register_trigger(&conn, &trigger).unwrap();
        }

        let state = Arc::new(DaemonState {
            db,
            router: ModelRouter::new(
                aether_core::ModelBackend::OllamaMlx {
                    endpoint: "http://127.0.0.1:11434".into(),
                    model: "qwen2.5:3b".into(),
                },
                None,
            ),
            auth_token: String::new(),
        });

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state_for_handler = Arc::clone(&state);
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            handle_webhook_connection(&mut socket, &state_for_handler).await.unwrap();
        });

        let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let req = "POST /automation/webhook/trg-pr HTTP/1.1\r\n\
                   Host: localhost\r\n\
                   Content-Type: application/json\r\n\
                   X-Aether-Webhook-Secret: secret\r\n\
                   Content-Length: 19\r\n\r\n\
                   {\"action\":\"opened\"}";
        stream.write_all(req.as_bytes()).await.unwrap();
        let mut resp = vec![0u8; 1024];
        let n = stream.read(&mut resp).await.unwrap();
        let text = String::from_utf8_lossy(&resp[..n]);
        assert!(text.contains("202") || text.contains("Accepted"));

        let conn = state.db.conn();
        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM automation_queue WHERE trigger_id = 'trg-pr'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 1);
    }
}
