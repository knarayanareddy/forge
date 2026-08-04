//! HOOK-01 — `PreToolUse` hook blocks a destructive op that a prompt instruction (and even an
//! explicit grant) alone does not prevent (Phase 10 slice 10.7).
//!
//! Exercises `execute_structured_loop` — the same production entry point every daemon path uses —
//! with a plan that explicitly targets a denylisted path (`.env`), in a workspace that already
//! has an explicit write grant. Both an "instruction" (the plan itself asks for it) and a "grant"
//! (the workspace is fully writable) are present; the hook must still block, because neither can
//! override it. A companion case proves the hook does not over-block an ordinary file.

use aether_core::{LoopConfig, LoopError, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;

fn seed_session_and_grant(db: &Database, session_id: &str, workspace: &std::path::Path) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'HOOK-01', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    // An explicit, unrestricted write grant for the whole workspace — the hook must block the
    // sensitive path anyway, proving the grant alone is not sufficient to allow it.
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace.to_string_lossy().to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn test_hook01_impl(db: &Database) -> Result<(), String> {
    // --- Case 1: the plan itself explicitly instructs writing a denylisted path. ---
    let session_blocked = "sess-hook01-blocked";
    let tmp_blocked = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_blocked = tmp_blocked.path().to_path_buf();
    seed_session_and_grant(db, session_blocked, &workspace_blocked)?;

    let mut config_blocked = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_blocked.to_string(),
        workspace: workspace_blocked.clone(),
    };
    let plan_blocked = vec![
        ToolInvocation::FsWrite {
            path: ".env".into(),
            content: "SECRET_KEY=should-never-be-written-by-an-agent".into(),
        },
        ToolInvocation::Done,
    ];
    let (result_blocked, _events) = {
        let conn = db.conn();
        execute_structured_loop(
            &conn,
            &mut config_blocked,
            plan_blocked,
            None,
            &HashMap::new(),
            None,
            "hook01-blocked",
        )
    };
    match result_blocked {
        Err(LoopError::Turn(reason)) => {
            if !reason.contains("PreToolUse hook blocked") {
                return Err(format!(
                    "expected a PreToolUse hook denial reason, got: {reason}"
                ));
            }
        }
        other => {
            return Err(format!(
                "expected the .env write to be hard-blocked by the PreToolUse hook, got {:?}",
                other
            ))
        }
    }
    if workspace_blocked.join(".env").exists() {
        return Err(".env must never be created on disk — the hook must block before the write, not after".into());
    }

    // A read of the same sensitive path must be blocked too, not just writes.
    let ssh_dir = workspace_blocked.join(".ssh");
    std::fs::create_dir_all(&ssh_dir).map_err(|e| e.to_string())?;
    std::fs::write(ssh_dir.join("id_rsa"), "not-a-real-key").map_err(|e| e.to_string())?;

    let mut config_read_blocked = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_blocked.to_string(),
        workspace: workspace_blocked.clone(),
    };
    let plan_read_blocked = vec![
        ToolInvocation::FsRead {
            path: ".ssh/id_rsa".into(),
        },
        ToolInvocation::Done,
    ];
    let (result_read_blocked, _events) = {
        let conn = db.conn();
        execute_structured_loop(
            &conn,
            &mut config_read_blocked,
            plan_read_blocked,
            None,
            &HashMap::new(),
            None,
            "hook01-read-blocked",
        )
    };
    if result_read_blocked.is_ok() {
        return Err("expected reading a private key path to be blocked by the PreToolUse hook".into());
    }

    // --- Case 2: an ordinary path in the same workspace must be unaffected. ---
    let session_ok = "sess-hook01-ok";
    let tmp_ok = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_ok = tmp_ok.path().to_path_buf();
    seed_session_and_grant(db, session_ok, &workspace_ok)?;

    let mut config_ok = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_ok.to_string(),
        workspace: workspace_ok.clone(),
    };
    let plan_ok = vec![
        ToolInvocation::FsWrite {
            path: "notes.txt".into(),
            content: "ordinary-content".into(),
        },
        ToolInvocation::VerifyContains {
            path: "notes.txt".into(),
            text: "ordinary-content".into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ];
    let (result_ok, _events) = {
        let conn = db.conn();
        execute_structured_loop(
            &conn,
            &mut config_ok,
            plan_ok,
            None,
            &HashMap::new(),
            None,
            "hook01-ok",
        )
    };
    let run = result_ok.map_err(|e| format!("expected ordinary write to succeed, got {e}"))?;
    if !run.done {
        return Err("expected the ordinary-path plan to complete".into());
    }
    if !workspace_ok.join("notes.txt").exists() {
        return Err("expected notes.txt to actually be written".into());
    }

    Ok(())
}
