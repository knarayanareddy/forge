//! Repair-enabling tool errors (P1-9 / LOOP-05).
//!
//! A refusal that only says *what* was refused costs the caller twice: once for the failed step and
//! again for every replan attempt spent trying to fix something no plan can fix. The reference
//! prompt makes rejections repair-enabling **by construction** — the error carries the constraint
//! value ("rejected with the byte limit in the error") or the state needed to retry ("return the
//! current content so you can retry with its version").
//!
//! This module is the forge-side version of that. Every tool failure is classified into a
//! [`ToolError`] with four parts:
//!
//! * `reason` — what was refused or failed (unchanged from the producing site, so existing
//!   assertions, `classify_denial`, and audit rows keep working);
//! * `remedy` — what would make it succeed, addressed to whoever can act on it (the planner when
//!   the plan can change, the human when it cannot);
//! * `constraint` — the machine-readable value the retry needs (the path, the required permission,
//!   the connected-server inventory, the expected substring);
//! * `retryable` — whether *a replan of the same goal* can plausibly succeed. This is the field
//!   [`crate::run_structured_with_replan`] consults before spending one of its bounded attempts.
//!
//! Classification is marker-based over the message each producing site already emits, exactly like
//! [`crate::classify_denial`], so there is one vocabulary to keep in sync rather than two.
//!
//! Rendering is additive and reversible-by-inspection: `reason | remedy: … | constraint: {…} |
//! retryable: no`. `reason` stays the leading substring, which is what keeps every
//! `contains("Write denied")`-style assertion in the harness and the audit trail valid.

use crate::error_detail::{classify_denial, DenialCategory};
use crate::loop_engine::ToolObservation;
use serde_json::{json, Value};

/// Separator between the producing site's own message and the remedy text. Anything at or after
/// this marker in a message is *this module's* output, so re-classifying an already-rendered error
/// strips it first and never mistakes a remedy for a reason.
pub const REMEDY_SEPARATOR: &str = " | remedy: ";

/// What the caller knows about the environment, used to turn "no such server" into an actionable
/// inventory instead of a dead end. Empty is a valid state and means "nothing is connected".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Inventory {
    /// MCP servers currently connected and pinned (the allowlist's `name`s).
    pub connected_mcp_servers: Vec<String>,
    /// Skill ids currently installed *and trusted* enough to execute.
    pub installed_skills: Vec<String>,
}

impl Inventory {
    pub fn new(connected_mcp_servers: Vec<String>, installed_skills: Vec<String>) -> Self {
        Self {
            connected_mcp_servers,
            installed_skills,
        }
    }

    fn servers_rendered(&self) -> String {
        if self.connected_mcp_servers.is_empty() {
            "none are connected".to_string()
        } else {
            format!("connected: {}", self.connected_mcp_servers.join(", "))
        }
    }

    fn skills_rendered(&self) -> String {
        if self.installed_skills.is_empty() {
            "no skills are installed".to_string()
        } else {
            format!("installed: {}", self.installed_skills.join(", "))
        }
    }
}

/// A tool failure with the information needed to act on it.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolError {
    /// The producing site's own message, verbatim, always the leading substring of [`Self::render`].
    pub reason: String,
    /// What would make this succeed.
    pub remedy: String,
    /// Machine-readable values a retry needs. `None` when the reason already carries them.
    pub constraint: Option<Value>,
    /// Whether replanning the same goal can plausibly succeed. `false` means stop and report.
    pub retryable: bool,
}

impl ToolError {
    /// Classify a failure message with no environment knowledge.
    pub fn classify(message: &str) -> Self {
        Self::classify_with_inventory(message, &Inventory::default())
    }

    /// Classify a failure message, using `inventory` to make "not connected"/"unknown skill"
    /// actionable rather than terminal.
    pub fn classify_with_inventory(message: &str, inventory: &Inventory) -> Self {
        // Never classify our own rendering as if it were a fresh producer message.
        let reason = message
            .split(REMEDY_SEPARATOR)
            .next()
            .unwrap_or(message)
            .trim()
            .to_string();
        let lower = reason.to_ascii_lowercase();
        let contains = |needle: &str| lower.contains(needle);

        // --- Not retryable: no plan for the same goal can make these succeed. ---
        if contains("write denied for target path") {
            let path = after_marker(&reason, "Write denied for target path ");
            return Self {
                remedy: format!(
                    "requires a write grant for {path} — select the folder in AetherForge (or \
                     approve the grant request) and re-run; this plan cannot self-repair, so stop \
                     and report instead of retrying"
                ),
                constraint: Some(json!({ "permission": "write", "path": path })),
                retryable: false,
                reason,
            };
        }
        if contains("workspace write grant required") {
            return Self {
                remedy: "requires a write grant on the workspace — select the folder in \
                         AetherForge before running tools; this plan cannot self-repair"
                    .to_string(),
                constraint: Some(json!({ "permission": "write" })),
                retryable: false,
                reason,
            };
        }
        if contains("read denied for") {
            let path = after_marker(&reason, "Read denied for ");
            return Self {
                remedy: format!(
                    "requires a read grant for {path} — select the folder in AetherForge (or \
                     approve the grant request); a plan cannot grant itself access, so stop and \
                     report"
                ),
                constraint: Some(json!({ "permission": "read", "path": path })),
                retryable: false,
                reason,
            };
        }
        if contains("mcp_call grant required for workspace") {
            return Self {
                remedy: "requires an `mcp_call` grant on the workspace — approve MCP use for this \
                         session; this plan cannot self-repair"
                    .to_string(),
                constraint: Some(json!({ "permission": "mcp_call" })),
                retryable: false,
                reason,
            };
        }
        // Checked before the generic security-violation branch on purpose: an unconnected server is
        // reported as `SecurityViolation("MCP Server 'x' is not in the curated allowlist")`, and the
        // useful remedy there is the inventory of what *is* connected, not a re-pin instruction.
        if contains("is not in the curated allowlist") || contains("unknown mcp server") {
            return Self {
                remedy: format!(
                    "no MCP server by that name is connected ({}); a plan cannot connect or pin a \
                     server, so stop and report the missing capability",
                    inventory.servers_rendered()
                ),
                constraint: Some(json!({
                    "connected_servers": inventory.connected_mcp_servers,
                })),
                retryable: false,
                reason,
            };
        }
        if contains("security violation")
            || contains("hash mismatch")
            || contains("unverified digest pin")
            || contains("pin is pending")
        {
            return Self {
                remedy: "the MCP server no longer matches its content pin; execution stays blocked \
                         until a person reviews the new build and re-pins it — this plan cannot \
                         self-repair"
                    .to_string(),
                constraint: Some(json!({ "requires": "re-pin after review" })),
                retryable: false,
                reason,
            };
        }
        if contains("path escapes workspace")
            || contains("absolute path denied")
            || contains("workspace canonicalize failed")
        {
            return Self {
                remedy: "every path must stay relative to the granted workspace; restate the goal \
                         against a workspace path — this plan cannot self-repair"
                    .to_string(),
                constraint: Some(json!({ "requires": "workspace-relative path" })),
                retryable: false,
                reason,
            };
        }
        if contains("pretooluse hook blocked") || contains("userpromptsubmit hook blocked") {
            return Self {
                remedy: "policy blocks this target or request; restate the goal without it — this \
                         plan cannot self-repair and must not be retried verbatim"
                    .to_string(),
                constraint: Some(json!({
                    "category": format!("{:?}", classify_denial(&reason)),
                })),
                retryable: false,
                reason,
            };
        }
        if contains("max iterations") || contains("token budget exceeded") {
            return Self {
                remedy: "the run is out of budget; raise `max_iterations`/`max_tokens` or narrow \
                         the goal — retrying the same plan fails identically"
                    .to_string(),
                constraint: Some(json!({ "budget": "exhausted" })),
                retryable: false,
                reason,
            };
        }
        if contains("sandbox-exec required") || contains("requires darwin") {
            return Self {
                remedy: "this step needs macOS `sandbox-exec`; run the goal on Darwin — no plan \
                         change can satisfy it here"
                    .to_string(),
                constraint: Some(json!({ "platform": "darwin" })),
                retryable: false,
                reason,
            };
        }
        if contains("brokered secret") && contains("is not configured") {
            return Self {
                remedy: "the named secret is not in the keychain; a plan cannot create it — ask \
                         for it to be stored, or drop the step that needs it"
                    .to_string(),
                constraint: Some(json!({ "requires": "stored secret" })),
                retryable: false,
                reason,
            };
        }
        if contains("inject-01 blocked replan") {
            return Self {
                remedy: "the replan was induced by untrusted tool output, not by the goal; re-issue \
                         the goal explicitly if the step is genuinely wanted"
                    .to_string(),
                constraint: Some(json!({ "requires": "explicit user intent" })),
                retryable: false,
                reason,
            };
        }
        if contains("unknown skill_id") || contains("skill not installed") {
            // Repairable *only* if there is something to repair it to.
            let retryable = !inventory.installed_skills.is_empty();
            return Self {
                remedy: format!(
                    "that skill is not available ({}); {}",
                    inventory.skills_rendered(),
                    if retryable {
                        "use one of the installed ids, or drop the step if the goal does not need it"
                    } else {
                        "a plan cannot install a skill, so stop and report the missing capability"
                    }
                ),
                constraint: Some(json!({
                    "installed_skills": inventory.installed_skills,
                })),
                retryable,
                reason,
            };
        }

        // --- Retryable: the plan itself can change to make these succeed. ---
        if contains("missing ") && contains(" in ") {
            let (expected, path) = split_verify_detail(&reason);
            return Self {
                remedy: format!(
                    "read {path} back (fs_read, or fs_read with offset/limit when it is large) and \
                     either write the content that should be there or verify the substring that \
                     actually is; the expected value was {expected}"
                ),
                constraint: Some(json!({ "path": path, "expected_substring": expected })),
                retryable: true,
                reason,
            };
        }
        if contains("issue(s)") {
            return Self {
                remedy: "fix the syntax at the reported line and lint the same path again; the \
                         diagnostics above name the line"
                    .to_string(),
                constraint: Some(json!({ "requires": "corrected source" })),
                retryable: true,
                reason,
            };
        }
        if contains("cannot lint ") {
            return Self {
                remedy: "that file is not on disk yet — write it with fs_write before linting it \
                         with python_lint_file"
                    .to_string(),
                constraint: Some(json!({ "requires": "prior fs_write" })),
                retryable: true,
                reason,
            };
        }
        if contains("offset ") && contains("past the end") {
            return Self {
                remedy: "the requested read window starts beyond the end of the file; re-read from \
                         offset 0 or use the size reported in the truncation marker"
                    .to_string(),
                constraint: Some(json!({ "requires": "in-range offset" })),
                retryable: true,
                reason,
            };
        }

        // Unknown failures stay retryable: refusing to replan something we do not understand would
        // trade burned budget for silently abandoned goals.
        Self {
            reason,
            remedy: "inspect the failing observation, correct that one step, and retry; if the \
                     same step fails a second time, stop and report it rather than retrying again"
                .to_string(),
            constraint: None,
            retryable: true,
        }
    }

    /// Convenience constructors for the producing sites, so a denial leaves the site already
    /// carrying its remedy instead of being decorated somewhere upstream.
    pub fn write_denied(path: &str) -> Self {
        Self::classify(&format!("Write denied for target path {path}"))
    }

    /// See [`Self::write_denied`].
    pub fn read_denied(path: &str) -> Self {
        Self::classify(&format!("Read denied for {path}"))
    }

    /// The root cause of a verify failure: the first hard failure among the run's observations when
    /// there is one, otherwise the verify detail itself.
    ///
    /// This is what stops budget burn. A denied `mcp_call` or `skill_execute` is recorded as a
    /// *failed observation* and the loop keeps going, so the step that eventually trips
    /// `verify_contains` is a symptom — replanning against the symptom spends every attempt on a
    /// cause no plan can remove.
    pub fn root_cause(
        failed_tool: &str,
        detail: &str,
        observations: &[ToolObservation],
        inventory: &Inventory,
    ) -> Self {
        if let Some(obs) = observations.iter().find(|o| !o.success && o.tool != failed_tool) {
            let mut root = Self::classify_with_inventory(&obs.output, inventory);
            if !root.retryable {
                let cause = root.reason.clone();
                root.reason = format!(
                    "{failed_tool} failed because {tool} failed earlier and cannot be repaired by \
                     a new plan: {cause}",
                    tool = obs.tool
                );
                return root;
            }
        }
        Self::classify_with_inventory(detail, inventory)
    }

    /// Render for a caller that can act on it: reason first (so every existing substring assertion
    /// and audit row still matches), then remedy, constraint, and retryability.
    pub fn render(&self) -> String {
        let mut out = format!("{}{}{}", self.reason, REMEDY_SEPARATOR, self.remedy);
        if let Some(constraint) = &self.constraint {
            out.push_str(" | constraint: ");
            out.push_str(&constraint.to_string());
        }
        out.push_str(if self.retryable {
            " | retryable: yes"
        } else {
            " | retryable: no"
        });
        out
    }

    /// The rendering, for callers that only need the string form.
    pub fn to_string_lossy(&self) -> String {
        self.render()
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render())
    }
}

/// Everything after `marker`, with any trailing rendered suffix removed.
fn after_marker(message: &str, marker: &str) -> String {
    let start = message
        .find(marker)
        .map(|i| i + marker.len())
        .unwrap_or(message.len());
    message[start..].trim().to_string()
}

/// `Missing "<text>" in <path>` → (text, path). Falls back to empty strings rather than guessing.
fn split_verify_detail(message: &str) -> (String, String) {
    let body = after_marker(message, "Missing ");
    // body looks like: `"NEEDLE" in notes.txt`
    let mut parts = body.splitn(2, "\" in ");
    let expected = parts
        .next()
        .unwrap_or("")
        .trim_start_matches('"')
        .to_string();
    let path = parts.next().unwrap_or("").trim().to_string();
    (expected, path)
}

/// True when a failure is one no replan can fix — the predicate the bounded-replan loop consults.
pub fn is_non_retryable(message: &str) -> bool {
    !ToolError::classify(message).retryable
}

/// The coarse bucket this failure belongs to, reusing the wave-1 denial vocabulary so an error and
/// a denial about the same thing cannot be labelled differently.
pub fn failure_category(message: &str) -> DenialCategory {
    classify_denial(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(iteration: usize, tool: &str, success: bool, output: &str) -> ToolObservation {
        ToolObservation {
            iteration,
            tool: tool.to_string(),
            success,
            output: output.to_string(),
        }
    }

    #[test]
    fn reason_is_always_the_leading_substring() {
        for message in [
            "Write denied for target path /w/report.txt",
            "Read denied for /w/notes.txt",
            "Missing \"LOOP-05\" in marker.txt",
            "MCP Server 'nope' is not in the curated allowlist",
            "Max iterations (8) exceeded",
            "something nobody anticipated",
        ] {
            let rendered = ToolError::classify(message).render();
            assert!(
                rendered.starts_with(message),
                "rendering must keep the producer's message first: {rendered}"
            );
            assert!(rendered.contains(REMEDY_SEPARATOR), "no remedy: {rendered}");
            assert!(
                rendered.contains("retryable: yes") || rendered.contains("retryable: no"),
                "retryability must be explicit: {rendered}"
            );
        }
    }

    #[test]
    fn grant_denials_are_terminal_and_name_the_permission() {
        let err = ToolError::write_denied("/w/report.txt");
        assert!(!err.retryable);
        assert!(err.remedy.contains("write grant"), "{}", err.remedy);
        assert!(err.remedy.contains("cannot self-repair"), "{}", err.remedy);
        let constraint = err.constraint.expect("constraint");
        assert_eq!(constraint["permission"], "write");
        assert_eq!(constraint["path"], "/w/report.txt");

        let read = ToolError::read_denied("/w/notes.txt");
        assert!(!read.retryable);
        assert_eq!(read.constraint.expect("constraint")["permission"], "read");
    }

    #[test]
    fn unknown_server_carries_the_connected_inventory() {
        let inventory = Inventory::new(vec!["filesystem".into(), "forge-local".into()], vec![]);
        let err = ToolError::classify_with_inventory(
            "Security violation: MCP Server 'nope' is not in the curated allowlist",
            &inventory,
        );
        // An unconnected server is reported as a `SecurityViolation`, but the useful remedy is the
        // inventory of what *is* connected — so the allowlist marker must win over the generic one.
        assert!(!err.retryable);
        assert!(err.remedy.contains("filesystem"), "{}", err.remedy);
        assert!(err.remedy.contains("forge-local"), "{}", err.remedy);
        let bare = ToolError::classify_with_inventory(
            "Unknown MCP server 'nope'",
            &inventory,
        );
        assert!(!bare.retryable);
        assert!(bare.remedy.contains("filesystem"), "{}", bare.remedy);
        assert!(bare.remedy.contains("forge-local"), "{}", bare.remedy);
        let constraint = bare.constraint.expect("constraint");
        assert_eq!(constraint["connected_servers"][0], "filesystem");
    }

    #[test]
    fn unknown_skill_is_retryable_only_when_there_is_something_to_retry_with() {
        let none = ToolError::classify("Unknown skill_id pdf-extract");
        assert!(!none.retryable, "no installed skills means no repair");
        assert!(none.remedy.contains("no skills are installed"), "{}", none.remedy);

        let some = ToolError::classify_with_inventory(
            "Unknown skill_id pdf-extract",
            &Inventory::new(vec![], vec!["summarize".into()]),
        );
        assert!(some.retryable);
        assert!(some.remedy.contains("summarize"), "{}", some.remedy);
    }

    #[test]
    fn a_pin_violation_is_terminal_with_a_re_pin_remedy() {
        let err = ToolError::classify(
            "Security violation: MCP entry script hash mismatch for /opt/node/index.js: \
             computed 729dc851, expected ac12c030",
        );
        assert!(!err.retryable);
        assert!(err.remedy.contains("re-pin"), "{}", err.remedy);
        assert!(
            !err.remedy.contains("connected:"),
            "a pin violation is not a missing-server problem: {}",
            err.remedy
        );
    }

    #[test]
    fn verify_and_lint_failures_stay_retryable_and_carry_the_constraint_value() {
        let err = ToolError::classify("Missing \"LOOP-05-verified\" in marker.txt");
        assert!(err.retryable);
        let constraint = err.constraint.expect("constraint");
        assert_eq!(constraint["expected_substring"], "LOOP-05-verified");
        assert_eq!(constraint["path"], "marker.txt");

        let lint = ToolError::classify("1 issue(s) in report.py: [SyntaxIssue { line: 1 }]");
        assert!(lint.retryable);
        assert!(lint.remedy.contains("line"), "{}", lint.remedy);

        let absent = ToolError::classify("cannot lint report.py: No such file or directory");
        assert!(absent.retryable);
        assert!(absent.remedy.contains("fs_write"), "{}", absent.remedy);
    }

    #[test]
    fn root_cause_looks_past_the_symptom_to_the_hard_failure() {
        let observations = vec![
            obs(
                1,
                "mcp_call",
                false,
                "Security violation: MCP Server 'nope' is not in the curated allowlist",
            ),
            obs(2, "fs_write", true, "Wrote 12 bytes to marker.txt"),
            obs(3, "verify_contains", false, "Missing \"LOOP-05\" in marker.txt"),
        ];
        let root = ToolError::root_cause(
            "verify_contains",
            "Missing \"LOOP-05\" in marker.txt",
            &observations,
            &Inventory::new(vec!["filesystem".into()], vec![]),
        );
        assert!(!root.retryable, "the allowlist failure is the cause, not the verify miss");
        assert!(root.reason.contains("mcp_call"), "{}", root.reason);
        assert!(root.remedy.contains("filesystem"), "{}", root.remedy);

        // With no hard failure earlier, the verify miss is the cause and stays retryable.
        let retryable = ToolError::root_cause(
            "verify_contains",
            "Missing \"LOOP-05\" in marker.txt",
            &[observations[1].clone(), observations[2].clone()],
            &Inventory::default(),
        );
        assert!(retryable.retryable);
    }

    #[test]
    fn reclassifying_a_rendered_error_does_not_nest() {
        let once = ToolError::write_denied("/w/a.txt").render();
        let twice = ToolError::classify(&once);
        assert_eq!(twice.reason, "Write denied for target path /w/a.txt");
        assert!(!twice.retryable);
        assert_eq!(twice.render().matches(REMEDY_SEPARATOR).count(), 1);
    }

    #[test]
    fn failures_reuse_the_denial_vocabulary() {
        assert_eq!(
            failure_category("Write denied for target path /w/a.txt"),
            DenialCategory::Permission
        );
        assert_eq!(
            failure_category("PreToolUse hook blocked access to a sensitive path"),
            DenialCategory::PathPolicy
        );
        assert!(is_non_retryable("Max iterations (8) exceeded"));
        assert!(!is_non_retryable("Missing \"x\" in y.txt"));
    }
}
