use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionDecision {
    Approved,
    Denied,
    AutoAllowed,
}

pub struct PermissionManager;

impl PermissionManager {
    /// Grant-based enforcement:
    /// Path is allowed ONLY if an explicit capability_grants row exists for this session (or app-lifetime session_id IS NULL)
    /// matching resource_path and permission_type, and is_stale = 0.
    pub fn check_file_access(
        conn: &Connection,
        session_id: &str,
        path: &str,
        operation: &str,
    ) -> Result<PermissionDecision> {
        let mut stmt = conn.prepare(
            "SELECT COUNT(*) FROM capability_grants 
             WHERE (session_id = ?1 OR session_id IS NULL) 
               AND resource_path = ?2 
               AND permission_type = ?3 
               AND is_stale = 0"
        )?;
        
        let count: i64 = stmt.query_row(
            params![session_id, path, operation],
            |row| row.get(0)
        ).unwrap_or(0);

        if count > 0 {
            Ok(PermissionDecision::Approved)
        } else {
            Ok(PermissionDecision::Denied)
        }
    }

    pub fn audit_decision(
        conn: &Connection,
        session_id: &str,
        tool_name: &str,
        arguments_json: &str,
        decision: &PermissionDecision,
        exit_code: Option<i32>,
        duration_ms: Option<i64>,
    ) -> Result<()> {
        let decision_str = match decision {
            PermissionDecision::Approved => "approved",
            PermissionDecision::Denied => "denied",
            PermissionDecision::AutoAllowed => "auto_allowed",
        };

        let prev_hash: String = conn.query_row(
            "SELECT content_hash FROM audit_log ORDER BY id DESC LIMIT 1;",
            [],
            |row| row.get(0)
        ).unwrap_or_else(|_| "GENESIS_HASH".to_string());

        let mut hasher = Sha256::new();
        hasher.update(prev_hash.as_bytes());
        hasher.update(session_id.as_bytes());
        hasher.update(tool_name.as_bytes());
        hasher.update(arguments_json.as_bytes());
        hasher.update(decision_str.as_bytes());
        let content_hash = format!("{:x}", hasher.finalize());

        conn.execute(
            "INSERT INTO audit_log (session_id, tool_name, arguments_json, decision, exit_code, execution_duration_ms, prev_hash, content_hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                session_id,
                tool_name,
                arguments_json,
                decision_str,
                exit_code,
                duration_ms,
                prev_hash,
                content_hash
            ],
        )?;

        Ok(())
    }
}

pub struct FileMutator;

impl FileMutator {
    /// Bulk rename files matching regex pattern ^test_.*\.txt$ -> archive_.*\.txt inside directory
    /// Checks grants via PermissionManager, records in undo_journal, performs rename, and supports rollback.
    pub fn bulk_rename_with_undo(
        conn: &Connection,
        session_id: &str,
        dir_path: &Path,
    ) -> Result<Vec<(PathBuf, PathBuf)>, String> {
        let dir_str = dir_path.to_string_lossy();
        let access = PermissionManager::check_file_access(conn, session_id, &dir_str, "write")
            .map_err(|e| e.to_string())?;
        
        if access != PermissionDecision::Approved {
            return Err(format!("Access denied to directory {} for session {}", dir_str, session_id));
        }

        let mut renames = Vec::new();
        let entries = fs::read_dir(dir_path).map_err(|e| e.to_string())?;

        let re = regex::Regex::new(r"^test_.*\.txt$").map_err(|e| e.to_string())?;

        for entry in entries {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_file() {
                if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                    if re.is_match(file_name) {
                        let new_name = file_name.replace("test_", "archive_");
                        let new_path = path.with_file_name(new_name);
                        renames.push((path, new_path));
                    }
                }
            }
        }

        let mut applied_renames = Vec::new();

        for (orig, dest) in &renames {
            let inverse_patch = serde_json::json!({
                "from": dest.to_string_lossy(),
                "to": orig.to_string_lossy()
            }).to_string();

            conn.execute(
                "INSERT INTO undo_journal (session_id, op_type, target_path, inverse_patch, status)
                 VALUES (?1, 'file_rename', ?2, ?3, 'pending')",
                params![session_id, orig.to_string_lossy().to_string(), inverse_patch],
            ).map_err(|e| e.to_string())?;
            
            let undo_id = conn.last_insert_rowid();

            fs::rename(orig, dest).map_err(|e| e.to_string())?;

            conn.execute(
                "UPDATE undo_journal SET status = 'applied' WHERE id = ?1",
                params![undo_id],
            ).map_err(|e| e.to_string())?;

            applied_renames.push((orig.clone(), dest.clone()));
        }

        Ok(applied_renames)
    }

    /// Rollback unapplied/applied renames in undo_journal for a session
    pub fn rollback(conn: &Connection, session_id: &str) -> Result<(), String> {
        let mut stmt = conn.prepare(
            "SELECT id, target_path, inverse_patch FROM undo_journal 
             WHERE session_id = ?1 AND status = 'applied' ORDER BY id DESC"
        ).map_err(|e| e.to_string())?;

        let rows = stmt.query_map(params![session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        }).map_err(|e| e.to_string())?;

        for row in rows {
            let (id, _target_path, inverse_patch_str) = row.map_err(|e| e.to_string())?;
            let patch: serde_json::Value = serde_json::from_str(&inverse_patch_str).map_err(|e| e.to_string())?;
            
            let current_path = patch["from"].as_str().ok_or("Invalid patch from")?;
            let original_path = patch["to"].as_str().ok_or("Invalid patch to")?;

            if Path::new(current_path).exists() {
                fs::rename(current_path, original_path).map_err(|e| e.to_string())?;
            }

            conn.execute(
                "UPDATE undo_journal SET status = 'reverted' WHERE id = ?1",
                params![id],
            ).map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}
