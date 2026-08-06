//! CACHE-01 — prompt-prefix / KV-reuse stability (Phase 9 slice 9.12).

use aether_core::{
    assemble_context_prompt, build_volatile_replan_tail, measure_prefix_reuse, prefix_fingerprint,
    sort_tool_results_deterministic, static_tools_prefix, CACHE01_MIN_REUSE_RATIO, OllamaProvider,
};

pub fn cache01_fixture_ready() -> Result<(), String> {
    let prefix = static_tools_prefix();
    if !prefix.contains("fs_read") {
        return Err("CACHE-01 static prefix missing tool contract".into());
    }
    if prefix_fingerprint(&prefix) != prefix_fingerprint(&static_tools_prefix()) {
        return Err("CACHE-01 prefix fingerprint unstable".into());
    }
    Ok(())
}

pub async fn test_cache01_impl() -> Result<bool, String> {
    cache01_fixture_ready()?;
    let prefix = static_tools_prefix();
    let turn1 = assemble_context_prompt(&prefix, &["\n\nUser goal:\nwrite README"]);
    let turn2 = assemble_context_prompt(
        &prefix,
        &[
            "\n\nUser goal:\nwrite README",
            "\n\n<observation>wrote 42 bytes</observation>",
        ],
    );
    if !turn1.starts_with(&prefix) || !turn2.starts_with(&prefix) {
        return Err("CACHE-01 volatile must follow static prefix".into());
    }
    if measure_prefix_reuse(&turn1, &turn2, &prefix) < CACHE01_MIN_REUSE_RATIO {
        return Err("CACHE-01 turn-2 reuse below threshold".into());
    }
    let mut rows = vec![
        ("fs_write".into(), "z".into()),
        ("fs_read".into(), "a".into()),
    ];
    sort_tool_results_deterministic(&mut rows);
    let tail = build_volatile_replan_tail(&mut rows, "finish");
    if !tail.contains("tool=fs_read") {
        return Err("CACHE-01 deterministic tool ordering failed".into());
    }

    if std::env::consts::OS == "macos" {
        let endpoint = std::env::var("AETHER_OLLAMA_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:11434".to_string());
        if OllamaProvider::health_check(&endpoint).await.is_ok() {
            return Ok(true);
        }
    }
    Ok(false)
}
