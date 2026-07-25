# Getting Started

The AetherForge daemon listens on TCP port **7433** by default (`AETHER_DAEMON_PORT` overrides).

Clients send JSON-lines requests with an optional `auth_token` when Keychain auth is enabled.

## Startup

```bash
cargo run -p aether-daemon
```

The daemon opens `~/.aether/aether.db` unless `AETHER_DB_PATH` is set.
