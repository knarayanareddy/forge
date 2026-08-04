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

1. POST mock Slack JSON to `handle_mock_slack_post` **without** grant → **deny** + audit
2. POST to localhost mock server (`accept_one_mock_request`) **without** grant → **HTTP 403**
3. `GatewayGrant::grant` → accept → `run_gateway_inbound` → marker + `gate_response.txt` artifact
4. POST to localhost mock server **with** grant → **HTTP 200** + response artifact

No outbound HTTPS in harness; optional `profiles/sandbox_gateway.sb` deferred for production Slack egress.

## GATE-02 production adapters (Telegram / Discord)

GATE-02 adds production webhook ingress and outbound REST for **Telegram** and **Discord** on the same `GatewayGrant` gate as GATE-01.

| Channel | Module | Payload field |
|---------|--------|---------------|
| Slack | `gateway/slack.rs` | `event.text` (Events API) |
| Telegram | `gateway/telegram.rs` | `message.text` (Bot API webhook JSON) |
| Discord | `gateway/discord.rs` | `content` (interaction/webhook JSON) |

All channels route through `GatewayRouter::handle_inbound` after `GatewayGrant::check`. Tokens use `store_gateway_token` / `load_gateway_token` (Keychain on Darwin).

## RED-01 extension

Gateway deny path is fail-closed: missing grant, disabled channel, or unknown `channel_id` never enqueues a loop run.


### Production webhook server

Set `AETHER_GATEWAY_PORT` (e.g. `7444`) to bind `127.0.0.1` and accept:

`POST /gateway/{slack|telegram|discord}/{channel_id}`

Telegram verifies `X-Telegram-Bot-Api-Secret-Token` when `AETHER_TELEGRAM_WEBHOOK_SECRET` or `AETHER_GATEWAY_WEBHOOK_SECRET_{CHANNEL_ID}` is set.

### Token resolution (Telegram / Discord)

| Precedence | Env var |
|------------|---------|
| 1 | `AETHER_GATEWAY_TOKEN_{CHANNEL_ID}` (uppercase, `-` → `_`) |
| 2 | `AETHER_TELEGRAM_BOT_TOKEN` or `AETHER_DISCORD_BOT_TOKEN` |
| 3 | Keychain via `store_gateway_token(channel_id, token)` (Darwin) |

### Telegram long poll

Comma-separated gateway channel ids in `AETHER_TELEGRAM_LONG_POLL_CHANNELS` spawn background `getUpdates` loops (production egress).

### Harness (GATE-02)

`tests/golden_harness/src/gate02.rs` mirrors GATE-01 using the frozen Telegram fixture `gate02_telegram.json` and the localhost mock server path `/gateway/telegram/{channel_id}` — no real Telegram network in CI.
