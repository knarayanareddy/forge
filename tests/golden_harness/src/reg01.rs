//! REG-01 — model registry parse + deferred mlx/gguf honesty (soft harness).

use aether_core::{
    load_discovered_registry, BackendKind, ModelBackend, ModelRegistry, ModelRouter,
    PromptComplexity, RegistryError,
};
use std::path::PathBuf;

pub fn reg01_fixture_ready() -> Result<(), String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/registry.toml");
    ModelRegistry::load_from_path(&path).map_err(|e| e.to_string())
}

pub async fn test_reg01_impl() -> Result<(), String> {
    reg01_fixture_ready()?;
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/registry.toml");
    let registry = ModelRegistry::load_from_path(&fixture_path).map_err(|e| e.to_string())?;

    match registry.resolve_backend(&registry.default_profile).map_err(|e| e.to_string())? {
        ModelBackend::OllamaMlx { model, .. } if model == "qwen2.5:3b" => {}
        other => return Err(format!("expected ollama-local, got {other:?}")),
    }

    let router = ModelRouter::from_registry(&registry).map_err(|e| e.to_string())?;
    match router.primary() {
        ModelBackend::OllamaMlx { .. } => {}
        other => return Err(format!("router primary expected OllamaMlx, got {other:?}")),
    }

    if registry.chat_profile_ids().iter().any(|id| id == "ollama-embed") {
        return Err("embed must not be in chat_profile_ids".into());
    }

    let mlx = registry.profile("mlx-qwen-3b").map_err(|e| e.to_string())?;
    if mlx.backend != BackendKind::Mlx || mlx.is_inference_ready() {
        return Err("mlx profile honesty mismatch".into());
    }
    let mlx_err = ModelRouter::new(
        registry.resolve_backend("mlx-qwen-3b").map_err(|e| e.to_string())?,
        None,
    )
    .complete("forge", PromptComplexity::Simple)
    .await
    .expect_err("mlx must fail closed");
    if !mlx_err.to_string().contains("not wired") {
        return Err(format!("mlx error missing not wired: {mlx_err}"));
    }

    let gguf_err = ModelRouter::new(
        registry.resolve_backend("gguf-qwen-3b").map_err(|e| e.to_string())?,
        None,
    )
    .complete("forge", PromptComplexity::Simple)
    .await
    .expect_err("gguf must fail closed");
    if !gguf_err.to_string().contains("not wired") {
        return Err("gguf error missing not wired".into());
    }

    assert!(matches!(
        registry.resolve_backend("missing"),
        Err(RegistryError::ProfileNotFound(_))
    ));

    if let Ok(loaded) = load_discovered_registry() {
        if loaded.default_profile != registry.default_profile {
            return Err("discovered registry mismatch".into());
        }
    }
    Ok(())
}
