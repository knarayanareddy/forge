//! UNDO-01 — undo journal restores agent-controlled writes (Phase 9 slice 9.7-9.8).
//!
//! Exercises the same production entry point every daemon execution path uses
//! (`aether_daemon::task_runner::execute_structured_loop`) so the journal entries checked here are
//! the exact rows `ToolRegistry::execute` would produce in the real daemon, then calls the same
//! `aether_permissions::undo_pending_writes` function the daemon's `undo_writes` IPC method calls.
//!
//! Scope note: `undo_pending_writes` unwinds every still-applied journal entry for a session, most
//! recent first — it is session-scoped, not scoped to a single run/turn (`undo_journal` has no
//! run/turn column yet). This harness asserts exactly that behavior rather than a narrower
//! per-turn guarantee the code does not implement.

use aether_core::{LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use aether_permissions::undo_pending_writes;
use std::collections::HashMap;

fn seed_session_and_grant(db: &Database, session_id: &str, workspace: &std::path::Path) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'UNDO-01', 'active')",
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

fn run_plan(
    db: &Database,
    session_id: &str,
    workspace: std::path::PathBuf,
    plan: Vec<ToolInvocation>,
) -> Result<aether_core::LoopRunResult, String> {
    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
            provider_input_tokens: 0,
            provider_output_tokens: 0,
        session_id: session_id.to_string(),
        workspace,
    };
    let conn = db.conn();
    let (result, _events) =
        execute_structured_loop(&conn, &mut config, plan, None, &HashMap::new(), None, "undo01-run");
    result.map_err(|e| e.to_string())
}

pub fn test_undo01_impl(db: &Database) -> Result<(), String> {
    // --- Case 1: multi-file create + git_init, then undo everything in one call. ---
    let session_multi = "sess-undo01-multi";
    let tmp_multi = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_multi = tmp_multi.path().to_path_buf();
    seed_session_and_grant(db, session_multi, &workspace_multi)?;

    let plan = vec![
        ToolInvocation::FsWrite {
            path: "a.txt".into(),
            content: "created-a".into(),
        },
        ToolInvocation::VerifyContains {
            path: "a.txt".into(),
            text: "created-a".into(),
        },
        ToolInvocation::FsWrite {
            path: "b.txt".into(),
            content: "created-b".into(),
        },
        ToolInvocation::VerifyContains {
            path: "b.txt".into(),
            text: "created-b".into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::GitInit {
            branch: "undo01".into(),
        },
        ToolInvocation::Done,
    ];
    let run = run_plan(db, session_multi, workspace_multi.clone(), plan)?;
    if !run.done {
        return Err("expected multi-file+git plan to complete".into());
    }

    let a_path = workspace_multi.join("a.txt");
    let b_path = workspace_multi.join("b.txt");
    let git_path = workspace_multi.join(".git");
    if !a_path.exists() || !b_path.exists() || !git_path.exists() {
        return Err("expected a.txt, b.txt, and .git to exist before undo".into());
    }

    let report = {
        let conn = db.conn();
        undo_pending_writes(&conn, session_multi)?
    };
    if report.reverted.len() != 2 {
        return Err(format!(
            "expected 2 reverted writes (a.txt, b.txt), got {:?}",
            report.reverted
        ));
    }
    if a_path.exists() || b_path.exists() {
        return Err("expected undo to remove both newly-created files".into());
    }
    if !git_path.exists() {
        return Err("git_init must not be undone by undo_pending_writes".into());
    }
    if report.not_undone.len() != 1 || !report.not_undone[0].reason.contains("not automatically undone") {
        return Err(format!(
            "expected exactly one enumerated non-undoable git_init entry, got {:?}",
            report.not_undone
        ));
    }

    // Idempotency: a second call must not error, must revert nothing new, and must still
    // enumerate the (still-applied) git_init marker rather than silently dropping it.
    let report2 = {
        let conn = db.conn();
        undo_pending_writes(&conn, session_multi)?
    };
    if !report2.reverted.is_empty() {
        return Err(format!(
            "second undo call should have nothing left to revert, got {:?}",
            report2.reverted
        ));
    }
    if report2.not_undone.len() != 1 {
        return Err("second undo call should still report the git_init marker".into());
    }

    // --- Case 2: overwrite of a pre-existing file must restore the original bytes exactly. ---
    let session_overwrite = "sess-undo01-overwrite";
    let tmp_overwrite = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_overwrite = tmp_overwrite.path().to_path_buf();
    seed_session_and_grant(db, session_overwrite, &workspace_overwrite)?;

    let c_path = workspace_overwrite.join("c.txt");
    std::fs::write(&c_path, "pre-existing-c").map_err(|e| e.to_string())?;

    let plan2 = vec![
        ToolInvocation::FsWrite {
            path: "c.txt".into(),
            content: "overwritten-c".into(),
        },
        ToolInvocation::VerifyContains {
            path: "c.txt".into(),
            text: "overwritten-c".into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ];
    let run2 = run_plan(db, session_overwrite, workspace_overwrite.clone(), plan2)?;
    if !run2.done {
        return Err("expected overwrite plan to complete".into());
    }
    if std::fs::read_to_string(&c_path).map_err(|e| e.to_string())? != "overwritten-c" {
        return Err("expected c.txt to be overwritten before undo".into());
    }

    let report3 = {
        let conn = db.conn();
        undo_pending_writes(&conn, session_overwrite)?
    };
    if report3.reverted.len() != 1 {
        return Err(format!(
            "expected exactly one reverted write for the overwrite case, got {:?}",
            report3.reverted
        ));
    }
    if !report3.not_undone.is_empty() {
        return Err("overwrite-only run must have nothing non-undoable".into());
    }
    let restored = std::fs::read_to_string(&c_path).map_err(|e| e.to_string())?;
    if restored != "pre-existing-c" {
        return Err(format!(
            "expected byte-identical restore of pre-existing content, got {:?}",
            restored
        ));
    }

    Ok(())
}
