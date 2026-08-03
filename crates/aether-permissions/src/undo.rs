//! Run-level undo over `undo_journal` (Phase 9 slices 9.7-9.8).
//!
//! Journals every agent-controlled file write and git-repository initialization so the effects of
//! recent tool calls can be reversed without re-running inference. This is deliberately narrower
//! than the roadmap's original `undo_run(run_id)` framing: `undo_journal` has no run/turn
//! identifier column, so [`undo_pending_writes`] unwinds every still-`applied` journal entry for a
//! session, most recent first, rather than a single isolated turn. That is a real, honestly-scoped
//! capability — "undo everything since the last undo" — not the imprecise superset "undo just the
//! last run" the original phrasing implied. Turn-scoped undo is a documented follow-up that needs
//! a schema column, not a redefinition of what this function already does.
//!
//! Git repository initialization is journaled as a marker only: reverting `git init` (and any
//! commits/branches layered on top of it) is out of scope here — the entry is reported as
//! `not_undone` with a reason rather than silently skipped, per this project's anti-theater rule
//! that non-undoable side effects must be enumerated, not dropped.

use aether_sandbox::ProductionSandbox;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::path::Path;

/// One journal entry that undo declined to reverse, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotUndone {
    pub target_path: String,
    pub reason: String,
}

/// Outcome of a single [`undo_pending_writes`] call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UndoReport {
    /// Paths successfully restored to (or removed back to) their pre-write state, in the order
    /// they were undone (most recent write first).
    pub reverted: Vec<String>,
    pub not_undone: Vec<NotUndone>,
}

impl UndoReport {
    pub fn is_empty(&self) -> bool {
        self.reverted.is_empty() && self.not_undone.is_empty()
    }
}

/// Snapshot the file at `target` (if it exists), write `new_content`, and journal both so the
/// write can be undone later. Call this instead of `ProductionSandbox::write_file` directly for
/// any agent-controlled write that should be reversible.
///
/// Grant checks are the caller's responsibility — this function assumes the write is already
/// authorized and focuses solely on snapshot + write + journal.
pub fn journal_file_write(
    conn: &Connection,
    session_id: &str,
    workspace: &Path,
    target: &Path,
    new_content: &str,
) -> Result<(), String> {
    let target_str = target.to_string_lossy().to_string();
    let workspace_str = workspace.to_string_lossy().to_string();

    // Read before write: this is the only chance to capture the pre-mutation state.
    let previous = ProductionSandbox::read_to_string(workspace, target).ok();
    let inverse_patch = serde_json::json!({
        "op": "write",
        "workspace": workspace_str,
        "had_previous": previous.is_some(),
        "previous_content": previous.unwrap_or_default(),
    })
    .to_string();

    conn.execute(
        "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
         VALUES (?1, 'file_write', ?2, ?3, 'pending')",
        params![session_id, target_str, inverse_patch],
    )
    .map_err(|e| e.to_string())?;
    let undo_id = conn.last_insert_rowid();

    ProductionSandbox::write_file(workspace, target, new_content.as_bytes())
        .map_err(|e| e.to_string())?;

    conn.execute(
        "UPDATE undo_journal SET status = 'applied' WHERE id = ?1",
        params![undo_id],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Record that `git init` ran successfully, as a non-undoable marker. This must be called only
/// after the git operation has already succeeded — it never fails the caller's tool call, since
/// the journal entry is best-effort bookkeeping, not the source of truth for whether git ran.
pub fn journal_git_init(
    conn: &Connection,
    session_id: &str,
    workspace: &Path,
    branch: &str,
) -> Result<(), String> {
    let workspace_str = workspace.to_string_lossy().to_string();
    let inverse_patch = serde_json::json!({
        "op": "git_init",
        "workspace": workspace_str,
        "branch": branch,
    })
    .to_string();

    conn.execute(
        "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
         VALUES (?1, 'file_write', ?2, ?3, 'applied')",
        params![session_id, workspace_str, inverse_patch],
    )
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Unwind every still-`applied` journal entry for `session_id`, most recent first. Writes are
/// restored to their pre-write content or removed if the agent created them; git markers are
/// reported as `not_undone` rather than reverted or skipped.
///
/// Equivalent to [`undo_since`] with a watermark of `0` — every journal entry ever recorded for
/// this session is fair game.
pub fn undo_pending_writes(conn: &Connection, session_id: &str) -> Result<UndoReport, String> {
    undo_since(conn, session_id, 0)
}

/// The highest `undo_journal.id` recorded for `session_id` so far (`0` if none yet). A checkpoint
/// that records this value can later be rewound with [`undo_since`], which undoes only entries
/// recorded strictly after it — the entries a checkpoint at that watermark did not yet know about
/// (Phase 10 slice 10.1 / CKPT-01).
pub fn current_undo_watermark(conn: &Connection, session_id: &str) -> Result<i64, String> {
    let watermark: Option<i64> = conn
        .query_row(
            "SELECT MAX(id) FROM undo_journal WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    Ok(watermark.unwrap_or(0))
}

/// Unwind every still-`applied` journal entry for `session_id` with `id > since_id`, most recent
/// first. Writes are restored to their pre-write content or removed if the agent created them;
/// git markers are reported as `not_undone` rather than reverted or skipped.
pub fn undo_since(conn: &Connection, session_id: &str, since_id: i64) -> Result<UndoReport, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, target_path, inverse_patch FROM undo_journal
             WHERE session_id = ?1 AND status = 'applied' AND id > ?2
             ORDER BY id DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String, String)> = stmt
        .query_map(params![session_id, since_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);

    let mut report = UndoReport::default();

    for (id, target_path, inverse_patch_str) in rows {
        let patch: Value = serde_json::from_str(&inverse_patch_str).map_err(|e| e.to_string())?;
        let op = patch.get("op").and_then(Value::as_str).unwrap_or("write");

        match op {
            "write" => {
                let workspace_str = patch
                    .get("workspace")
                    .and_then(Value::as_str)
                    .ok_or("undo_journal entry missing workspace")?;
                let workspace = Path::new(workspace_str);
                let target = Path::new(&target_path);
                let had_previous = patch
                    .get("had_previous")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);

                if had_previous {
                    let previous_content = patch
                        .get("previous_content")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    ProductionSandbox::write_file(workspace, target, previous_content.as_bytes())
                        .map_err(|e| e.to_string())?;
                } else {
                    ProductionSandbox::remove_file(workspace, target).map_err(|e| e.to_string())?;
                }

                conn.execute(
                    "UPDATE undo_journal SET status = 'reverted' WHERE id = ?1",
                    params![id],
                )
                .map_err(|e| e.to_string())?;
                report.reverted.push(target_path);
            }
            "git_init" => {
                report.not_undone.push(NotUndone {
                    target_path,
                    reason: "git repository initialization is not automatically undone".into(),
                });
            }
            other => {
                report.not_undone.push(NotUndone {
                    target_path,
                    reason: format!("unknown undo operation: {other}"),
                });
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_db::Database;

    fn seed_session(db: &Database, session_id: &str) {
        db.conn()
            .execute(
                "INSERT INTO sessions (id, title, status) VALUES (?1, 'undo-test', 'active')",
                params![session_id],
            )
            .unwrap();
    }

    #[test]
    fn journal_then_undo_restores_overwritten_content() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-overwrite";
        seed_session(&db, session_id);
        let workspace = tempfile::tempdir().unwrap();
        let target = workspace.path().join("a.txt");

        let conn = db.conn();
        journal_file_write(&conn, session_id, workspace.path(), &target, "original").unwrap();
        journal_file_write(&conn, session_id, workspace.path(), &target, "overwritten").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "overwritten");

        let report = undo_pending_writes(&conn, session_id).unwrap();
        assert_eq!(report.reverted.len(), 2);
        assert!(report.not_undone.is_empty());
        // Unwinding both journal entries in reverse must return the file to its pre-existence
        // state (it never existed before the first journal_file_write call).
        assert!(!target.exists());
    }

    #[test]
    fn undo_removes_a_freshly_created_file() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-create";
        seed_session(&db, session_id);
        let workspace = tempfile::tempdir().unwrap();
        let target = workspace.path().join("new.txt");

        let conn = db.conn();
        journal_file_write(&conn, session_id, workspace.path(), &target, "hello").unwrap();
        assert!(target.exists());

        let report = undo_pending_writes(&conn, session_id).unwrap();
        assert_eq!(report.reverted, vec![target.to_string_lossy().to_string()]);
        assert!(!target.exists());
    }

    #[test]
    fn undo_restores_single_overwrite_to_prior_content() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-single";
        seed_session(&db, session_id);
        let workspace = tempfile::tempdir().unwrap();
        let target = workspace.path().join("a.txt");

        std::fs::write(&target, "pre-existing").unwrap();

        let conn = db.conn();
        journal_file_write(&conn, session_id, workspace.path(), &target, "new-content").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new-content");

        let report = undo_pending_writes(&conn, session_id).unwrap();
        assert_eq!(report.reverted.len(), 1);
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "pre-existing");
    }

    #[test]
    fn git_init_marker_is_reported_not_undone() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-git";
        seed_session(&db, session_id);
        let workspace = tempfile::tempdir().unwrap();

        let conn = db.conn();
        journal_git_init(&conn, session_id, workspace.path(), "main").unwrap();

        let report = undo_pending_writes(&conn, session_id).unwrap();
        assert!(report.reverted.is_empty());
        assert_eq!(report.not_undone.len(), 1);
        assert!(report.not_undone[0]
            .reason
            .contains("not automatically undone"));
    }

    #[test]
    fn undo_is_idempotent_on_second_call() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-idempotent";
        seed_session(&db, session_id);
        let workspace = tempfile::tempdir().unwrap();
        let target = workspace.path().join("a.txt");

        let conn = db.conn();
        journal_file_write(&conn, session_id, workspace.path(), &target, "hello").unwrap();
        let first = undo_pending_writes(&conn, session_id).unwrap();
        assert_eq!(first.reverted.len(), 1);

        let second = undo_pending_writes(&conn, session_id).unwrap();
        assert!(second.is_empty());
    }

    #[test]
    fn watermark_scoped_undo_leaves_earlier_writes_alone() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-watermark";
        seed_session(&db, session_id);
        let workspace = tempfile::tempdir().unwrap();
        let before_a = workspace.path().join("before.txt");
        let after_b = workspace.path().join("after.txt");

        let conn = db.conn();
        journal_file_write(&conn, session_id, workspace.path(), &before_a, "kept").unwrap();
        let watermark = current_undo_watermark(&conn, session_id).unwrap();
        assert_eq!(watermark, 1);

        journal_file_write(&conn, session_id, workspace.path(), &after_b, "discarded").unwrap();

        let report = undo_since(&conn, session_id, watermark).unwrap();
        assert_eq!(report.reverted, vec![after_b.to_string_lossy().to_string()]);
        assert!(before_a.exists(), "write recorded before the watermark must survive");
        assert!(!after_b.exists(), "write recorded after the watermark must be undone");
    }

    #[test]
    fn watermark_is_zero_for_a_session_with_no_journal_entries() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-watermark-empty";
        seed_session(&db, session_id);
        let conn = db.conn();
        assert_eq!(current_undo_watermark(&conn, session_id).unwrap(), 0);
    }

    #[test]
    fn multi_file_run_restores_every_file_byte_identical() {
        let db = Database::open_in_memory().unwrap();
        let session_id = "sess-undo-multi";
        seed_session(&db, session_id);
        let workspace = tempfile::tempdir().unwrap();
        let a = workspace.path().join("a.txt");
        let b = workspace.path().join("b.txt");
        std::fs::write(&a, "a-before").unwrap();

        let conn = db.conn();
        journal_file_write(&conn, session_id, workspace.path(), &a, "a-after").unwrap();
        journal_file_write(&conn, session_id, workspace.path(), &b, "b-after").unwrap();

        let report = undo_pending_writes(&conn, session_id).unwrap();
        assert_eq!(report.reverted.len(), 2);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a-before");
        assert!(!b.exists());
    }
}
