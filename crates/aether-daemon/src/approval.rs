//! Immutable, single-use approval records.
//!
//! A human approves the exact normalized plan persisted here. The follow-up IPC request carries
//! only an opaque approval id; it never re-runs the planner and cannot substitute a different
//! workspace, session, or plan.

use aether_core::{secure_random_token, ToolInvocation};
use aether_permissions::{PermissionDecision, PermissionManager};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_APPROVAL_TTL_SECS: u64 = 10 * 60;
const MAX_APPROVAL_TTL_SECS: u64 = 60 * 60;

#[derive(Debug, Clone)]
pub struct PendingApprovalEnvelope {
    pub approval_id: String,
    pub plan_json: String,
    pub plan_digest: String,
    pub expires_at: u64,
    pub max_iterations: usize,
    pub max_tokens: usize,
}

#[derive(Debug, Clone)]
pub struct ApprovedPlan {
    pub approval_id: String,
    pub session_id: String,
    pub workspace: PathBuf,
    pub prompt: String,
    pub plan: Vec<ToolInvocation>,
    pub max_iterations: usize,
    pub max_tokens: usize,
}

fn now_secs() -> Result<u64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| error.to_string())
}

fn approval_ttl_secs() -> u64 {
    std::env::var("AETHER_APPROVAL_TTL_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(DEFAULT_APPROVAL_TTL_SECS)
        .clamp(1, MAX_APPROVAL_TTL_SECS)
}

fn canonical_workspace(workspace: &Path) -> Result<PathBuf, String> {
    workspace
        .canonicalize()
        .map_err(|error| format!("approval workspace canonicalization failed: {error}"))
}

fn plan_digest(
    session_id: &str,
    workspace: &Path,
    plan_json: &str,
    max_iterations: usize,
    max_tokens: usize,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"aether-approval-v2\0");
    hasher.update(session_id.as_bytes());
    hasher.update(b"\0");
    hasher.update(workspace.to_string_lossy().as_bytes());
    hasher.update(b"\0");
    hasher.update(plan_json.as_bytes());
    hasher.update(b"\0");
    hasher.update(max_iterations.to_le_bytes());
    hasher.update(max_tokens.to_le_bytes());
    format!("{:x}", hasher.finalize())
}

pub fn create_pending_approval(
    conn: &Connection,
    session_id: &str,
    workspace: &Path,
    prompt: &str,
    plan: &[ToolInvocation],
    max_iterations: usize,
    max_tokens: usize,
) -> Result<PendingApprovalEnvelope, String> {
    let approval_id = format!(
        "approval.{}",
        secure_random_token()
            .map_err(|error| error.to_string())?
            .trim_start_matches("v2.")
    );
    let workspace = canonical_workspace(workspace)?;
    let plan_json = serde_json::to_string(plan).map_err(|error| error.to_string())?;
    let digest = plan_digest(
        session_id,
        &workspace,
        &plan_json,
        max_iterations,
        max_tokens,
    );
    let created_at = now_secs()?;
    let expires_at = created_at.saturating_add(approval_ttl_secs());

    conn.execute(
        "INSERT INTO pending_approvals
         (approval_id, session_id, workspace_path, prompt, plan_json, plan_digest,
          max_iterations, max_tokens, status, created_at, expires_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending', ?9, ?10)",
        params![
            approval_id,
            session_id,
            workspace.to_string_lossy().to_string(),
            prompt,
            plan_json,
            digest,
            max_iterations as i64,
            max_tokens as i64,
            created_at as i64,
            expires_at as i64,
        ],
    )
    .map_err(|error| error.to_string())?;
    PermissionManager::audit_decision(
        conn,
        session_id,
        "approval_requested",
        &serde_json::json!({
            "approval_id": approval_id,
            "plan_digest": digest,
            "expires_at": expires_at,
            "max_iterations": max_iterations,
            "max_tokens": max_tokens,
        })
        .to_string(),
        &PermissionDecision::Denied,
        None,
        None,
    )
    .map_err(|error| error.to_string())?;

    Ok(PendingApprovalEnvelope {
        approval_id,
        plan_json,
        plan_digest: digest,
        expires_at,
        max_iterations,
        max_tokens,
    })
}

/// Atomically consume an approval while the daemon's database mutex is held by the caller.
/// The status transition happens before execution, making replay fail closed even if execution
/// later fails or the client disconnects.
pub fn consume_pending_approval(
    conn: &Connection,
    approval_id: &str,
    expected_session_id: Option<&str>,
    expected_workspace: Option<&Path>,
) -> Result<ApprovedPlan, String> {
    let (
        session_id,
        workspace_path,
        prompt,
        plan_json,
        stored_digest,
        max_iterations,
        max_tokens,
        status,
        expires_at,
    ): (String, String, String, String, String, i64, i64, String, i64) = conn
        .query_row(
            "SELECT session_id, workspace_path, prompt, plan_json, plan_digest,
                    max_iterations, max_tokens, status, expires_at
             FROM pending_approvals WHERE approval_id = ?1",
            params![approval_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ))
            },
        )
        .map_err(|_| "approval id not found".to_string())?;

    if status != "pending" {
        return Err(format!("approval id is {status}, not pending"));
    }
    if now_secs()? > expires_at.max(0) as u64 {
        conn.execute(
            "UPDATE pending_approvals SET status = 'expired' WHERE approval_id = ?1 AND status = 'pending'",
            params![approval_id],
        )
        .map_err(|error| error.to_string())?;
        return Err("approval id has expired".into());
    }
    if let Some(expected) = expected_session_id {
        if expected != session_id {
            return Err("approval session does not match request session".into());
        }
    }

    let workspace = canonical_workspace(Path::new(&workspace_path))?;
    let max_iterations = usize::try_from(max_iterations)
        .map_err(|_| "stored approval max_iterations is invalid")?;
    let max_tokens = usize::try_from(max_tokens)
        .map_err(|_| "stored approval max_tokens is invalid")?;
    let actual_digest = plan_digest(
        &session_id,
        &workspace,
        &plan_json,
        max_iterations,
        max_tokens,
    );
    if actual_digest != stored_digest {
        return Err("stored approval plan digest mismatch; execution blocked".into());
    }
    if let Some(expected) = expected_workspace {
        if canonical_workspace(expected)? != workspace {
            return Err("approval workspace does not match request workspace".into());
        }
    }

    let updated = conn
        .execute(
            "UPDATE pending_approvals
             SET status = 'consumed', consumed_at = ?2
             WHERE approval_id = ?1 AND status = 'pending'",
            params![approval_id, now_secs()? as i64],
        )
        .map_err(|error| error.to_string())?;
    if updated != 1 {
        return Err("approval id was already consumed".into());
    }

    let plan = serde_json::from_str(&plan_json).map_err(|error| {
        format!("stored approval plan is invalid; execution blocked: {error}")
    })?;
    PermissionManager::audit_decision(
        conn,
        &session_id,
        "approval_consumed",
        &serde_json::json!({
            "approval_id": approval_id,
            "plan_digest": stored_digest,
            "max_iterations": max_iterations,
            "max_tokens": max_tokens,
        })
        .to_string(),
        &PermissionDecision::Approved,
        Some(0),
        None,
    )
    .map_err(|error| error.to_string())?;
    Ok(ApprovedPlan {
        approval_id: approval_id.to_string(),
        session_id,
        workspace,
        prompt,
        plan,
        max_iterations,
        max_tokens,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aether_db::Database;

    #[test]
    fn approval_is_bound_to_plan_session_workspace_and_single_use() {
        let db = Database::open_in_memory().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let conn = db.conn();
        conn.execute(
            "INSERT INTO sessions (id, title, status) VALUES ('approval-session', 't', 'active')",
            [],
        )
        .unwrap();
        let plan = vec![ToolInvocation::FsWrite {
            path: "out.txt".into(),
            content: "approved bytes".into(),
        }];
        let envelope = create_pending_approval(
            &conn,
            "approval-session",
            workspace.path(),
            "write approved bytes",
            &plan,
            8,
            16_384,
        )
        .unwrap();
        let id = envelope.approval_id;

        assert!(consume_pending_approval(
            &conn,
            &id,
            Some("wrong-session"),
            Some(workspace.path())
        )
        .is_err());
        let approved = consume_pending_approval(
            &conn,
            &id,
            Some("approval-session"),
            Some(workspace.path()),
        )
        .unwrap();
        assert_eq!(approved.plan.len(), 1);
        assert!(consume_pending_approval(
            &conn,
            &id,
            Some("approval-session"),
            Some(workspace.path())
        )
        .is_err());
    }
}
