use crate::automation::{AutomationScheduler, AutomationTrigger, TriggerConfig, TriggerType};
use crate::automation_webhook;
use crate::protocol::{EventLine, RequestLine};
use crate::task_runner::{run_task, ExecutionMode, RunTaskParams};
use crate::DaemonState;
use aether_permissions::{PermissionDecision, PermissionManager, PrincipalAuthorization};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::{timeout, Duration};

const DEFAULT_MAX_IPC_CONNECTIONS: usize = 64;
const DEFAULT_MAX_IPC_LINE_BYTES: usize = 1_048_576;
const DEFAULT_IPC_READ_TIMEOUT_SECS: u64 = 30;

fn principal_id(auth_token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aether-principal-v1\0");
    hasher.update(auth_token.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Returns true when the caller supplied a valid daemon auth token, or auth is disabled.
pub fn ipc_auth_ok(provided: Option<&str>, expected: &str) -> bool {
    if expected.is_empty() {
        return true;
    }
    aether_core::verify_daemon_auth_token_expected(provided.unwrap_or(""), expected)
}

fn bounded_env_usize(name: &str, default: usize, maximum: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
        .clamp(1, maximum)
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    max_bytes: usize,
    read_timeout: Duration,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut line = Vec::new();
    loop {
        let available = timeout(read_timeout, reader.fill_buf())
            .await
            .map_err(|_| "IPC read timed out")??;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }
        let take = available
            .iter()
            .position(|byte| *byte == b'\n')
            .map(|position| position + 1)
            .unwrap_or(available.len());
        if line.len().saturating_add(take) > max_bytes {
            return Err("IPC request exceeded configured line limit".into());
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take);
        if line.last() == Some(&b'\n') {
            break;
        }
    }
    if line.last() == Some(&b'\n') {
        line.pop();
        if line.last() == Some(&b'\r') {
            line.pop();
        }
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|_| "IPC request must be UTF-8".into())
}

pub async fn serve(addr: String, state: Arc<DaemonState>) -> Result<(), Box<dyn std::error::Error>> {
    let scheduler_state = Arc::clone(&state);
    tokio::spawn(async move {
        automation_webhook::run_automation_scheduler(scheduler_state).await;
    });

    if let Ok(webhook_port) = std::env::var("AETHER_AUTOMATION_WEBHOOK_PORT") {
        let webhook_addr = format!("127.0.0.1:{}", webhook_port);
        let webhook_state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = automation_webhook::serve_webhook(webhook_addr, webhook_state).await {
                tracing::error!("automation webhook server failed: {}", e);
            }
        });
    }

    if let Ok(gateway_port) = std::env::var("AETHER_GATEWAY_PORT") {
        let gateway_addr = format!("127.0.0.1:{}", gateway_port);
        let gateway_state = Arc::clone(&state);
        tokio::spawn(async move {
            if let Err(e) = crate::gateway::gateway_server::serve_gateway_webhooks(
                gateway_addr,
                gateway_state,
            )
            .await
            {
                tracing::error!("gateway webhook server failed: {}", e);
            }
        });
    }

    if let Ok(channels) = std::env::var("AETHER_TELEGRAM_LONG_POLL_CHANNELS") {
        for channel_id in channels.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let poll_state = Arc::clone(&state);
            let channel_id = channel_id.to_string();
            tokio::spawn(async move {
                crate::gateway::telegram::run_long_poll(poll_state, channel_id).await;
            });
        }
    }


    let listener = TcpListener::bind(&addr).await?;
    let connection_limit = Arc::new(Semaphore::new(bounded_env_usize(
        "AETHER_MAX_IPC_CONNECTIONS",
        DEFAULT_MAX_IPC_CONNECTIONS,
        1_024,
    )));
    loop {
        let (socket, peer) = listener.accept().await?;
        let Ok(permit) = Arc::clone(&connection_limit).try_acquire_owned() else {
            tracing::warn!("rejecting IPC connection: concurrency limit reached");
            drop(socket);
            continue;
        };
        tracing::info!("client connected: {}", peer);
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let _permit = permit;
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
    let mut reader = BufReader::new(reader);
    let max_line = bounded_env_usize(
        "AETHER_MAX_IPC_LINE_BYTES",
        DEFAULT_MAX_IPC_LINE_BYTES,
        8 * 1_048_576,
    );
    let read_timeout = Duration::from_secs(
        bounded_env_usize(
            "AETHER_IPC_READ_TIMEOUT_SECS",
            DEFAULT_IPC_READ_TIMEOUT_SECS as usize,
            300,
        ) as u64,
    );

    while let Some(line) = read_bounded_line(&mut reader, max_line, read_timeout).await? {
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

        if request.method != "ping" && !ipc_auth_ok(request.params.auth_token.as_deref(), &state.auth_token) {
            {
                let conn = state.db.conn();
                let _ = PermissionManager::audit_decision(
                    &conn,
                    request.params.session_id.as_deref().unwrap_or("__daemon__"),
                    "ipc_authentication",
                    &serde_json::json!({"method": request.method}).to_string(),
                    &PermissionDecision::Denied,
                    Some(1),
                    None,
                );
            }
            write_event(
                &mut writer,
                EventLine::error("Invalid or missing auth_token".into()),
            )
            .await?;
            continue;
        }

        let caller_principal = principal_id(&state.auth_token);
        match request.method.as_str() {
            "run_task" => {
                if request.params.approved.unwrap_or(false) {
                    write_event(
                        &mut writer,
                        EventLine::error(
                            "bare approved=true is forbidden; submit the daemon-issued approval_id"
                                .into(),
                        ),
                    )
                    .await?;
                    continue;
                }
                if request.params.prompt.is_empty() && request.params.approval_id.is_none() {
                    write_event(
                        &mut writer,
                        EventLine::error("Missing prompt or approval_id parameter".into()),
                    )
                    .await?;
                    continue;
                }
                let execution_mode = match ExecutionMode::parse(
                    request.params.execution_mode.as_deref(),
                ) {
                    Ok(mode) => mode,
                    Err(error) => {
                        write_event(&mut writer, EventLine::error(error)).await?;
                        continue;
                    }
                };

                let max_iterations = request.params.max_iterations.unwrap_or(8);
                if !(1..=64).contains(&max_iterations) {
                    write_event(
                        &mut writer,
                        EventLine::error("max_iterations must be between 1 and 64".into()),
                    )
                    .await?;
                    continue;
                }
                if request.params.max_tokens.is_some_and(|value| value == 0 || value > 1_000_000) {
                    write_event(
                        &mut writer,
                        EventLine::error("max_tokens must be between 1 and 1,000,000".into()),
                    )
                    .await?;
                    continue;
                }
                let params = RunTaskParams {
                    prompt: request.params.prompt,
                    session_id: request.params.session_id,
                    workspace_path: request.params.workspace_path,
                    max_iterations: Some(max_iterations),
                    max_tokens: request.params.max_tokens,
                    execution_mode,
                    approval_id: request.params.approval_id,
                    principal_id: caller_principal.clone(),
                };

                if let Err(e) = run_task(&mut writer, &state, &params).await {
                    write_event(&mut writer, EventLine::error(e.to_string())).await?;
                }
            }
            "ping" => {
                let proof = request
                    .params
                    .client_nonce
                    .as_deref()
                    .and_then(|nonce| aether_core::daemon_server_proof(&state.auth_token, nonce));
                write_event(&mut writer, EventLine::pong(proof)).await?;
            }
            "grant_workspace" => match handle_grant_workspace(&state, &request, &caller_principal) {
                Ok(path) => {
                    write_event(&mut writer, EventLine::workspace_granted(&path)).await?;
                }
                Err(e) => {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("grant_workspace failed: {}", e)),
                    )
                    .await?;
                }
            },
            "create_checkpoint" => match handle_create_checkpoint(&state, &request, &caller_principal) {
                Ok(checkpoint_id) => {
                    write_event(&mut writer, EventLine::checkpoint_created(checkpoint_id)).await?;
                }
                Err(e) => {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("create_checkpoint failed: {}", e)),
                    )
                    .await?;
                }
            },
            "rewind_checkpoint" => match handle_rewind_checkpoint(&state, &request, &caller_principal) {
                Ok(report) => {
                    let not_undone = report
                        .not_undone
                        .into_iter()
                        .map(|n| format!("{}: {}", n.target_path, n.reason))
                        .collect();
                    write_event(
                        &mut writer,
                        EventLine::rewind_complete(
                            report.reverted_paths,
                            not_undone,
                            report.turns_truncated,
                        ),
                    )
                    .await?;
                }
                Err(e) => {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("rewind_checkpoint failed: {}", e)),
                    )
                    .await?;
                }
            },
            "undo_writes" => match handle_undo_writes(&state, &request, &caller_principal) {
                Ok(report) => {
                    let not_undone = report
                        .not_undone
                        .into_iter()
                        .map(|n| format!("{}: {}", n.target_path, n.reason))
                        .collect();
                    write_event(
                        &mut writer,
                        EventLine::undo_complete(report.reverted, not_undone),
                    )
                    .await?;
                }
                Err(e) => {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("undo_writes failed: {}", e)),
                    )
                    .await?;
                }
            },
            "register_automation" => {
                if let Err(e) = handle_register_automation(&state, &request, &caller_principal).await {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("register_automation failed: {}", e)),
                    )
                    .await?;
                } else {
                    write_event(
                        &mut writer,
                        EventLine::automation_registered(
                            request.params.trigger_id.as_deref().unwrap_or(""),
                        ),
                    )
                    .await?;
                }
            }
            "automation_tick" => {
                let tick_count = {
                    let conn = state.db.conn();
                    let mut scheduler = AutomationScheduler::new();
                    scheduler
                        .tick_cron(&conn, std::time::SystemTime::now())
                        .map(|o| o.len())
                        .map_err(|e| e.to_string())?
                };
                write_event(
                    &mut writer,
                    EventLine::automation_tick(tick_count),
                )
                .await?;
            }
            "automation_run" => {
                if let Err(e) = handle_automation_run(&state, &mut writer).await {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("automation_run failed: {}", e)),
                    )
                    .await?;
                }
            }
            "list_consolidation_pending" => match handle_list_consolidation_pending(
                &state,
                &caller_principal,
            ) {
                Ok(runs) => {
                    write_event(&mut writer, EventLine::consolidation_list(runs)).await?;
                }
                Err(e) => {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("list_consolidation_pending failed: {}", e)),
                    )
                    .await?;
                }
            },
            "apply_consolidation" => match handle_apply_consolidation(&state, &request, &caller_principal) {
                Ok(applied) => {
                    let run_id = request.params.run_id.unwrap_or(0);
                    write_event(
                        &mut writer,
                        EventLine::consolidation_applied(run_id, applied),
                    )
                    .await?;
                }
                Err(e) => {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("apply_consolidation failed: {}", e)),
                    )
                    .await?;
                }
            },
            "reject_consolidation" => match handle_reject_consolidation(&state, &request, &caller_principal) {
                Ok(()) => {
                    let run_id = request.params.run_id.unwrap_or(0);
                    write_event(
                        &mut writer,
                        EventLine::consolidation_rejected(run_id),
                    )
                    .await?;
                }
                Err(e) => {
                    write_event(
                        &mut writer,
                        EventLine::error(format!("reject_consolidation failed: {}", e)),
                    )
                    .await?;
                }
            },
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

fn handle_grant_workspace(
    state: &Arc<DaemonState>,
    request: &RequestLine,
    principal_id: &str,
) -> Result<String, String> {
    let session_id = request
        .params
        .session_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or("missing session_id")?;
    let requested = request
        .params
        .workspace_path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
        .ok_or("missing workspace_path")?;
    let workspace = std::path::Path::new(requested)
        .canonicalize()
        .map_err(|e| format!("workspace canonicalization failed: {e}"))?;
    if !workspace.is_dir() {
        return Err("workspace_path must be an existing directory".into());
    }
    let workspace = workspace.to_string_lossy().to_string();
    let conn = state.db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status)
         VALUES (?1, 'Workspace Session', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    if PrincipalAuthorization::claim_or_check_session(&conn, session_id, principal_id)
        .map_err(|error| error.to_string())?
        != PermissionDecision::Approved
    {
        return Err("session belongs to another principal".into());
    }
    for capability in ["read", "write"] {
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             SELECT ?1, ?2, ?3
             WHERE NOT EXISTS (
                 SELECT 1 FROM capability_grants
                 WHERE session_id = ?1 AND resource_path = ?2 AND permission_type = ?3
                   AND is_stale = 0
             )",
            rusqlite::params![session_id, workspace, capability],
        )
        .map_err(|e| e.to_string())?;
    }
    PermissionManager::audit_decision(
        &conn,
        session_id,
        "grant_workspace",
        &serde_json::json!({
            "workspace_path": workspace,
            "capabilities": ["read", "write"],
            "source": "authenticated_ui"
        })
        .to_string(),
        &PermissionDecision::Approved,
        Some(0),
        None,
    )
    .map_err(|e| e.to_string())?;
    Ok(workspace)
}

fn handle_create_checkpoint(
    state: &Arc<DaemonState>,
    request: &RequestLine,
    principal_id: &str,
) -> Result<i64, String> {
    let session_id = request
        .params
        .session_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or("missing session_id")?;
    let conn = state.db.conn();
    if PrincipalAuthorization::check_session(&conn, session_id, principal_id)
        .map_err(|error| error.to_string())?
        != PermissionDecision::Approved
    {
        return Err("session ownership check failed".into());
    }
    crate::checkpoint::create_checkpoint(&conn, session_id).map(|c| c.id)
}

fn handle_rewind_checkpoint(
    state: &Arc<DaemonState>,
    request: &RequestLine,
    principal_id: &str,
) -> Result<crate::checkpoint::RewindReport, String> {
    let checkpoint_id = request.params.checkpoint_id.ok_or("missing checkpoint_id")?;
    let conn = state.db.conn();
    let session_id: String = conn
        .query_row(
            "SELECT session_id FROM checkpoints WHERE id = ?1",
            rusqlite::params![checkpoint_id],
            |row| row.get(0),
        )
        .map_err(|_| "checkpoint not found".to_string())?;
    if PrincipalAuthorization::check_session(&conn, &session_id, principal_id)
        .map_err(|error| error.to_string())?
        != PermissionDecision::Approved
    {
        return Err("checkpoint ownership check failed".into());
    }
    crate::checkpoint::rewind_to_checkpoint(&conn, checkpoint_id)
}

fn handle_undo_writes(
    state: &Arc<DaemonState>,
    request: &RequestLine,
    principal_id: &str,
) -> Result<aether_permissions::UndoReport, String> {
    let session_id = request
        .params
        .session_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .ok_or("missing session_id")?;
    let conn = state.db.conn();
    if PrincipalAuthorization::check_session(&conn, session_id, principal_id)
        .map_err(|error| error.to_string())?
        != PermissionDecision::Approved
    {
        return Err("session ownership check failed".into());
    }
    aether_permissions::undo_pending_writes(&conn, session_id)
}

fn handle_list_consolidation_pending(
    state: &Arc<DaemonState>,
    principal_id: &str,
) -> Result<Vec<aether_db::ConsolidationRunListItem>, String> {
    state
        .db
        .list_consolidation_runs_for_principal("review_pending", principal_id)
        .map_err(|e| e.to_string())
}

fn handle_apply_consolidation(
    state: &Arc<DaemonState>,
    request: &RequestLine,
    principal_id: &str,
) -> Result<usize, String> {
    let run_id = request.params.run_id.ok_or("missing run_id")?;
    {
        let conn = state.db.conn();
        let session_id: String = conn
            .query_row(
                "SELECT session_id FROM consolidation_runs WHERE id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )
            .map_err(|_| "consolidation run not found".to_string())?;
        if PrincipalAuthorization::check_session(&conn, &session_id, principal_id)
            .map_err(|error| error.to_string())?
            != PermissionDecision::Approved
        {
            return Err("consolidation ownership check failed".into());
        }
    }
    state
        .db
        .apply_consolidation_run(run_id)
        .map_err(|e| e.to_string())
}

fn handle_reject_consolidation(
    state: &Arc<DaemonState>,
    request: &RequestLine,
    principal_id: &str,
) -> Result<(), String> {
    let run_id = request.params.run_id.ok_or("missing run_id")?;
    {
        let conn = state.db.conn();
        let session_id: String = conn
            .query_row(
                "SELECT session_id FROM consolidation_runs WHERE id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )
            .map_err(|_| "consolidation run not found".to_string())?;
        if PrincipalAuthorization::check_session(&conn, &session_id, principal_id)
            .map_err(|error| error.to_string())?
            != PermissionDecision::Approved
        {
            return Err("consolidation ownership check failed".into());
        }
    }
    state
        .db
        .reject_consolidation_run(run_id)
        .map_err(|e| e.to_string())
}

async fn handle_automation_run(
    state: &Arc<DaemonState>,
    writer: &mut tokio::net::tcp::OwnedWriteHalf,
) -> Result<(), String> {
    let results = {
        let conn = state.db.conn();
        AutomationScheduler::run_pending(&conn, 4, |trigger| {
            crate::task_runner::run_automation_trigger(&conn, trigger)
        })?
    };

    write_event(
        writer,
        EventLine::automation_run_complete(results.len()),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

async fn handle_register_automation(
    state: &Arc<DaemonState>,
    request: &RequestLine,
    principal_id: &str,
) -> Result<(), String> {
    if request.params.grant_automation.unwrap_or(false) {
        return Err(
            "grant_automation via IPC is forbidden; grant AutomationGrant through the UI or an authenticated grant flow"
                .into(),
        );
    }

    let trigger_id = request
        .params
        .trigger_id
        .as_deref()
        .ok_or("missing trigger_id")?;
    let session_id = request
        .params
        .session_id
        .as_deref()
        .ok_or("missing session_id")?;
    let trigger_type = TriggerType::parse(
        request
            .params
            .trigger_type
            .as_deref()
            .ok_or("missing trigger_type")?,
    )
    .ok_or("invalid trigger_type")?;

    let config: TriggerConfig = request
        .params
        .config_json
        .as_deref()
        .map(|s| serde_json::from_str(s).unwrap_or_default())
        .unwrap_or_default();

    let trigger = AutomationTrigger {
        trigger_id: trigger_id.to_string(),
        trigger_type,
        session_id: session_id.to_string(),
        config,
        task_prompt: request
            .params
            .task_prompt
            .clone()
            .unwrap_or_default(),
        workspace_path: request.params.workspace_path.clone(),
        enabled: true,
        last_fired_at: None,
    };

    let conn = state.db.conn();
    if PrincipalAuthorization::claim_or_check_session(&conn, session_id, principal_id)
        .map_err(|error| error.to_string())?
        != PermissionDecision::Approved
    {
        return Err("automation session ownership check failed".into());
    }
    AutomationScheduler::register_trigger(&conn, &trigger)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_auth_rejects_missing_and_wrong_token() {
        let expected = "test-token-abc";
        assert!(!ipc_auth_ok(None, expected));
        assert!(!ipc_auth_ok(Some(""), expected));
        assert!(!ipc_auth_ok(Some("wrong"), expected));
        assert!(ipc_auth_ok(Some(expected), expected));
    }

    #[test]
    fn ipc_auth_allows_when_auth_disabled() {
        assert!(ipc_auth_ok(None, ""));
        assert!(ipc_auth_ok(Some("any-token"), ""));
    }

    #[test]
    fn ipc_auth_fail_closed_when_token_configured() {
        assert!(!ipc_auth_ok(None, "configured-token"));
        assert!(!ipc_auth_ok(Some("wrong"), "configured-token"));
    }

    #[test]
    fn register_automation_rejects_grant_automation_flag() {
        let request: RequestLine = serde_json::from_str(
            r#"{"method":"register_automation","params":{"grant_automation":true,"trigger_id":"t","session_id":"s","trigger_type":"cron"}}"#,
        )
        .expect("parse");
        let state = Arc::new(DaemonState {
            db: aether_db::Database::open_in_memory().expect("db"),
            router: aether_core::ModelRouter::from_env().expect("router"),
            auth_token: "tok".into(),
        });
        let pending = handle_register_automation(&state, &request, "test-principal");
        let rt = tokio::runtime::Runtime::new().expect("runtime");
        let err = rt.block_on(pending).expect_err("must reject grant_automation");
        assert!(err.contains("grant_automation via IPC is forbidden"));
    }

    #[test]
    fn workspace_grant_is_explicit_audited_and_idempotent() {
        let workspace = tempfile::tempdir().expect("workspace");
        let request_json = serde_json::json!({
            "method": "grant_workspace",
            "params": {
                "session_id": "workspace-grant-test",
                "workspace_path": workspace.path(),
            }
        });
        let request: RequestLine =
            serde_json::from_value(request_json).expect("grant request parse");
        let state = Arc::new(DaemonState {
            db: aether_db::Database::open_in_memory().expect("db"),
            router: aether_core::ModelRouter::from_env().expect("router"),
            auth_token: "tok".into(),
        });

        let canonical = handle_grant_workspace(&state, &request, "test-principal").expect("grant");
        handle_grant_workspace(&state, &request, "test-principal")
            .expect("idempotent grant");
        assert_eq!(
            canonical,
            workspace.path().canonicalize().unwrap().to_string_lossy()
        );

        let conn = state.db.conn();
        let grants: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM capability_grants
                 WHERE session_id = 'workspace-grant-test'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(grants, 2, "read + write only, without duplicates");
        let audits: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM audit_log
                 WHERE session_id = 'workspace-grant-test'
                   AND tool_name = 'grant_workspace'
                   AND decision = 'approved'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(audits, 2);
    }
}
