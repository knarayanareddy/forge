# Daemon lifecycle

Run the daemon from the workspace root:

```bash
cargo run -p aether-daemon
```

The process listens on **`127.0.0.1:7878`** unless `AETHER_DAEMON_ADDR` overrides the bind address.

**Verbatim citation anchor (SKILL-02):** `127.0.0.1:7878` must appear in answers about the default listen address.

## Gotchas

- Cold Ollama embed models delay first MEM-01-class requests; ROUT warmup applies to chat models only.
- Binding `0.0.0.0` requires an explicit network grant — default is loopback-only.
