use aether_permissions::{PermissionDecision, PermissionManager};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Goal the executor must satisfy; checked by the verifier before any `fs_write` commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakerCheckerGoal {
    pub expected_content: String,
}

/// Read-only checker node — no write/git/mcp grants (CHECK-01 / slice 7.4).
pub struct VerifierNode;

impl VerifierNode {
    pub fn verifier_session_id(executor_session: &str) -> String {
        format!("{executor_session}::verifier")
    }

    /// Insert verifier session with read-only workspace grant (never write).
    pub fn ensure_read_only_context(
        conn: &Connection,
        verifier_session: &str,
        workspace: &Path,
    ) -> Result<(), String> {
        let ws = workspace.to_string_lossy().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'Verifier', 'active')",
            rusqlite::params![verifier_session],
        )
        .map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT OR IGNORE INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'read')",
            rusqlite::params![verifier_session, ws],
        )
        .map_err(|e| e.to_string())?;

        let write_decision =
            PermissionManager::check_file_access(conn, verifier_session, &ws, "write")
                .map_err(|e| e.to_string())?;
        if write_decision == PermissionDecision::Approved {
            return Err("Verifier session must not hold write grant".into());
        }

        let read_decision =
            PermissionManager::check_file_access(conn, verifier_session, &ws, "read")
                .map_err(|e| e.to_string())?;
        if read_decision != PermissionDecision::Approved {
            return Err("Verifier session requires read grant on workspace".into());
        }

        Ok(())
    }

    /// Deny when proposed write content does not satisfy the frozen goal marker.
    pub fn verify_fs_write_proposal(
        conn: &Connection,
        verifier_session: &str,
        workspace: &Path,
        path: &str,
        content: &str,
        goal: &MakerCheckerGoal,
    ) -> Result<(), String> {
        Self::ensure_read_only_context(conn, verifier_session, workspace)?;

        let full = resolve_workspace_path(&workspace.to_path_buf(), path)?;
        let full_str = full.to_string_lossy().to_string();

        let read_decision =
            PermissionManager::check_file_access(conn, verifier_session, &full_str, "read")
                .map_err(|e| e.to_string())?;
        if read_decision != PermissionDecision::Approved {
            return Err(format!(
                "Verifier read grant missing for proposed path {}",
                full_str
            ));
        }

        if !content.contains(&goal.expected_content) {
            let args = serde_json::json!({
                "path": path,
                "reason": "content_mismatch",
                "expected_marker": goal.expected_content,
            })
            .to_string();
            PermissionManager::audit_decision(
                conn,
                verifier_session,
                "verifier_deny",
                &args,
                &PermissionDecision::Denied,
                Some(1),
                None,
            )
            .map_err(|e| e.to_string())?;
            return Err(format!(
                "Verifier denied fs_write: content missing marker {:?}",
                goal.expected_content
            ));
        }

        Ok(())
    }
}

fn resolve_workspace_path(workspace: &PathBuf, rel: &str) -> Result<PathBuf, String> {
    use aether_permissions::{canonicalize_access_path, path_is_subpath};

    let workspace_canon = workspace
        .canonicalize()
        .map_err(|e| format!("Workspace canonicalize failed: {}", e))?;

    let rel = rel.trim_start_matches('\u{FEFF}');
    if rel.starts_with('/') || rel.starts_with('\\') {
        return Err(format!("Absolute path denied outside workspace: {}", rel));
    }

    let joined = workspace_canon.join(rel);
    let joined_str = joined.to_string_lossy().to_string();
    let resolved = canonicalize_access_path(&joined_str).map_err(|e| e.to_string())?;

    if !path_is_subpath(&resolved, &workspace_canon) {
        return Err(format!("Path escapes workspace grant: {}", rel));
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_db::Database;
    use tempfile::tempdir;

    #[test]
    fn verifier_has_read_not_write() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let tmp = tempdir().unwrap();
        let ws = tmp.path();
        let ws_str = ws.to_string_lossy().to_string();

        VerifierNode::ensure_read_only_context(&conn, "exec::verifier", ws).unwrap();

        assert_eq!(
            PermissionManager::check_file_access(&conn, "exec::verifier", &ws_str, "read").unwrap(),
            PermissionDecision::Approved
        );
        assert_eq!(
            PermissionManager::check_file_access(&conn, "exec::verifier", &ws_str, "write")
                .unwrap(),
            PermissionDecision::Denied
        );
    }

    #[test]
    fn verifier_denies_content_mismatch() {
        let db = Database::open_in_memory().unwrap();
        let conn = db.conn();
        let tmp = tempdir().unwrap();
        let ws = tmp.path();
        let goal = MakerCheckerGoal {
            expected_content: "GOAL-MARKER".into(),
        };

        let err = VerifierNode::verify_fs_write_proposal(
            &conn,
            "sess::verifier",
            ws,
            "out.txt",
            "wrong-body",
            &goal,
        )
        .unwrap_err();
        assert!(err.contains("Verifier denied"));
    }
}
