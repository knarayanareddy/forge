# AetherForge Threat Model

## Assets

Workspaces, repositories, Keychain/BYOK/gateway/MCP secrets, daemon authority, approvals, audit/undo/session history, graph/vector memory, installed skills/MCP/model artifacts, CI and signed releases.

## Adversaries

Malicious prompt or authorized sender; poisoned memory/document/tool result; compromised skill/MCP/model/dependency/provider; local process; remote webhook attacker; CI supply-chain attacker; accidental crash/misconfiguration.

## Trust boundaries

1. Swift client ↔ authenticated loopback daemon.
2. User/memory text ↔ planner.
3. Model plan ↔ deterministic authorization and exact approval.
4. Daemon ↔ sandboxed tools/MCP and scoped secrets.
5. Provider webhook ↔ signature/sender/replay gate.
6. SQLite ↔ integrity-bound filesystem artifacts/logs.
7. Source/CI ↔ signed installed application.

## Non-negotiable controls

- Model text is data, never authority.
- Every persistent mutation, skill, secret and external call requires exact informed approval or an immutable revision-bound delegated grant.
- Sessions and objects are principal-owned.
- Missing policy, auth, pin, audit or sandbox fails closed.
- Untrusted data is bounded by size, time, cost and concurrency.
- Retained facts have provenance, confidence, ownership, TTL and deletion.
- Release inputs are pinned and outputs signed, notarized, inventoried and attested.

## Abuse cases

- Prompt/memory adds an unrequested tool or file effect.
- Skill/MCP rug pull after approval.
- Local daemon impersonation or token relay.
- Cross-session checkpoint/memory/consolidation access.
- Webhook replay or changed automation after grant.
- Hung/oversized IPC or MCP blocks all sessions.
- Mutable review artifact changes after display.
- BYOK/FAL credential forwarded to untrusted origin.
- Crash splits filesystem, transcript and DB state.

## Review cadence

Update this document for every new tool, network path, retained data type, identity mode or deployment topology. Review before each release and at least annually.
