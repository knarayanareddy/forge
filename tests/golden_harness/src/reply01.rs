//! REPLY-01 — a finished run must answer, not sign off, and must present what it produced.
//!
//! Before this task the only thing a completed structured loop handed back was
//! `LoopRunResult::summary`, which is *the last observation*. For any plan ending in `done` that is
//! the literal string `"plan complete"`: a sign-off that names nothing, so every caller either shows
//! the user "plan complete" or invents its own wording. Wave 1 gave the gateway one private
//! composition; P1-8 promotes it to a contract in core (`aether_core::FinalReply`) with a pre-emit
//! self-check, and emits it as a stream event, a wire event, and a session-log record.
//!
//! Three rules, all asserted below:
//!
//! 1. **state the answer, not the status** — a reply that is only a status token is rejected, and
//!    the rejection is visible in `validate()` rather than trusted to composition;
//! 2. **present what you produced** — every path the run wrote is named in the reply *and* exists on
//!    disk, because an artifact nobody is told about is unreachable;
//! 3. **stay deliverable** — bounded, with the artifact list protected from the bound and any
//!    omitted detail reported rather than silently dropped.
//!
//! Deterministic: frozen plans through the production entry point, no model and no network.

use aether_core::{
    FinalReply, LoopConfig, LoopStreamEvent, ToolInvocation, ToolObservation,
    DEFAULT_MAX_LOOP_TOKENS, MAX_FINAL_REPLY_CHARS,
};
use aether_daemon::headless::loop_event_to_line;
use aether_daemon::session_log::{SessionLogPayload, SessionLogWriter};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;
use std::path::PathBuf;
use tempfile::TempDir;

const ANSWER: &str = "REPLY-01-ANSWER";
const ARTIFACT: &str = "reply01.txt";
const NOTES: &str = "notes.txt";
const SCRIPT: &str = "tool.py";

struct EnvGuard {
    key: &'static str,
    previous: Option<String>,
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var(self.key, value),
            None => std::env::remove_var(self.key),
        }
    }
}

struct Run {
    /// Kept alive so `workspace` stays valid for the assertions that follow.
    _tmp: TempDir,
    workspace: PathBuf,
    session_id: String,
    outcome: Result<aether_core::LoopRunResult, String>,
    events: Vec<LoopStreamEvent>,
}

/// Run a frozen plan through the production loop in a fresh workspace with read *and* write grants —
/// the pair `select_workspace` creates, and the pair CHECK-02 proved an on-disk lint needs.
fn run_plan(
    db: &Database,
    case_id: &str,
    seed: &[(&str, &str)],
    plan: Vec<ToolInvocation>,
) -> Result<Run, String> {
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let session_id = format!("sess-reply01-{case_id}");
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'REPLY-01', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    for capability in ["read", "write"] {
        conn.execute(
            "INSERT INTO capability_grants (session_id, resource_path, permission_type)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![session_id, workspace.to_string_lossy().to_string(), capability],
        )
        .map_err(|e| e.to_string())?;
    }
    for (name, content) in seed {
        std::fs::write(workspace.join(name), content)
            .map_err(|e| format!("seeding {name} failed: {e}"))?;
    }

    let mut config = LoopConfig {
        max_iterations: 12,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_id.clone(),
        workspace: workspace.clone(),
    };
    let (result, events) = execute_structured_loop(
        &conn,
        &mut config,
        plan,
        None,
        &HashMap::new(),
        None,
        &format!("reply01-{case_id}"),
    );
    Ok(Run {
        _tmp: tmp,
        workspace,
        session_id,
        outcome: result.map_err(|e| e.to_string()),
        events,
    })
}

/// The ordinary "write the answer, prove it, lint it" goal shape.
fn write_plan() -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: ARTIFACT.into(),
            content: format!("{ANSWER}\n"),
        },
        ToolInvocation::VerifyContains {
            path: ARTIFACT.into(),
            text: ANSWER.into(),
        },
        ToolInvocation::PythonLint {
            source: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::Done,
    ]
}

/// Two artifacts, one of them Python, so the reply has to present both and the gate has to see the
/// `.py` linted as a file (CHECK-02's rule, exercised here only as a means to a second artifact).
fn two_artifact_plan() -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: NOTES.into(),
            content: format!("{ANSWER} in notes\n"),
        },
        ToolInvocation::VerifyContains {
            path: NOTES.into(),
            text: ANSWER.into(),
        },
        ToolInvocation::FsWrite {
            path: SCRIPT.into(),
            content: "def ok():\n    return 1\n".into(),
        },
        ToolInvocation::VerifyContains {
            path: SCRIPT.into(),
            text: "def ok".into(),
        },
        ToolInvocation::PythonLintFile {
            path: SCRIPT.into(),
        },
        ToolInvocation::Done,
    ]
}

/// Writes without verifying: the verify shell refuses `done`, so the run has no answer to give.
fn unverified_plan() -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: "never-verified.txt".into(),
            content: "unverified".into(),
        },
        ToolInvocation::Done,
    ]
}

fn obs(tool: &str, success: bool, output: &str) -> ToolObservation {
    ToolObservation {
        iteration: 0,
        tool: tool.to_string(),
        success,
        output: output.to_string(),
    }
}

/// The reply event a run emitted, if any.
fn reply_event(run: &Run) -> Option<(String, Vec<String>)> {
    run.events.iter().find_map(|event| match event {
        LoopStreamEvent::FinalReply { text, artifacts } => {
            Some((text.clone(), artifacts.clone()))
        }
        _ => None,
    })
}

pub fn test_reply01_impl(db: &Database) -> Result<(), String> {
    let log_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let _guard = EnvGuard {
        key: "AETHER_SESSION_LOG_DIR",
        previous: std::env::var("AETHER_SESSION_LOG_DIR").ok(),
    };
    std::env::set_var("AETHER_SESSION_LOG_DIR", log_dir.path());

    // --- A. the contract itself -------------------------------------------------------------

    // A1. The sign-off the loop used to hand back is not an answer, and composition knows it: the
    // reply states what was produced and presents the artifact.
    let reply = FinalReply::compose(
        "plan complete",
        4,
        &[
            obs("fs_write", true, "Wrote 16 bytes to reply01.txt"),
            obs("verify_contains", true, "found"),
            obs("python_lint", true, "ok"),
            obs("done", true, "plan complete"),
        ],
        &[ARTIFACT.to_string()],
    );
    if !reply.validate().is_empty() {
        return Err(format!(
            "a composed reply must satisfy its own contract: {:?}",
            reply.validate()
        ));
    }
    if FinalReply::is_bare_status(&reply.text) {
        return Err(format!("the reply is still a sign-off: {:?}", reply.text));
    }
    if !reply.text.contains(ARTIFACT) {
        return Err(format!(
            "the reply must present what the run wrote; got: {:?}",
            reply.text
        ));
    }
    if reply.text == "plan complete" {
        return Err("the reply must not be the last observation verbatim".into());
    }

    // A2. A summary that already answers and already presents is kept word for word: the contract
    // is a floor, not a rewrite.
    let own_words = format!("Wrote {ARTIFACT} containing the answer, verified and linted clean.");
    let kept = FinalReply::compose(&own_words, 4, &[obs("done", true, "plan complete")], &[ARTIFACT.to_string()]);
    if kept.text != own_words {
        return Err(format!(
            "a substantive summary that presents its artifact must be kept verbatim, got: {:?}",
            kept.text
        ));
    }

    // A3. A substantive summary that ignores an artifact keeps its wording and gets the accounting
    // it owes — rule 2 is not negotiable, rule 1 is respected.
    let partial = FinalReply::compose(
        "Both files are ready.",
        6,
        &[obs("done", true, "plan complete")],
        &[NOTES.to_string(), SCRIPT.to_string()],
    );
    if !partial.text.starts_with("Both files are ready.") {
        return Err(format!(
            "the run's own wording must survive, got: {:?}",
            partial.text
        ));
    }
    for path in [NOTES, SCRIPT] {
        if !partial.text.contains(path) {
            return Err(format!("the accounting must present {path}, got: {:?}", partial.text));
        }
    }
    if !partial.validate().is_empty() {
        return Err(format!(
            "the appended accounting must satisfy the contract: {:?}",
            partial.validate()
        ));
    }

    // A4. The self-check has teeth. `compose` cannot produce these, so they are built by hand: the
    // point is that `validate` names the rules that were broken instead of returning an empty list
    // by default. A check that cannot fail is decoration.
    let sign_off = FinalReply {
        text: "Done.".to_string(),
        artifacts: vec![ARTIFACT.to_string()],
    };
    let defects = sign_off.validate();
    if defects.len() != 2 {
        return Err(format!(
            "a bare status token that presents nothing must report both defects, got: {defects:?}"
        ));
    }
    if !defects[0].contains("status token") || !defects[1].contains(ARTIFACT) {
        return Err(format!("defects must name the rule and the artifact: {defects:?}"));
    }
    let over_long = FinalReply {
        text: "x".repeat(MAX_FINAL_REPLY_CHARS + 1),
        artifacts: vec![],
    };
    if over_long.validate().is_empty() {
        return Err("an over-long reply must be rejected by the self-check".into());
    }

    // A5. The bound protects the artifacts: step detail is what gets dropped, and the drop is
    // reported rather than silent (the P1-6 rule applied to a reply).
    let noisy: Vec<ToolObservation> = (0..80)
        .map(|i| obs("fs_write", true, &format!("Wrote {} bytes to file{i}.txt", 40 + i)))
        .collect();
    let bounded = FinalReply::compose("plan complete", 81, &noisy, &["keep-me.txt".to_string()]);
    if bounded.text.chars().count() > MAX_FINAL_REPLY_CHARS {
        return Err(format!(
            "the reply exceeded its own bound: {} chars",
            bounded.text.chars().count()
        ));
    }
    if !bounded.text.contains("keep-me.txt") {
        return Err("the bound must not cost the artifact list".into());
    }
    if !bounded.validate().is_empty() {
        return Err(format!(
            "a bounded reply must still satisfy the contract: {:?}",
            bounded.validate()
        ));
    }

    // --- B. the production loop emits it ----------------------------------------------------

    // B1. The ordinary goal: the reply is validated, presents exactly what was written, is not the
    // sign-off, and every presented artifact really is on disk.
    let run = run_plan(db, "write", &[], write_plan())?;
    let result = run
        .outcome
        .as_ref()
        .map_err(|e| format!("the write plan must succeed, got: {e}"))?;
    if !result.done {
        return Err("the write plan must reach done".into());
    }
    if !result.reply.validate().is_empty() {
        return Err(format!(
            "the loop's reply must satisfy its own contract: {:?}",
            result.reply.validate()
        ));
    }
    if result.reply.artifacts != vec![ARTIFACT.to_string()] {
        return Err(format!(
            "the reply must present exactly what the run wrote, got: {:?}",
            result.reply.artifacts
        ));
    }
    if result.reply.text == result.summary {
        return Err(format!(
            "the reply must not be the last observation ({:?})",
            result.summary
        ));
    }
    if FinalReply::is_bare_status(&result.reply.text) {
        return Err(format!("the loop emitted a sign-off: {:?}", result.reply.text));
    }
    for path in &result.reply.artifacts {
        if !run.workspace.join(path).exists() {
            return Err(format!(
                "the reply presents {path}, which is not on disk — presenting an unreachable \
                 artifact is the defect, not the fix"
            ));
        }
    }

    // B2. A read-only goal has no artifacts, and its answer is what it read.
    let run = run_plan(
        db,
        "read",
        &[(NOTES, &format!("{ANSWER} lives in notes\n"))],
        vec![
            ToolInvocation::FsRead {
                path: NOTES.into(),
                offset: None,
                limit: None,
            },
            ToolInvocation::Done,
        ],
    )?;
    let result = run
        .outcome
        .as_ref()
        .map_err(|e| format!("the read plan must succeed, got: {e}"))?;
    if !result.reply.artifacts.is_empty() {
        return Err(format!(
            "a read-only run must not claim artifacts, got: {:?}",
            result.reply.artifacts
        ));
    }
    if !result.reply.text.contains(ANSWER) {
        return Err(format!(
            "a read-only reply must state what it read, got: {:?}",
            result.reply.text
        ));
    }
    if !result.reply.validate().is_empty() {
        return Err(format!(
            "the read-only reply must satisfy the contract: {:?}",
            result.reply.validate()
        ));
    }

    // B3. Two artifacts, both presented, in write order.
    let run = run_plan(db, "two", &[], two_artifact_plan())?;
    let result = run
        .outcome
        .as_ref()
        .map_err(|e| format!("the two-artifact plan must succeed, got: {e}"))?;
    if result.reply.artifacts != vec![NOTES.to_string(), SCRIPT.to_string()] {
        return Err(format!(
            "both artifacts must be presented in write order, got: {:?}",
            result.reply.artifacts
        ));
    }
    for path in &result.reply.artifacts {
        if !result.reply.text.contains(path) {
            return Err(format!(
                "the reply must name {path}, got: {:?}",
                result.reply.text
            ));
        }
        if !run.workspace.join(path).exists() {
            return Err(format!("presented artifact {path} is not on disk"));
        }
    }
    if !result.reply.validate().is_empty() {
        return Err(format!(
            "the two-artifact reply must satisfy the contract: {:?}",
            result.reply.validate()
        ));
    }

    // B4. A run that failed has no answer to give: no reply is emitted, and the failure stays a
    // failure. Emitting a confident reply for a refused run is the theater this rule blocks.
    let run = run_plan(db, "refused", &[], unverified_plan())?;
    if run.outcome.is_ok() {
        return Err("an unverified write must be refused before done".into());
    }
    if reply_event(&run).is_some() {
        return Err("a refused run must not emit a final reply".into());
    }

    // --- C. the answer reaches the surfaces a person actually sees --------------------------

    // C1. The event stream carries it, before the sign-off, equal to the run's reply.
    let run = run_plan(db, "surfaces", &[], write_plan())?;
    let result = run
        .outcome
        .as_ref()
        .map_err(|e| format!("the surfaces plan must succeed, got: {e}"))?;
    let (text, artifacts) = reply_event(&run)
        .ok_or_else(|| "the loop must emit a FinalReply event on success".to_string())?;
    if text != result.reply.text || artifacts != result.reply.artifacts {
        return Err(format!(
            "the emitted reply must equal the run's reply: {:?} / {:?}",
            text, artifacts
        ));
    }
    let reply_at = run
        .events
        .iter()
        .position(|e| matches!(e, LoopStreamEvent::FinalReply { .. }))
        .unwrap_or(usize::MAX);
    let done_at = run
        .events
        .iter()
        .position(|e| matches!(e, LoopStreamEvent::Done { .. }))
        .unwrap_or(usize::MAX);
    if reply_at >= done_at {
        return Err(format!(
            "the reply must precede the sign-off in the stream (reply at {reply_at}, done at \
             {done_at})"
        ));
    }

    // C2. An adapter on the other end of the socket receives the answer, not just `done`: the wire
    // event carries both the text and the paths to present.
    let event = run
        .events
        .iter()
        .find(|e| matches!(e, LoopStreamEvent::FinalReply { .. }))
        .ok_or_else(|| "no FinalReply event to map".to_string())?;
    let line = loop_event_to_line(event)
        .ok_or_else(|| "the reply event must map to a wire event, not be dropped".to_string())?;
    if line.event_type != "final_reply" {
        return Err(format!("unexpected wire event type: {:?}", line.event_type));
    }
    if line.text.as_deref() != Some(result.reply.text.as_str()) {
        return Err(format!(
            "the wire event must carry the reply text, got: {:?}",
            line.text
        ));
    }
    if line.artifacts.clone().unwrap_or_default() != result.reply.artifacts {
        return Err(format!(
            "the wire event must carry the artifacts to present, got: {:?}",
            line.artifacts
        ));
    }

    // C3. The transcript on disk carries it too, so the answer survives the process: a session log
    // that records the sign-off but not the reply cannot be replayed into what the user saw.
    let records = SessionLogWriter::from_env()
        .read_session_log(&run.session_id)
        .map_err(|e| format!("reading the session log failed: {e}"))?;
    let logged = records.iter().find_map(|record| match &record.payload {
        SessionLogPayload::FinalReply { text, artifacts } => Some((text.clone(), artifacts.clone())),
        _ => None,
    });
    let (logged_text, logged_artifacts) = logged
        .ok_or_else(|| "the session log must contain a FinalReply record".to_string())?;
    if logged_text != result.reply.text || logged_artifacts != result.reply.artifacts {
        return Err(format!(
            "the logged reply must equal the live one: {:?} / {:?}",
            logged_text, logged_artifacts
        ));
    }
    let logged_reply_seq = records
        .iter()
        .find(|r| matches!(r.payload, SessionLogPayload::FinalReply { .. }))
        .map(|r| r.seq)
        .unwrap_or(u64::MAX);
    let logged_done_seq = records
        .iter()
        .find(|r| matches!(r.payload, SessionLogPayload::Done { .. }))
        .map(|r| r.seq)
        .unwrap_or(0);
    if logged_reply_seq >= logged_done_seq {
        return Err(format!(
            "the logged reply must precede the logged sign-off (seq {logged_reply_seq} vs \
             {logged_done_seq})"
        ));
    }

    Ok(())
}
