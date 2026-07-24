use rusqlite::{Connection, Result};

/// Production crash recovery: invoked on every `Database::open`.
///
/// Rules:
/// - Orphan `pending` undo_journal entries (in-flight mutations interrupted by SIGTERM/crash)
///   are marked `reverted` — the filesystem may be inconsistent; journal state must not remain pending.
/// - Active sessions are preserved (SQLite WAL handles durability).
pub struct RecoveryManager;

impl RecoveryManager {
    pub fn recover_on_startup(conn: &Connection) -> Result<RecoveryReport> {
        let pending_before: i64 = conn.query_row(
            "SELECT COUNT(*) FROM undo_journal WHERE status = 'pending';",
            [],
            |row| row.get(0),
        )?;

        if pending_before > 0 {
            conn.execute(
                "UPDATE undo_journal SET status = 'reverted' WHERE status = 'pending';",
                [],
            )?;
        }

        let sessions_restored: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sessions WHERE status = 'active';",
            [],
            |row| row.get(0),
        )?;

        Ok(RecoveryReport {
            pending_reverted: pending_before as u64,
            sessions_restored: sessions_restored as u64,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveryReport {
    pub pending_reverted: u64,
    pub sessions_restored: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    #[test]
    fn test_recovery_reverts_pending_journal() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();

        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('s1', 'Test', 'active')",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
             VALUES ('s1', 'file_rename', '/tmp/a.txt', '{}', 'pending')",
            [],
        )
        .unwrap();

        let report = RecoveryManager::recover_on_startup(&conn).unwrap();
        assert_eq!(report.pending_reverted, 1);

        let pending: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM undo_journal WHERE status = 'pending';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(pending, 0);

        let reverted: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM undo_journal WHERE status = 'reverted';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reverted, 1);
    }
}
