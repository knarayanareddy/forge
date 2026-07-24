use crate::protocol::{EventLine, RequestLine};
use crate::DaemonState;
use aether_core::PromptComplexity;
use futures::StreamExt;
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
                if request.params.prompt.is_empty() {
                    write_event(
                        &mut writer,
                        EventLine::error("Missing prompt parameter".into()),
                    )
                    .await?;
                    continue;
                }

                if let Some(session_id) = &request.params.session_id {
                    let conn = state.db.conn();
                    conn.execute(
                        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'Daemon Session', 'active')",
                        rusqlite::params![session_id],
                    )?;
                }

                let mut stream = Box::pin(
                    state
                        .router
                        .complete_stream(&request.params.prompt, PromptComplexity::Simple)
                        .await
                        .map_err(|e| format!("Stream start failed: {}", e))?,
                );

                let mut full_content = String::new();
                let mut ttft_ms = 0u128;
                let mut model = String::new();
                let mut saw_first = false;

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(chunk) => {
                            if let Some(ms) = chunk.ttft_ms {
                                ttft_ms = ms;
                                saw_first = true;
                                if !chunk.text.is_empty() {
                                    write_event(
                                        &mut writer,
                                        EventLine::token_with_ttft(chunk.text.clone(), ms),
                                    )
                                    .await?;
                                }
                            } else if !chunk.text.is_empty() {
                                write_event(&mut writer, EventLine::token(chunk.text.clone()))
                                    .await?;
                            }

                            if model.is_empty() {
                                model = chunk.model.clone();
                            }
                            full_content.push_str(&chunk.text);

                            if chunk.done {
                                break;
                            }
                        }
                        Err(e) => {
                            write_event(&mut writer, EventLine::error(e.to_string())).await?;
                            break;
                        }
                    }
                }

                if !saw_first {
                    ttft_ms = 0;
                }

                write_event(
                    &mut writer,
                    EventLine::done(full_content, ttft_ms, model),
                )
                .await?;
            }
            "ping" => {
                write_event(
                    &mut writer,
                    EventLine {
                        event_type: "pong".into(),
                        text: None,
                        ttft_ms: None,
                        content: None,
                        message: None,
                        model: None,
                    },
                )
                .await?;
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
