//! Outbound denial verbosity — "state the principle, not the detection mechanics" (RED-02).
//!
//! A denial has two audiences. The local principal (CLI, daemon IPC, the macOS app) is the person
//! the policy exists to protect and needs the full reason to act on it. A *remote* requester —
//! gateway inbound from Slack/Telegram/Discord, or an automation trigger fired by a webhook — is
//! not the principal, and a full reason tells them exactly which cue tripped, where the line sits,
//! and what to rephrase next time. Narrating the boundary teaches how to route around it, and it
//! does so in the one place the requester is guaranteed to read.
//!
//! So the producer keeps full detail and records it in the audit log; the outbound boundary renders
//! it through [`render_denial`] at [`ErrorDetailLevel::Principle`], which discloses only the policy
//! *category* plus a correlation reference support can look up. Which rule fired, which pattern
//! matched, and which path was rejected stay inside the process.
//!
//! This is enforced at the boundary rather than asked for in a prompt, because a prompt cannot be
//! relied on to stay quiet and a `format!` can. See `docs/REVIEW_FABLE51_HARNESS.md` finding P1-4:
//! the reference harness states this rule for refusals ("not which cues tripped, where the line
//! sits, or what test it applied"), and it is channel-agnostic — it applies to a coding agent's
//! denials exactly as much as to a chat assistant's.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// How much a denial may reveal to whoever receives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorDetailLevel {
    /// Local principal: full detail, including the rule and the offending value.
    Full,
    /// Remote / non-local requester: policy category + correlation reference only.
    Principle,
}

impl Default for ErrorDetailLevel {
    fn default() -> Self {
        // Fail toward debuggability: an entry point that forgets to declare itself is treated as
        // local. Remote boundaries opt into `Principle` explicitly (see `run_gateway_inbound`).
        ErrorDetailLevel::Full
    }
}

/// Policy category a denial belongs to.
///
/// Deliberately coarse. The category is safe to disclose to a remote requester; the specific rule
/// that produced it is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenialCategory {
    /// Rejected at the UserPromptSubmit hook.
    PromptPolicy,
    /// Rejected at the PreToolUse sensitive-path hook.
    PathPolicy,
    /// Path left the workspace, or was absolute.
    WorkspaceEscape,
    /// No capability grant or approval for the operation.
    Permission,
    /// A plan step correlated with untrusted tool output (INJECT-01).
    UntrustedInduction,
    /// The post-write verify shell refused `done` (CHECK-01 / CHECK-02).
    Verification,
    /// Iteration or token budget exhausted.
    Budget,
    /// Not classified — disclosed as a generic workspace policy refusal.
    Other,
}

impl DenialCategory {
    /// The only phrase about this denial a remote requester is allowed to see.
    pub fn public_label(self) -> &'static str {
        match self {
            DenialCategory::PromptPolicy => "request policy",
            DenialCategory::PathPolicy => "protected path policy",
            DenialCategory::WorkspaceEscape => "workspace boundary policy",
            DenialCategory::Permission => "capability grant policy",
            DenialCategory::UntrustedInduction => "untrusted input policy",
            DenialCategory::Verification => "verification policy",
            DenialCategory::Budget => "budget policy",
            DenialCategory::Other => "workspace policy",
        }
    }
}

/// Classify a full denial message by the stable markers its producing site emits.
///
/// Order matters: several messages mention both a path and a policy, so the most specific producer
/// is matched first and the permission bucket is the fallback for anything still saying "denied".
pub fn classify_denial(full: &str) -> DenialCategory {
    if full.contains("UserPromptSubmit hook") {
        return DenialCategory::PromptPolicy;
    }
    if full.contains("PreToolUse hook") {
        return DenialCategory::PathPolicy;
    }
    if full.contains("Path escapes workspace")
        || full.contains("Absolute path denied")
        || full.contains("Workspace canonicalize failed")
    {
        return DenialCategory::WorkspaceEscape;
    }
    if full.contains("INJECT-01") || full.contains("blocked replan") || full.contains("correlated")
    {
        return DenialCategory::UntrustedInduction;
    }
    if full.contains("Loop blocked")
        || full.contains("Verifier denied")
        || full.contains("Verify failed")
    {
        return DenialCategory::Verification;
    }
    if full.contains("Max iterations")
        || full.contains("Token budget")
        || full.contains("budget exceeded")
    {
        return DenialCategory::Budget;
    }
    if full.contains("Write denied")
        || full.contains("Read denied")
        || full.contains("grant")
        || full.contains("approval")
        || full.contains("denied")
    {
        return DenialCategory::Permission;
    }
    DenialCategory::Other
}

fn fnv1a64(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Per-process salt for [`reference_id`].
///
/// The deny pattern lists are public source, so a reference id that was a pure hash of the message
/// would be a confirmation oracle: a remote requester could enumerate the handful of patterns, hash
/// each, and learn which one fired. Salting with a per-process nonce makes the id stable *within*
/// one daemon lifetime (so support can correlate a user report against the audit log) but not
/// reproducible across runs, which removes the oracle.
fn process_nonce() -> u64 {
    static NONCE: OnceLock<u64> = OnceLock::new();
    *NONCE.get_or_init(|| {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::from(d.subsec_nanos()))
            .unwrap_or(0);
        (nanos ^ (u64::from(std::process::id()) << 16)) | 1
    })
}

/// Short correlation handle for a denial, safe to disclose at [`ErrorDetailLevel::Principle`].
pub fn reference_id(full: &str) -> String {
    format!(
        "{:08x}",
        ((fnv1a64(full) ^ process_nonce()) & 0x0000_0000_FFFF_FFFF) as u32
    )
}

/// Render a denial for its audience.
///
/// `Full` returns the producer's message unchanged. `Principle` returns only the policy category
/// and a correlation reference — callers are responsible for having already recorded the full
/// message somewhere durable (audit log / session log) before redacting it.
pub fn render_denial(level: ErrorDetailLevel, full: &str) -> String {
    match level {
        ErrorDetailLevel::Full => full.to_string(),
        ErrorDetailLevel::Principle => format!(
            "blocked by {} [ref {}]",
            classify_denial(full).public_label(),
            reference_id(full)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{
        pre_tool_use_path_check, DEFAULT_DENY_PATH_PATTERNS, DEFAULT_DENY_PROMPT_PATTERNS,
        DEFAULT_REDACT_OUTPUT_PATTERNS, HookDecision, HookEngine,
    };
    use std::path::Path;

    /// Every real deny site's message, produced by calling the site rather than by hand-copying a
    /// string — so this test tracks the messages as they actually are.
    fn real_denials() -> Vec<String> {
        let mut out = Vec::new();
        for pattern in DEFAULT_DENY_PROMPT_PATTERNS {
            if let Err(reason) = crate::enforce_user_prompt_submit(&format!("please {pattern} now")) {
                out.push(reason);
            }
        }
        for pattern in DEFAULT_DENY_PATH_PATTERNS {
            if let HookDecision::Deny(reason) =
                pre_tool_use_path_check(Path::new(&format!("/workspace/{pattern}")))
            {
                out.push(reason);
            }
        }
        out.push("Absolute path denied outside workspace: /etc/passwd".into());
        out.push("Path escapes workspace grant: ../../outside.txt".into());
        out.push("Write denied for target path /workspace/.env".into());
        out.push("Read denied for /home/user/.ssh/id_rsa".into());
        out.push(
            "Loop blocked: report.py was written but never linted as an artifact".into(),
        );
        out.push("INJECT-01 blocked replan 1: obs#2→mcp_call: correlated with tool output".into());
        out
    }

    #[test]
    fn principle_level_leaks_no_pattern_path_or_rule() {
        let denials = real_denials();
        assert!(
            denials.len() >= 8,
            "expected the deny sites to actually fire, got {}",
            denials.len()
        );
        for full in &denials {
            let rendered = render_denial(ErrorDetailLevel::Principle, full);
            // None of the secret vocabulary may survive the boundary.
            for forbidden in DEFAULT_DENY_PROMPT_PATTERNS
                .iter()
                .chain(DEFAULT_DENY_PATH_PATTERNS.iter())
                .chain(DEFAULT_REDACT_OUTPUT_PATTERNS.iter())
            {
                assert!(
                    !rendered.to_ascii_lowercase().contains(forbidden),
                    "principle rendering leaked {forbidden:?}: {rendered}"
                );
            }
            for marker in [
                "UserPromptSubmit",
                "PreToolUse",
                "hook",
                "matching",
                "Loop blocked",
                "INJECT-01",
                "escapes workspace",
                "Absolute path",
                "/etc/passwd",
                ".ssh",
                ".env",
                "report.py",
                "obs#",
            ] {
                assert!(
                    !rendered.contains(marker),
                    "principle rendering leaked detection mechanics {marker:?}: {rendered}"
                );
            }
            // It must still say *something* true and useful.
            assert!(rendered.starts_with("blocked by "), "unusable rendering: {rendered}");
            assert!(rendered.contains("[ref "), "missing correlation reference: {rendered}");
        }
    }

    #[test]
    fn full_level_is_unchanged_for_the_local_principal() {
        let full = "PreToolUse hook blocked access to a sensitive path matching \".env\": /w/.env";
        assert_eq!(render_denial(ErrorDetailLevel::Full, full), full);
        assert_eq!(ErrorDetailLevel::default(), ErrorDetailLevel::Full);
    }

    #[test]
    fn classification_covers_every_producer() {
        assert_eq!(
            classify_denial("UserPromptSubmit hook blocked prompt matching \"dump secrets\""),
            DenialCategory::PromptPolicy
        );
        assert_eq!(
            classify_denial("PreToolUse hook blocked access to a sensitive path"),
            DenialCategory::PathPolicy
        );
        assert_eq!(
            classify_denial("Path escapes workspace grant: ../x"),
            DenialCategory::WorkspaceEscape
        );
        assert_eq!(
            classify_denial("Write denied for target path /w/a.txt"),
            DenialCategory::Permission
        );
        assert_eq!(
            classify_denial("INJECT-01 blocked replan 1: obs#2→mcp_call"),
            DenialCategory::UntrustedInduction
        );
        assert_eq!(
            classify_denial("Loop blocked: done before verify_contains after fs_write"),
            DenialCategory::Verification
        );
        assert_eq!(
            classify_denial("Max iterations (8) exceeded"),
            DenialCategory::Budget
        );
        assert_eq!(classify_denial("something else entirely"), DenialCategory::Other);
    }

    #[test]
    fn reference_id_is_stable_in_process_and_distinct_per_message() {
        let a = reference_id("Write denied for target path /w/a.txt");
        let b = reference_id("Write denied for target path /w/a.txt");
        let c = reference_id("Write denied for target path /w/b.txt");
        assert_eq!(a, b, "same denial must correlate to the same reference");
        assert_ne!(a, c, "different denials must not share a reference");
        assert_eq!(a.len(), 8, "reference must stay short enough to quote");
        assert!(
            a.chars().all(|ch| ch.is_ascii_hexdigit()),
            "reference must be hex: {a}"
        );
    }

    #[test]
    fn hook_engine_still_allows_ordinary_input() {
        // Guard against the redaction layer being "fixed" by denying everything.
        assert_eq!(
            HookEngine::production().run_user_prompt_submit("write hello.txt and verify it"),
            HookDecision::Allow
        );
    }
}
