use aether_core::{
    LoopConfig, MakerCheckerGoal, OrchestrationGraph, ReActLoopEngine, ToolInvocation,
};
use aether_permissions::{PermissionDecision, PermissionManager};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use tempfile::tempdir;

pub fn check01_fixture_ready() -> Result<usize, String> {
    let fixture: Value = serde_json::from_str(include_str!("../fixtures/check01_plans.json"))
        .map_err(|e| format!("CHECK-01 fixture parse: {}", e))?;
    let n = fixture
        .get("bad_plans")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    if n < 8 {
        return Err(format!("CHECK-01 requires >=8 bad plans, got {}", n));
    }
    Ok(n)
}

fn build_checker_prompt(expected_content: &str, loop_steps: &Value) -> String {
    serde_json::json!({
        "checker_goal": { "expected_content": expected_content },
        "loop": loop_steps,
    })
    .to_string()
}

fn parse_loop_plan(loop_steps: &Value) -> Result<Vec<ToolInvocation>, String> {
    let prompt = serde_json::json!({ "loop": loop_steps }).to_string();
    ReActLoopEngine::parse_plan_from_prompt(&prompt)
        .ok_or_else(|| "Failed to parse frozen loop plan".into())
}

fn ensure_executor_grants(
    conn: &rusqlite::Connection,
    session_id: &str,
    workspace_str: &str,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR IGNORE INTO sessions (id, title, status) VALUES (?1, 'CHECK-01 executor', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;

    conn.execute(
        "INSERT OR IGNORE INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace_str],
    )
    .map_err(|e| e.to_string())?;

    let verifier_session = aether_core::VerifierNode::verifier_session_id(session_id);
    let verifier_write = PermissionManager::check_file_access(
        conn,
        &verifier_session,
        workspace_str,
        "write",
    )
    .map_err(|e| e.to_string())?;
    if verifier_write == PermissionDecision::Approved {
        return Err("Verifier session must not have write grant (CHECK-01)".into());
    }

    Ok(())
}

pub async fn test_check01_impl(conn: &rusqlite::Connection) -> Result<(), String> {
    let fixture: Value = serde_json::from_str(include_str!("../fixtures/check01_plans.json"))
        .map_err(|e| format!("CHECK-01 fixture parse: {}", e))?;

    let bad_plans = fixture
        .get("bad_plans")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "CHECK-01 missing bad_plans".to_string())?;

    if bad_plans.len() < 8 {
        return Err(format!(
            "CHECK-01 requires >=8 bad plans, got {}",
            bad_plans.len()
        ));
    }

    let tmp = tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let workspace_str = workspace.to_string_lossy().to_string();

    let graph = OrchestrationGraph::new(true, 8);
    let mut rejected = 0usize;
    let mut escapes = 0usize;

    for bad in bad_plans {
        let id = bad
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "bad plan missing id".to_string())?;
        let expected_content = bad
            .get("expected_content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let loop_steps = bad
            .get("loop")
            .ok_or_else(|| format!("bad plan {} missing loop", id))?;

        let session_id = format!("sess-check01-{}", id);
        ensure_executor_grants(conn, &session_id, &workspace_str)?;

        let prompt = build_checker_prompt(expected_content, loop_steps);
        let goal = OrchestrationGraph::parse_checker_goal(&prompt)
            .ok_or_else(|| format!("Bad plan {} missing checker_goal", id))?;
        let plan = parse_loop_plan(loop_steps)?;

        let mut config = LoopConfig {
            max_iterations: 8,
            max_tokens: 0,
            tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
            session_id: session_id.clone(),
            workspace: workspace.clone(),
        };

        let result = graph.run_maker_checker(
            conn,
            &mut config,
            &goal,
            plan,
            None,
            &HashMap::new(),
            |_| {},
        );

        if result.is_ok() {
            return Err(format!(
                "Bad plan {} should be rejected by verifier, but loop succeeded",
                id
            ));
        }

        rejected += 1;

        if let Some(steps) = loop_steps.as_array() {
            for step in steps {
                if let Some(path) = step.get("path").and_then(|v| v.as_str()) {
                    let full = workspace.join(path);
                    if full.exists() {
                        escapes += 1;
                        let _ = fs::remove_file(&full);
                    }
                }
            }
        }
    }

    if rejected < 8 {
        return Err(format!(
            "Expected >=8 bad plans rejected, got {}",
            rejected
        ));
    }

    if escapes > 0 {
        return Err(format!(
            "CHECK-01 escape: {} unverified write(s) committed from bad plans",
            escapes
        ));
    }

    let good = fixture
        .get("good_plan")
        .ok_or_else(|| "CHECK-01 missing good_plan".to_string())?;
    let session_id = good
        .get("session_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "good plan missing session_id".to_string())?;
    let expected_content = good
        .get("expected_content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "good plan missing expected_content".to_string())?;
    let loop_steps = good
        .get("loop")
        .ok_or_else(|| "good plan missing loop".to_string())?;

    ensure_executor_grants(conn, session_id, &workspace_str)?;

    let good_prompt = build_checker_prompt(expected_content, loop_steps);
    let goal = MakerCheckerGoal {
        expected_content: expected_content.to_string(),
    };
    let plan = parse_loop_plan(loop_steps)?;

    let mut good_config = LoopConfig {
        max_iterations: 8,
        max_tokens: 0,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: session_id.to_string(),
        workspace: workspace.clone(),
    };

    graph
        .run_maker_checker(
            conn,
            &mut good_config,
            &goal,
            plan,
            None,
            &HashMap::new(),
            |_| {},
        )
        .map_err(|e| format!("Good CHECK-01 plan failed: {}", e))?;

    let marker = workspace.join("check_marker.txt");
    if !marker.exists() {
        return Err("CHECK-01 good plan did not create marker file".into());
    }

    if OrchestrationGraph::parse_checker_goal(&good_prompt).is_none() {
        return Err("Good prompt must include checker_goal".into());
    }

    Ok(())
}
