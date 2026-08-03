//! Checkpoint + rewind (Phase 10 slice 10.1 / CKPT-01).
//!
//! A checkpoint is a named point a session can later be rewound to: the [`undo_journal`] watermark
//! (via `aether_permissions::current_undo_watermark`) plus the session-log turn count at the
//! moment it was taken. Rewinding undoes every file mutation recorded since that watermark
//! ([`aether_permissions::undo_since`]) *and* truncates the session log back to that many turns in
//! the same call — a checkpoint is only meaningful if the filesystem and the transcript agree on
//! what happened afterward.
//!
//! Scope: this is an explicit, caller-invoked API (a daemon IPC method pair), not an automatic
//! checkpoint taken before every single mutating tool call. `undo_journal` already gives
//! per-write granularity; a checkpoint is the coarser, named point on top of it that a session can
//! actually be asked to return to later, including after other work has happened in between.
//!
//! [`undo_journal`]: aether_permissions

use crate::session_log::{SessionLogPayload, SessionLogWriter};
use aether_permissions::{current_undo_watermark, undo_since, NotUndone};
use rusqlite::{params, Connection};

/// A rewindable point captured for a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRecord {
    pub id: i64,
    pub session_id: String,
    pub undo_watermark: i64,
    pub turn_watermark: u32,
}

/// Outcome of rewinding to a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewindReport {
    pub reverted_paths: Vec<String>,
    pub not_undone: Vec<NotUndone>,
    pub turns_truncated: u32,
}

fn current_turn_count(session_id: &str) -> Result<u32, String> {
    let log = SessionLogWriter::from_env()
        .read_session_log(session_id)
        .map_err(|e| e.to_string())?;
    Ok(log
        .iter()
        .filter(|r| matches!(r.payload, SessionLogPayload::TurnStart { .. }))
        .count() as u32)
}

/// Capture the current watermark for `session_id` as a new checkpoint.
pub fn create_checkpoint(conn: &Connection, session_id: &str) -> Result<CheckpointRecord, String> {
    let undo_watermark = current_undo_watermark(conn, session_id)?;
    let turn_watermark = current_turn_count(session_id)?;

    conn.execute(
        "INSERT INTO checkpoints (session_id, undo_watermark, turn_watermark) VALUES (?1, ?2, ?3)",
        params![session_id, undo_watermark, turn_watermark],
    )
    .map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();

    Ok(CheckpointRecord {
        id,
        session_id: session_id.to_string(),
        undo_watermark,
        turn_watermark,
    })
}

fn load_checkpoint(conn: &Connection, checkpoint_id: i64) -> Result<CheckpointRecord, String> {
    conn.query_row(
        "SELECT id, session_id, undo_watermark, turn_watermark FROM checkpoints WHERE id = ?1",
        params![checkpoint_id],
        |row| {
            Ok(CheckpointRecord {
                id: row.get(0)?,
                session_id: row.get(1)?,
                undo_watermark: row.get(2)?,
                turn_watermark: row.get::<_, i64>(3)? as u32,
            })
        },
    )
    .map_err(|_| format!("checkpoint {checkpoint_id} not found"))
}

/// Rewind a session to a previously-captured checkpoint: undo every file mutation recorded since
/// its watermark, and truncate the session log back to its turn count. Rewinding to the same
/// checkpoint twice (with nothing new in between) is safe and reports zero reverted paths and zero
/// truncated turns the second time — [`undo_since`] is itself idempotent once nothing is left
/// `applied` past the watermark, and re-truncating an already-truncated log is a no-op.
pub fn rewind_to_checkpoint(conn: &Connection, checkpoint_id: i64) -> Result<RewindReport, String> {
    let checkpoint = load_checkpoint(conn, checkpoint_id)?;

    let undo_report = undo_since(conn, &checkpoint.session_id, checkpoint.undo_watermark)?;

    let current_turns = current_turn_count(&checkpoint.session_id)?;
    let turns_truncated = current_turns.saturating_sub(checkpoint.turn_watermark);
    SessionLogWriter::from_env()
        .truncate_after_turn(&checkpoint.session_id, checkpoint.turn_watermark)
        .map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE checkpoints SET last_rewound_at = CURRENT_TIMESTAMP WHERE id = ?1",
        params![checkpoint_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(RewindReport {
        reverted_paths: undo_report.reverted,
        not_undone: undo_report.not_undone,
        turns_truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task_runner::execute_structured_loop;
    use aether_core::{LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS};
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

    fn seed_session_and_grant(db: &Database, session_id: &str, workspace: &std::path::Path) {
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'CKPT', 'active')",
            params![session_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, 'write')",
            params![session_id, workspace.to_string_lossy().to_string()],
        )
        .unwrap();
    }

    fn run_turn(
        db: &Database,
        session_id: &str,
        workspace: std::path::PathBuf,
        plan: Vec<ToolInvocation>,
    ) {
        let mut config = LoopConfig {
            max_iterations: 8,
            max_tokens: DEFAULT_MAX_LOOP_TOKENS,
            tokens_used: 0,
            session_id: session_id.to_string(),
            workspace,
        };
        let conn = db.conn();
        let (result, _events) =
            execute_structured_loop(&conn, &mut config, plan, None, &HashMap::new(), None, "ckpt-turn");
        result.expect("turn should succeed");
    }

    #[test]
    fn checkpoint_captures_zero_watermark_before_any_write() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-ckpt-fresh";
        db.conn()
            .execute(
                "INSERT INTO sessions (id, title, status) VALUES (?1, 'CKPT', 'active')",
                params![session_id],
            )
            .unwrap();

        let conn = db.conn();
        let checkpoint = create_checkpoint(&conn, session_id).unwrap();
        assert_eq!(checkpoint.undo_watermark, 0);
        assert_eq!(checkpoint.turn_watermark, 0);
    }

    #[test]
    fn rewind_restores_files_and_truncates_session_log_together() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-ckpt-rewind";
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        seed_session_and_grant(&db, session_id, &workspace);

        let log_dir = tempfile::tempdir().unwrap();
        let _guard = EnvGuard {
            key: "AETHER_SESSION_LOG_DIR",
            previous: std::env::var("AETHER_SESSION_LOG_DIR").ok(),
        };
        std::env::set_var("AETHER_SESSION_LOG_DIR", log_dir.path());

        // Turn 1: establish a baseline file. Checkpoint AFTER this turn — it must survive rewind.
        run_turn(
            &db,
            session_id,
            workspace.clone(),
            vec![
                ToolInvocation::FsWrite {
                    path: "keep.txt".into(),
                    content: "baseline".into(),
                },
                ToolInvocation::VerifyContains {
                    path: "keep.txt".into(),
                    text: "baseline".into(),
                },
                ToolInvocation::PythonLint {
                    source: "def ok():\n    return 1\n".into(),
                },
                ToolInvocation::Done,
            ],
        );

        let checkpoint = {
            let conn = db.conn();
            create_checkpoint(&conn, session_id).unwrap()
        };
        assert_eq!(checkpoint.turn_watermark, 1);

        // Turn 2: work that should be fully discarded by a rewind to the checkpoint above.
        run_turn(
            &db,
            session_id,
            workspace.clone(),
            vec![
                ToolInvocation::FsWrite {
                    path: "discard.txt".into(),
                    content: "should not survive".into(),
                },
                ToolInvocation::VerifyContains {
                    path: "discard.txt".into(),
                    text: "should not survive".into(),
                },
                ToolInvocation::PythonLint {
                    source: "def ok():\n    return 1\n".into(),
                },
                ToolInvocation::Done,
            ],
        );

        let keep_path = workspace.join("keep.txt");
        let discard_path = workspace.join("discard.txt");
        assert!(keep_path.exists());
        assert!(discard_path.exists());

        let report = {
            let conn = db.conn();
            rewind_to_checkpoint(&conn, checkpoint.id).unwrap()
        };
        assert_eq!(report.reverted_paths.len(), 1);
        assert_eq!(report.turns_truncated, 1);
        assert!(keep_path.exists(), "checkpointed file must survive rewind");
        assert!(!discard_path.exists(), "post-checkpoint file must be undone");

        let log = SessionLogWriter::from_env()
            .read_session_log(session_id)
            .unwrap();
        assert!(
            log.iter().all(|r| r.turn_index <= 1),
            "session log must be truncated back to the checkpoint's turn count"
        );

        // Rewinding again with nothing new in between must be a safe no-op.
        let second_report = {
            let conn = db.conn();
            rewind_to_checkpoint(&conn, checkpoint.id).unwrap()
        };
        assert!(second_report.reverted_paths.is_empty());
        assert_eq!(second_report.turns_truncated, 0);
    }

    #[test]
    fn rewind_to_unknown_checkpoint_fails_closed() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        assert!(rewind_to_checkpoint(&conn, 999_999).is_err());
    }
}
