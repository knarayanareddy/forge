//! LOOP-05 — a failure no plan can repair must cost **zero** replans, and every failure must carry
//! the remedy that would have made it succeed.
//!
//! `run_structured_with_replan` (LOOP-04) gives a run [`MAX_LOOP_REPLANS`] bounded self-correction
//! attempts. That budget is only worth spending on failures a *plan* can fix. Two things went wrong
//! before this task (finding P1-9):
//!
//! 1. The refusals that matter most said only what was refused. `"Write denied for target path /w/x"`
//!    and `"Read denied for /w/x"` name the victim and never the cure, so neither the planner nor a
//!    human reading the log knows what would make it allowed.
//! 2. A hard failure is often invisible at the point the run gives up. `mcp_call` and `skill_execute`
//!    record a **failed observation** and the loop keeps going, so the step that finally trips
//!    `verify_contains` is a symptom. Replanning against the symptom spends every attempt on a cause
//!    no plan can remove — on Linux that ends in `replan 1 failed: Ollama …`, i.e. the budget burned
//!    *and* the real reason buried under a model-availability error.
//!
//! Both halves are asserted here, and both are deterministic: the terminal cases break **before** any
//! planner call, so this task needs no model and no network. LOOP-04 keeps the live half (a retryable
//! failure genuinely self-correcting through Ollama); LOOP-05 proves the accounting that makes that
//! budget meaningful.

use aether_core::{
    build_nl_verify_repair_prompt, classify_denial, Inventory, LoopConfig, ModelBackend, ModelRouter,
    ToolError, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS,
};
use aether_core::DenialCategory;
use aether_daemon::task_runner::{run_structured_with_replan, MAX_LOOP_REPLANS};
use aether_db::Database;
use aether_mcp::{McpAllowlist, McpServerConfig};
use serde_json::json;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

const GOAL: &str = "Write loop05_marker.txt containing exactly LOOP-05-verified, verify it, then finish.";
const MARKER: &str = "loop05_marker.txt";
const MARKER_TEXT: &str = "LOOP-05-verified";
/// A server that is in the allowlist but is never called — it exists so the remedy for an
/// unconnected server can name what *is* connected, which is the difference between a dead end and
/// an actionable error.
const CONNECTED_SERVER: &str = "forge-local";
/// A server that is deliberately not in the allowlist.
const ABSENT_SERVER: &str = "not-connected";

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}

struct Run {
    _tmp: TempDir,
    workspace: PathBuf,
    message: String,
    replans: usize,
}

/// Run one plan through the production replan entry point. `grants` is the set of capability rows
/// the session gets; an empty set reproduces "the user never selected this folder".
async fn run_with_replan(
    db: &Database,
    case_id: &str,
    grants: &[&str],
    allowlist: Option<&McpAllowlist>,
    plan: Vec<ToolInvocation>,
    max_iterations: usize,
) -> Result<Run, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let session_id = format!("sess-loop05-{case_id}");
    {
        let conn = db.conn();
        conn.execute(
            "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'LOOP-05', 'active')",
            rusqlite::params![session_id],
        )
        .map_err(|e| e.to_string())?;
        for capability in grants {
            conn.execute(
                "INSERT INTO capability_grants (session_id, resource_path, permission_type)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    session_id,
                    workspace.to_string_lossy().to_string(),
                    capability
                ],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    let router = ModelRouter::new(
        ModelBackend::OllamaMlx {
            endpoint: "http://localhost:11434".into(),
            model: "qwen2.5:3b".into(),
        },
        None,
    );
    let mut config = LoopConfig {
        max_iterations,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id,
        workspace: workspace.clone(),
    };

    let (result, _events, replans) = run_structured_with_replan(
        db,
        &mut config,
        plan,
        allowlist,
        &HashMap::new(),
        &router,
        GOAL,
    )
    .await;

    let message = match result {
        Ok(run) => {
            return Err(format!(
                "case {case_id} was expected to fail, but completed (done={}, replans={replans})",
                run.done
            ))
        }
        Err(e) => e.to_string(),
    };
    Ok(Run {
        _tmp: tmp,
        workspace,
        message,
        replans,
    })
}

/// An allowlist with one pinned-but-never-spawned entry, so "not connected" has a real inventory to
/// report against. The pin is never checked for [`ABSENT_SERVER`] — `verify_and_get` refuses the
/// name before any spawn, so no node process is involved.
fn allowlist_with_one_server() -> McpAllowlist {
    McpAllowlist {
        servers: vec![McpServerConfig {
            name: CONNECTED_SERVER.into(),
            version: "1.0.0".into(),
            command: "/bin/true".into(),
            args: vec![],
            sha256_pin: "unused-by-this-task".repeat(4),
            entry_sha256_pin: None,
            tools_hash_pin: None,
            default_policy: "deny".into(),
        }],
    }
}

fn write_verify_done_plan(marker_text: &str) -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: MARKER.into(),
            content: marker_text.into(),
        },
        ToolInvocation::VerifyContains {
            path: MARKER.into(),
            text: MARKER_TEXT.into(),
        },
        ToolInvocation::Done,
    ]
}

pub async fn test_loop05_impl(db: &Database) -> Result<(), String> {
    let log_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let _guard = EnvGuard {
        key: "AETHER_SESSION_LOG_DIR",
        previous: std::env::var("AETHER_SESSION_LOG_DIR").ok(),
    };
    std::env::set_var("AETHER_SESSION_LOG_DIR", log_dir.path());

    // --- Case 1: no write grant. Terminal, remedy-bearing, zero replans. ----------------------
    let run = run_with_replan(
        db,
        "no-grant",
        &[],
        None,
        write_verify_done_plan("not-the-right-content"),
        8,
    )
    .await?;
    if run.replans != 0 {
        return Err(format!(
            "a missing grant is not repairable by a new plan, yet {} of {MAX_LOOP_REPLANS} replans \
             were spent: {}",
            run.replans, run.message
        ));
    }
    for required in [
        "Write denied for target path",
        "write grant",
        "cannot self-repair",
        "retryable: no",
        MARKER,
    ] {
        if !run.message.contains(required) {
            return Err(format!(
                "the write refusal must carry its remedy; missing {required:?} in: {}",
                run.message
            ));
        }
    }
    // `resolve_workspace_path` canonicalizes, so the denial names the *resolved* path: on macOS a
    // tempdir hands back `/var/folders/...` while the resolved form is `/private/var/folders/...`.
    // Compare against the canonical form, or this assertion is platform-dependent.
    let denied_path = run
        .workspace
        .canonicalize()
        .map_err(|e| format!("workspace canonicalize failed: {e}"))?
        .join(MARKER);
    if !run
        .message
        .contains(&denied_path.to_string_lossy().to_string())
    {
        return Err(format!(
            "the remedy must name the exact path that needs the grant ({}): {}",
            denied_path.display(),
            run.message
        ));
    }
    if run.workspace.join(MARKER).exists() {
        return Err("a denied write must not have created the file".into());
    }
    // The remedy must not have disturbed the wave-1 classification vocabulary.
    if classify_denial(&run.message) != DenialCategory::Permission {
        return Err(format!(
            "a remedy-bearing write denial must still classify as Permission, got {:?}",
            classify_denial(&run.message)
        ));
    }
    if ToolError::classify(&run.message).retryable {
        return Err(format!(
            "re-classifying the rendered error must stay terminal: {}",
            run.message
        ));
    }

    // --- Case 2: the budget-burn case. A hard failure behind a verify symptom. ---------------
    let allowlist = allowlist_with_one_server();
    let mut plan = vec![ToolInvocation::McpCall {
        server: ABSENT_SERVER.into(),
        tool: "list_directory".into(),
        args: json!({}),
        secret_env: None,
    }];
    plan.extend(write_verify_done_plan("not-the-right-content"));
    let run = run_with_replan(
        db,
        "unconnected-server",
        &["read", "write", "mcp_call"],
        Some(&allowlist),
        plan,
        8,
    )
    .await?;
    if run.replans != 0 {
        return Err(format!(
            "an unconnected MCP server cannot be repaired by replanning, yet {} of \
             {MAX_LOOP_REPLANS} replans were spent (on Linux that ends in a model-availability \
             error that hides the real cause): {}",
            run.replans, run.message
        ));
    }
    for required in [
        "is not in the curated allowlist",
        ABSENT_SERVER,
        CONNECTED_SERVER,
        "verify_contains failed because mcp_call failed earlier",
        "retryable: no",
    ] {
        if !run.message.contains(required) {
            return Err(format!(
                "the root cause must surface with the connected inventory; missing {required:?} \
                 in: {}",
                run.message
            ));
        }
    }

    // --- Case 3: budget exhaustion is terminal too, not a reason to retry the same plan. -----
    let run = run_with_replan(
        db,
        "budget",
        &["read", "write"],
        None,
        vec![
            ToolInvocation::FsWrite {
                path: "a.txt".into(),
                content: "a".into(),
            },
            ToolInvocation::FsWrite {
                path: "b.txt".into(),
                content: "b".into(),
            },
            ToolInvocation::Done,
        ],
        1,
    )
    .await?;
    if run.replans != 0 {
        return Err(format!(
            "an exhausted budget must not be answered with another attempt, got {} replans: {}",
            run.replans, run.message
        ));
    }
    // The error *type* must stay `MaxIterations` — LOOP-04's bounded-failure case asserts it, and a
    // caller distinguishing "out of budget" from "tool refused" loses that if it is flattened into a
    // `Turn`. So the remedy lives in the classification, not in a rewritten message.
    if !run.message.contains("Max iterations") {
        return Err(format!(
            "the budget failure must still read as a budget failure: {}",
            run.message
        ));
    }
    let budget_error = ToolError::classify(&run.message);
    if budget_error.retryable {
        return Err(format!(
            "an exhausted budget is terminal — retrying the same plan fails identically: {}",
            budget_error.render()
        ));
    }
    if !budget_error.remedy.contains("budget") {
        return Err(format!(
            "the budget remedy must name the budget: {}",
            budget_error.remedy
        ));
    }

    // --- Case 4: retryable failures stay retryable, and the repair prompt carries the cure. ---
    // This is the half that must NOT change: LOOP-04's self-correction depends on a verify miss
    // still being worth one attempt.
    let detail = format!("Missing \"{MARKER_TEXT}\" in {MARKER}");
    let observations = vec![
        aether_core::ToolObservation {
            iteration: 1,
            tool: "fs_write".into(),
            success: true,
            output: format!("Wrote 25 bytes to {MARKER}"),
        },
        aether_core::ToolObservation {
            iteration: 2,
            tool: "verify_contains".into(),
            success: false,
            output: detail.clone(),
        },
    ];
    let classified = ToolError::root_cause(
        "verify_contains",
        &detail,
        &observations,
        &Inventory::new(vec![CONNECTED_SERVER.into()], vec!["summarize".into()]),
    );
    if !classified.retryable {
        return Err(format!(
            "a verify miss with no hard failure behind it must stay retryable, or LOOP-04's \
             self-correction is dead: {}",
            classified.render()
        ));
    }
    let constraint = classified
        .constraint
        .clone()
        .ok_or_else(|| "a retryable verify miss must carry its constraint values".to_string())?;
    if constraint["expected_substring"] != MARKER_TEXT || constraint["path"] != MARKER {
        return Err(format!(
            "the constraint must carry the values the retry needs, got {constraint}"
        ));
    }

    let repair_prompt = build_nl_verify_repair_prompt(
        GOAL,
        &["fs_write".to_string()],
        "verify_contains",
        &detail,
        &classified.remedy,
        classified.constraint.as_ref(),
    );
    for required in [
        "What would make it succeed:",
        "Constraint values for the retry:",
        MARKER_TEXT,
        MARKER,
        "\"action\":\"done\"",
    ] {
        if !repair_prompt.contains(required) {
            return Err(format!(
                "the repair prompt must carry the remedy and the constraint value; missing \
                 {required:?}"
            ));
        }
    }

    // An inventory turns "unknown skill" from a dead end into a repair, and the absence of one keeps
    // it terminal — both are honest, and which one you get must depend on the environment, not on
    // guesswork.
    let with_skills = ToolError::classify_with_inventory(
        "Unknown skill_id pdf-extract",
        &Inventory::new(vec![], vec!["summarize".into()]),
    );
    if !with_skills.retryable || !with_skills.remedy.contains("summarize") {
        return Err(format!(
            "an unknown skill is repairable when something is installed: {}",
            with_skills.render()
        ));
    }
    let without_skills = ToolError::classify("Unknown skill_id pdf-extract");
    if without_skills.retryable {
        return Err(format!(
            "with nothing installed there is no repair to offer: {}",
            without_skills.render()
        ));
    }

    // --- Case 5: the rendered form is stable enough to assert on, and idempotent. ------------
    let once = ToolError::write_denied("/w/report.txt").render();
    let twice = ToolError::classify(&once).render();
    if once != twice {
        return Err(format!(
            "rendering a rendered error must not nest or drift:\n  once:  {once}\n  twice: {twice}"
        ));
    }
    if !once.starts_with("Write denied for target path /w/report.txt") {
        return Err(format!(
            "the producer's own message must stay the leading substring, so existing assertions and \
             audit rows keep matching: {once}"
        ));
    }

    Ok(())
}
