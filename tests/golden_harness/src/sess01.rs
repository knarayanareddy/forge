//! SESS-01 — JSONL session log completeness and deterministic replay (Phase 9 slice 9.5-9.6).
//!
//! Exercises the same production entry point every daemon execution path uses
//! (`aether_daemon::task_runner::execute_structured_loop`) so the session log written here is
//! the exact artifact the daemon would produce — not a harness-only reimplementation.

use aether_core::{LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS};
use aether_daemon::session_log::{trajectory_from_log, SessionLogPayload, SessionLogRecord, SessionLogWriter};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;

const GOOD_PROMPT: &str = "sess01-good-plan";
const BAD_PROMPT: &str = "sess01-bad-plan";

fn frozen_good_plan() -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: "sess01_marker.txt".into(),
            content: "SESS-01-verified".into(),
        },
        ToolInvocation::VerifyContains {
            path: "sess01_marker.txt".into(),
            text: "SESS-01-verified".into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ]
}

/// Writes without verifying — the loop engine's verify shell must reject this before `done`.
fn frozen_bad_plan() -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: "sess01_bad.txt".into(),
            content: "unverified".into(),
        },
        ToolInvocation::Done,
    ]
}

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

fn run_frozen_case(
    db: &Database,
    session_id: &str,
    plan: Vec<ToolInvocation>,
    prompt: &str,
) -> Result<(Result<aether_core::LoopRunResult, String>, Vec<SessionLogRecord>), String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    {
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'SESS-01', 'active')",
            rusqlite::params![session_id],
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, 'write')",
            rusqlite::params![session_id, workspace.to_string_lossy().to_string()],
        )
        .map_err(|e| e.to_string())?;
    }

    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_id.to_string(),
        workspace,
    };

    let (result, _events) = {
        let conn = db.conn();
        execute_structured_loop(&conn, &mut config, plan, None, &HashMap::new(), None, prompt)
    };

    let log = SessionLogWriter::from_env()
        .read_session_log(session_id)
        .map_err(|e| e.to_string())?;

    Ok((result.map_err(|e| e.to_string()), log))
}

pub fn test_sess01_impl(db: &Database) -> Result<(), String> {
    let log_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let _guard = EnvGuard {
        key: "AETHER_SESSION_LOG_DIR",
        previous: std::env::var("AETHER_SESSION_LOG_DIR").ok(),
    };
    std::env::set_var("AETHER_SESSION_LOG_DIR", log_dir.path());

    let session_a = "sess-sess01-a";
    let session_b = "sess-sess01-b";
    let session_err = "sess-sess01-err";

    // Two independent sessions/workspaces running the identical frozen plan must produce
    // identical live trajectories — this is the "inference stubbed" determinism baseline a
    // structured JSON plan gives us without needing to fake an LLM.
    let (result_a, log_a) = run_frozen_case(db, session_a, frozen_good_plan(), GOOD_PROMPT)?;
    let (result_b, _log_b) = run_frozen_case(db, session_b, frozen_good_plan(), GOOD_PROMPT)?;
    let run_a = result_a?;
    let run_b = result_b?;

    if !run_a.done || !run_b.done {
        return Err("expected both frozen runs to complete".into());
    }

    let live_trajectory_a: Vec<&str> = run_a.observations.iter().map(|o| o.tool.as_str()).collect();
    let live_trajectory_b: Vec<&str> = run_b.observations.iter().map(|o| o.tool.as_str()).collect();
    if live_trajectory_a != live_trajectory_b {
        return Err(format!(
            "frozen plan produced different live trajectories: {:?} vs {:?}",
            live_trajectory_a, live_trajectory_b
        ));
    }

    // The log, parsed with zero knowledge of the live run, must reconstruct the same trajectory.
    let logged_trajectory = trajectory_from_log(&log_a);
    let expected: Vec<String> = vec![
        "fs_write".into(),
        "verify_contains".into(),
        "python_lint".into(),
        "done".into(),
    ];
    if logged_trajectory != expected {
        return Err(format!(
            "session log trajectory mismatch: expected {:?}, got {:?}",
            expected, logged_trajectory
        ));
    }
    if logged_trajectory.iter().map(String::as_str).collect::<Vec<_>>() != live_trajectory_a {
        return Err("logged trajectory diverges from the live run's own observations".into());
    }

    let turn_start = log_a.first().ok_or("log missing leading TurnStart record")?;
    match &turn_start.payload {
        SessionLogPayload::TurnStart { prompt } if prompt == GOOD_PROMPT => {}
        other => {
            return Err(format!(
                "expected TurnStart({:?}), got {:?}",
                GOOD_PROMPT, other
            ))
        }
    }

    let done_record = log_a
        .iter()
        .rev()
        .find(|r| matches!(r.payload, SessionLogPayload::Done { .. }))
        .ok_or("log missing terminal Done record")?;
    if let SessionLogPayload::Done {
        iterations,
        summary,
        tokens_used,
        provider_input_tokens: _,
        provider_output_tokens: _,
    } = &done_record.payload
    {
        if *iterations != run_a.iterations
            || *tokens_used != run_a.tokens_used
            || summary != &run_a.summary
        {
            return Err("logged Done payload does not match the live LoopRunResult".into());
        }
    }

    for window in log_a.windows(2) {
        if window[1].seq <= window[0].seq {
            return Err("session log seq must be strictly increasing".into());
        }
    }
    if log_a
        .iter()
        .any(|r| r.schema_version != aether_daemon::session_log::SESSION_LOG_SCHEMA_VERSION)
    {
        return Err("session log schema_version drifted from the pinned constant".into());
    }

    // A second turn on the SAME session must append, not overwrite, and increment turn_index.
    let (result_a2, log_a_after_second_turn) =
        run_frozen_case(db, session_a, frozen_good_plan(), GOOD_PROMPT)?;
    result_a2?;
    let turn_indices: std::collections::HashSet<u32> = log_a_after_second_turn
        .iter()
        .map(|r| r.turn_index)
        .collect();
    if turn_indices.len() != 2 {
        return Err(format!(
            "expected 2 distinct turn_index values after two turns on {}, got {:?}",
            session_a, turn_indices
        ));
    }

    // Failures must be logged too — a session log that only records success is theater.
    let (result_bad, bad_log) = run_frozen_case(db, session_err, frozen_bad_plan(), BAD_PROMPT)?;
    if result_bad.is_ok() {
        return Err("expected the unverified-write plan to be rejected".into());
    }
    if !bad_log
        .iter()
        .any(|r| matches!(&r.payload, SessionLogPayload::Error { .. }))
    {
        return Err("expected an Error record for the rejected plan".into());
    }

    Ok(())
}
