# Aether Daemon IPC — Phase 1

Phase 1 uses **TCP JSON-lines** on localhost as the canonical IPC between clients (CLI, tests, future SwiftUI app) and `aether-daemon`.

Default address: `127.0.0.1:7433` (override with `AETHER_DAEMON_ADDR` or `AETHER_DAEMON_PORT`).

## Start the daemon

```bash
cargo run -p aether-daemon
```

Environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `AETHER_DAEMON_ADDR` | `127.0.0.1:7433` | Bind address |
| `AETHER_DAEMON_PORT` | `7433` | Port (used when ADDR unset) |
| `AETHER_DB_PATH` | `~/.aether/aether.db` | SQLite database |
| `AETHER_OLLAMA_ENDPOINT` | `http://localhost:11434` | Ollama base URL |
| `AETHER_CHAT_MODEL` | `qwen2.5:3b` | Chat model for routing |

## Protocol

One JSON object per line (JSON-lines). Client sends requests; server streams events.

### Request: `run_task`

```json
{"method":"run_task","params":{"prompt":"Say hello in one word","session_id":"demo-1"}}
```

Optional params (Phase 3 loop):

| Param | Default | Description |
|-------|---------|-------------|
| `workspace_path` | — | Required for JSON loop plans; workspace for tool execution |
| `max_iterations` | `8` | Loop iteration cap |

### Response events (streamed)

| `type` | Fields | Description |
|--------|--------|-------------|
| `token` | `text`, optional `ttft_ms` | Partial model output |
| `plan` | `iteration`, `action` | Loop plan step (Phase 3) |
| `tool` | `iteration`, `tool`, `output` | Tool execution result |
| `observe` | `iteration`, `summary` | Observation after tool |
| `verify` | `iteration`, `passed`, `detail` | Verifier result |
| `done` | `content`, `ttft_ms`, `model` | Completion summary |
| `error` | `message` | Failure |
| `pong` | — | Reply to `ping` |

### Loop plan in `prompt` (Phase 3)

When `prompt` is JSON with a `loop` array, the daemon runs a structured ReAct loop instead of LLM streaming:

```json
{"loop":[
  {"action":"fs_write","path":"hello.txt","content":"forge"},
  {"action":"verify_contains","path":"hello.txt","text":"forge"},
  {"action":"done"}
]}
```

Requires `workspace_path` (or `AETHER_WORKSPACE` env). Grants are auto-inserted for the workspace write path.

Example:

```bash
printf '%s\n' '{"method":"run_task","params":{"session_id":"demo","workspace_path":"/tmp/aether-loop","prompt":"{\"loop\":[{\"action\":\"fs_write\",\"path\":\"x.txt\",\"content\":\"ok\"},{\"action\":\"done\"}]}"}}' | nc 127.0.0.1 7433
```

Example stream:

```json
{"type":"token","text":"Hello","ttft_ms":42}
{"type":"token","text":"!"}
{"type":"done","content":"Hello!","ttft_ms":42,"model":"qwen2.5:3b"}
```

### Request: `ping`

```json
{"method":"ping","params":{}}
```

Response:

```json
{"type":"pong"}
```

## Manual test with netcat

```bash
# Terminal 1
cargo run -p aether-daemon

# Terminal 2
printf '%s\n' '{"method":"ping","params":{}}' | nc 127.0.0.1 7433
printf '%s\n' '{"method":"run_task","params":{"prompt":"Reply with one word: forge"}}' | nc 127.0.0.1 7433
```

Requires Ollama running with a chat model (e.g. `qwen2.5:3b`).

## Rust test client

```bash
cargo run -p aether-daemon --example stream_client
```

Or run the workspace integration test:

```bash
cargo test -p aether-daemon daemon_stream_smoke -- --ignored
```

## FFI contract (Phase 1)

`aether_ffi_daemon_ipc()` returns a C string like `tcp-json-lines:127.0.0.1:7433`. SwiftUI (Phase 4) should use the same TCP protocol rather than in-process FFI for agent execution.
