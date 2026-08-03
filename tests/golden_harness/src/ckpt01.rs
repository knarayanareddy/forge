//! CKPT-01 — checkpoint + rewind restores files AND truncates the session log (Phase 10 slice 10.1).
//!
//! Exercises `aether_daemon::checkpoint::{create_checkpoint, rewind_to_checkpoint}` directly —
//! the same functions the daemon's `create_checkpoint`/`rewind_checkpoint` IPC methods call — on
//! top of `execute_structured_loop`, the same production entry point every daemon path uses.

use aether_core::{LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS};
use aether_daemon::checkpoint::{create_checkpoint, rewind_to_checkpoint};
use aether_daemon::session_log::SessionLogWriter;
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;

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

fn seed_session_and_grant(db: &Database, session_id: &str, workspace: &std::path::Path) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'CKPT-01', 'active')",
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

fn run_turn(
    db: &Database,
    session_id: &str,
    workspace: std::path::PathBuf,
    plan: Vec<ToolInvocation>,
) -> Result<(), String> {
    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: session_id.to_string(),
        workspace,
    };
    let conn = db.conn();
    let (result, _events) =
        execute_structured_loop(&conn, &mut config, plan, None, &HashMap::new(), None, "ckpt01-turn");
    result.map(|_| ()).map_err(|e| e.to_string())
}

fn write_verify_lint_plan(path: &str, content: &str, verify_text: &str) -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: path.into(),
            content: content.into(),
        },
        ToolInvocation::VerifyContains {
            path: path.into(),
            text: verify_text.into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ]
}

pub fn test_ckpt01_impl(db: &Database) -> Result<(), String> {
    let log_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let _guard = EnvGuard {
        key: "AETHER_SESSION_LOG_DIR",
        previous: std::env::var("AETHER_SESSION_LOG_DIR").ok(),
    };
    std::env::set_var("AETHER_SESSION_LOG_DIR", log_dir.path());

    let session_id = "sess-ckpt01";
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    seed_session_and_grant(db, session_id, &workspace)?;

    // Turn 1: baseline work. Checkpoint is taken AFTER this turn — it must survive any rewind.
    run_turn(
        db,
        session_id,
        workspace.clone(),
        write_verify_lint_plan("keep.txt", "baseline-content", "baseline-content"),
    )?;

    let checkpoint = {
        let conn = db.conn();
        create_checkpoint(&conn, session_id)?
    };
    if checkpoint.turn_watermark != 1 {
        return Err(format!(
            "expected checkpoint turn_watermark 1, got {}",
            checkpoint.turn_watermark
        ));
    }

    // Turns 2 and 3: work that a rewind to the checkpoint above must fully discard.
    run_turn(
        db,
        session_id,
        workspace.clone(),
        write_verify_lint_plan("discard_a.txt", "turn-two", "turn-two"),
    )?;
    run_turn(
        db,
        session_id,
        workspace.clone(),
        write_verify_lint_plan("discard_b.txt", "turn-three", "turn-three"),
    )?;

    let keep_path = workspace.join("keep.txt");
    let discard_a = workspace.join("discard_a.txt");
    let discard_b = workspace.join("discard_b.txt");
    if !(keep_path.exists() && discard_a.exists() && discard_b.exists()) {
        return Err("expected all three files to exist before rewind".into());
    }

    let log_before_rewind = SessionLogWriter::from_env()
        .read_session_log(session_id)
        .map_err(|e| e.to_string())?;
    if log_before_rewind.iter().map(|r| r.turn_index).max() != Some(3) {
        return Err("expected 3 turns logged before rewind".into());
    }

    let report = {
        let conn = db.conn();
        rewind_to_checkpoint(&conn, checkpoint.id)?
    };
    if report.reverted_paths.len() != 2 {
        return Err(format!(
            "expected 2 files reverted (discard_a, discard_b), got {:?}",
            report.reverted_paths
        ));
    }
    if report.turns_truncated != 2 {
        return Err(format!(
            "expected 2 turns truncated, got {}",
            report.turns_truncated
        ));
    }
    if !keep_path.exists() {
        return Err("checkpointed file must survive rewind".into());
    }
    if discard_a.exists() || discard_b.exists() {
        return Err("post-checkpoint files must be undone by rewind".into());
    }

    let log_after_rewind = SessionLogWriter::from_env()
        .read_session_log(session_id)
        .map_err(|e| e.to_string())?;
    if log_after_rewind.iter().any(|r| r.turn_index > 1) {
        return Err("session log must be truncated back to the checkpoint's turn count".into());
    }
    if log_after_rewind.is_empty() {
        return Err("rewind must not delete the checkpointed turn's own log records".into());
    }

    // A fresh turn after rewind must resume numbering from turn 2, not turn 4 — proof the
    // truncation is real on disk, not just hidden from this reader.
    run_turn(
        db,
        session_id,
        workspace.clone(),
        write_verify_lint_plan("after_rewind.txt", "resumed", "resumed"),
    )?;
    let log_final = SessionLogWriter::from_env()
        .read_session_log(session_id)
        .map_err(|e| e.to_string())?;
    if log_final.iter().map(|r| r.turn_index).max() != Some(2) {
        return Err("expected the post-rewind turn to be numbered 2, not resume at 4".into());
    }

    // Rewinding again to the same checkpoint with nothing new past the (now-truncated) log must
    // still work and be a safe no-op for the file side, since the new turn 2's write is AFTER the
    // checkpoint's watermark and must also be discarded — checkpoints remain valid after use.
    let second_report = {
        let conn = db.conn();
        rewind_to_checkpoint(&conn, checkpoint.id)?
    };
    if second_report.reverted_paths.len() != 1 {
        return Err(format!(
            "expected the second rewind to discard the post-rewind turn's write, got {:?}",
            second_report.reverted_paths
        ));
    }
    if workspace.join("after_rewind.txt").exists() {
        return Err("second rewind must also discard work added after the first rewind".into());
    }
    if !keep_path.exists() {
        return Err("checkpointed file must still survive a second rewind".into());
    }

    // Rewinding to an unknown checkpoint must fail closed, not silently no-op.
    let conn = db.conn();
    if rewind_to_checkpoint(&conn, i64::MAX).is_ok() {
        return Err("expected rewind to an unknown checkpoint id to fail".into());
    }

    Ok(())
}
