//! LOOP-02 — NL-generated plan through `ReActLoopEngine` verify shell + trajectory eval (Slice 6.6).

use aether_core::{
    validate_nl_plan_gold_trajectory, LoopConfig, ModelBackend, ModelRouter, OllamaProvider,
    ReActLoopEngine, LOOP02_EVAL_PROMPT, LOOP02_GOLD_TOOL_ORDER,
};
use std::collections::HashMap;
use std::fs;

pub async fn test_loop_02_impl(conn: &rusqlite::Connection) -> Result<(), String> {
    if std::env::consts::OS != "macos" {
        return Err("Ollama NL planner offline (LOOP-02 fail-closed off Darwin)".into());
    }

    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let chat_model =
        std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());

    OllamaProvider::health_check(&endpoint)
        .await
        .map_err(|e| format!("Ollama offline or unreachable: {}", e))?;

    OllamaProvider::warm_chat_model(&endpoint, &chat_model, 3)
        .await
        .map_err(|e| format!("Chat model warmup failed: {}", e))?;

    let router = ModelRouter::new(
        ModelBackend::OllamaMlx {
            endpoint: endpoint.clone(),
            model: chat_model.clone(),
        },
        None,
    );

    let max_iterations = 6usize;
    let plan = aether_core::run_nl_planner(&router, LOOP02_EVAL_PROMPT, max_iterations)
        .await
        .map_err(|e| format!("NlPlanner failed: {}", e))?;

    validate_nl_plan_gold_trajectory(&plan)
        .map_err(|e| format!("LOOP-02 gold trajectory (harness): {}", e))?;

    if plan.len() > max_iterations {
        return Err(format!(
            "Plan has {} steps, exceeds max_iterations {}",
            plan.len(),
            max_iterations
        ));
    }

    let session_id = "sess-loop-02";
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES (?1, 'LOOP-02 Session', 'active')",
        rusqlite::params![session_id],
    )
    .map_err(|e| e.to_string())?;

    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let workspace = tmp.path().to_path_buf();
    let workspace_str = workspace.to_string_lossy().to_string();

    conn.execute(
        "INSERT INTO capability_grants (session_id, resource_path, permission_type) VALUES (?1, ?2, 'write')",
        rusqlite::params![session_id, workspace_str],
    )
    .map_err(|e| e.to_string())?;

    let token_budget = 4096usize;
    let mut config = LoopConfig {
        max_iterations,
        max_tokens: token_budget,
        tokens_used: 0,
        session_id: session_id.into(),
        workspace: workspace.clone(),
    };

    let engine = ReActLoopEngine::new(max_iterations);
    let result = engine
        .run_structured(
            conn,
            &mut config,
            plan,
            None,
            &HashMap::new(),
            |_| {},
        )
        .map_err(|e| e.to_string())?;

    if !result.done {
        return Err("Expected loop to finish with done=true".into());
    }

    if result.iterations > max_iterations {
        return Err(format!(
            "Step count {} exceeds max_iterations {}",
            result.iterations, max_iterations
        ));
    }

    if config.tokens_used > token_budget {
        return Err(format!(
            "Token budget exceeded: {} / {}",
            config.tokens_used, token_budget
        ));
    }

    let observed: Vec<&str> = result
        .observations
        .iter()
        .map(|o| o.tool.as_str())
        .collect();

    for (index, expected) in LOOP02_GOLD_TOOL_ORDER.iter().enumerate() {
        let actual = observed
            .get(index)
            .ok_or_else(|| format!("Missing trajectory step {} (expected {})", index, expected))?;
        if *actual != *expected {
            return Err(format!(
                "Trajectory mismatch at step {}: expected {}, got {}",
                index, expected, actual
            ));
        }
    }

    if observed.len() != LOOP02_GOLD_TOOL_ORDER.len() {
        return Err(format!(
            "Redundant loop steps: expected {} tools, got {}",
            LOOP02_GOLD_TOOL_ORDER.len(),
            observed.len()
        ));
    }

    let marker = workspace.join("loop02_marker.txt");
    if !marker.exists() {
        return Err("loop02_marker.txt not created".into());
    }

    let content = fs::read_to_string(&marker).map_err(|e| e.to_string())?;
    if !content.contains("LOOP-02-verified") {
        return Err(format!("Unexpected marker content: {}", content));
    }

    Ok(())
}
