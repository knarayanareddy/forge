//! PERM-02 — batched approval gate: zero side effects execute before approval (Phase 10 slices
//! 10.8-10.9).
//!
//! Exercises `aether_core::evaluate_approval_gate` — the exact function the daemon's `run_task`
//! path (`run_loop_task`/`run_nl_loop_task_with_replan`) calls before ever invoking
//! `execute_structured_loop` — driving the same gate-then-execute pattern the daemon uses, so this
//! proves the actual contract: when the gate blocks, nothing has run yet.

use aether_core::{evaluate_approval_gate, LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;

fn seed_session_and_grant(db: &Database, session_id: &str, workspace: &std::path::Path) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'PERM-02', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace.to_string_lossy().to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Mirrors the daemon's own gate-then-execute pattern in `run_loop_task`: only call
/// `execute_structured_loop` if the gate clears. Returns `Err("BLOCKED: ...")` if the gate fired,
/// so callers can distinguish "blocked" from "ran and failed."
fn run_with_gate(
    db: &Database,
    session_id: &str,
    workspace: std::path::PathBuf,
    plan: Vec<ToolInvocation>,
    approved: bool,
) -> Result<aether_core::LoopRunResult, String> {
    if let Some(risky) = evaluate_approval_gate(&workspace, &plan, approved) {
        return Err(format!(
            "BLOCKED: {} step(s) require approval: {:?}",
            risky.len(),
            risky
        ));
    }
    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: session_id.to_string(),
        workspace,
    };
    let conn = db.conn();
    let (result, _events) =
        execute_structured_loop(&conn, &mut config, plan, None, &HashMap::new(), None, "perm02-run");
    result.map_err(|e| e.to_string())
}

pub fn test_perm02_impl(db: &Database) -> Result<(), String> {
    // --- Case 1: overwriting an existing file is blocked without approval, with zero side effects. ---
    let session_blocked = "sess-perm02-blocked";
    let tmp_blocked = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_blocked = tmp_blocked.path().to_path_buf();
    seed_session_and_grant(db, session_blocked, &workspace_blocked)?;

    let existing_path = workspace_blocked.join("existing.txt");
    std::fs::write(&existing_path, "original-content").map_err(|e| e.to_string())?;

    let plan_overwrite = vec![
        ToolInvocation::FsWrite {
            path: "existing.txt".into(),
            content: "clobbered".into(),
        },
        ToolInvocation::VerifyContains {
            path: "existing.txt".into(),
            text: "clobbered".into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ];

    let blocked_result = run_with_gate(
        db,
        session_blocked,
        workspace_blocked.clone(),
        plan_overwrite.clone(),
        false,
    );
    match &blocked_result {
        Err(msg) if msg.starts_with("BLOCKED") => {}
        other => return Err(format!("expected overwrite plan to be blocked, got {:?}", other)),
    }
    let content_after_block = std::fs::read_to_string(&existing_path).map_err(|e| e.to_string())?;
    if content_after_block != "original-content" {
        return Err(format!(
            "expected zero side effects while blocked, file now contains: {content_after_block}"
        ));
    }

    // Approving the SAME plan must let it proceed and actually overwrite the file.
    let approved_run = run_with_gate(
        db,
        session_blocked,
        workspace_blocked.clone(),
        plan_overwrite,
        true,
    )?;
    if !approved_run.done {
        return Err("expected the approved overwrite plan to complete".into());
    }
    let content_after_approval = std::fs::read_to_string(&existing_path).map_err(|e| e.to_string())?;
    if content_after_approval != "clobbered" {
        return Err(format!(
            "expected the approved plan to actually overwrite the file, got: {content_after_approval}"
        ));
    }

    // --- Case 2: an mcp_call is always risky, even with no existing files at stake. ---
    let workspace_mcp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mcp_plan = vec![
        ToolInvocation::McpCall {
            server: "filesystem".into(),
            tool: "write_file".into(),
            args: serde_json::json!({"path": "x.txt", "content": "y"}),
        },
        ToolInvocation::Done,
    ];
    if evaluate_approval_gate(&workspace_mcp.path().to_path_buf(), &mcp_plan, false).is_none() {
        return Err("expected an mcp_call step to require approval".into());
    }
    if evaluate_approval_gate(&workspace_mcp.path().to_path_buf(), &mcp_plan, true).is_some() {
        return Err("expected approval to clear the mcp_call gate".into());
    }

    // --- Case 3: a plan writing only brand-new files needs no approval at all. ---
    let session_ok = "sess-perm02-ok";
    let tmp_ok = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_ok = tmp_ok.path().to_path_buf();
    seed_session_and_grant(db, session_ok, &workspace_ok)?;

    let plan_new_file = vec![
        ToolInvocation::FsWrite {
            path: "brand_new.txt".into(),
            content: "fresh".into(),
        },
        ToolInvocation::VerifyContains {
            path: "brand_new.txt".into(),
            text: "fresh".into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ];
    let unapproved_new_file_run =
        run_with_gate(db, session_ok, workspace_ok.clone(), plan_new_file, false)?;
    if !unapproved_new_file_run.done {
        return Err("expected a new-file-only plan to run without needing approval".into());
    }
    if !workspace_ok.join("brand_new.txt").exists() {
        return Err("expected brand_new.txt to actually be written".into());
    }

    Ok(())
}
