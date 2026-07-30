# Phase 8.0 Closure Evidence

**Status:** **CLOSED.** Post-merge Darwin full harness reports **22/22 (22 hard / 0 soft)** on
`main` @ `432ace9` (Actions run
[`30565128737`](https://github.com/knarayanareddy/forge/actions/runs/30565128737)).

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
| Closure fix | Sandboxed git never fails on Seatbelt's `/etc` deny | `GIT_CONFIG_NOSYSTEM=1` unit test + Darwin CI |
| Phase 9 slice 9.5-9.6 | Every daemon execution path logs a complete, order-preserving JSONL transcript | SESS-01 |

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
- PR #7 (explicit workspace grants + Darwin sandbox remediation): Linux + Darwin-fast + Swift
  build green (Actions run `30206906211`); merged to `main`.
- **First post-merge Darwin full-harness run on `main` (run `30209078971`) failed at 19/21** —
  see "Second Darwin finding" below. This is recorded here rather than only in a private log,
  consistent with this project's anti-theater discipline: a merged PR is not "closed" until its
  own canonical-platform gate has actually run green.
- Fix applied on `cursor/phase9-session-log-1259` (PR #8); re-verified locally before merge: Linux
  live harness with Ollama + MCP **19/22** (FS-02, SB-01, and OS-gated LOOP-02 fail closed, as
  expected — SESS-01 and all previously-green tasks pass).
- Workspace unit tests: green (35 daemon tests, up from 29, after the SESS-01/session-log
  additions).
- `aether-sandbox` cross-check for `aarch64-apple-darwin`: green.
- MCP allowlist scan: green.
- PR #8 merged to `main` @ `432ace9`. **Post-merge Darwin full harness: 22/22 (22 hard / 0
  soft)** — `GIT-01` and `SB-01`'s embedded `git_init` step both pass; no ROUT-01 flake on this
  run. This is the first canonical-platform confirmation that Phase 8.0 (through SB-01) plus
  Phase 9 slice 9.5-9.6 (SESS-01) are simultaneously green on Darwin.
- For context: the two nightly runs that executed while PR #8 was still open (`30436009622`,
  `30526216491`) both failed on the same pre-fix `main` commit (`49bfe86`) with the identical
  `/etc/gitconfig` EPERM error, independently corroborating the diagnosis before the fix was even
  merged.

## First Darwin finding and remediation (SB-01 xcrun/profile issues)

The first SB-01 Darwin run correctly failed rather than bypassing Seatbelt:

- `/usr/bin/git` and `/usr/bin/python3` were xcrun shims that required cache writes outside the
  workspace;
- `/dev/null` was unintentionally denied as a non-workspace write.

Remediation resolves developer tools with trusted `xcrun --find` before entering Seatbelt, invokes
the real binary under the profile, permits only `/dev/null` outside the workspace, and makes Python
infrastructure failures explicit rather than returning a false `syntax OK`. Verified by a
subsequent Darwin-fast run after fixing sandbox-profile discovery for Cargo's package-scoped test
working directories (`aarch64-apple-darwin` unit tests were failing to find the profile at all).

## Second Darwin finding and remediation (git EPERM on `/etc/gitconfig`)

With the first fix in place, git could finally execute inside the sandbox — and immediately hit a
second, previously-masked failure on the canonical Darwin runner:

```
Git command failed: git init failed: fatal: unable to access '/etc/gitconfig': Operation not permitted
```

`profiles/sandbox_tool.sb` denies all reads under `/etc`. Seatbelt returns **EPERM**, not
**ENOENT**, for a denied read. A missing config file (`ENOENT`) is normal and git ignores it; a
permission-denied error on that same path (`EPERM`) is fatal, so **every** sandboxed git
invocation — including `GIT-01` and `SB-01`'s own `git_init` step — failed on Darwin regardless of
repository state, once the sandbox was actually reached.

**Fix:** set `GIT_CONFIG_NOSYSTEM=1` unconditionally in `ProductionSandbox::command`. This tells
git to skip the system config file entirely rather than loosening the `/etc` deny for every
sandboxed tool. A unit test asserts the variable is present in every sandboxed child's environment.

## Closure gates — all satisfied

- [x] Explicit workspace grant fix merged to `main` (PR #7).
- [x] Darwin sandbox xcrun/profile-discovery fixes merged and Darwin-fast verified.
- [x] Git `/etc/gitconfig` EPERM fix merged to `main` (PR #8).
- [x] Post-merge Darwin full harness reports **22/22 hard** (21 Phase-8.0 tasks + SESS-01) —
      run [`30565128737`](https://github.com/knarayanareddy/forge/actions/runs/30565128737).

Phase 8.1+ (and Phase 9 slices 9.7+) may now proceed.

## Known residuals (not hidden)

- `sandbox-exec` is deprecated by Apple; no equivalent arbitrary-child replacement is currently
  available. The app bundles the profile and fails closed.
- The Seatbelt profile is allow-default with network deny and workspace-only writes, not a
  deny-default container.
- Semantic storage remains globally indexed; retrieval prevents disclosure by mandatory
  session-prefixed filtering. A schema migration should add `session_id`.
- Streamed completion still has an existing 16-token generation cap; Phase 8.1 budget work must
  separate ROUT benchmark settings from user-chat generation settings.
- `SessionLogWriter::append_turn` re-reads the whole session log to compute the next `turn_index`/
  `seq` — O(session length) per call. Acceptable at MVP scale; a persistent counter or index file
  is the natural follow-up once sessions grow large (Phase 10, checkpoints/fork/replay all build
  on this log).
- The daemon holds its SQLite mutex for the duration of a structured loop run, including the
  session-log file write at the end. This was already true before session logging existed; it is
  not made worse by this change, but it is not fixed either.
