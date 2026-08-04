# Model registry (Phase 10.10 / roadmap 8.9–8.11)

AetherForge routes chat through a **TOML model registry** instead of hard-coded Ollama env vars.

## Discovery order

1. `AETHER_MODEL_REGISTRY` — explicit path
2. `./models/registry.toml` — repo checkout
3. `~/.aether/registry.toml` — user override

Profile overrides: `AETHER_MODEL_PROFILE`, `AETHER_MODEL_PROFILE_COMPLEX`.

BYOK env (`AETHER_BYOK_PROVIDER` + Keychain) still wins over the registry when configured.

## Profiles (`models/registry.toml`)

| Profile | Backend | Runnable today |
|---------|---------|----------------|
| `ollama-local` | ollama | yes — default chat |
| `ollama-complex` | ollama | yes — complex fallback |
| `openai-mini` | openai_compatible | yes — Keychain BYOK |
| `ollama-embed` | ollama (`role = embed`) | embed only, excluded from chat routing |
| `mlx-qwen-3b` | mlx | **download only** — inference deferred (MLX-01) |
| `gguf-qwen-3b` | gguf | **download only** — inference deferred (MLX-01) |

Deferred backends fail closed at completion time with `not wired for inference yet`.

## Download weights (no inference)

```bash
./scripts/download-model.sh --profile gguf-qwen-3b --registry models/registry.toml
# or
cargo run -p aether-core --bin aether-download-model -- --profile gguf-qwen-3b
```

Requires `hf_repo` + `hf_file` on the profile. Optional `sha256` is verified after download.

MLX HF bundles may need manual placement until multi-artifact sync is implemented.

## Daemon routing

`aether-daemon` calls `ModelRouter::from_env()` which loads the discovered registry and logs the active profile id. No gateway or cost-accounting changes in this slice.

## Harness

**REG-01** (hard, Ollama-independent): parses the fixture registry, checks chat/embed separation, and asserts mlx/gguf fail closed with honest errors.
