//! SUB-01 — subagent delegation with measured parent-context saving (Phase 10 slices 10.3-10.4).
//!
//! Exercises `execute_structured_loop` — the same production entry point every daemon path uses —
//! with a `subagent_task` step delegating a read-heavy batch, and proves the anti-theater bar this
//! slice has to clear: the parent's observation is a distilled summary materially smaller than the
//! raw content the subagent read, not a second loop call's worth of full file contents.

use aether_core::{LoopConfig, ToolInvocation, DEFAULT_MAX_LOOP_TOKENS, MAX_SUBAGENT_FILES};
use aether_daemon::task_runner::execute_structured_loop;
use aether_db::Database;
use std::collections::HashMap;

fn seed_session_and_grant(db: &Database, session_id: &str, workspace: &std::path::Path) -> Result<(), String> {
    let conn = db.conn();
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'SUB-01', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type)
         VALUES (?1, ?2, 'read')",
        rusqlite::params![session_id, workspace.to_string_lossy().to_string()],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn test_sub01_impl(db: &Database) -> Result<(), String> {
    // --- Case 1: delegating a read-heavy batch returns a materially smaller parent observation
    // than the raw content -- the actual point of a subagent, not just "ran the loop again." ---
    let session_id = "sess-sub01-delegate";
    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    seed_session_and_grant(db, session_id, &workspace)?;

    let file_count = 8;
    let bytes_per_file = 3_000;
    let mut paths = Vec::new();
    let mut total_raw_bytes = 0usize;
    for i in 0..file_count {
        let name = format!("doc{i}.txt");
        let content = format!("doc{i}-").repeat(bytes_per_file / 6);
        total_raw_bytes += content.len();
        std::fs::write(workspace.join(&name), &content).map_err(|e| e.to_string())?;
        paths.push(name);
    }

    let mut config = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: session_id.to_string(),
        workspace: workspace.clone(),
    };
    let plan = vec![
        ToolInvocation::SubagentTask { paths: paths.clone() },
        ToolInvocation::Done,
    ];
    let (result, _events) = {
        let conn = db.conn();
        execute_structured_loop(&conn, &mut config, plan, None, &HashMap::new(), None, "sub01-delegate")
    };
    let run = result.map_err(|e| e.to_string())?;
    if !run.done {
        return Err("expected the subagent delegation plan to complete".into());
    }

    let subagent_obs = run
        .observations
        .iter()
        .find(|o| o.tool == "subagent_task")
        .ok_or("expected a subagent_task observation")?;
    if !subagent_obs.success {
        return Err(format!("expected subagent_task to succeed, got: {}", subagent_obs.output));
    }

    // Every file must still be named in the summary (sufficient to complete a task from), but the
    // summary itself must be a fraction of the raw bytes it summarizes.
    for path in &paths {
        if !subagent_obs.output.contains(path) {
            return Err(format!("expected the distilled summary to mention {path}"));
        }
    }
    if subagent_obs.output.len() >= total_raw_bytes / 2 {
        return Err(format!(
            "expected a materially smaller parent observation: {} bytes summary vs {} bytes raw content",
            subagent_obs.output.len(),
            total_raw_bytes
        ));
    }

    // The parent's OWN token accounting (which drives its own budget) must reflect only the
    // distilled observation, not the raw file contents -- this is what "parent context stays
    // below threshold" actually means in this codebase's existing token-estimation seam.
    let estimated_from_summary_alone = (subagent_obs.output.len() + 3) / 4;
    if run.tokens_used > estimated_from_summary_alone * 4 {
        return Err(format!(
            "expected the loop's token accounting to stay close to the distilled summary's size, got {} tokens for a {}-byte summary",
            run.tokens_used, subagent_obs.output.len()
        ));
    }

    // --- Case 2: the subagent's own file-count budget is independent of the parent's
    // max_iterations, and is enforced even though it's a single parent-level step. ---
    let session_over_budget = "sess-sub01-over-budget";
    let tmp2 = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace2 = tmp2.path().to_path_buf();
    seed_session_and_grant(db, session_over_budget, &workspace2)?;

    let mut too_many_paths = Vec::new();
    for i in 0..(MAX_SUBAGENT_FILES + 1) {
        let name = format!("f{i}.txt");
        std::fs::write(workspace2.join(&name), "x").map_err(|e| e.to_string())?;
        too_many_paths.push(name);
    }
    let mut config2 = LoopConfig {
        max_iterations: 8,
        max_tokens: DEFAULT_MAX_LOOP_TOKENS,
        tokens_used: 0,
        session_id: session_over_budget.to_string(),
        workspace: workspace2.clone(),
    };
    let plan2 = vec![
        ToolInvocation::SubagentTask { paths: too_many_paths },
        ToolInvocation::Done,
    ];
    let (result2, _events2) = {
        let conn = db.conn();
        execute_structured_loop(&conn, &mut config2, plan2, None, &HashMap::new(), None, "sub01-over-budget")
    };
    match result2 {
        Ok(run2) => {
            let obs = run2
                .observations
                .iter()
                .find(|o| o.tool == "subagent_task")
                .ok_or("expected a subagent_task observation even when over budget")?;
            if obs.success {
                return Err("expected the over-budget subagent delegation to fail".into());
            }
            if !obs.output.contains("budget exceeded") {
                return Err(format!("expected a budget-exceeded reason, got: {}", obs.output));
            }
        }
        Err(e) => {
            return Err(format!(
                "expected the over-budget case to surface as a failed observation, not a hard loop error: {e}"
            ))
        }
    }

    Ok(())
}
