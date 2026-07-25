# Gateway — Network Grant Model (Phase 7)

Phase 7 adds an **OpenClaw-style inbound gateway** (Slack first; Telegram/Discord stubs) gated by explicit **GatewayGrant** rows — extending the RED-01 adversarial model to network ingress.

## Grant model

| Layer | Type | Scope |
|-------|------|-------|
| File / git / MCP / skill | `PermissionManager` | Phase 1–5 |
| Automation trigger | `AutomationGrant` | per `trigger_id` + `session_id` |
| **Network inbound** | **`GatewayGrant`** | per `channel_id` + `session_id` |

Inbound chat payloads **must not** reach `run_task` / `ReActLoopEngine` without an approved `GatewayGrant`. Denied attempts are hash-chained into `audit_log` under `tool_name = gateway_inbound`.

## Daemon components

- `crates/aether-permissions` — `GatewayGrant::check/grant/revoke/audit_event`
- `crates/aether-daemon/src/gateway/` — `GatewayRouter`, Slack/Telegram/Discord adapters, localhost mock server
- `crates/aether-core/src/keychain.rs` — `store_gateway_token` / `load_gateway_token` (macOS Keychain BYOK pattern extended)

## Channel registration

Register a channel in SQLite (`gateway_channels`) with:

- `channel_id` — stable identifier (e.g. `gate01-slack-frozen`)
- `channel_type` — `slack` | `telegram` | `discord`
- `session_id` — daemon session owning the run
- `task_prompt` — frozen JSON loop plan (`{"loop":[...]}`)
- `workspace_path` — granted workspace for fs_write

Grant inbound access:

```sql
-- via GatewayGrant::grant(conn, channel_id, session_id, channel_type)
```

Store Slack bot token (production):

```rust
aether_core::store_gateway_token(channel_id, token)?;
```

## Mock-first harness (GATE-01)

`tests/golden_harness/src/gate01.rs` exercises the production grant gate without real Slack:

1. POST mock Slack JSON to `handle_mock_slack_post` **without** grant → **403 deny** + audit
2. `GatewayGrant::grant` → accept → `run_gateway_inbound` → marker + `gate_response.txt` artifact

No outbound HTTPS in harness; optional `profiles/sandbox_gateway.sb` deferred for production Slack egress.

## RED-01 extension

Gateway deny path is fail-closed: missing grant, disabled channel, or unknown `channel_id` never enqueues a loop run.
