//! PreToolUse hooks (Phase 10 slice 10.7 / HOOK-01).
//!
//! A hook is a hard override evaluated *before* a tool runs, on top of (not instead of) grant
//! checks: even an explicit user grant and an explicit prompt instruction telling the agent to
//! touch a denylisted path cannot make a hook allow it. This is the anti-theater bar this slice
//! has to clear — a hook that only logs an advisory warning is not a hook, it is a comment.
//!
//! Scope: one concrete hook (a path denylist covering common secret/credential file shapes),
//! wired into the two tools that carry a concrete filesystem path (`fs_write`, `fs_read`). The
//! roadmap's fuller hook engine (`SessionStart`/`UserPromptSubmit`/`PostToolUse`/etc., pluggable
//! shell/HTTP/LLM actions, hooks merging across sources) is Phase 10 slice 10.6, not implemented
//! here — this slice only proves `PreToolUse` can genuinely block.

use std::path::Path;

/// Case-insensitive substring patterns matched against the resolved (canonicalized) target path.
/// No grant and no prompt instruction can override a match — see module docs.
pub const DEFAULT_DENY_PATH_PATTERNS: &[&str] = &[
    ".env",
    "id_rsa",
    "id_ed25519",
    ".ssh/",
    ".git/config",
    ".aws/credentials",
    ".aws/config",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    Allow,
    Deny(String),
}

/// Evaluate the `PreToolUse` hook against a resolved filesystem path. Call this with the fully
/// resolved (workspace-joined) path, not the raw tool argument, so a path that only *looks* safe
/// before resolution can't slip through.
pub fn pre_tool_use_path_check(resolved_path: &Path) -> HookDecision {
    let path_str = resolved_path.to_string_lossy().to_ascii_lowercase();
    for pattern in DEFAULT_DENY_PATH_PATTERNS {
        if path_str.contains(pattern) {
            return HookDecision::Deny(format!(
                "PreToolUse hook blocked access to a sensitive path matching {pattern:?}: {}",
                resolved_path.display()
            ));
        }
    }
    HookDecision::Allow
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn blocks_dotenv_regardless_of_casing() {
        let decision = pre_tool_use_path_check(&PathBuf::from("/workspace/project/.ENV"));
        assert!(matches!(decision, HookDecision::Deny(_)));
    }

    #[test]
    fn blocks_ssh_private_key() {
        let decision = pre_tool_use_path_check(&PathBuf::from("/home/user/.ssh/id_rsa"));
        assert!(matches!(decision, HookDecision::Deny(_)));
    }

    #[test]
    fn blocks_git_config() {
        let decision = pre_tool_use_path_check(&PathBuf::from("/workspace/repo/.git/config"));
        assert!(matches!(decision, HookDecision::Deny(_)));
    }

    #[test]
    fn allows_ordinary_workspace_file() {
        let decision = pre_tool_use_path_check(&PathBuf::from("/workspace/project/notes.txt"));
        assert_eq!(decision, HookDecision::Allow);
    }

    #[test]
    fn deny_reason_names_the_matched_pattern() {
        match pre_tool_use_path_check(&PathBuf::from("/workspace/.env")) {
            HookDecision::Deny(reason) => assert!(reason.contains(".env")),
            HookDecision::Allow => panic!("expected deny"),
        }
    }
}
