# Aether Daemon IPC — Phase 1

Phase 1 uses **TCP JSON-lines** on localhost as the canonical IPC between clients (CLI, tests, future SwiftUI app) and `aether-daemon`.

Default address: `127.0.0.1:7433` (override with `AETHER_DAEMON_ADDR` or `AETHER_DAEMON_PORT`).

## Authentication (Tier A)

On macOS, the daemon stores an IPC auth token in Keychain:

- **Service:** `AetherForge`
- **Account:** `daemon-auth-token`

The token is created on first daemon startup (`ensure_daemon_auth_token`). Clients must include it in request params:

```json
{"method":"run_task","params":{"auth_token":"<token>","prompt":"Say hello","session_id":"demo-1"}}
```

**Fail-closed:** `run_task` without a valid `auth_token` returns `{"type":"error","message":"Invalid or missing auth_token"}`.

`ping` does not require auth (health check only).

The Swift app (`DaemonAuth.swift`) reads the token from Keychain and `DaemonProcessManager` spawns the daemon subprocess when needed.

On non-macOS, auth is optional unless `AETHER_DAEMON_AUTH_TOKEN` is set.

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

### Authentication (Tier A)

On macOS, the daemon stores an IPC auth token in Keychain (service `AetherForge`, account `daemon-auth-token`) on startup. **`run_task` requires a valid `auth_token` param** — requests without it are rejected fail-closed.

```json
{"method":"run_task","params":{"prompt":"Hello","session_id":"demo-1","auth_token":"<from-keychain>"}}
```

The Swift app loads the same Keychain entry and includes `auth_token` on every `run_task`. `ping` accepts an optional `auth_token` but does not require it.

Retrieve the token for manual testing (after daemon has started once):

```bash
security find-generic-password -s AetherForge -a daemon-auth-token -w 2>/dev/null \
  || tr -d '\n' < ~/.aether/daemon_auth_token
```

## Protocol

One JSON object per line (JSON-lines). Client sends requests; server streams events.

### Request: `run_task`

Plain prompt (streams Ollama tokens):

```json
{"method":"run_task","params":{"prompt":"Say hello in one word","session_id":"demo-1"}}
```

Structured loop plan (Phase 3 — emits `plan`/`tool`/`observe`/`verify` events):

```json
{
  "method": "run_task",
  "params": {
    "session_id": "loop-1",
    "workspace_path": "/tmp/aether-workspace",
    "max_iterations": 8,
    "prompt": "{\"loop\":[{\"tool\":\"fs_write\",\"path\":\"out.txt\",\"content\":\"forge\"},{\"tool\":\"verify_contains\",\"path\":\"out.txt\",\"text\":\"forge\"},{\"tool\":\"done\"}]}"
  }
}
```

| Param | Required | Description |
|-------|----------|-------------|
| `prompt` | yes | Plain text for Ollama, or JSON with `"loop":[...]` tool steps |
| `session_id` | no | Session for grants / audit |
| `workspace_path` | loop only | Workspace directory (or set `AETHER_WORKSPACE`) |
| `max_iterations` | no | Loop cap (default 8) |
| `auth_token` | yes (macOS) | Daemon IPC token from Keychain |
| `auth_token` | yes (Darwin) | Keychain daemon auth token — required on macOS |

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
TOKEN=$(security find-generic-password -s AetherForge -a daemon-auth-token -w 2>/dev/null)
printf '%s\n' "{\"method\":\"run_task\",\"params\":{\"prompt\":\"Reply with one word: forge\",\"auth_token\":\"$TOKEN\"}}" | nc 127.0.0.1 7433
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

## FFI contract (Phase 1 + 4)

`aether_ffi_daemon_ipc()` returns a C string like `tcp-json-lines:127.0.0.1:7433`.  
`aether_daemon_default_port()` returns `7433` (or `AETHER_DAEMON_PORT`).

Build for Swift linking:

```bash
./scripts/build-ffi.sh
```

SwiftUI (Phase 4) uses the same TCP protocol for agent execution; FFI supplies default host/port only.

### Swift client pattern (Phase 4)

```swift
import AetherFFI

let endpoint = DaemonConfig.load() // host/port from FFI
// TCP JSON-lines — one request line, streamed event lines
let payload = ["method": "run_task", "params": [
    "prompt": "Say hello",
    "session_id": sessionId,
    "workspace_path": workspacePath  // optional; daemon grants on loop plans
]] as [String: Any]
```

Events are parsed by `type`: `token`, `plan`, `tool`, `observe`, `verify`, `done`, `error`.  
See `macos/AetherForgeApp/DaemonClient.swift` for the reference implementation.
