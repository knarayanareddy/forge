//! Headless `--json` NDJSON mode (Phase 10 slice 10.12 / HEAD-01).

use crate::protocol::EventLine;
use crate::task_runner::{execute_structured_loop, RunTaskParams};
use crate::DaemonState;
use aether_core::{
    evaluate_approval_gate, LoopConfig, LoopStreamEvent, OrchestrationGraph, ReActLoopEngine,
    resolve_default_max_loop_tokens,
};
use aether_db::Database;
use aether_mcp::McpAllowlist;
use aether_skills::{SkillDefinition, SkillLoader};
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadlessExitCode {
    Success = 0,
    Error = 1,
    PendingApproval = 2,
}

impl HeadlessExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug, Clone)]
pub struct HeadlessOptions {
    pub prompt: String,
    pub session_id: String,
    pub workspace_path: Option<String>,
    pub max_iterations: Option<usize>,
    pub max_tokens: Option<usize>,
    pub approved: bool,
}

pub fn parse_headless_args(args: &[String]) -> Result<HeadlessOptions, String> {
    if !args.iter().any(|a| a == "--json") {
        return Err("--json flag required".into());
    }
    let mut prompt = None;
    let mut session_id = "headless".to_string();
    let mut workspace_path = None;
    let mut max_iterations = None;
    let mut max_tokens = None;
    let mut approved = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--prompt" => {
                i += 1;
                prompt = Some(args.get(i).ok_or("missing value for --prompt")?.clone());
            }
            "--session-id" => {
                i += 1;
                session_id = args.get(i).ok_or("missing value for --session-id")?.clone();
            }
            "--workspace" => {
                i += 1;
                workspace_path = Some(args.get(i).ok_or("missing value for --workspace")?.clone());
            }
            "--max-iterations" => {
                i += 1;
                max_iterations = Some(
                    args.get(i)
                        .ok_or("missing value for --max-iterations")?
                        .parse()
                        .map_err(|e| format!("invalid --max-iterations: {e}"))?,
                );
            }
            "--max-tokens" => {
                i += 1;
                max_tokens = Some(
                    args.get(i)
                        .ok_or("missing value for --max-tokens")?
                        .parse()
                        .map_err(|e| format!("invalid --max-tokens: {e}"))?,
                );
            }
            "--approved" => approved = true,
            _ => {}
        }
        i += 1;
    }
    Ok(HeadlessOptions {
        prompt: prompt.ok_or("--prompt is required in --json mode")?,
        session_id,
        workspace_path,
        max_iterations,
        max_tokens,
        approved,
    })
}

fn resolve_workspace(workspace_path: Option<&str>) -> Result<PathBuf, String> {
    match workspace_path {
        Some(p) => Ok(PathBuf::from(p)),
        None => std::env::current_dir().map_err(|e| e.to_string()),
    }
}

fn ensure_session_and_grant(
    conn: &rusqlite::Connection,
    session_id: &str,
    workspace: &PathBuf,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'headless', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    let workspace_str = workspace.to_string_lossy().to_string();
    let existing: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM capability_grants WHERE session_id = ?1 AND resource_path = ?2",
            rusqlite::params![session_id, workspace_str],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if existing == 0 {
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, 'write')",
            rusqlite::params![session_id, workspace_str],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn load_allowlist() -> Option<McpAllowlist> {
    aether_mcp::McpAllowlist::resolve_filesystem().ok()
}

fn load_skills() -> HashMap<String, SkillDefinition> {
    SkillLoader::load_directory(std::path::Path::new("skills"))
        .unwrap_or_default()
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect()
}

pub fn loop_event_to_line(event: &LoopStreamEvent) -> Option<EventLine> {
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
        LoopStreamEvent::ProviderTokens { .. } => None,
        LoopStreamEvent::Budget {
            iteration,
            max_iterations,
            tokens_used,
            max_tokens,
            ..
        } => Some(EventLine::budget(
            *iteration,
            *max_iterations,
            *tokens_used,
            *max_tokens,
        )),
        LoopStreamEvent::Done { .. } | LoopStreamEvent::Error { .. } => None,
    }
}

fn emit_event_line(out: &mut impl Write, event: &EventLine) -> io::Result<()> {
    let json = serde_json::to_string(event)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    writeln!(out, "{json}")
}

pub fn run_headless_task<W: Write>(
    state: &DaemonState,
    opts: &HeadlessOptions,
    out: &mut W,
) -> HeadlessExitCode {
    let workspace = match resolve_workspace(opts.workspace_path.as_deref()) {
        Ok(w) => w,
        Err(e) => {
            let _ = emit_event_line(out, &EventLine::error(e));
            return HeadlessExitCode::Error;
        }
    };
    let allowlist = load_allowlist();
    let skills = load_skills();
    let mut config = LoopConfig {
        max_iterations: opts.max_iterations.unwrap_or(8),
        max_tokens: opts.max_tokens.unwrap_or_else(resolve_default_max_loop_tokens),
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: opts.session_id.clone(),
        workspace: workspace.clone(),
    };
    let checker_goal = OrchestrationGraph::parse_checker_goal(&opts.prompt);
    let plan = match ReActLoopEngine::parse_plan_from_prompt(&opts.prompt) {
        Some(p) => p,
        None => {
            let _ = emit_event_line(
                out,
                &EventLine::error(
                    "headless --json requires structured JSON plan {\"loop\":[...]}".into(),
                ),
            );
            return HeadlessExitCode::Error;
        }
    };
    let conn = state.db.conn();
    if let Err(e) = ensure_session_and_grant(&conn, &opts.session_id, &workspace) {
        let _ = emit_event_line(out, &EventLine::error(e));
        return HeadlessExitCode::Error;
    }
    if let Some(risky) = evaluate_approval_gate(&workspace, &plan, opts.approved) {
        let _ = emit_event_line(out, &EventLine::pending_approval(&risky));
        return HeadlessExitCode::PendingApproval;
    }
    let (result, events) = execute_structured_loop(
        &conn,
        &mut config,
        plan,
        allowlist.as_ref(),
        &skills,
        checker_goal.as_ref(),
        opts.prompt.trim(),
    );
    for event in &events {
        if let Some(line) = loop_event_to_line(event) {
            let _ = emit_event_line(out, &line);
        }
    }
    match result {
        Ok(run) => {
            let _ = emit_event_line(
                out,
                &EventLine::done_with_tokens(
                    run.summary.clone(),
                    0,
                    "loop".into(),
                    Some(run.tokens_used),
                ),
            );
            HeadlessExitCode::Success
        }
        Err(e) => {
            let _ = emit_event_line(out, &EventLine::error(e.to_string()));
            HeadlessExitCode::Error
        }
    }
}

pub async fn run_headless_cli(args: Vec<String>) -> HeadlessExitCode {
    let opts = match parse_headless_args(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return HeadlessExitCode::Error;
        }
    };
    let db_path = std::env::var("AETHER_DB_PATH").unwrap_or_else(|_| {
        format!(
            "{}/.aether/aether-headless.db",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())
        )
    });
    if let Some(parent) = PathBuf::from(&db_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let db = match Database::open(&db_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("database open failed: {e}");
            return HeadlessExitCode::Error;
        }
    };
    let router = match aether_core::ModelRouter::from_env() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("router init failed: {e}");
            return HeadlessExitCode::Error;
        }
    };
    #[cfg(target_os = "macos")]
    let auth_token = aether_core::ensure_daemon_auth_token().unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let auth_token = std::env::var("AETHER_DAEMON_AUTH_TOKEN").unwrap_or_default();
    let state = DaemonState {
        db,
        router,
        auth_token,
    };
    let mut stdout = io::stdout().lock();
    run_headless_task(&state, &opts, &mut stdout)
}
