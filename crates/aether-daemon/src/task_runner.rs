use crate::ingest::{post_turn_graph_ingest, IngestConfig};
use crate::protocol::EventLine;
use crate::automation::AutomationTrigger;
use crate::DaemonState;
use aether_core::{
    LoopConfig, LoopStreamEvent, PromptComplexity, ReActLoopEngine, DEFAULT_MAX_LOOP_TOKENS,
};
use aether_mcp::McpAllowlist;
use aether_skills::SkillLoader;
use futures::StreamExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedWriteHalf;

pub struct RunTaskParams {
    pub prompt: String,
    pub session_id: Option<String>,
    pub workspace_path: Option<String>,
    pub max_iterations: Option<usize>,
    pub max_tokens: Option<usize>,
}

pub async fn run_task(
    writer: &mut OwnedWriteHalf,
    state: &Arc<DaemonState>,
    params: &RunTaskParams,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(plan) = ReActLoopEngine::parse_plan_from_prompt(&params.prompt) {
        return run_loop_task(writer, state, params, plan).await;
    }

    if let Some(nl_goal) = params.prompt.strip_prefix("nl:") {
        let max_iterations = params.max_iterations.unwrap_or(8);
        match aether_core::run_nl_planner(&state.router, nl_goal.trim(), max_iterations).await {
            Ok(plan) => return run_loop_task(writer, state, params, plan).await,
            Err(e) => {
                write_event(
                    writer,
                    EventLine::error(format!("NlPlanner failed: {}", e)),
                )
                .await?;
                return Ok(());
            }
        }
    }

    run_stream_task(writer, state, params).await
}

async fn run_loop_task(
    writer: &mut OwnedWriteHalf,
    state: &Arc<DaemonState>,
    params: &RunTaskParams,
    plan: Vec<aether_core::ToolInvocation>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let session_id = params
        .session_id
        .clone()
        .unwrap_or_else(|| "daemon-loop".into());

    let workspace = resolve_workspace(params.workspace_path.as_deref())?;

    let allowlist = load_allowlist();
    let skills = load_skills();
    let max_iterations = params.max_iterations.unwrap_or(8);
    let max_tokens = params.max_tokens.unwrap_or(DEFAULT_MAX_LOOP_TOKENS);
    let mut config = LoopConfig {
        max_iterations,
        max_tokens,
        tokens_used: 0,
        session_id: session_id.clone(),
        workspace,
    };
    let engine = ReActLoopEngine::new(config.max_iterations);

    let (result, events) = {
        let conn = state.db.conn();
        ensure_session_and_grants(&conn, &session_id, &config.workspace)?;
        let mut events = Vec::new();
        let result = engine.run_structured(
            &conn,
            &mut config,
            plan,
            allowlist.as_ref(),
            &skills,
            |event| events.push(event),
        );
        (result, events)
    };

    for event in &events {
        if let Some(line) = loop_event_to_line(event) {
            write_event(writer, line).await?;
        }
    }

    match result {
        Ok(run) => {
            write_event(
                writer,
                EventLine::done_with_tokens(
                    run.summary.clone(),
                    0,
                    "loop".into(),
                    Some(run.tokens_used),
                ),
            )
            .await?;

            post_turn_ingest(
                state,
                &session_id,
                params.prompt.trim(),
                run.summary.trim(),
            )
            .await;
        }
        Err(e) => {
            write_event(writer, EventLine::error(e.to_string())).await?;
        }
    }

    Ok(())
}

async fn run_stream_task(
    writer: &mut OwnedWriteHalf,
    state: &Arc<DaemonState>,
    params: &RunTaskParams,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(session_id) = &params.session_id {
        {
            let conn = state.db.conn();
            conn.execute(
                "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'Daemon Session', 'active')",
                rusqlite::params![session_id],
            )?;
        }
    }

    let mut stream = Box::pin(
        state
            .router
            .complete_stream(&params.prompt, PromptComplexity::Simple)
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
                            writer,
                            EventLine::token_with_ttft(chunk.text.clone(), ms),
                        )
                        .await?;
                    }
                } else if !chunk.text.is_empty() {
                    write_event(writer, EventLine::token(chunk.text.clone())).await?;
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
                write_event(writer, EventLine::error(e.to_string())).await?;
                break;
            }
        }
    }

    if !saw_first {
        ttft_ms = 0;
    }

    write_event(
        writer,
        EventLine::done(full_content.clone(), ttft_ms, model),
    )
    .await?;

    if let Some(session_id) = &params.session_id {
        post_turn_ingest(
            state,
            session_id,
            params.prompt.trim(),
            full_content.trim(),
        )
        .await;
    }

    Ok(())
}

/// Execute a dequeued automation trigger via the same loop shell as `run_task` (AUTO-01 / slice 7.3).
pub fn run_automation_trigger(
    conn: &rusqlite::Connection,
    trigger: &AutomationTrigger,
) -> Result<(), String> {
    let workspace = resolve_workspace(trigger.workspace_path.as_deref())?;
    ensure_session_and_grants(
        conn,
        &trigger.session_id,
        &workspace,
    )?;

    let plan = if let Some(plan) = ReActLoopEngine::parse_plan_from_prompt(&trigger.task_prompt) {
        plan
    } else {
        return Err(format!(
            "automation trigger {} requires structured plan: prompt",
            trigger.trigger_id
        ));
    };

    let allowlist = load_allowlist();
    let skills = load_skills();
    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: trigger.session_id.clone(),
        workspace,
    };
    let engine = ReActLoopEngine::new(config.max_iterations);

    let result = engine.run_structured(
        conn,
        &mut config,
        plan,
        allowlist.as_ref(),
        &skills,
        |_| {},
    );

    match result {
        Ok(run) if run.done => Ok(()),
        Ok(_) => Err("automation loop did not reach done".into()),
        Err(e) => Err(e.to_string()),
    }
}

async fn post_turn_ingest(
    state: &Arc<DaemonState>,
    session_id: &str,
    user_text: &str,
    assistant_text: &str,
) {
    post_turn_graph_ingest(
        &state.db,
        &state.router,
        &IngestConfig::default(),
        session_id,
        user_text,
        assistant_text,
    )
    .await;
}

fn resolve_workspace(workspace: Option<&str>) -> Result<PathBuf, String> {
    if let Some(ws) = workspace {
        return Ok(PathBuf::from(ws));
    }
    std::env::var("AETHER_WORKSPACE")
        .map(PathBuf::from)
        .map_err(|_| "Loop plan requires workspace_path param or AETHER_WORKSPACE env".into())
}

fn ensure_session_and_grants(
    conn: &rusqlite::Connection,
    session_id: &str,
    workspace: &PathBuf,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'Loop Session', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;

    let ws = workspace.to_string_lossy().to_string();
    conn.execute(
        "INSERT OR IGNORE INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, ws],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn load_allowlist() -> Option<McpAllowlist> {
    let path = std::path::Path::new("mcp_allowlist.json");
    if path.exists() {
        McpAllowlist::load_from_file(path).ok()
    } else {
        aether_mcp::discover_filesystem_mcp()
            .ok()
            .map(|p| McpAllowlist {
                servers: vec![p.to_allowlist_entry()],
            })
    }
}

fn load_skills() -> HashMap<String, aether_skills::SkillDefinition> {
    let skills_root = std::path::Path::new("skills");
    SkillLoader::load_directory(skills_root)
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect()
}

fn loop_event_to_line(event: &LoopStreamEvent) -> Option<EventLine> {
    match event {
        LoopStreamEvent::Plan { iteration, action } => Some(EventLine::plan(*iteration, action)),
        LoopStreamEvent::Tool {
            iteration,
            tool,
            output,
        } => Some(EventLine::tool(*iteration, tool, output)),
        LoopStreamEvent::Observe { iteration, summary } => {
            Some(EventLine::observe(*iteration, summary))
        }
        LoopStreamEvent::Verify {
            iteration,
            passed,
            detail,
        } => Some(EventLine::verify(*iteration, *passed, detail)),
        LoopStreamEvent::Budget {
            iteration,
            max_iterations,
            tokens_used,
            max_tokens,
        } => Some(EventLine::budget(
            *iteration,
            *max_iterations,
            *tokens_used,
            *max_tokens,
        )),
        LoopStreamEvent::Done { .. } | LoopStreamEvent::Error { .. } => None,
    }
}

async fn write_event(
    writer: &mut OwnedWriteHalf,
    event: EventLine,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let json = serde_json::to_string(&event)?;
    writer.write_all(json.as_bytes()).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;
    Ok(())
}
