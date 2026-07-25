use crate::protocol::{EventLine, RequestLine};
use crate::task_runner::{run_task, RunTaskParams};
use crate::DaemonState;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

pub async fn serve(addr: String, state: Arc<DaemonState>) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(&addr).await?;
    loop {
        let (socket, peer) = listener.accept().await?;
        tracing::info!("client connected: {}", peer);
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = handle_client(socket, state).await {
                tracing::warn!("client session ended: {}", e);
            }
        });
    }
}

async fn handle_client(
    socket: TcpStream,
    state: Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (reader, mut writer) = socket.into_split();
    let mut lines = BufReader::new(reader).lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: RequestLine = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                write_event(
                    &mut writer,
                    EventLine::error(format!("Invalid JSON request: {}", e)),
                )
                .await?;
                continue;
            }
        };

        match request.method.as_str() {
            "run_task" => {
                let token = request.params.auth_token.as_deref().unwrap_or("");
                match aether_core::verify_daemon_auth_token(token) {
                    Ok(true) => {}
                    Ok(false) => {
                        write_event(
                            &mut writer,
                            EventLine::error("Invalid or missing auth_token".into()),
                        )
                        .await?;
                        continue;
                    }
                    Err(e) => {
                        write_event(
                            &mut writer,
                            EventLine::error(format!("Auth token check failed: {}", e)),
                        )
                        .await?;
                        continue;
                    }
                }

                if request.params.prompt.is_empty() {
                    write_event(
                        &mut writer,
                        EventLine::error("Missing prompt parameter".into()),
                    )
                    .await?;
                    continue;
                }

                let params = RunTaskParams {
                    prompt: request.params.prompt,
                    session_id: request.params.session_id,
                    workspace_path: request.params.workspace_path,
                    max_iterations: request.params.max_iterations,
                };

                if let Err(e) = run_task(&mut writer, &state, &params).await {
                    write_event(&mut writer, EventLine::error(e.to_string())).await?;
                }
            }
            "ping" => {
                write_event(&mut writer, EventLine::pong()).await?;
            }
            other => {
                write_event(
                    &mut writer,
                    EventLine::error(format!("Unknown method: {}", other)),
                )
                .await?;
            }
        }
    }

    Ok(())
}

async fn write_event(
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
    event: EventLine,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::to_string(&event)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}
