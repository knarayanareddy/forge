# forge

AetherForge MVP harness baseline (Darwin MCP-01 slice, 6/10 readiness).

## Layout

- Rust workspace: `Cargo.toml`, `crates/`, `tests/golden_harness/`
- Sandbox profile: `profiles/sandbox_tool.sb`
- MCP allowlist: `mcp_allowlist.json`
- Canonical spec: `docs/AetherForge_Canonical_Spec_v1.2.3.md`

## Build

```bash
cargo build --workspace
cargo run -p golden_harness
```
