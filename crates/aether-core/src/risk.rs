//! Pre-flight approval gate (Phase 10 slices 10.8-10.9 / PERM-02).
//!
//! Classifies a plan's steps *before any tool runs*, so a batch of risky operations can be shown
//! to a human for confirmation with a hard guarantee that nothing has executed yet — this module
//! never touches the filesystem beyond a read-only existence check, never calls `ToolRegistry`,
//! and has no side effects of its own.
//!
//! Scope: this tool set has no explicit "delete" action and no network-egress concept, so the two
//! roadmap examples ("deletions", "unseen-domain egress") are mapped onto their closest real
//! analogs: overwriting a file that already exists (destroying its prior content, the same
//! destructive shape as a delete) and any `mcp_call` (the only way this agent reaches outside the
//! workspace/local tools at all). Read-only and already-vetted operations (`fs_read`,
//! `verify_contains`, `python_lint`, `git_init`, `skill_execute`, `done`) are never risky.

use crate::loop_engine::resolve_workspace_path;
use crate::ToolInvocation;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RiskyStep {
    pub index: usize,
    pub tool: String,
    pub reason: String,
}

/// Scan a plan for steps that must not execute without explicit human approval. Pure and
/// read-only: the only filesystem interaction is checking whether a write target already exists.
pub fn find_steps_requiring_approval(workspace: &PathBuf, plan: &[ToolInvocation]) -> Vec<RiskyStep> {
    let mut risky = Vec::new();
    for (index, step) in plan.iter().enumerate() {
        match step {
            ToolInvocation::FsWrite { path, .. } => {
                if let Ok(full) = resolve_workspace_path(workspace, path) {
                    if full_exists(&full) {
                        risky.push(RiskyStep {
                            index,
                            tool: "fs_write".into(),
                            reason: format!("would overwrite existing file: {path}"),
                        });
                    }
                }
            }
            ToolInvocation::McpCall { server, tool, .. } => {
                risky.push(RiskyStep {
                    index,
                    tool: "mcp_call".into(),
                    reason: format!("external tool call to {server}/{tool}"),
                });
            }
            _ => {}
        }
    }
    risky
}

fn full_exists(path: &Path) -> bool {
    path.exists()
}

/// Returns `Some(risky_steps)` when the plan must be blocked pending approval, `None` when it is
/// safe to proceed (no risky steps, or the caller already has explicit approval). Callers must
/// treat `Some` as a hard stop: do not invoke `ToolRegistry`/`execute_structured_loop` for any
/// step of the plan, not just the risky ones — a batched approval screen shows the whole plan
/// before anything runs.
pub fn evaluate_approval_gate(
    workspace: &PathBuf,
    plan: &[ToolInvocation],
    approved: bool,
) -> Option<Vec<RiskyStep>> {
    if approved {
        return None;
    }
    let risky = find_steps_requiring_approval(workspace, plan);
    if risky.is_empty() {
        None
    } else {
        Some(risky)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_file_write_is_not_risky() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let plan = vec![ToolInvocation::FsWrite {
            path: "new.txt".into(),
            content: "hello".into(),
        }];
        assert!(find_steps_requiring_approval(&workspace, &plan).is_empty());
        assert_eq!(evaluate_approval_gate(&workspace, &plan, false), None);
    }

    #[test]
    fn overwriting_existing_file_is_risky() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        std::fs::write(workspace.join("existing.txt"), "old").unwrap();
        let plan = vec![ToolInvocation::FsWrite {
            path: "existing.txt".into(),
            content: "new".into(),
        }];
        let risky = find_steps_requiring_approval(&workspace, &plan);
        assert_eq!(risky.len(), 1);
        assert_eq!(risky[0].tool, "fs_write");
        assert!(risky[0].reason.contains("existing.txt"));
    }

    #[test]
    fn mcp_call_is_always_risky() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let plan = vec![ToolInvocation::McpCall {
            server: "filesystem".into(),
            tool: "write_file".into(),
            args: serde_json::json!({}),
            secret_env: None,
        }];
        let risky = find_steps_requiring_approval(&workspace, &plan);
        assert_eq!(risky.len(), 1);
        assert_eq!(risky[0].tool, "mcp_call");
    }

    #[test]
    fn gate_blocks_without_approval_and_clears_with_it() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        std::fs::write(workspace.join("existing.txt"), "old").unwrap();
        let plan = vec![ToolInvocation::FsWrite {
            path: "existing.txt".into(),
            content: "new".into(),
        }];

        let blocked = evaluate_approval_gate(&workspace, &plan, false);
        assert!(blocked.is_some());
        assert_eq!(blocked.unwrap().len(), 1);

        assert_eq!(evaluate_approval_gate(&workspace, &plan, true), None);
    }

    #[test]
    fn read_only_and_prevetted_tools_are_never_risky() {
        let tmp = tempfile::tempdir().unwrap();
        let workspace = tmp.path().to_path_buf();
        let plan = vec![
            ToolInvocation::FsRead { path: "a.txt".into() },
            ToolInvocation::VerifyContains { path: "a.txt".into(), text: "x".into() },
            ToolInvocation::PythonLint { source: "def ok(): pass".into() },
            ToolInvocation::GitInit { branch: "main".into() },
            ToolInvocation::SkillExecute {
                skill_id: "s1".into(),
                variables: Default::default(),
            },
            ToolInvocation::Done,
        ];
        assert!(find_steps_requiring_approval(&workspace, &plan).is_empty());
    }
}
