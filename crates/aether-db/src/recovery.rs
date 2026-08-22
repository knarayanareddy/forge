use rusqlite::{params, Connection, Result};
use serde_json::Value;
use std::fs;
use std::path::Path;

/// Production crash reconciliation invoked on every `Database::open`.
///
/// A `file_write` journal row is inserted as `pending` before the filesystem mutation and promoted
/// to `applied` afterwards. Startup must determine which side of that boundary a crash reached:
///
/// - target equals `new_content`: the write happened; promote to `applied` so undo remains usable;
/// - target still equals the captured previous state: no mutation happened; mark `reverted`;
/// - anything else: fail startup rather than relabel or overwrite ambiguous user data.
///
/// Legacy/non-write pending rows carry no filesystem write contract and are marked reverted, which
/// preserves the pre-existing recovery behavior for rename fixtures while new writes use the
/// stronger reconciliation protocol.
pub struct RecoveryManager;

impl RecoveryManager {
    pub fn recover_on_startup(conn: &Connection) -> Result<RecoveryReport> {
        let mut statement = conn.prepare(
            "SELECT id, op_type, target_path, inverse_patch
             FROM undo_journal WHERE status = 'pending' ORDER BY id ASC",
        )?;
        let pending = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        let mut report = RecoveryReport::default();
        for (id, op_type, target_path, inverse_patch) in pending {
            if op_type != "file_write" {
                conn.execute(
                    "UPDATE undo_journal SET status = 'reverted' WHERE id = ?1 AND status = 'pending'",
                    params![id],
                )?;
                report.pending_reverted += 1;
                continue;
            }

            let patch: Value = serde_json::from_str(&inverse_patch).map_err(|error| {
                recovery_error(format!("journal {id} has invalid inverse patch: {error}"))
            })?;
            if patch.get("op").and_then(Value::as_str) != Some("write") {
                // git_init markers are written directly as applied; a legacy file_write marker
                // without a write contract is safe to classify as not-yet-applied.
                conn.execute(
                    "UPDATE undo_journal SET status = 'reverted' WHERE id = ?1 AND status = 'pending'",
                    params![id],
                )?;
                report.pending_reverted += 1;
                continue;
            }

            let had_previous = patch
                .get("had_previous")
                .and_then(Value::as_bool)
                .ok_or_else(|| recovery_error(format!("journal {id} missing had_previous")))?;
            let previous_content = patch
                .get("previous_content")
                .and_then(Value::as_str)
                .ok_or_else(|| recovery_error(format!("journal {id} missing previous_content")))?;
            let new_content = patch
                .get("new_content")
                .and_then(Value::as_str)
                .ok_or_else(|| recovery_error(format!("journal {id} missing new_content")))?;

            let target = Path::new(&target_path);
            let actual = match fs::read_to_string(target) {
                Ok(content) => Some(content),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => {
                    return Err(recovery_error(format!(
                        "journal {id} cannot read {}: {error}",
                        target.display()
                    )))
                }
            };

            let mutation_happened = actual.as_deref() == Some(new_content);
            let mutation_not_started = if had_previous {
                actual.as_deref() == Some(previous_content)
            } else {
                actual.is_none()
            };

            let new_status = if mutation_happened {
                report.pending_promoted_to_applied += 1;
                "applied"
            } else if mutation_not_started {
                report.pending_reverted += 1;
                "reverted"
            } else {
                report.pending_unresolved += 1;
                return Err(recovery_error(format!(
                    "journal {id} target {} matches neither previous nor intended content; manual recovery required",
                    target.display()
                )));
            };

            conn.execute(
                "UPDATE undo_journal SET status = ?1 WHERE id = ?2 AND status = 'pending'",
                params![new_status, id],
            )?;
        }

        report.sessions_restored = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE status = 'active';",
            [],
            |row| row.get::<_, i64>(0),
        )? as u64;
        Ok(report)
    }
}

fn recovery_error(message: String) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message,
    )))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    pub pending_reverted: u64,
    pub pending_promoted_to_applied: u64,
    pub pending_unresolved: u64,
    pub sessions_restored: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn seed_session(db: &Database, id: &str) {
        db.conn()
            .execute(
                "INSERT INTO sessions (id, title, status) VALUES (?1, 'Test', 'active')",
                params![id],
            )
            .unwrap();
    }

    #[test]
    fn pending_write_that_never_mutated_is_reverted() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "s-before");
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("new.txt");
        let patch = serde_json::json!({
            "op": "write",
            "workspace": dir.path(),
            "had_previous": false,
            "previous_content": "",
            "new_content": "desired"
        });
        let conn = db.conn();
        conn.execute(
            "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
             VALUES ('s-before', 'file_write', ?1, ?2, 'pending')",
            params![target.to_string_lossy(), patch.to_string()],
        )
        .unwrap();

        let report = RecoveryManager::recover_on_startup(&conn).unwrap();
        assert_eq!(report.pending_reverted, 1);
        assert_eq!(report.pending_promoted_to_applied, 0);
    }

    #[test]
    fn crash_after_write_preserves_inverse_by_promoting_applied() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "s-after");
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing.txt");
        fs::write(&target, "desired").unwrap();
        let patch = serde_json::json!({
            "op": "write",
            "workspace": dir.path(),
            "had_previous": true,
            "previous_content": "original",
            "new_content": "desired"
        });
        let conn = db.conn();
        conn.execute(
            "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
             VALUES ('s-after', 'file_write', ?1, ?2, 'pending')",
            params![target.to_string_lossy(), patch.to_string()],
        )
        .unwrap();

        let report = RecoveryManager::recover_on_startup(&conn).unwrap();
        assert_eq!(report.pending_promoted_to_applied, 1);
        let status: String = conn
            .query_row(
                "SELECT status FROM undo_journal WHERE session_id = 's-after'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "applied");
    }

    #[test]
    fn ambiguous_target_fails_closed_and_stays_pending() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "s-ambiguous");
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("existing.txt");
        fs::write(&target, "third-party-change").unwrap();
        let patch = serde_json::json!({
            "op": "write",
            "workspace": dir.path(),
            "had_previous": true,
            "previous_content": "original",
            "new_content": "desired"
        });
        let conn = db.conn();
        conn.execute(
            "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
             VALUES ('s-ambiguous', 'file_write', ?1, ?2, 'pending')",
            params![target.to_string_lossy(), patch.to_string()],
        )
        .unwrap();

        assert!(RecoveryManager::recover_on_startup(&conn).is_err());
        let status: String = conn
            .query_row(
                "SELECT status FROM undo_journal WHERE session_id = 's-ambiguous'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(status, "pending");
    }

    #[test]
    fn legacy_non_write_pending_entry_is_reverted() {
        let db = Database::open_in_memory().unwrap();
        seed_session(&db, "s-legacy");
        let conn = db.conn();
        conn.execute(
            "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
             VALUES ('s-legacy', 'file_rename', '/tmp/a.txt', '{}', 'pending')",
            [],
        )
        .unwrap();
        let report = RecoveryManager::recover_on_startup(&conn).unwrap();
        assert_eq!(report.pending_reverted, 1);
    }
}
