# AetherForge End-to-End Industry Review

- **Review date:** 2026-08-20
- **Repository / revision:** `knarayanareddy/forge` @ `c86e2381174baf2565c1dc2f791661f9cbee18b1`
- **Review mode:** code-grounded, adversarial, multi-discipline expert panel
- **Decision:** **NO-GO for a public 1.0 or security-sensitive production release**

---

## 1. Executive assessment

AetherForge is an ambitious and unusually broad prototype: a local Rust daemon, a SwiftUI macOS client, permission and undo layers, a semantic/graph memory subsystem, structured planning, MCP and skill integrations, automation/gateway adapters, packaging scripts, and two AI-film production pipelines. The repository also shows a strong intent to make safety claims executable rather than purely documentary.

That intent has not yet translated into production assurance. The current “1.0 engineering complete” and “Darwin canonical 51/51” claims are not defensible against the implementation and current CI state:

- The latest Linux build/unit/harness job for this exact commit is green, but the Darwin canonical job is red. Every scheduled canonical run from 2026-08-08 through 2026-08-20 failed, as did the post-merge push on 2026-08-07.
- The Darwin workflow has a deterministic count defect: the registry contains **41 hard + 10 soft** tasks, while the workflow rejects any hard count other than **42** ([`.github/workflows/ci.yml:158-163`](../.github/workflows/ci.yml), [`tests/golden_harness/src/main.rs:144-195`](../tests/golden_harness/src/main.rs)). Even a perfect current-registry result fails.
- The normal SwiftUI workflow calls `grant_workspace`, but the client does not treat `workspace_granted` as terminal. The server keeps the connection open and the client performs a blocking `recv` without a socket timeout. The user-visible operation can hang indefinitely before a task starts.
- Ordinary text from the Chat UI does **not** invoke the natural-language tool planner. Only prompts prefixed with `nl:` do; all other non-JSON prompts go to plain completion ([`task_runner.rs:184-202`](../crates/aether-daemon/src/task_runner.rs)). The UI does not add that prefix. Moreover, Ollama plain completion is capped at **16 generated tokens and a 512-token context** ([`aether-core/src/lib.rs:547-568`](../crates/aether-core/src/lib.rs)). The primary app path is therefore a very short chatbot, not the advertised general agent.
- Several safety controls are present in evaluation-specific or library code but not integrated into the production path. The clearest example is skill pinning: `install_skill`/`admit_skill` are tested, while production loads skills directly from `skills/` and `SkillExecutor::execute` neither requires a pin nor requires a capability manifest.
- Important destructive and recovery paths violate their own safety descriptions: `git_init` overwrites `README.md` without approval or reversible journaling; crash recovery marks pending writes “reverted” without touching the filesystem; checkpoints update files, logs, and database state non-atomically.
- Gateway and webhook adapters are not safe to expose: Slack and Discord signatures are not verified, Telegram webhook authentication is optional, sender/chat identity is not bound to the grant, and replay/idempotency protection is absent.

### Overall maturity scorecard

| Discipline | Score | Panel conclusion |
|---|---:|---|
| Product functionality | 3/10 | Major daemon capabilities exist, but the ordinary UI path does not reach them reliably. |
| Architecture | 5/10 | Good crate decomposition; runtime composition, protocol semantics, and trust boundaries need redesign. |
| Application security | 3/10 | Thoughtful controls exist, but authentication, approval binding, gateway verification, supply-chain integration, and sandbox scope have critical gaps. |
| Reliability / data integrity | 3/10 | Crash recovery, checkpoint atomicity, stream terminal semantics, and queue recovery are below production standard. |
| Rust correctness / safety | 5/10 | Mostly safe and parameterized code, but one vector fallback creates a potentially misaligned `&[f32]` (undefined behavior), and panic/poison paths remain. |
| Data / memory / AI | 4/10 | Useful schema and provenance ideas; isolation, lifecycle, transactional ingest, ranking time, and prompt-defense claims are incomplete. |
| Test / evaluation quality | 5/10 | Broad test inventory and 51-task harness, but fixed-corpus tests, permissive thresholds, missing UI E2E, and a permanently failing canonical gate weaken assurance. |
| Performance / scalability | 4/10 | Appropriate MVP bounds in places; a single global DB mutex and synchronous subprocesses serialize core work. |
| macOS UX / accessibility | 3/10 | Native UI foundation is clear, but blocking IPC breaks key tabs and no Swift tests or accessibility audit exist. |
| DevEx / documentation | 4/10 | Rich documentation, but extensive score, protocol, install, and feature-status drift creates operational risk. |
| Release / supply chain | 2/10 | No successful release workflow, no license/SBOM/provenance, overbroad entitlements, unpinned external tooling, and incomplete app resources. |
| Film pipelines | 4/10 (Blindspot), 2/10 (Continuity) | Strong data-driven creative design; one pipeline is currently import-broken and neither is CI/reproducibility hardened. |
| Governance | 2/10 | Public repository with no license/security policy and no human-reviewed merged PRs in the sampled history. |

### Positive foundations worth preserving

1. Crate boundaries separate core, DB, daemon, permissions, sandbox, MCP, skills, and FFI concerns.
2. SQL values are generally parameterized; no material SQL-injection path was found in the inspected code.
3. Tool subprocesses avoid `sh -c`, clear their inherited environment, and use argument arrays.
4. Workspace path handling explicitly considers traversal, symlinks, encoded traversal, and Unicode confusables.
5. Structured planning uses constrained JSON schemas and bounded iteration/token concepts.
6. Semantic-memory insertion uses a transaction for text/vector pairing; consolidation apply also uses a DB transaction.
7. Security-oriented frozen corpora exist for poisoned skills, prompt/result induction, secrets, and path handling.
8. The sandbox documentation is commendably honest that its Darwin profile is allow-by-default and Linux execution is not OS-isolated.
9. Film manifests treat editorial timing and prompt composition as data, which is a sound reproducibility direction.

---

## 2. Method and evidence

### Expert personas applied

The review used the following perspectives as a single coordinated panel:

1. **Principal architect** — boundaries, coupling, runtime composition, product-vs-probe integration.
2. **Rust systems engineer** — ownership/concurrency, panic and unsafe code, IO semantics, process management.
3. **Application security / red-team engineer** — authentication, authorization, confused deputy, injection, sandbox, secrets, webhook abuse.
4. **Data and AI-memory engineer** — schema, isolation, graph correctness, ingest atomicity, privacy, evaluation validity.
5. **SRE / performance engineer** — failure recovery, blocking behavior, timeouts, backpressure, observability, scalability.
6. **QA / evaluation scientist** — test design, CI gates, flake control, oracle quality, coverage blind spots.
7. **macOS / SwiftUI engineer** — lifecycle, socket client, Keychain/bookmarks, UX, accessibility, concurrency.
8. **DevSecOps / release engineer** — reproducibility, dependency governance, signing, notarization, workflow hardening, artifact contents.
9. **Media-pipeline engineer** — paid-call safety, provenance, deterministic assembly, dependency and asset hygiene.
10. **Open-source / product governance lead** — licensing, contribution process, privacy claims, release evidence.

### Repository inventory

- Roughly **47K lines** across tracked Rust, Swift, Python, shell, Markdown, JSON, TOML, and workflow files.
- Nine Rust workspace members including the golden harness.
- 119 tracked Rust files, 23 Swift files, 19 Python files, 17 shell scripts, and 50 Markdown files.
- Approximately 194 `#[test]` / `#[tokio::test]` annotations were found by static count, plus the 51-task executable harness. Some annotations are in orphan/uncompiled source (for example `context_compact.rs`), so this is not an executed-test count.
- The repository contains about 67 MiB in tracked files over 500 KiB, mainly generated film JPG/WAV assets.

### Checks performed

| Check | Result |
|---|---|
| Git branch/status | Correct Arena branch; initially clean. |
| Shell syntax (`bash -n`) | Pass for tracked `.sh` files. |
| JSON parse | Pass for 29 tracked JSON files. |
| TOML parse | Pass for 12 tracked TOML files. |
| Python byte-compilation | Pass; it created untracked `__pycache__` directories, later removed. |
| Documentation scoreboard script | Pass, but it checks only selected score strings and missed broad documentation drift. |
| MCP static allowlist script | Pass; optional external scanner unavailable. |
| Heuristic secret scan | No apparent live credential found; fixtures intentionally contain fake secrets. This is not a substitute for gitleaks/secret scanning. |
| Department of Continuity generation entry points | Fail immediately: `ModuleNotFoundError: falgen`. |
| Blindspot pacing check | Could not execute in the base environment because Pillow is not installed. |
| Local Rust build/test/fmt/clippy | Not run: Rust was absent and network bootstrap was unavailable. |
| Independent CI evidence | GitHub Linux build, unit tests, and threshold harness are green for this exact commit on 2026-08-20. The macOS PR fast job (including Swift build) was green on 2026-08-07. Canonical Darwin full is currently red. |
| Dependency vulnerability audit | Not available: no `cargo-audit`, `cargo-deny`, OSV, or equivalent gate is configured or locally available. Vulnerability absence is therefore **unknown**, not established. |

---

## 3. Release-blocking findings

Severity meanings:

- **Critical:** credible path to unsafe authority, secret compromise, data loss, or inability to use the primary product path.
- **High:** major control failure or correctness defect that must be fixed before production.
- **Medium:** material engineering debt likely to cause incidents, drift, or poor operation.
- **Low:** quality/polish issue with bounded immediate impact.

### CRIT-01 — Swift IPC terminal semantics make the normal app flow hang

**Evidence**

- The daemon handles multiple request lines per persistent socket and does not close after `workspace_granted`, `undo_complete`, `checkpoint_created`, `rewind_complete`, consolidation events, automation events, or `pending_approval` ([`server.rs:74-303`](../crates/aether-daemon/src/server.rs)).
- `DaemonClient.collectEvents` stops only for `done`, `error`, or `pong` ([`DaemonClient.swift:277-293`](../macos/AetherForgeApp/DaemonClient.swift)).
- The same terminal predicate is used by streaming tasks ([`DaemonClient.swift:258-274`](../macos/AetherForgeApp/DaemonClient.swift)); `pending_approval` is not terminal.
- `streamSync` checks a wall-clock deadline before calling blocking `recv`, but never configures `SO_RCVTIMEO`, nonblocking IO, or a cancellation source. Once inside `recv`, the deadline is ineffective ([`DaemonClient.swift:295-320`](../macos/AetherForgeApp/DaemonClient.swift)).
- Every Chat run with a workspace first awaits `grantWorkspace` ([`AppModel.swift:88-102`](../macos/AetherForgeApp/AppModel.swift)).
- The client performs one `write` and accepts any positive byte count instead of sending all bytes; partial TCP writes can truncate larger requests ([`DaemonClient.swift:298-300`](../macos/AetherForgeApp/DaemonClient.swift)).

**Impact**

Workspace grant can block before a task starts. Undo, checkpoint, rewind, memory consolidation, automation acknowledgements, and approval can also block. Pending approval can leave the Chat UI permanently “Running”. This is a primary-path release blocker, not an edge case.

**Required fix**

Define explicit protocol framing and terminal behavior. Prefer one request per connection, or attach request IDs and a terminal flag to every response. Treat every unary acknowledgement and `pending_approval` as terminal. Implement `sendAll`, bounded line/frame sizes, connect/read/write timeouts, and task cancellation. Add real Swift↔daemon integration tests for every method.

### CRIT-02 — Human approval is not bound to the reviewed plan

**Evidence**

- The server accepts a bare `approved: bool` ([`protocol.rs:11-41`](../crates/aether-daemon/src/protocol.rs)).
- A blocked NL request returns only human-readable risky-step strings, not a canonical plan, digest, nonce, or expiry ([`task_runner.rs:499-515`](../crates/aether-daemon/src/task_runner.rs)).
- The app approves by resending the original prompt with `approved: true` ([`AppModel.swift:55-64`](../macos/AetherForgeApp/AppModel.swift)).
- For `nl:` prompts, that second request calls the model planner again. The newly generated plan may differ, while `approved: true` bypasses all risk classification ([`risk.rs:65-78`](../crates/aether-core/src/risk.rs)).
- File existence is checked during preflight and not atomically bound to the later write, creating a TOCTOU window.

**Impact**

A user can approve plan A and execute plan B. A non-deterministic or compromised model can change paths, content, MCP calls, or other actions on the second generation. The PERM-02 guarantee is valid only for the exact in-memory plan evaluated in one request, not the UI workflow.

**Required fix**

Persist the canonical normalized plan server-side and return an opaque, single-use approval ID bound to `SHA-256(session || workspace || plan || expiry || nonce)`. Approval must execute that stored plan exactly, after revalidating grants and target state. Never re-plan after approval.

### CRIT-03 — Crash recovery can lose the only usable inverse operation

**Evidence**

- File journaling inserts `pending`, mutates the filesystem, then updates to `applied` ([`undo.rs:60-85`](../crates/aether-permissions/src/undo.rs)).
- A crash can occur after the write and before the status update.
- Startup recovery does not inspect or restore the file. It blindly changes every `pending` entry to `reverted` ([`recovery.rs:10-23`](../crates/aether-db/src/recovery.rs)).

**Impact**

A file can remain modified while the database permanently claims the inverse was applied. Subsequent undo ignores the row. This is silent data loss / integrity corruption and contradicts RES-01 and “reverted” semantics.

**Required fix**

Use a crash-consistent write protocol: record prior content plus before/after hashes, write to a same-directory temporary file, fsync, atomically rename, fsync the directory, and transition journal state transactionally. Recovery must reconcile actual target hash/state and either finish or apply the inverse; it must never relabel without filesystem action. Add fault injection at every journal/write/status boundary.

### CRIT-04 — `git_init` performs hidden destructive writes outside approval and undo guarantees

**Evidence**

- The risk classifier explicitly treats `git_init` as pre-vetted/non-risky ([`risk.rs:8-13`](../crates/aether-core/src/risk.rs)).
- `GitOps::init_commit_and_branch` runs `git init`, unconditionally overwrites `README.md`, commits it, and creates a branch ([`aether-core/src/lib.rs:1111-1129`](../crates/aether-core/src/lib.rs)).
- The prior README is not journaled. The git journal marker is written only after success and is explicitly non-undoable ([`loop_engine.rs:315-337`](../crates/aether-core/src/loop_engine.rs)).

**Impact**

An existing repository can have its README replaced and repository state changed without a destructive-operation approval. “Undo Last Writes” cannot restore it.

**Required fix**

Split git operations into explicit actions. Refuse `git_init` if `.git` already exists unless separately approved. Do not create/overwrite README implicitly. Present every write and repository mutation in approval UI, journal file content before mutation, and use a worktree/status preflight.

### CRIT-05 — Gateway/webhook authorization is not provider authentication

**Evidence**

- Telegram accepts webhooks without a secret when no environment variable is configured ([`telegram.rs:75-87`](../crates/aether-daemon/src/gateway/telegram.rs)).
- Slack request signatures/timestamps are not validated at all ([`slack.rs`](../crates/aether-daemon/src/gateway/slack.rs)).
- Discord Ed25519 signatures/timestamps are not validated ([`gateway_server.rs:44-60`](../crates/aether-daemon/src/gateway/gateway_server.rs), [`discord.rs`](../crates/aether-daemon/src/gateway/discord.rs)).
- The Telegram payload’s `chat.id` and sender identity are not compared to an authorized principal before a granted channel executes.
- There is no persisted provider event/update ID, replay window, or idempotency key.
- Long polling resets `offset` to zero after restart ([`telegram.rs:128-163`](../crates/aether-daemon/src/gateway/telegram.rs)).
- The HTTP listeners bind to loopback, but the feature is described as a webhook gateway; exposing it through a tunnel/proxy makes these defects remotely exploitable.

**Impact**

Once a `GatewayGrant` exists, anyone who can reach the endpoint—or message the bot in an unauthorized chat—can trigger the stored tool plan repeatedly. Retries/replays can duplicate writes. Slack/Discord routes are not production webhook integrations.

**Required fix**

Disable external gateway claims until each provider has mandatory signature validation, timestamp tolerance, constant-time comparison, sender/workspace authorization, persisted event IDs, replay rejection, rate limits, request size/time limits, and idempotent execution. Bind grants to provider tenant/channel/user identities and a digest of the approved stored plan.

### CRIT-06 — Local daemon authentication is predictable and one-sided

**Evidence**

- The daemon token is `SHA-256(time_nanos || pid)`, not output from a CSPRNG ([`keychain.rs:284-293`](../crates/aether-core/src/keychain.rs)); this maps to CWE-330-style insufficient randomness.
- The app decides that a daemon is legitimate if an unauthenticated `ping` returns `pong` ([`DaemonProcessManager.swift:54-63`](../macos/AetherForgeApp/DaemonProcessManager.swift)).
- The protocol authenticates the client to the server but does not authenticate the server to the client.
- A local process can bind port 7433 first, answer `pong`, then receive the bearer token, prompts, workspace paths, and the BYOK key that Settings first attempts to send through the currently unsupported daemon method ([`DaemonClient.swift:231-240`](../macos/AetherForgeApp/DaemonClient.swift)).
- The token is duplicated from Keychain into `~/.aether/daemon_auth_token`; creation writes first and chmods afterward ([`keychain.rs:314-340`](../crates/aether-core/src/keychain.rs)).

**Impact**

A local port-squatting process can impersonate the daemon and collect high-value material. Token entropy depends on partially guessable startup state. The plaintext fallback expands the secret’s attack surface.

**Required fix**

Generate at least 256 bits with `SecRandomCopyBytes`/`getrandom`. Prefer a Unix domain socket inside a mode-0700 directory with peer-credential checks and a random socket name. Add a server-authenticated challenge or pinned local public key. Remove the fallback token file unless strictly required; if retained, create atomically with mode 0600 and no symlink following. Never send BYOK material to an endpoint that has not authenticated as the expected daemon.

### CRIT-07 — Canonical CI is structurally red while documentation claims green

**Evidence**

- Static registry count: 51 tasks, 41 `hard_on_darwin: true`, 10 soft.
- Workflow requires `HARD == 42` while its own error and success text say 41 hard / 10 soft ([`.github/workflows/ci.yml:155-163`](../.github/workflows/ci.yml)).
- GitHub evidence: all 13 scheduled runs from Aug 8–20 failed in `Golden harness (Darwin canonical 51/51)`; Linux jobs passed. The post-merge push on Aug 7 also failed. The last PR fast job passed because it skips the full Darwin harness.
- README still calls the gate 51/51 and the product engineering-complete ([`README.md:3-67`](../README.md)).

**Impact**

The main release signal has been continuously red since merge, yet public docs present it as green. This invalidates closure and anti-theater claims.

**Required fix**

Derive expected hard/soft totals from one machine-readable registry, fail on per-task regressions, and make the harness process itself nonzero on failed mandatory tasks. Correct the workflow to 41, then investigate any remaining task failures. Do not restore 1.0 claims until a new exact-commit canonical run passes repeatedly.

### CRIT-08 — The user-facing app does not reach the advertised agent behavior

**Evidence**

- `run_task` invokes NL planning only when the prompt begins `nl:`. Structured JSON works; all other prompts are plain completion ([`task_runner.rs:184-202`](../crates/aether-daemon/src/task_runner.rs)).
- Chat UI sends the text exactly as entered ([`AppModel.swift:47-52`](../macos/AetherForgeApp/AppModel.swift)).
- Plain Ollama completion allows only 16 output tokens and 512 context tokens ([`aether-core/src/lib.rs:547-568`](../crates/aether-core/src/lib.rs)).
- `PromptComplexity::Complex` is defined but has no production caller; the configured complex profile is effectively dead routing.
- README says plain-text prompts route through `NlPlanner`, which is not what the code does.

**Impact**

Normal users cannot ask the app in natural language to read/write/lint/use tools. They get a heavily truncated chat response. Most “agent” functionality is reachable only through manual JSON, a hidden prefix, automation fixtures, or harness code.

**Required fix**

Define a real product routing policy: ordinary intent should enter a planner with an explicit chat-only escape hatch, or UI should clearly separate Chat and Agent modes. Set context/output limits from profile and budget, not TTFT benchmark constants. Wire complex routing based on measurable intent/complexity. Add end-to-end user stories from typed prompt to approved tool effect and visible response.

---

## 4. High-severity findings by expert domain

### 4.1 Architecture and product integration

#### HIGH-01 — Packaged app omits required runtime assets

`create-dmg.sh` stages the app binary, daemon, and sandbox profile only ([`create-dmg.sh:75-94`](../scripts/create-dmg.sh)). It does not include:

- `mcp_allowlist.json`
- `skills/`
- `models/registry.toml`
- model schemas/configuration documentation

The model registry can fall back to environment defaults, but Settings reports registry unavailable. Skills load relative to process CWD and become empty. MCP resolution can proceed without the curated file and lose its tool-description pin. The release smoke test does not launch the staged app and exercise these resources.

**Fix:** create a versioned resource manifest, bundle all required assets, resolve via `Bundle.main.resourceURL` / executable-relative paths, and add an installed-artifact E2E test.

#### HIGH-02 — Core capabilities are probes/libraries rather than complete product surfaces

Examples:

- “User-inspectable memory” CRUD exists in `aether-db` but has no IPC/UI surface; the Memory tab only reviews consolidation.
- Session fork/resume exists but is not exposed by daemon IPC or UI.
- Subagent UI displays prior events but does not initiate delegation.
- Gateway registration/grants lack a complete product flow.
- `aether-download-model` is a stub that only prints usage; the actual `download_file` library is never called.
- Local MLX/GGUF profiles are catalog/deferred, not inference backends.

This is acceptable for a research prototype but inconsistent with a broad “engineering complete” claim.

#### HIGH-03 — One global SQLite connection/mutex serializes core work

`Database` wraps one `rusqlite::Connection` in `Arc<Mutex<_>>` ([`aether-db/src/lib.rs:25-75`](../crates/aether-db/src/lib.rs)). Structured loops hold the guard while running synchronous file, git, Python, skill, and MCP subprocesses. MCP has no timeout. The Tokio task performing this work also executes blocking process and filesystem calls directly.

**Impact:** one slow/hung tool blocks memory, automation, gateway, checkpoint, and other sessions; Tokio worker capacity can be exhausted.

**Fix:** use a connection pool or DB actor, short transactions, `spawn_blocking` for process/filesystem work, explicit tool deadlines/cancellation, and per-session concurrency controls.

### 4.2 Security, permissions, sandbox, and supply chain

#### HIGH-04 — Production skill execution bypasses the advertised pin/admission gate

Production loads every `skills/*/SKILL.md` directly ([`task_runner.rs:829-835`](../crates/aether-daemon/src/task_runner.rs)). `SkillExecutor::execute` re-runs string scans but:

- does not call `admit_skill`,
- does not require a persisted content pin,
- permits `capabilities: None` and skips manifest validation ([`aether-skills/src/lib.rs:138-146`](../crates/aether-skills/src/lib.rs)).

The SKILL-03 harness validates `install_skill`/`admit_skill`, but that gate is not the production `ToolRegistry` path. A post-install rug pull is therefore not blocked by production pin comparison.

**Fix:** create one persisted `InstalledSkill` registry and make `ToolRegistry` require admission before every execution. Remove the legacy no-manifest path for production. Bind approval and grants to the skill digest.

#### HIGH-05 — MCP curated pinning fails open and the executable pin is trust-on-discovery

`resolve_filesystem` discovers `node`, computes its current hash, and uses that computed value as the execution pin ([`aether-mcp/src/lib.rs:182-217,266-277`](../crates/aether-mcp/src/lib.rs)). It uses the repository file only for the entry script and tools hash. If `mcp_allowlist.json` is absent, malformed, or lacks the server, loading silently continues with runtime-computed script/executable hashes and no tools hash ([`aether-mcp/src/lib.rs:70-104`](../crates/aether-mcp/src/lib.rs)). Environment overrides can select an arbitrary executable whose digest is then self-approved.

There is also a verify-to-spawn TOCTOU window, no protocol timeout, and no response/line limit ([`runtime.rs:213-269`](../crates/aether-mcp/src/runtime.rs)).

**Fix:** fail closed when curated policy is absent/invalid; pin approved package/version/content in a signed install record; open verified file descriptors or use immutable bundle paths; validate canonical executable/entry paths; add initialize/list/call deadlines and response caps.

#### HIGH-06 — Sandbox claims exceed its actual isolation

- Darwin profile is `(allow default)`, denies network and writes outside workspace, but permits reads almost everywhere except `/etc` and `/private/etc` ([`profiles/sandbox_tool.sb`](../profiles/sandbox_tool.sb)).
- Linux runs child commands natively with environment scrubbing but no Landlock/bubblewrap/seccomp isolation ([`aether-sandbox/src/lib.rs:247-265`](../crates/aether-sandbox/src/lib.rs)).
- Path validation and later child open are separate operations, creating TOCTOU exposure off Darwin.
- PreToolUse sensitive-path matching is a small substring denylist, not an OS policy.

The sandbox document acknowledges much of this, but top-level product/security claims should use the same scope.

#### HIGH-07 — Hardened-runtime entitlements are unnecessarily dangerous

The same entitlements enable JIT, unsigned executable memory, and disable library validation ([`AetherForge.entitlements`](../packaging/entitlements/AetherForge.entitlements)). The distribution helper applies them to both daemon and app ([`distribution.sh:35-71`](../scripts/lib/distribution.sh)). No reviewed code path demonstrates a need for these privileges.

**Fix:** remove all three by default. If one dependency proves it needs a privilege, use separate least-privilege entitlement files per executable and document the threat tradeoff.

#### HIGH-08 — MCP argument confinement checks only a top-level `path`

`validate_mcp_arguments_in_workspace` examines only `args.path` ([`loop_engine.rs:733-739`](../crates/aether-core/src/loop_engine.rs)). Tools commonly use arrays or names such as `paths`, `source`, and `destination`. The curated filesystem server receives a workspace root and should enforce its own boundary, but the daemon’s generic claim is incomplete and user-addable servers cannot rely on that convention.

**Fix:** enforce per-tool JSON schemas and capability descriptors, not a generic field-name heuristic.

### 4.3 Rust correctness and process safety

#### HIGH-09 — Vector fallback creates a potentially misaligned `&[f32]`

The linear semantic-search fallback reads SQLite data into `Vec<u8>` and casts the byte pointer to `*const f32`, then creates a shared slice ([`aether-db/src/lib.rs:650-664`](../crates/aether-db/src/lib.rs)). A `Vec<u8>` allocation is not guaranteed to have `f32` alignment. Constructing a misaligned `&[f32]` is undefined behavior even if the platform tolerates unaligned loads.

**Fix:** decode `chunks_exact(4)` with `f32::from_le_bytes`, or use a crate with explicit POD/alignment guarantees and validate endian/length.

#### HIGH-10 — Stream failure produces both `error` and `done`

On a model stream error, `run_stream_task` writes an `error`, breaks, then unconditionally writes `done` and ingests partial content ([`task_runner.rs:601-665`](../crates/aether-daemon/src/task_runner.rs)). This gives clients contradictory terminal states and can commit incomplete assistant memory.

**Fix:** return immediately after an error; persist an explicit failed/partial turn only under a defined policy.

#### HIGH-11 — MCP and child-process execution can hang without bounds

MCP reads blocking lines until matching response ID with no deadline or child-liveness check. Other `Command::output` calls have no timeout. A pinned but buggy server can block while the global DB mutex is held.

**Fix:** asynchronous process supervision, deadlines, stdout/stderr caps, kill-on-timeout, and audit of timeout outcome.

#### HIGH-12 — Panic can poison the global database mutex

Database lock acquisition uses `.lock().unwrap()` throughout. A panic while holding the guard poisons the mutex and causes later accesses to panic. One attacker-influenced candidate exists in `inject::truncate`, which slices UTF-8 by byte offset (`&s[..max]`) and can panic when a multibyte observation crosses the boundary ([`inject.rs:289-295`](../crates/aether-core/src/inject.rs)).

**Fix:** character-safe truncation, eliminate panic paths on untrusted input, and recover/replace poisoned state rather than unwrapping.

### 4.4 Data integrity, memory, and privacy

#### HIGH-13 — Checkpoint rewind is not atomic across files, JSONL, and DB

Rewind first applies filesystem inverses, then reads/truncates JSONL, then updates checkpoint metadata ([`checkpoint.rs:89-110`](../crates/aether-daemon/src/checkpoint.rs)). Any failure between those steps leaves the filesystem and transcript inconsistent. The DB transaction cannot cover the external files.

**Fix:** use a durable multi-phase operation journal with prepared/committed states and compensating recovery. Write JSONL replacements atomically and fsync. Surface partial-recovery state instead of reporting a single success/failure.

#### HIGH-14 — Consolidation review artifact is mutable and not bound to the reviewed run

Consolidation stores only a plaintext artifact path. Apply later reads the current JSON file and executes its node IDs/survivors ([`consolidate.rs:207-253`](../crates/aether-db/src/consolidate.rs)). It does not verify:

- an artifact hash captured at review creation,
- `preview.run_id == run_id`,
- `preview.session_id` ownership,
- that duplicate and survivor belong to that session,
- that the Markdown displayed by Swift corresponds to the JSON applied.

A modified JSON artifact can supersede arbitrary graph nodes while showing a benign Markdown preview.

**Fix:** persist the canonical diff in the DB or store a cryptographic digest and display exactly those bytes. Validate run/session/node ownership in the apply transaction.

#### HIGH-15 — Session transcript is fail-open, collision-prone, and not tamper-evident

- Logging failures are warnings and execution still succeeds ([`task_runner.rs:173-179`](../crates/aether-daemon/src/task_runner.rs)).
- Sanitization maps many IDs to the same filename (`a/b` and `a_b`) and has no length bound ([`session_log.rs:94-112`](../crates/aether-daemon/src/session_log.rs)).
- Append reads and reparses the complete file each turn, then appends without file locking or fsync.
- Record sequence/session consistency is not validated on read.
- Truncation uses direct `fs::write`, not atomic replacement.

This does not support a strong “source of truth” or forensic guarantee.

#### HIGH-16 — Memory ingest is non-transactional and can permanently partially apply

Conversation user/assistant rows are separate inserts; graph nodes and edges are inserted one by one; semantic chunk and links use separate transactions ([`ingest.rs:155-175,228-247,263-278`](../crates/aether-daemon/src/ingest.rs)). Fail-open errors can leave partial graph/conversation state. Deterministic IDs can then make retries collide.

**Fix:** define idempotency keys and transactional stages, use UPSERT where replay is intended, and record ingest state per turn.

### 4.5 Reliability, performance, and operations

#### HIGH-17 — Automation queue has no crash recovery or idempotency

Rows transition to `running`, execute synchronously, then become completed/failed ([`automation.rs:384-473`](../crates/aether-daemon/src/automation.rs)). Daemon crash leaves `running` rows forever. Cron marks a trigger fired before queue insert without one transaction. Webhook retry can enqueue duplicates. Grants are bound to trigger/session IDs, not a digest of task/workspace/config; `INSERT OR REPLACE` can change an already-granted trigger.

**Fix:** transactional claim/lease, lease expiry recovery, dedupe key, attempt count/dead-letter state, and grant binding to immutable trigger revision.

#### HIGH-18 — HTTP/IPC parsers lack robust resource and time bounds

Daemon JSONL has no line-size cap and accepts unbounded connections. Webhook parsers use a hand-written single-read header parser, no read timeout, and weak 64 KiB limiting. `String::truncate(content_length)` can panic if `content_length` is not a UTF-8 boundary after lossy decoding ([`gateway_server.rs:86-136`](../crates/aether-daemon/src/gateway/gateway_server.rs)).

**Fix:** use a maintained HTTP stack, strict body/header limits, deadlines, backpressure, connection caps, and byte-oriented parsing.

#### HIGH-19 — Model downloader is nonfunctional and unsafe for model-scale files

`aether-download-model` only prints a usage line ([`aether-download-model.rs`](../crates/aether-core/src/bin/aether-download-model.rs)). The unused library implementation buffers the full response in memory, writes directly to destination, and permits profiles without a checksum ([`hf_hub.rs`](../crates/aether-core/src/hf_hub.rs)).

**Fix:** implement the CLI, stream to an atomic temporary file while hashing, require digest/size, support resume, and never expose deferred profiles as downloadable-ready without this gate.

### 4.6 macOS product, UX, and accessibility

#### HIGH-20 — Security-scoped bookmark language overstates enforcement

The app creates a security-scoped bookmark, but the app is not configured with App Sandbox and does not transfer the bookmark extension to the daemon. The daemon receives a raw path. Logical grants are meaningful, but the bookmark is not the daemon’s OS enforcement boundary.

**Fix:** either sandbox the appropriate process and correctly broker security-scoped access, or describe bookmarks as persistence/user-consent evidence only.

#### HIGH-21 — No Swift tests or real app E2E

`Package.swift` defines no test target. CI builds Swift but does not exercise socket behavior, workspace selection, approval, undo, checkpoint, consolidation, settings, app launch, or accessibility. The critical IPC defect is a direct consequence.

**Fix:** add protocol unit tests, fake-server timeout/partial-write tests, and macOS integration/UI tests against a real daemon. Include VoiceOver labels/focus, keyboard-only navigation, reduced motion/contrast, Dynamic Type-equivalent sizing, and screen-reader status announcements in acceptance criteria.

#### HIGH-22 — Safety and memory UI state is incomplete

Checkpoint IDs exist only in the in-memory `SafetyModel`; after app restart there is no list API, so persisted checkpoints are not discoverable. “Undo Last Writes” actually undoes every still-applied session write, not the last turn. The label is misleading. Memory CRUD is not exposed.

**Fix:** align labels to semantics or implement run IDs, list persisted checkpoints, require confirmation for broad undo, and expose memory export/edit/delete with provenance.

### 4.7 CI, release, dependencies, and governance

#### HIGH-23 — CI gates are permissive and non-reproducible

- Linux accepts any score ≥30/51; it does not require named expected tasks, so regressions can be masked by unrelated passes.
- Darwin PRs skip the full harness; the first full signal arrives after merge.
- Harness `main` prints failures but does not set a failing process exit status; CI must scrape text.
- Rust `stable`, Node 20, global MCP latest, Homebrew Ollama latest, and mutable Ollama model tags are unpinned.
- Cargo commands omit `--locked`; no MSRV file exists.
- No `cargo fmt --check`, Clippy, rustdoc, cargo-audit/deny, code coverage, Miri, sanitizer, fuzz, or property test gate.
- GitHub Actions are tag-pinned, not commit-SHA-pinned. Current runs warn that Node 20 actions are deprecated/forced onto Node 24.

#### HIGH-24 — Release workflow has never produced evidence

No Release workflow runs exist in GitHub history. The workflow can upload unsigned DMGs by default. It does not run the full test suite, generate SBOM/provenance, validate bundled runtime resources, or launch the installed artifact. `Package.resolved` is absent, so Swift dependencies are not repository-locked.

#### HIGH-25 — Public repository lacks legal/security governance

No `LICENSE`, `SECURITY.md`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, CODEOWNERS, changelog, Dependabot/Renovate policy, or published support window was found. GitHub reports no repository license. The sampled 47 merged PRs were all authored by the repository owner; five had bot comments and none had human review. This creates legal ambiguity and a severe review/bus-factor risk.

---

## 5. Medium-severity findings

### Architecture / code quality

1. **Duplicate compaction implementations:** `compaction.rs` is exported while `context_compact.rs` is orphaned/uncompiled and defines a divergent API and behavior. Remove one source of truth.
2. **Version inconsistency:** Cargo crates are `0.1.0`, FFI returns `1.4.0-phase4`, release defaults to `0.1.0`, and README calls the product 1.0. Establish one semver/build metadata source.
3. **Silent configuration fallback:** invalid/missing model registry or selected profile silently falls back to environment Ollama ([`aether-core/src/lib.rs:137-152`](../crates/aether-core/src/lib.rs)). Surface configuration errors.
4. **Unsupported UI IPC probes:** Settings calls daemon methods not implemented by `server.rs` (`get_model_config`, `store_byok_key`, `delete_byok_key`) and relies on “Unknown method” fallback. Either implement/version-negotiate or remove.
5. **Relative resource discovery:** skills, allowlist, and registry depend on process CWD in development and packaging. Use explicit resource roots.

### Security / authorization

6. **Incomplete audit coverage:** direct fs read/write, git, verify, and some denied paths do not consistently call `audit_decision`; session logs are not equivalent to permission-decision logs.
7. **Audit hash limitations:** the global hash chain excludes exit code, duration, and created timestamp; it is unkeyed and can be recomputed by anyone with DB write access. Describe it as accidental-tamper detection, not non-repudiation.
8. **Approval taxonomy is incomplete:** implicit skill writes, git writes, subprocess effects, and new-file creation are treated as safe. “New file” can still contain sensitive or executable content.
9. **BYOK endpoint redirection:** a user-controlled registry `base_url` causes the Keychain API key to be sent to that endpoint. Custom endpoints may be intended, but UI must display and confirm the exact origin before first credential use.
10. **Secrets in process command arguments:** macOS `security add-generic-password ... -w <secret>` puts secret material in child argv ([`keychain.rs:72-94`](../crates/aether-core/src/keychain.rs)). Prefer Security.framework/keyring APIs without CLI argv.
11. **Hook/DLP controls are brittle:** fixed substring blocks/redactions are case-sensitive in output and easy to evade; false positives block legitimate security-analysis prompts. Treat them as defense-in-depth only.

### Data / AI correctness

12. **Hard-coded graph “current date”:** graph-v2 decay uses `2026-08-04` forever ([`graph_v2.rs:33-35`](../crates/aether-db/src/graph_v2.rs)). Ranking recency is already stale and will become increasingly incorrect.
13. **Session isolation by string prefix:** retrieval filters chunk IDs with `starts_with("{session_id}::")`; IDs such as `a` and `a::b` overlap. Store/query an explicit `session_id` column in semantic memory.
14. **Global retrieval starvation:** search is global, over-fetches 64, then filters to session. Other sessions can crowd out valid results; the code acknowledges this. Query isolation must happen in SQL/vector index.
15. **No session ownership boundary:** any authenticated client can select any session ID/checkpoint/run ID. This may be acceptable for a single-user daemon, but it must be explicit and cannot support multi-user use.
16. **Plaintext sensitive retention:** prompts, tool outputs, prior file content, graph facts, and conversations are stored in SQLite/JSONL without encryption, TTL, redaction policy, or complete deletion UI.
17. **Graph extraction cap only covers nodes:** edges/properties/evidence lengths lack explicit application caps beyond model output tokens.
18. **Graph time approximation:** date parsing uses an approximate `year*365.25 + month*30 + day`; use real UTC duration arithmetic.
19. **User-memory edit lacks session parameter/check:** `update_user_memory_fact` updates by node ID alone, unlike delete. Add ownership verification before exposure.
20. **Tool-result injection protection is replan-only and heuristic:** original plans and fixed skill/MCP content rely on separate controls; correlation by long substrings can miss paraphrase/encoding and create false confidence.
21. **Verification shell does not lint written code:** any successful `python_lint` source satisfies the write gate, even if unrelated to the written file. The harness’s valid dummy Python source demonstrates this design.
22. **Structured engine reports success without explicit `done`:** direct structured plans that exhaust normally can emit `Done`/`done: true` even if no `ToolInvocation::Done` was encountered. NL validation catches this, direct JSON parsing does not.

### Reliability / performance

23. **Session log is O(total history) per append** and rewrites the full file on truncation. Move transcript storage to transactional DB rows or indexed append metadata.
24. **Large file reads are fully buffered:** fs read and subagent read load entire UTF-8 files before truncating output; binary and large-file behavior is poor.
25. **Undo stores complete prior text in SQLite:** there is no size limit and metadata/mode/xattrs are not restored. “Byte-identical” applies only to supported UTF-8 content.
26. **No observability contract:** there are logs but no metrics for queue depth, tool latency, model errors, memory ingest failures, approval outcomes, DB contention, or recovery; app-spawned daemon stdout/stderr is discarded.
27. **No graceful shutdown/drain:** server loops and background tasks do not coordinate cancellation, queue leasing, or in-flight journal completion.
28. **Model HTTP error bodies are unbounded before formatting** and may expose provider response details to clients/logs.
29. **Telegram token appears in request URL:** request errors can include URLs and risk token leakage in logs. Use APIs/header patterns that avoid logging credential-bearing URLs and sanitize errors.
30. **Checkpoint and consolidation actions lack optimistic revision checks**, so state can drift between display and apply.

### Test / evaluation quality

31. **Fixed-corpus overfitting risk:** many “industry” claims rely on a small frozen corpus and exact keyword/trajectory thresholds rather than property tests, mutation tests, fuzzing, or independent red-team evaluation.
32. **Soft probes count toward 51/51:** top-line pass count combines production gates and soft probes, obscuring release-critical status.
33. **ROUT metric can accept invalid zero timing:** missing `prompt_eval_duration` becomes zero for a resident model ([`aether-core/src/lib.rs:928-944`](../crates/aether-core/src/lib.rs)). A merged bot review flagged this; it remains present.
34. **ROUT residency errors become nonresident:** harness uses `unwrap_or(false)` for `/api/ps` measurement ([`golden_harness/main.rs:851-860`](../tests/golden_harness/src/main.rs)). This can distort the metric.
35. **No test for actual crash window:** RES-01 does not prove filesystem restoration across each journal boundary.
36. **No malformed/slow protocol suite:** IPC and webhook parsers need fuzz/property testing and slowloris/oversize/Unicode boundary cases.
37. **No permission matrix generated from tool metadata:** hand-maintained tests can miss new implicit side effects.
38. **Headless output ignores write errors:** broken stdout can still lead to success ([`headless.rs:179-260`](../crates/aether-daemon/src/headless.rs)).

### Documentation / product claims

39. `AGENTS.md` and `docs/INSTALL.md` still describe 29/33-task eras and old Linux scores.
40. `DAEMON_IPC.md` uses stale `"tool"` plan fields, says grants are auto-inserted, duplicates auth rows, and conflicts with current behavior.
41. `docs/LINUX_CI.md` shows both 41/10 and historical 42/9 as if current output.
42. README’s Phase 8 row says gates remain while later text says closed.
43. Install docs say consolidation apply/reject is unimplemented even though code/UI exists.
44. README duplicates Phase 8 closure links and contains stale commit baselines.
45. The scoreboard script passes despite these contradictions, showing that it validates selected strings rather than documentation truth.

---

## 6. Film-pipeline review

The film directories are operationally separate products and should be split into separate repositories or, at minimum, independent workspaces/CI jobs. Their large generated assets and dependencies should not affect the agent-runtime repository.

### 6.1 Blindspot

**Strengths**

- Clear manifests for blocks, shots, timeline, narration, model endpoints, and sheets.
- Paid generation can be dry-run; prompt composition is recorded.
- Assembly commands use subprocess argument arrays rather than shell interpolation.
- Generated output is ignored locally.
- Creative continuity and cost gates are documented better than in many generative-media projects.

**Findings**

1. `requests>=2.31` and `pillow>=10.0` are ranges without a lock or hash; ffmpeg/font versions are unspecified.
2. No CI validates manifests, timing, prompt length, dry runs, or assembly.
3. Endpoints marked unverified can still be used after only a warning; paid-call safety should require `--allow-unverified-endpoint`.
4. API queue `status_url` and `response_url` are trusted and requested with the `Authorization: Key <FAL_KEY>` header. A malicious/compromised response could redirect credential-bearing requests to another host ([`film/blindspot/scripts/falgen.py:47-105`](../film/blindspot/scripts/falgen.py)). Validate HTTPS origin against an allowlist and do not attach credentials to returned URLs by default.
5. Downloads write directly to final names without temporary files, checksums, content-type/size checks, or resume.
6. Metadata may retain signed result URLs and provider payloads. Define a redaction/retention policy before publishing outputs.
7. Reproduction remains dependent on mutable cloud model endpoints and potentially provider-generated seeds; pin endpoint revisions and record explicit seed/model version where available.
8. The “one command” pipeline intentionally pauses for manual sheet promotion, so it is a staged workflow rather than fully unattended reproducibility. That is fine, but wording should be precise.

### 6.2 Department of Continuity

**Critical operational defect**

Five generation scripts insert `<repo>/scripts` into `sys.path` and import `falgen`, but `<repo>/scripts/falgen.py` does not exist. Direct execution fails immediately with `ModuleNotFoundError`. A copy exists at `film/department-of-continuity/shared_scripts/falgen.py`, but scripts do not import it. That copy also defaults to loading a `models` manifest that is absent from the Department’s manifests.

Affected entry points include:

- `generate_references.py`
- `generate_state_edits.py`
- `generate_proof_keyframes.py`
- `generate_proof_motion.py`
- `generate_scratch_audio.py`

**Additional findings**

1. No `requirements.txt`, lockfile, environment guide, or project `.gitignore` exists.
2. Two tracked `__pycache__/*.pyc` files exist under Blindspot; Department generated caches were not ignored by root policy.
3. Department tracks roughly 66 MiB of generated JPG/WAV output, including a 9.4 MiB scratch mix. Use Git LFS or artifact storage and retain manifests/checksums/provenance in Git.
4. Scripts use hard-coded Linux paths (`/usr/share/fonts`, `/opt/cursor/artifacts`), conflicting with the repo’s canonical macOS platform.
5. Temporary concat-list files are not deleted in `assemble_proofs.py`.
6. There is no master end-to-end runner, check-only mode, paid-call confirmation, or endpoint manifest local to this production.
7. AI-generated approved references lack tracked generation metadata, source terms/license status, model version, prompt, seed, and human approval record beside the assets.
8. Creative files are strong, but production engineering is not reproducible or portable in its current state.

---

## 7. Standards-oriented gap map

This is not a certification. The mapping highlights controls expected in mature engineering programs.

| Standard lens | Current gap |
|---|---|
| **NIST SSDF** | No security policy, threat model, dependency vulnerability gate, release provenance, incident process, or independent review requirement. |
| **SLSA / artifact integrity** | No SBOM, provenance attestation, hermetic/pinned toolchain, action SHA pinning, or verified resource manifest. |
| **OWASP ASVS-style input/auth controls** | One-sided local auth, predictable token generation, no request-size limits, weak webhook origin verification, and incomplete authorization binding. |
| **OWASP LLM application risks** | Heuristic prompt/result injection defenses, unbound approvals, excessive implicit tool effects, production skill-pinning gap, memory poisoning/retention concerns. |
| **CWE-330** | Predictable daemon auth token construction. |
| **CWE-345 / 346** | Insufficient verification of webhook origin and local daemon identity. |
| **CWE-367** | Approval/path/verified-file TOCTOU windows. |
| **CWE-400** | Unbounded IPC lines/connections and child/MCP waits. |
| **CWE-662 / concurrency integrity** | Cross-resource operations lack atomicity; global mutex creates availability coupling. |
| **Apple platform hardening** | Overbroad entitlements, no app sandbox enforcement for bookmark claims, no UI security E2E. |
| **Open-source readiness** | No license, support/security policy, contribution rules, release notes, or legal provenance for media assets. |

---

## 8. Remediation roadmap

### Phase 0 — Stop-the-line (0–3 days)

1. Remove/qualify “1.0 engineering complete” and current 51/51 claims until canonical CI is green.
2. Fix the 41-vs-42 Darwin gate and make mandatory harness failures set a nonzero exit code.
3. Mark the current DMG/release and gateway features experimental; do not expose webhook ports.
4. Disable the Swift BYOK-over-IPC attempt until mutual daemon authentication exists.
5. Fix IPC terminal semantics, partial writes, socket timeouts, and `pending_approval` termination.
6. Route ordinary UI agent requests intentionally; remove the 16-token production limit.
7. Add a LICENSE or make the repository private until licensing is decided.

### Phase 1 — Authority and data integrity (week 1–2)

1. Implement CSPRNG token generation and authenticated Unix-domain IPC.
2. Bind approval to an immutable, stored plan digest and single-use nonce.
3. Make `git_init` non-destructive and fully approval/journal aware.
4. Redesign write journaling and startup recovery with fault-injection tests.
5. Implement atomic checkpoint operation journaling.
6. Integrate skill install/admit pins into production ToolRegistry.
7. Make MCP policy fail closed; add process timeouts and immutable verified install records.
8. Correct the unsafe vector decoding.
9. Sign/hash consolidation diffs and validate run/session/node ownership.

### Phase 2 — Platform hardening (week 2–4)

1. Replace hand-written HTTP parsing with a maintained server stack.
2. Add mandatory provider signature/sender/replay verification and idempotency.
3. Pool or actor-isolate DB access; move blocking work to dedicated executors.
4. Add queue leases/recovery/dead-letter behavior.
5. Add explicit semantic-memory session columns and query-time isolation.
6. Make ingest idempotent/transactional and add retention/delete/export policy.
7. Replace hard-coded graph time with real UTC time.
8. Remove unnecessary hardened-runtime entitlements.
9. Bundle registry, skills, allowlist, and schemas as versioned app resources.

### Phase 3 — Verification and release engineering (week 3–6)

1. Add Swift unit/integration/UI/accessibility tests for every tab and IPC method.
2. Add Rust fmt, Clippy (`-D warnings` after cleanup), rustdoc, cargo-deny/audit, coverage, Miri for unsafe code, and targeted fuzz/property tests.
3. Require named per-task CI outcomes rather than aggregate Linux threshold.
4. Pin Rust MSRV/toolchain, Node, npm package version/integrity, Ollama version, model digest, Swift resolution, and GitHub Action SHAs.
5. Generate CycloneDX/SPDX SBOM and signed provenance; use `cargo --locked`.
6. Run an installed-app E2E from the staged signed bundle, including bundled resources, sandbox, Keychain, and daemon launch.
7. Require at least one independent human approval for security/release changes.
8. Publish privacy/security/support docs and a release changelog.

### Phase 4 — Film pipeline separation and reproducibility

1. Move each film to its own repository/environment.
2. Fix Department imports and add a local model endpoint manifest.
3. Lock Python and system-media dependencies; containerize Linux rendering or document a macOS equivalent.
4. Add JSON Schema validation, dry-run CI, paid-call confirmation, URL-origin validation, atomic downloads, and metadata redaction.
5. Move generated assets to LFS/artifact storage and track checksums plus rights/provenance records.

---

## 9. Minimum production acceptance gates

A release candidate should not be called 1.0 until all of the following are objectively demonstrated:

### Functional

- A fresh installed app can select a workspace, send ordinary natural language, display the exact proposed plan, approve it once, execute that exact plan, show streamed progress, and complete without blocking.
- Undo and checkpoint survive app/daemon restart and accurately enumerate non-reversible effects.
- Model settings, skills, MCP policy, and resource discovery work from the staged app—not from a source checkout.

### Security

- Local daemon impersonation test fails to capture token/BYOK/prompts.
- Approval replay, plan substitution, workspace TOCTOU, and altered-skill/MCP artifacts fail closed.
- Provider webhook signature, sender, timestamp, replay, and idempotency tests pass.
- Linux is either explicitly unsupported for tool execution or receives a real OS sandbox.
- Entitlement review shows least privilege.

### Reliability / integrity

- Fault injection before/after every file-journal state transition restores or deterministically reconciles state.
- Checkpoint fault injection cannot silently split filesystem/transcript/DB state.
- Hung MCP/tool/model operations time out, release resources, and do not block other sessions.
- Automation recovers abandoned leases and does not duplicate a provider event.

### Quality / release

- Exact-commit Linux and Darwin required matrices pass repeatedly.
- Swift tests and app E2E pass on macOS.
- Format, lint, audit/deny, docs, fuzz targets, and artifact E2E are required checks.
- Release artifact has SBOM, provenance, checksums, Developer ID signature, notarization, stapling, and a valid configured Sparkle feed/key—or update functionality is omitted.
- Public repository has explicit licensing, security reporting, privacy/retention, and support policies.

---

## 10. Final disposition by persona

- **Principal architect:** promising modular prototype; not yet an integrated 1.0 product.
- **AppSec lead:** no-go until local server authentication, immutable approval, gateway verification, production skill/MCP admission, and entitlement issues are fixed.
- **Rust systems lead:** no-go for data-sensitive use until unsafe vector decoding and crash/atomicity semantics are corrected.
- **Data/AI lead:** useful memory research, but isolation, lifecycle, transactional ingest, recency, and poisoning claims need hardening.
- **SRE lead:** no-go for unattended automation; hangs, global serialization, missing queue leases, and contradictory stream terminals are incident-prone.
- **QA lead:** harness culture is a strength, but current canonical CI is red and major product paths are untested.
- **macOS lead:** source builds have passed CI, but the real UI workflow is blocked by client protocol semantics and lacks E2E/accessibility evidence.
- **Release lead:** no-go; no successful release workflow or installed-artifact validation, incomplete resources, broad entitlements, and no provenance.
- **Media lead:** Blindspot is a credible creative pipeline prototype; Department of Continuity is currently broken and both need separate dependency/provenance controls.
- **Governance lead:** public distribution/contribution is not ready without a license, policies, and independent review.

**Bottom line:** retain the repository’s strong safety-oriented design intent, but reset the public status to **pre-release engineering prototype**. Fix the primary app path and authority/data-integrity blockers first; then rebuild the scorecard around installed-product evidence rather than feature probes.