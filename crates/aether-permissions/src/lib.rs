use rusqlite::{Connection, Result, params};
use serde::{Deserialize, Serialize};
use sha2::{Sha256, Digest};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PermissionDecision {
    Approved,
    Denied,
    AutoAllowed,
}

pub struct PermissionManager;

impl PermissionManager {
    /// Grant-based enforcement with path canonicalization and subpath inheritance.
    /// A path is allowed when an explicit (or app-lifetime) grant matches exactly,
    /// or when the canonical requested path lies under a granted parent directory.
    pub fn check_file_access(
        conn: &Connection,
        session_id: &str,
        path: &str,
        operation: &str,
    ) -> Result<PermissionDecision> {
        let requested = match canonicalize_access_path(path) {
            Ok(p) => p,
            Err(_) => return Ok(PermissionDecision::Denied),
        };

        let mut stmt = conn.prepare(
            "SELECT resource_path FROM capability_grants
             WHERE (session_id = ?1 OR session_id IS NULL)
               AND permission_type = ?2
               AND is_stale = 0",
        )?;

        let grant_paths: Vec<String> = stmt
            .query_map(params![session_id, operation], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        for grant_path in grant_paths {
            let granted = match canonicalize_access_path(&grant_path) {
                Ok(p) => p,
                Err(_) => continue,
            };
            if paths_match(&requested, &granted) || path_is_subpath(&requested, &granted) {
                return Ok(PermissionDecision::Approved);
            }
        }

        Ok(PermissionDecision::Denied)
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

/// Normalize and canonicalize a filesystem path for permission checks.
/// Resolves `.` / `..` components without allowing traversal past the root,
/// then uses `fs::canonicalize` when the target exists.
pub fn canonicalize_access_path(path: &str) -> Result<PathBuf, String> {
    let path_obj = Path::new(path);
    for component in path_obj.components() {
        if matches!(component, Component::ParentDir) {
            return Err("Path traversal denied: '..' component not allowed".into());
        }
    }

    let normalized = normalize_path_components(path)?;
    if normalized.as_os_str().is_empty() {
        return Err("Empty path after normalization".into());
    }

    if normalized.exists() {
        fs::canonicalize(&normalized).map_err(|e| e.to_string())
    } else {
        Ok(normalized)
    }
}

fn normalize_path_components(path: &str) -> Result<PathBuf, String> {
    let path = Path::new(path);
    let mut stack: Vec<Component<'_>> = Vec::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                return Err("Path traversal denied: '..' component not allowed".into());
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                stack.push(component);
            }
        }
    }

    Ok(stack.iter().collect())
}

fn paths_match(a: &Path, b: &Path) -> bool {
    a == b
}

/// True when `child` is the same path as `parent` or a descendant (not a prefix false-positive).
pub fn path_is_subpath(child: &Path, parent: &Path) -> bool {
    if child == parent {
        return true;
    }
    if !child.starts_with(parent) {
        return false;
    }
    let parent_len = parent.as_os_str().len();
    let child_bytes = child.as_os_str().as_encoded_bytes();
    child_bytes.len() > parent_len && child_bytes[parent_len] == b'/'
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
        let dir_canonical = dir_path
            .canonicalize()
            .map_err(|e| format!("Failed to canonicalize directory: {}", e))?;
        let dir_str = dir_canonical.to_string_lossy().to_string();
        let access = PermissionManager::check_file_access(conn, session_id, &dir_str, "write")
            .map_err(|e| e.to_string())?;

        if access != PermissionDecision::Approved {
            return Err(format!("Access denied to directory {} for session {}", dir_str, session_id));
        }

        let mut renames = Vec::new();
        let entries = fs::read_dir(&dir_canonical).map_err(|e| e.to_string())?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_rejects_parent_traversal() {
        assert!(normalize_path_components("/tmp/workspace/../../../etc/passwd").is_err());
        assert!(canonicalize_access_path("/tmp/../etc/passwd").is_err());
    }

    #[test]
    fn test_subpath_matching() {
        let parent = PathBuf::from("/tmp/workspace");
        let child = PathBuf::from("/tmp/workspace/nested/file.txt");
        assert!(path_is_subpath(&child, &parent));
        assert!(!path_is_subpath(&PathBuf::from("/tmp/workspace-evil"), &parent));
    }
}
