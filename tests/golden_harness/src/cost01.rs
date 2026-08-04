//! COST-01 — provider-reported token accounting (frozen parsers + optional Ollama live).

use aether_core::{
    audit_loop_token_usage, ollama_token_usage, openai_token_usage, record_provider_token_usage,
    LoopConfig, LoopStreamEvent, ModelBackend, ModelRouter, OllamaProvider, ProviderTokenUsage,
};

const OLLAMA_FIXTURE: &str = r#"{"message":{"content":"ok"},"done":true,"prompt_eval_count":11,"eval_count":5}"#;
const OPENAI_FIXTURE: &str =
    r#"{"choices":[{"message":{"content":"ok"}}],"usage":{"prompt_tokens":9,"completion_tokens":4,"total_tokens":13}}"#;

pub fn cost01_fixture_ready() -> Result<(), String> {
    let ollama: serde_json::Value = serde_json::from_str(OLLAMA_FIXTURE)
        .map_err(|e| format!("COST-01 ollama fixture invalid: {e}"))?;
    let usage = ollama_token_usage(
        ollama.get("prompt_eval_count").and_then(|v| v.as_u64()),
        ollama.get("eval_count").and_then(|v| v.as_u64()),
    )
    .ok_or_else(|| "COST-01 ollama fixture must map to usage".to_string())?;
    if usage.input_tokens != 11 || usage.output_tokens != 5 {
        return Err(format!("COST-01 ollama fixture mismatch: {usage:?}"));
    }

    let openai: serde_json::Value = serde_json::from_str(OPENAI_FIXTURE)
        .map_err(|e| format!("COST-01 openai fixture invalid: {e}"))?;
    let u = openai.get("usage").ok_or("missing usage")?;
    let usage = openai_token_usage(
        u.get("prompt_tokens").and_then(|v| v.as_u64().map(|n| n as u32)),
        u.get("completion_tokens").and_then(|v| v.as_u64().map(|n| n as u32)),
    )
    .ok_or_else(|| "COST-01 openai fixture must map to usage".to_string())?;
    if usage.total() != 13 {
        return Err(format!("COST-01 openai fixture mismatch: {usage:?}"));
    }

    Ok(())
}

fn test_frozen_turn_audit() -> Result<(), String> {
    let db = aether_db::Database::open_in_memory().map_err(|e| e.to_string())?;
    let conn = db.conn();
    conn.execute(
        "INSERT INTO sessions (id, title, status) VALUES ('sess-cost01', 'COST-01', 'active')",
        [],
    )
    .map_err(|e| e.to_string())?;

    let tmp = tempfile::tempdir().map_err(|e| e.to_string())?;
    let mut config = LoopConfig {
        max_iterations: 2,
        max_tokens: 4096,
        tokens_used: 0,
        provider_input_tokens: 0,
        provider_output_tokens: 0,
        session_id: "sess-cost01".into(),
        workspace: tmp.path().to_path_buf(),
    };

    let usage = ProviderTokenUsage {
        input_tokens: 100,
        output_tokens: 25,
    };
    let mut events = Vec::new();
    record_provider_token_usage(
        &conn,
        &mut config,
        "cost01_frozen_turn",
        usage,
        Some(0),
        &mut |e| events.push(e),
    )
    .map_err(|e| e.to_string())?;

    if !events
        .iter()
        .any(|e| matches!(e, LoopStreamEvent::ProviderTokens { .. }))
    {
        return Err("COST-01 expected ProviderTokens stream event".into());
    }

    audit_loop_token_usage(&conn, "sess-cost01", "cost01_summary", usage, None)
        .map_err(|e| e.to_string())?;

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE session_id = 'sess-cost01' AND tool_name = 'loop_token_usage'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count < 2 {
        return Err(format!("COST-01 expected >=2 loop_token_usage audits, got {count}"));
    }

    Ok(())
}

async fn test_live_ollama_usage() -> Result<(), String> {
    let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:11434".to_string());
    let chat_model =
        std::env::var("AETHER_CHAT_MODEL").unwrap_or_else(|_| "qwen2.5:3b".to_string());

    OllamaProvider::health_check(&endpoint)
        .await
        .map_err(|e| format!("COST-01 live Ollama unavailable: {e}"))?;
    OllamaProvider::warm_chat_model(&endpoint, &chat_model, 1)
        .await
        .map_err(|e| format!("COST-01 warmup failed: {e}"))?;

    let router = ModelRouter::new(
        ModelBackend::OllamaMlx {
            endpoint,
            model: chat_model,
        },
        None,
    );

    let result = router
        .complete_json("Respond with JSON: {\"ok\":true}", 32)
        .await
        .map_err(|e| format!("COST-01 live completion failed: {e}"))?;

    let usage = result
        .token_usage
        .filter(|u| !u.is_empty())
        .ok_or_else(|| "COST-01 live Ollama completion missing provider token usage".to_string())?;

    if usage.total() == 0 {
        return Err("COST-01 live usage must be non-zero".into());
    }

    Ok(())
}

pub async fn test_cost01_impl() -> Result<bool, String> {
    cost01_fixture_ready()?;
    test_frozen_turn_audit()?;

    if std::env::consts::OS == "macos" {
        test_live_ollama_usage().await?;
        return Ok(true);
    }

    Ok(false)
}
