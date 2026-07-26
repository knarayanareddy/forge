# Phase 8.0 Closure Evidence

**Status:** Implementation complete; closure pending explicit-workspace-grant merge and Darwin
21/21 verification.

This is an evidence log, not a substitute for the binding checklist in
[ROADMAP_PHASE_8.0.md](./ROADMAP_PHASE_8.0.md).

## Implemented invariants

| Slice | Invariant | Evidence |
|-------|-----------|----------|
| 8.0a | Production NL validation does not enforce LOOP-02 gold order | `nl_planner` tests; LOOP-03 non-gold read-first case |
| 8.0a | Every IPC method except `ping` is authenticated | global server auth gate + daemon tests |
| 8.0a | Automation cannot self-grant over IPC | explicit rejection test |
| 8.0b | Turn text is embedded, stored, and linked to extracted graph entities | MEM-02 + live two-turn daemon recall |
| 8.0b | Recall is session-isolated and marked untrusted | MEM-02 foreign-secret case |
| 8.0c | Production FS/git/lint/MCP/skills/gateway paths use sandbox dispatch | SB-01 + retained task regressions |
| 8.0c | Child environment is scrubbed and network denied | SB-01 |
| 8.0d | ROUT measurement and thresholds are documented honestly | README + LINUX_CI |
| Closure fix | Tool execution never creates workspace grants | daemon unit test + live IPC proof |

## Live explicit-grant proof

Using a token-protected daemon and a structured write/verify/lint plan:

```text
run before grant → error: Workspace write grant required
grant without auth → error: Invalid or missing auth_token
grant with auth → workspace_granted
run after grant → done: plan complete
```

The database contained exactly `read` and `write` grants plus an approved
`grant_workspace` audit record.

## Regression evidence

- Main before SB-01: Darwin **20/20**, Linux default **14/20**.
- Current branch on Linux with Ollama + MCP: **18/21**; only Darwin-only FS-02/SB-01 and
  OS-gated LOOP-02 fail closed.
- Workspace unit tests: green.
- `aether-sandbox` cross-check for `aarch64-apple-darwin`: green.
- MCP allowlist scan: green.
- PR #7 Linux + Darwin-fast + Swift build: green after profile-discovery remediation
  (Actions run `30206906211`).

## Darwin findings and remediation

The first SB-01 Darwin run correctly failed rather than bypassing Seatbelt:

- `/usr/bin/git` and `/usr/bin/python3` were xcrun shims that required cache writes outside the
  workspace;
- `/dev/null` was unintentionally denied as a non-workspace write.

Remediation resolves developer tools with trusted `xcrun --find` before entering Seatbelt, invokes
the real binary under the profile, permits only `/dev/null` outside the workspace, and makes Python
infrastructure failures explicit rather than returning a false `syntax OK`.

## Remaining closure gates

- [ ] Explicit workspace grant fix merged to `main`.
- [ ] Post-merge Darwin full harness reports **21/21 hard**.

Phase 8.1+ may begin only after both boxes are checked in this file and the binding roadmap.

## Known residuals (not hidden)

- `sandbox-exec` is deprecated by Apple; no equivalent arbitrary-child replacement is currently
  available. The app bundles the profile and fails closed.
- The Seatbelt profile is allow-default with network deny and workspace-only writes, not a
  deny-default container.
- Semantic storage remains globally indexed; retrieval prevents disclosure by mandatory
  session-prefixed filtering. A schema migration should add `session_id`.
- Streamed completion still has an existing 16-token generation cap; Phase 8.1 budget work must
  separate ROUT benchmark settings from user-chat generation settings.

