//! FORK-01 — session fork / resume / side-branch (Phase 10 slice 10.2).

use aether_core::{LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS};
use aether_daemon::session_fork::{fork_session_at_turn, resume_snapshot, side_branch_from_turn};
use aether_daemon::session_log::{SessionLogPayload, SessionLogWriter};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;

fn frozen_plan(marker: &str) -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: format!("{marker}.txt"),
            content: marker.into(),
        },
        ToolInvocation::VerifyContains {
            path: format!("{marker}.txt"),
            text: marker.into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
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

fn run_turn(
    db: &Database,
    session_id: &str,
    workspace: std::path::PathBuf,
    plan: Vec<ToolInvocation>,
    prompt: &str,
) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'FORK-01', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR IGNORE INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace.to_string_lossy().to_string()],
    )
    .map_err(|e| e.to_string())?;
    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_id.to_string(),
        workspace,
    };
    let (result, _) =
        execute_structured_loop(&conn, &mut config, plan, None, &HashMap::new(), None, prompt);
    result.map_err(|e| e.to_string())?;
    Ok(())
}

pub fn fork01_fixture_ready() -> Result<(), String> {
    Ok(())
}

pub fn test_fork01_impl(db: &Database) -> Result<bool, String> {
    let log_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let _guard = EnvGuard {
        key: "AETHER_SESSION_LOG_DIR",
        previous: std::env::var("AETHER_SESSION_LOG_DIR").ok(),
    };
    std::env::set_var("AETHER_SESSION_LOG_DIR", log_dir.path());

    let parent = "sess-fork01-parent";
    let child = "sess-fork01-child";
    let branch = "sess-fork01-branch";
    let tmp_parent = tempfile::tempdir().map_err(|e| e.to_string())?;
    let tmp_child = tempfile::tempdir().map_err(|e| e.to_string())?;
    let tmp_branch = tempfile::tempdir().map_err(|e| e.to_string())?;

    run_turn(
        db,
        parent,
        tmp_parent.path().to_path_buf(),
        frozen_plan("fork-turn-1"),
        "fork-turn-1",
    )?;
    run_turn(
        db,
        parent,
        tmp_parent.path().to_path_buf(),
        frozen_plan("fork-turn-2"),
        "fork-turn-2",
    )?;

    fork_session_at_turn(parent, child, 1)?;
    if resume_snapshot(child)?.turn_count != 1 {
        return Err("FORK-01 forked child should have 1 turn".into());
    }

    run_turn(
        db,
        child,
        tmp_child.path().to_path_buf(),
        frozen_plan("fork-child-turn-2"),
        "fork-child-turn-2",
    )?;
    if resume_snapshot(parent)?.turn_count != 2 {
        return Err("FORK-01 parent must stay unchanged while child continues".into());
    }
    if resume_snapshot(child)?.turn_count != 2 {
        return Err("FORK-01 child should resume at turn 2".into());
    }

    run_turn(
        db,
        parent,
        tmp_parent.path().to_path_buf(),
        frozen_plan("fork-turn-3"),
        "fork-turn-3",
    )?;
    if resume_snapshot(parent)?.turn_count != 3 {
        return Err("FORK-01 resume on parent failed".into());
    }

    side_branch_from_turn(parent, branch, 2)?;
    run_turn(
        db,
        branch,
        tmp_branch.path().to_path_buf(),
        frozen_plan("branch-only"),
        "branch-only",
    )?;
    if resume_snapshot(parent)?.turn_count != 3 {
        return Err("FORK-01 side branch must not mutate parent".into());
    }

    let branch_log = SessionLogWriter::from_env()
        .read_session_log(branch)
        .map_err(|e| e.to_string())?;
    if !branch_log
        .iter()
        .any(|r| matches!(&r.payload, SessionLogPayload::TurnStart { prompt } if prompt == "branch-only"))
    {
        return Err("FORK-01 branch missing side-branch turn".into());
    }

    Ok(true)
}
