//! LOOP-04 — replan on verify failure (Phase 9 slice 9.9-9.10).
//!
//! Exercises `aether_daemon::task_runner::run_structured_with_replan`, the single production
//! entry point the daemon's `nl:`-prefixed `run_task` path uses. Two cases:
//!
//! 1. **Self-correction** (tolerant, like PLAN-01): a frozen plan writes the wrong content on
//!    purpose, so `verify_contains` deterministically fails on the first attempt. The failure is
//!    fed back to the NL planner, which must produce a corrective plan. Run several times in
//!    isolated sessions and require a minimum pass rate, since a 3B local model's *exact*
//!    correction is not guaranteed on every single attempt — the mechanism, not one lucky
//!    completion, is what this proves. A pass requires both a `done` result *and* at least one
//!    replan actually firing, so a first-attempt fluke cannot count as self-correction.
//! 2. **Bounded failure** (fully deterministic, no Ollama call needed): the same failing shape
//!    with `max_iterations` set so low that the shared budget is exhausted by the first failed
//!    attempt alone. This must fail cleanly with `MaxIterations`, never loop past budget, and the
//!    failure must be recorded in the session log — self-correction bounded by budget, not by luck
//!    running out first.

use aether_core::{
    LoopConfig, LoopError, ModelBackend, ModelRouter, OllamaProvider, ToolInvocation,
    DEFAULT_MAX_LOOP_TOKENS,
};
use aether_daemon::session_log::{SessionLogPayload, SessionLogWriter};
use aether_daemon::task_runner::run_structured_with_replan;
use aether_db::Database;
use std::collections::HashMap;

// Mentions linting explicitly: any successful `fs_write` requires a successful `python_lint`
// before `done` (the loop engine's verify shell, shared with LOOP-01/LOOP-02/PLAN-01), regardless
// of whether the written content is Python. Omitting this from the goal would make even a
// perfectly correct repair plan fail deterministically for a reason unrelated to self-correction.
const LOOP04_GOAL: &str = "Write a file named loop04_marker.txt containing exactly LOOP-04-verified. \
Verify loop04_marker.txt contains LOOP-04-verified. Lint this Python source: def ok():\n    return 1\n. \
Then finish.";

/// Deterministically wrong on purpose: `verify_contains` checks for the real goal text but the
/// file was written with something else, so this always fails on first execution.
fn frozen_failing_plan() -> Vec<ToolInvocation> {
    vec![
        ToolInvocation::FsWrite {
            path: "loop04_marker.txt".into(),
            content: "not-the-right-content-yet".into(),
        },
        ToolInvocation::VerifyContains {
            path: "loop04_marker.txt".into(),
            text: "LOOP-04-verified".into(),
        },
    ]
}

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

fn seed_session(db: &Database, session_id: &str, workspace: &std::path::Path) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'LOOP-04', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace.to_string_lossy().to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn test_loop04_impl(db: &Database) -> Result<(), String> {
    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let chat_model =
        std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());

    OllamaProvider::health_check(&endpoint)
        .await
        .map_err(|e| format!("LOOP-04 requires Ollama: {}", e))?;
    OllamaProvider::warm_chat_model(&endpoint, &chat_model, 1)
        .await
        .map_err(|e| format!("LOOP-04 model warmup failed: {}", e))?;

    let router = ModelRouter::new(
        ModelBackend::OllamaMlx {
            endpoint,
            model: chat_model,
        },
        None,
    );

    let log_dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let _guard = EnvGuard {
        key: "AETHER_SESSION_LOG_DIR",
        previous: std::env::var("AETHER_SESSION_LOG_DIR").ok(),
    };
    std::env::set_var("AETHER_SESSION_LOG_DIR", log_dir.path());

    // --- Case 1: self-correction, tolerant across several isolated trials. ---
    const TRIALS: usize = 5;
    const MIN_PASS_RATE: f64 = 0.6;
    let mut passed = 0usize;
    let mut failures = Vec::new();

    for trial in 0..TRIALS {
        let session_id = format!("sess-loop04-self-correct-{trial}");
        let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
        let workspace = tmp.path().to_path_buf();
        seed_session(db, &session_id, &workspace)?;

        let mut config = LoopConfig {
            max_iterations: 8,
            max_tokens: DEFAULT_MAX_LOOP_TOKENS,
            tokens_used: 0,
            session_id: session_id.clone(),
            workspace: workspace.clone(),
        };

        let (result, _events, replans) = run_structured_with_replan(
            db,
            &mut config,
            frozen_failing_plan(),
            None,
            &HashMap::new(),
            &router,
            LOOP04_GOAL,
        )
        .await;

        let marker = workspace.join("loop04_marker.txt");
        let content_ok = std::fs::read_to_string(&marker)
            .map(|c| c.contains("LOOP-04-verified"))
            .unwrap_or(false);

        match result {
            Ok(run) if run.done && replans >= 1 && content_ok => passed += 1,
            Ok(run) => failures.push(format!(
                "{session_id}: done={} replans={} content_ok={}",
                run.done, replans, content_ok
            )),
            Err(e) => failures.push(format!("{session_id}: replans={replans} error={e}")),
        }
    }

    let rate = passed as f64 / TRIALS as f64;
    if rate < MIN_PASS_RATE {
        return Err(format!(
            "LOOP-04 self-correction {}/{} ({:.0}%) below {:.0}%: {}",
            passed,
            TRIALS,
            rate * 100.0,
            MIN_PASS_RATE * 100.0,
            failures.join(" | ")
        ));
    }
    if !failures.is_empty() {
        eprintln!(
            "LOOP-04 tolerated {} self-correction miss(es): {}",
            failures.len(),
            failures.join(" | ")
        );
    }

    // --- Case 2: bounded failure — budget exhausted by the first failed attempt alone. This
    // never calls the planner's repair path (config.max_iterations hits 0 before that call), so
    // it is fully deterministic and needs no tolerance. ---
    let session_bounded = "sess-loop04-bounded";
    let tmp_bounded = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace_bounded = tmp_bounded.path().to_path_buf();
    seed_session(db, session_bounded, &workspace_bounded)?;

    let mut bounded_config = LoopConfig {
        max_iterations: 2,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: session_bounded.to_string(),
        workspace: workspace_bounded,
    };

    let (bounded_result, _events, bounded_replans) = run_structured_with_replan(
        db,
        &mut bounded_config,
        frozen_failing_plan(),
        None,
        &HashMap::new(),
        &router,
        LOOP04_GOAL,
    )
    .await;

    if bounded_replans != 0 {
        return Err(format!(
            "expected the bounded case to exhaust budget before attempting any replan, got {bounded_replans} replan(s)"
        ));
    }
    match bounded_result {
        Err(LoopError::MaxIterations(_)) => {}
        other => {
            return Err(format!(
                "expected a clean MaxIterations failure when budget is exhausted, got {:?}",
                other
            ))
        }
    }

    let bounded_log = SessionLogWriter::from_env()
        .read_session_log(session_bounded)
        .map_err(|e| e.to_string())?;
    if !bounded_log
        .iter()
        .any(|r| matches!(&r.payload, SessionLogPayload::Error { .. }))
    {
        return Err("expected the bounded failure to be recorded as an Error in the session log".into());
    }

    Ok(())
}
