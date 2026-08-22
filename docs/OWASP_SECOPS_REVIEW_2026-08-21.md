# AetherForge OWASP SecOps Review

- **Review date:** 2026-08-21
- **Scope:** current Arena working tree, including the critical-remediation patch represented by PR #51
- **Validated code revision:** PR #51 `ce71a60` (Linux and Darwin-fast passed)
- **Review type:** white-box SecOps / architecture / control assessment
- **Release decision:** **NO-GO for production, internet exposure, external skill/MCP ecosystems, or sensitive-data use**
- **Conditionally acceptable use:** single trusted developer, local macOS, loopback-only daemon, gateway/automation/MCP disabled, non-sensitive test workspace

---

## 1. Frameworks and interpretation

This review uses multiple OWASP views because AetherForge is simultaneously a desktop application,
localhost API, agentic AI runtime, RAG/vector-memory system, automation engine, MCP tool host, and
software-distribution pipeline.

Authoritative baselines:

1. [OWASP Top 10:2025](https://owasp.org/Top10/2025/) — conventional application risks.
2. [OWASP ASVS 5.0.0](https://owasp.org/www-project-application-security-verification-standard/) — verification-control baseline; released 2025-05-30.
3. [OWASP GenAI LLM Top 10:2026](https://github.com/GenAI-Security-Project/GenAI-LLM-Top10) — current LLM risk ordering.
4. [OWASP Top 10 for Agentic Applications 2026](https://genai.owasp.org/download/52117) — agent autonomy, tools, identity, memory, and trust.
5. [OWASP API Security Top 10:2023](https://owasp.org/API-Security/editions/2023/en/0x11-t10/) — daemon IPC and webhook/API risks.
6. [OWASP Software Component Verification Standard](https://scvs.owasp.org/scvs/preface/) — software supply-chain controls.

OWASP’s Top 10 is an awareness/risk framework rather than a certifiable checklist. OWASP itself
recommends ASVS for comprehensive verification. Accordingly, this document uses Top 10 categories
for risk communication and ASVS/SCVS concepts for release gates.

### Target assurance level

For a local agent that can read/write source trees, launch child processes, initialize repositories,
inject secrets into tools, and retain user memory, **ASVS Level 2-equivalent assurance** is the
minimum sensible target. Level 1-style opportunistic controls are insufficient because a compromised
planner or dependency can create persistent integrity and confidentiality impact.

### Important scope qualification

AetherForge is currently single-user and loopback-first. Some API authorization findings have lower
exploitability in that exact deployment than they would in a multi-user or network service. They
remain architectural blockers because:

- `AETHER_DAEMON_ADDR` can change the bind address;
- non-macOS authentication is optional;
- gateways and automation introduce remote inputs;
- future multi-user/service reuse would immediately turn session/run IDs into authorization objects;
- local malware, poisoned dependencies, malicious workspace content, and compromised model/tool
  infrastructure are explicitly relevant agent threats.

---

## 2. Executive security posture

### Overall verdict

**OWASP readiness: high residual risk.** The implementation has credible safety primitives, but the
controls are unevenly integrated. The strongest controls are path confinement, parameterized SQL/OS
commands, immutable approval records, webhook HMAC/replay checks, schema-constrained planning, and
macOS child environment scrubbing. The weakest areas are agentic supply chain, indirect prompt/memory
influence, principal/object authorization, data-at-rest protection, resource exhaustion, audit
completeness, and release provenance.

### Risk totals

| Severity | Count | Release meaning |
|---|---:|---|
| **Critical** | 2 | Must be fixed before enabling external skills/MCP or calling the agent production-safe. |
| **High** | 12 | Must be fixed before public/sensitive-data release. |
| **Medium** | 6 | Must be scheduled with owners and tested before 1.0. |

### Two red-line findings

1. **Agentic supply-chain policy is evaluated but not enforced by production execution.** Skill
   `install_skill`/`admit_skill` pinning is tested, but production loads skills directly and executes
   without requiring a pin or even a capability manifest. MCP policy can silently fall back to
   runtime self-computed executable/script hashes when the curated allowlist is absent or malformed.
2. **Initial planning can still be influenced by untrusted memory/context, while approval is not
   comprehensive or fully informed.** Retrieved memory is marked with textual trust delimiters but is
   placed in the same model message. Cross-call correlation runs only for repair plans. New-file writes
   and `skill_execute` do not require approval; the approval UI shows risky summaries, not the full
   canonical plan, arguments, or write content.

### Material improvements verified since the baseline review

- Versioned 256-bit OS-CSPRNG daemon token and legacy-token rotation.
- Nonce-bound client verification of daemon token possession.
- Complete Swift socket writes, receive/send timeouts, bounded response buffers, and terminal event handling.
- Expiring, single-use, server-stored approvals bound to plan/session/workspace; `approved:true` rejected.
- Crash-window journal reconciliation preserves undo or fails closed on ambiguity.
- `git_init` is approval-required, refuses an existing repo, and no longer overwrites README.
- Telegram/Slack/GitHub webhook authentication and provider-event replay protection.
- Discord webhook route fails closed pending Ed25519 verification.
- Explicit Agent/Chat routing; benchmark limits no longer constrain production chat.
- Misaligned vector-read undefined behavior and hard-coded graph time were fixed.
- Linux and Darwin-fast/Swift CI passed for PR #51.

These are meaningful controls, but they do not remove the residual findings below.

---

## 3. Threat model

### Security assets

- User workspaces and source repositories.
- BYOK cloud-model key, gateway bot tokens, brokered MCP secrets, daemon auth token.
- SQLite conversations, graph facts, vectors, permissions, approvals, undo journal, audit log.
- JSONL session transcripts and consolidation artifacts.
- Installed skills, MCP executable/script/tool descriptions, model registry and model weights.
- Signed/notarized app, update feed, release artifacts, CI credentials and provenance.
- Human approval decisions and their relationship to exact agent plans.

### Threat actors

1. Authenticated but malicious or compromised local client.
2. Malicious prompt author or authorized remote gateway sender.
3. Poisoned workspace document, retained conversation, retrieved memory, or tool result.
4. Malicious/compromised skill, MCP package, Node executable, model, model registry, or cloud endpoint.
5. Local same-user malware or daemon-port relay process.
6. Remote webhook attacker, replay attacker, or provider-account compromise.
7. CI/CD dependency/action/package compromise.
8. Accidental operator misconfiguration or crash during a multi-resource mutation.

### Primary trust boundaries

1. SwiftUI client ↔ TCP JSONL daemon.
2. User/memory text ↔ NL planner.
3. Model plan ↔ deterministic validator/approval gate.
4. Daemon ↔ filesystem/git/Python child processes.
5. Daemon ↔ MCP server and brokered secrets.
6. Daemon ↔ Ollama/BYOK endpoint.
7. Remote gateway/provider ↔ webhook adapters.
8. SQLite ↔ JSONL/artifact filesystem state.
9. Source checkout/CI ↔ packaged signed application.
10. Film pipeline ↔ fal queue/status/result/media URLs.

---

## 4. Prioritized finding register

### SEC-OWASP-01 — Production agentic skill/MCP admission fails open

- **Severity:** **Critical**
- **OWASP:** A03 Supply Chain; A08 Software/Data Integrity; LLM04 Supply Chain; ASI04 Agentic Supply Chain; ASI05 Unexpected Code Execution; SCVS
- **Evidence:**
  - Production loads `skills/` directly and converts definitions into the runtime map
    ([`task_runner.rs:902-911`](../crates/aether-daemon/src/task_runner.rs)).
  - `SkillExecutor::execute` scans strings and credential-shaped paths, but only validates a manifest
    *if present* and never calls `admit_skill` or checks a persisted pin
    ([`aether-skills/src/lib.rs:124-205`](../crates/aether-skills/src/lib.rs)).
  - Stronger `install_skill`/`admit_skill` pin checks exist separately
    ([`trust.rs:213-259`](../crates/aether-skills/src/trust.rs)) and therefore create false assurance.
  - MCP resolution computes hashes for whatever Node/script runtime discovery finds. If
    `mcp_allowlist.json` is absent, malformed, or missing the entry, execution can continue with
    self-computed pins and no tools hash ([`aether-mcp/src/lib.rs:70-104`](../crates/aether-mcp/src/lib.rs)).
  - MCP request/response waits have no protocol deadline or line-size cap
    ([`runtime.rs:213-269`](../crates/aether-mcp/src/runtime.rs)).
- **Exploit narrative:** a modified local skill can retain benign-enough wording to bypass substring
  scans and execute append/read actions without a verified installed digest. A replaced Node binary or
  missing curated allowlist can become self-trusted by runtime discovery. If an MCP grant is present,
  the process receives workspace access and potentially a brokered secret.
- **Impact:** persistent workspace modification, secret theft, process execution, denial of service,
  supply-chain compromise.
- **Required remediation:**
  1. Persist installed skill ID, publisher/source, manifest, content hash, reviewed capabilities, and
     install timestamp.
  2. Require `admit_skill` on every production execution; no manifest/no pin must be a hard failure.
  3. Bind a skill approval to digest and capability set; changing either requires reapproval.
  4. Make curated MCP policy mandatory and bundle it in the signed app.
  5. Pin executable, entry script/package integrity, tool schemas/descriptions, and approved tool names.
  6. Remove trust-on-discovery; environment override must require explicit admin enrollment.
  7. Add MCP initialize/list/call timeout, process kill, stdout cap, and protocol-state validation.

### SEC-OWASP-02 — Untrusted memory/context can steer initial plans outside comprehensive approval

- **Severity:** **Critical**
- **OWASP:** A05 Injection; A06 Insecure Design; LLM01 Prompt Injection; LLM03 Excessive Agency; LLM05 Data/Model Poisoning; ASI01 Goal Hijack; ASI02 Tool Misuse; ASI06 Memory/Context Poisoning; ASI09 Human-Agent Trust Exploitation
- **Evidence:**
  - Retrieved memory is textually labelled untrusted, but concatenated into one planner prompt
    ([`task_runner.rs:110-129`](../crates/aether-daemon/src/task_runner.rs)). OWASP notes that models do
    not reliably enforce a privilege boundary merely from textual delimiters.
  - Agent mode invokes the NL planner on that enriched goal before tool execution
    ([`task_runner.rs:230-247,538-560`](../crates/aether-daemon/src/task_runner.rs)).
  - Cross-call injection correlation is applied to repair plans, not the initial memory-enriched plan
    ([`task_runner.rs:390-499`](../crates/aether-daemon/src/task_runner.rs),
    [`inject.rs:164-287`](../crates/aether-core/src/inject.rs)).
  - Approval covers overwrite, MCP, and Git only. New-file writes and `skill_execute` are treated as
    non-risky ([`risk.rs:27-88`](../crates/aether-core/src/risk.rs)).
  - The Swift approval sheet displays risk summaries and the original prompt, not the entire exact
    stored plan, MCP arguments, skill variables, or replacement file content.
- **Exploit narrative:** poison retained session memory with indirect instructions. A later benign goal
  retrieves that text. The planner adds a new-file write or skill action. Because that class is not
  considered risky, it executes without human confirmation. Even where approval triggers, the user
  does not see all canonical arguments/content they are authorizing.
- **Impact:** goal redirection, persistent source/config changes, hidden manipulation, trust exploitation.
- **Required remediation:**
  1. Treat model output as untrusted policy input; authorize every plan with deterministic rules.
  2. Run initial plans through the same dependency/correlation policy as repairs.
  3. Track provenance from each plan field to trusted user text vs retrieved/tool content.
  4. Require approval for all writes, skill execution, secret use, and externally visible actions;
     support a narrowly scoped “accept new files” policy only if explicitly configured.
  5. Display the exact normalized plan, arguments, content diff, capabilities, model, memory sources,
     and digest before approval.
  6. Add memory quarantine, trust labels enforced outside the model, per-source allowlists, and
     poisoning detection/rollback.

### SEC-OWASP-03 — One bearer token is an all-powerful principal; object ownership is absent

- **Severity:** **High**
- **OWASP:** A01 Broken Access Control; API1 BOLA; API3 Object Property Authorization; API5 Function Authorization; ASI03 Identity/Privilege Abuse
- **Evidence:**
  - One daemon token authorizes every non-ping method ([`server.rs:99-106`](../crates/aether-daemon/src/server.rs)).
  - An authenticated client can grant arbitrary existing directories as read/write workspaces
    ([`server.rs:331-389`](../crates/aether-daemon/src/server.rs)).
  - Checkpoint rewind accepts only global checkpoint ID; consolidation apply/reject accepts only run ID.
  - Session IDs are caller-selected; no owner/client principal is stored.
  - Approval IDs are correctly session/workspace-bound, but that session is not owned by a distinct principal.
- **Impact:** token compromise gives grant creation, arbitrary-session execution, undo/rewind,
  consolidation mutation, and automation registration. Multi-user or service deployment is unsafe.
- **Required remediation:** use per-client identities and scoped capabilities; authorize every object by
  owner/session/workspace; split admin methods from task methods; never let an ordinary client mint its
  own workspace grant without proof of a user-mediated broker action.

### SEC-OWASP-04 — Local IPC can become cleartext network IPC and remains relayable

- **Severity:** **High**
- **OWASP:** A02 Security Misconfiguration; A04 Cryptographic Failures; A07 Authentication Failures; API2 Broken Authentication; API8 Misconfiguration
- **Evidence:**
  - `AETHER_DAEMON_ADDR` can override loopback; transport has no TLS or peer credential binding.
  - Non-macOS auth defaults to empty/disabled ([`main.rs:33-43`](../crates/aether-daemon/src/main.rs),
    [`server.rs:11-17`](../crates/aether-daemon/src/server.rs)).
  - CSPRNG tokens and nonce proofs are an improvement, but the proof is a custom
    `SHA-256(prefix || token || nonce)` construction rather than a standard HMAC.
  - A same-user attacker can potentially relay challenge/requests through a legitimate daemon instance;
    possession proof authenticates the secret holder, not the endpoint/process or peer credentials.
- **Impact:** token/prompt interception if remotely bound, local daemon impersonation/relay, full authority theft.
- **Required remediation:** enforce loopback in code unless an explicit TLS/mTLS server mode is enabled;
  fail startup if auth is empty; prefer a Unix-domain socket in a 0700 directory and verify peer UID;
  use HMAC-SHA256 or an asymmetric daemon identity; bind proof to protocol version, endpoint, and session.

### SEC-OWASP-05 — IPC, webhook, and MCP resource consumption is insufficiently bounded

- **Severity:** **High**
- **OWASP:** A10 Mishandling Exceptional Conditions; API4 Unrestricted Resource Consumption; API6 Sensitive Business Flows; LLM06 Unbounded Consumption; ASI08 Cascading Failures
- **Evidence:**
  - Daemon `BufReader::lines()` has no server-side line-size/read timeout; every connection gets a
    Tokio task ([`server.rs:61-81`](../crates/aether-daemon/src/server.rs)). Swift’s 1 MiB client cap
    does not protect the server from another client.
  - Caller controls `max_iterations` and `max_tokens`; `max_tokens=0` means unlimited
    ([`task_runner.rs:42-52,266-275`](../crates/aether-daemon/src/task_runner.rs),
    [`loop_engine.rs:886-923`](../crates/aether-core/src/loop_engine.rs)).
  - MCP blocking `read_line` can wait forever and allocate an unbounded line.
  - Hand-written HTTP parsers lack read deadlines/connection limits and can panic when byte
    `Content-Length` truncates a `String` at a non-UTF-8 boundary.
  - One global DB mutex amplifies a hung tool into daemon-wide denial.
- **Required remediation:** enforce server-side frame/body/header/concurrency limits; cap caller budgets;
  use per-tool/process deadlines; replace custom HTTP parser; use DB actor/pool with short transactions;
  add rate limiting and queue quotas per identity/channel.

### SEC-OWASP-06 — Sensitive data is retained in plaintext with incomplete lifecycle controls

- **Severity:** **High**
- **OWASP:** A04 Cryptographic Failures; LLM02 Sensitive Information Disclosure; LLM08 Hidden Context Exposure; ASVS data protection
- **Evidence:**
  - JSONL stores full prompts and tool/observation payloads
    ([`session_log.rs:143-193`](../crates/aether-daemon/src/session_log.rs)).
  - SQLite stores full conversations, graph facts, approval plans/prompts, and complete prior file
    content for undo.
  - `~/.aether` DB/session file modes are not explicitly created as 0600/0700; behavior depends on umask.
  - No retention TTL, selective purge, user export/delete for all zones, or encrypted database design.
  - Generic output redaction is small pattern matching; secret value redaction is exact-string only.
- **Impact:** local disclosure of proprietary source fragments, prompts, credentials, personal facts,
  and historical file contents; backups retain deleted workspace content.
- **Required remediation:** classify data; mode-restrict directories/files atomically; encrypt sensitive
  fields or database using a Keychain-wrapped key; minimize tool-output retention; redact before every
  sink; implement retention and cryptographic deletion; expose complete data inventory/export/delete.

### SEC-OWASP-07 — BYOK and secret handling can redirect or expose credentials

- **Severity:** **High**
- **OWASP:** A04 Cryptographic Failures; A07 Authentication Failures; API7 SSRF; API10 Unsafe API Consumption; LLM02 Disclosure; NHI secret leakage
- **Evidence:**
  - Registry `base_url` sets global `AETHER_BYOK_ENDPOINT`; bearer key is sent to that endpoint
    ([`model_registry.rs:86-96`](../crates/aether-core/src/model_registry.rs),
    [`aether-core/src/lib.rs:568-610,735-764`](../crates/aether-core/src/lib.rs)). HTTPS is not enforced.
  - Keychain CLI storage supplies secret as `security ... -w <password>` argv
    ([`keychain.rs:71-95`](../crates/aether-core/src/keychain.rs)).
  - Daemon token is duplicated to a 0600 fallback file even though Keychain exists.
  - Brokered MCP secrets are passed to the child environment; a malicious admitted MCP server receives them by design.
- **Required remediation:** explicit endpoint enrollment with hostname/TLS validation and confirmation;
  provider-specific endpoint allowlist; no global environment mutation; Security.framework/keyring-only
  writes without secret argv; remove token fallback by default; issue scoped/short-lived tool credentials.

### SEC-OWASP-08 — Consolidation’s reviewed artifact is not integrity-bound

- **Severity:** **High**
- **OWASP:** A08 Software/Data Integrity; LLM05 Data Poisoning; ASI06 Memory Poisoning
- **Evidence:** apply reads a mutable JSON path at execution time and does not verify artifact hash,
  run ID/session ID inside the artifact, or node ownership
  ([`consolidate.rs:198-253`](../crates/aether-db/src/consolidate.rs)). Swift displays a sibling
  Markdown file, not necessarily the exact JSON later applied.
- **Impact:** a local/agent process that can alter the artifact can supersede arbitrary graph nodes
  after the human reviews different content.
- **Required remediation:** store canonical diff bytes/hash in DB; render UI from those exact bytes;
  validate run/session/node/survivor ownership transactionally; sign or MAC external artifacts.

### SEC-OWASP-09 — Automation/gateway consent is not bound to immutable configuration

- **Severity:** **High**
- **OWASP:** A01 Broken Access Control; A06 Insecure Design; API6 Business Flow; LLM03 Excessive Agency; ASI02 Tool Misuse; ASI03 Privilege Abuse
- **Evidence:** grants bind trigger/channel and session identifiers, while registration uses
  `INSERT OR REPLACE`. A granted trigger can be changed to another task/workspace without changing
  the grant. Automation and gateway execution intentionally bypass the human plan approval gate.
- **Positive control:** Slack/Telegram/GitHub signatures, sender checks, and event replay tables now
  reduce unauthorized ingress; Discord fails closed.
- **Required remediation:** version every automation/channel config; bind grant to digest, workspace,
  capabilities, allowed tool set, remote sender, and expiry; require reapproval on any change; enforce
  per-run policy even for granted automation; add kill switch and run quota.

### SEC-OWASP-10 — Audit and alerting are incomplete and sometimes fail open

- **Severity:** **High**
- **OWASP:** A09 Security Logging and Alerting Failures; ASI08 Cascading Failures; ASI10 Rogue Agents
- **Evidence:**
  - Core fs read/write and Git permission checks do not consistently emit audit decisions.
  - Approval creation/consumption, auth failures, webhook signature/sender/replay denials, and
    consolidation review integrity are not comprehensively security-audited.
  - Session-log append failure only warns and execution succeeds
    ([`task_runner.rs:187-197`](../crates/aether-daemon/src/task_runner.rs)).
  - Some audit writes are deliberately discarded (`let _ = ...`).
  - App-launched daemon stdout/stderr goes to null, and no metrics/alert rules exist.
  - Audit hash chain is unkeyed and excludes fields such as duration/exit/timestamp; DB writers can recompute it.
- **Required remediation:** define mandatory security events and fail policy; structured logs with
  event IDs and redaction; HMAC/sign checkpoints; preserve local diagnostic logs securely; metrics
  and alerts for auth failures, approval replay, gateway denial, budget/timeouts, skill/MCP drift,
  recovery ambiguity, and audit sink failure.

### SEC-OWASP-11 — Semantic-memory isolation and poisoning controls are not structural

- **Severity:** **High**
- **OWASP:** API1 BOLA; LLM05 Data/Model Poisoning; LLM09 Vector/Embedding Weaknesses; ASI06 Memory/Context Poisoning
- **Evidence:** semantic storage is global. Retrieval searches globally, then filters by
  `chunk_id.starts_with("{session_id}::")` ([`task_runner.rs:65-91`](../crates/aether-daemon/src/task_runner.rs)).
  Session IDs `a` and `a::b` overlap; global top-64 results can starve the active session. Session ID
  ownership is not enforced. User and model output are automatically ingested into memory/graph
  ([`ingest.rs:330-480`](../crates/aether-daemon/src/ingest.rs)).
- **Required remediation:** explicit tenant/session columns in vector metadata and SQL/index-time
  authorization; opaque random session IDs; source trust/confidence; fact-review quarantine; robust
  delete/TTL; poisoning regression suite and cross-session property tests.

### SEC-OWASP-12 — CI/release supply-chain verification is below SCVS baseline

- **Severity:** **High**
- **OWASP:** A03 Software Supply Chain; A08 Integrity; LLM04 Supply Chain; ASI04; SCVS
- **Evidence:**
  - No `cargo-audit`, `cargo-deny`, OSV, Dependabot, secret scanner, CodeQL/SAST, SBOM, VEX, provenance,
    license policy, or artifact signing attestation gate.
  - Rust `stable`, global npm MCP latest, Homebrew Ollama latest, and mutable model tags are not immutable.
  - Actions use mutable major tags rather than commit SHAs; Node 20 actions generate deprecation warnings.
  - Cargo commands omit `--locked`; no `rust-toolchain.toml`; no `Package.resolved`.
  - Release workflow has never run and permits unsigned output by default.
  - No repository LICENSE or SECURITY policy exists.
- **Required remediation:** SCVS inventory and policy; locked toolchains/packages/model digests; action
  SHA pinning; SCA and license gate; CycloneDX SBOM plus VEX; SLSA-style provenance; mandatory signed
  and notarized release; security disclosure/response policy.

### SEC-OWASP-13 — Sandbox and app entitlements exceed least privilege

- **Severity:** **High**
- **OWASP:** A02 Misconfiguration; A06 Insecure Design; LLM03 Excessive Agency; ASI02 Tool Misuse; ASI05 Code Execution
- **Evidence:** Darwin Seatbelt uses `(allow default)`, denies network and writes outside workspace,
  but permits most reads outside workspace except `/etc` ([`sandbox_tool.sb`](../profiles/sandbox_tool.sb)).
  Linux child processes are not OS-sandboxed. Release entitlements allow JIT, unsigned executable
  memory, and disable library validation for binaries without a demonstrated need
  ([`AetherForge.entitlements`](../packaging/entitlements/AetherForge.entitlements)).
- **Required remediation:** deny-by-default profile with explicit runtime reads; separate profiles per
  tool; Linux Landlock/bubblewrap/seccomp or disable tooling; remove all three entitlements unless a
  documented, tested dependency requires one; separate app/daemon entitlement files.

### SEC-OWASP-14 — Third-party API URLs are trusted too broadly

- **Severity:** **High** for film pipeline; **Medium** for core trusted config
- **OWASP:** API7 SSRF; API10 Unsafe Consumption; A03 Supply Chain; A04 Credential Disclosure
- **Evidence:** fal queue response `status_url` and `response_url` are fetched with the FAL
  Authorization header without validating origin ([`film/blindspot/scripts/falgen.py:47-105`](../film/blindspot/scripts/falgen.py)).
  Media URLs are also unrestricted. BYOK endpoint comes from registry/environment.
- **Impact:** compromised API response/config can forward credentials to attacker-controlled HTTPS
  origin or use the host as an SSRF client.
- **Required remediation:** strict scheme/host/port allowlists; never forward credentials across
  origin; disable redirects or revalidate each hop; private/link-local/loopback IP denial where
  appropriate; DNS rebinding-safe resolution; response size/content-type limits.

### SEC-OWASP-15 — Exceptional-condition consistency remains incomplete

- **Severity:** **Medium**
- **OWASP:** A10 Mishandling Exceptional Conditions; A08 Integrity; ASI08 Cascading Failures
- **Evidence:** checkpoint rewind mutates filesystem, JSONL, and DB without a durable cross-resource
  transaction ([`checkpoint.rs:84-112`](../crates/aether-daemon/src/checkpoint.rs)). Automation rows
  can remain `running` after crash. Mutex locks use `unwrap`, so a panic can poison global DB access.
  Config parsing often uses `unwrap_or_default`, silently weakening trigger behavior. JSONL truncate
  is not atomic/fsynced.
- **Required remediation:** durable operation state machines, leases/recovery, atomic same-directory
  writes/fsync, poison recovery, typed fail-closed configuration, chaos/fault injection.

### SEC-OWASP-16 — Model misinformation and fact integrity lack user-visible confidence controls

- **Severity:** **Medium**
- **OWASP:** LLM07 Misinformation; ASI08 Cascading Failures; ASI09 Human-Agent Trust
- **Evidence:** graph extraction requires model-supplied evidence text but does not verify that
  evidence is an exact source span. Inferred facts and assistant output can enter retained memory.
  UI does not consistently show source trust/confidence at action time.
- **Required remediation:** source-span verification; extracted/inferred confidence policy; do not
  use inferred facts for authorization; citations in UI; contradiction and stale-fact warnings;
  human review for identity/security-sensitive memory.

### SEC-OWASP-17 — Output validation is strong for plan shape but incomplete end-to-end

- **Severity:** **Medium**
- **OWASP:** A05 Injection; LLM10 Improper Output Handling; ASI02 Tool Misuse
- **Positive controls:** JSON-schema-constrained plans; enum tool deserialization; workspace
  canonicalization; no `sh -c`; SQL parameters; verify shell; exact approved stored plan.
- **Residual gaps:** skill templates substitute arbitrary variable values; MCP argument confinement
  checks only top-level `path`; generic external tool schemas are not enforced; model/provider error
  bodies are surfaced; Markdown/artifact display is not bound to applied bytes.
- **Required remediation:** per-tool typed schemas and output contracts; contextual encoding; generic
  path-field capability metadata; output size/type limits; no raw provider body to user/audit.

### SEC-OWASP-18 — Approval is technically bound but not sufficiently informed or observable

- **Severity:** **Medium**
- **OWASP:** A06 Insecure Design; LLM03 Excessive Agency; ASI09 Human-Agent Trust
- **Evidence:** server plan binding and replay prevention are good. However, UI receives only risk
  strings and approval ID, not the full canonical plan/diff/digest. Approval events are not part of
  the hash-chained audit record. Caller can alter max-iteration/token values on the approval follow-up,
  although the tool plan remains unchanged.
- **Required remediation:** return/display signed plan envelope with exact args/diffs, model/profile,
  capabilities, budget, expiry and provenance; bind budget and policy version; audit requested,
  displayed, approved, consumed, expired, failed, and completed states.

### SEC-OWASP-19 — Session-log file naming can collide and integrity is not verified

- **Severity:** **Medium**
- **OWASP:** A01 Access Control; A08 Data Integrity; A09 Logging
- **Evidence:** attacker-influenced session IDs are lossy-sanitized, so different IDs can map to one
  JSONL filename ([`session_log.rs:94-140`](../crates/aether-daemon/src/session_log.rs)). Records are
  not MACed and reader does not verify expected session ID or sequence continuity.
- **Required remediation:** opaque UUID-to-filename map or hash; mode 0600; file lock/atomic append
  design; HMAC records; verify session/seq/turn invariants; migrate transcript into transactional DB.

### SEC-OWASP-20 — Security governance and incident response are absent

- **Severity:** **Medium**
- **OWASP:** A09 Logging/Alerting; OWASP modern AppSec program guidance; ASVS/SCVS governance
- **Evidence:** public repository has no `SECURITY.md`, vulnerability reporting SLA, security owner,
  threat model ownership, incident playbook, support window, dependency policy, or human-review rule.
- **Required remediation:** security policy/contact, CVSS/agentic severity SLAs, incident and token
  rotation playbooks, release security signoff, independent review, annual threat-model exercise.

---

## 5. OWASP Top 10:2025 crosswalk

| Category | Status | AetherForge-specific assessment |
|---|---|---|
| **A01 Broken Access Control** | **Red** | One admin bearer; no object ownership; arbitrary workspace-grant minting; global IDs. |
| **A02 Security Misconfiguration** | **Red** | Auth optional off macOS, configurable cleartext bind, allow-default sandbox, broad entitlements. |
| **A03 Software Supply Chain Failures** | **Red / Critical** | Skill pin gate not in production; MCP trust-on-discovery; no SCA/SBOM/provenance; mutable CI inputs. |
| **A04 Cryptographic Failures** | **Red** | Data at rest plaintext; custom daemon proof; token fallback; secret argv; BYOK endpoint not TLS-pinned. |
| **A05 Injection** | **Red** | SQL/OS injection posture is good, but prompt/memory injection remains a high-impact control-plane path. |
| **A06 Insecure Design** | **Red / Critical** | Model data/control conflation, partial approval taxonomy, mutable automation consent, non-atomic cross-resource operations. |
| **A07 Authentication Failures** | **Amber/Red** | CSPRNG and proof improved; still one bearer, no rate limit/rotation UI/peer credentials, non-mac optional auth. |
| **A08 Software or Data Integrity Failures** | **Red** | Mutable consolidation artifact, weak transcript integrity, skill/MCP policy gaps, unsigned default releases. |
| **A09 Logging and Alerting Failures** | **Red** | Audit gaps/fail-open sinks, no alerting/metrics, app discards daemon diagnostics. |
| **A10 Mishandling Exceptional Conditions** | **Red** | Unbounded server/MCP input, custom HTTP parsing, DB lock amplification, checkpoint/queue recovery gaps. |

No Top 10 category is fully green. This does not mean every sub-control is absent; it means each
category has at least one release-significant residual finding.

---

## 6. OWASP GenAI LLM Top 10:2026 crosswalk

| LLM risk | Status | Relevant controls and gaps |
|---|---|---|
| **LLM01 Prompt Injection** | **Red** | Text delimiters, deny phrases, schemas, and repair correlation help; initial memory-enriched planning remains model-enforced rather than policy-enforced. |
| **LLM02 Sensitive Information Disclosure** | **Red** | Secret env scrubbing and exact redaction help; prompts, tool outputs, prior file content, graph memory, logs, and custom endpoints remain disclosure surfaces. |
| **LLM03 Excessive Agency** | **Red** | Immutable approval helps; approval omits new writes/skills, automations bypass per-run approval, one token can mint broad grants. |
| **LLM04 Supply Chain** | **Red / Critical** | Production skill pin/admit integration missing; MCP policy can fail open; model/dependency/release provenance absent. |
| **LLM05 Data and Model Poisoning** | **Red** | Session namespace exists but structural tenant isolation/trust/quarantine and source-span validation are missing. |
| **LLM06 Unbounded Consumption** | **Red** | Iteration/token telemetry exists; caller can select unlimited and IPC/MCP/tool/network paths remain insufficiently bounded. |
| **LLM07 Misinformation** | **Amber/Red** | Evidence/provenance fields and review workflow exist; source truth and confidence enforcement are incomplete. |
| **LLM08 Hidden Context Exposure** | **Amber/Red** | No classic secret system prompt, but memory/tool/error context and policy details can leak; retention is broad. |
| **LLM09 Vector and Embedding Weaknesses** | **Red** | Global retrieval then prefix filter is not structural isolation; poisoning and starvation remain. |
| **LLM10 Improper Output Handling** | **Amber** | Strong plan schema/path/argument-array controls; per-MCP-tool schemas, artifact binding, output caps and contextual encoding remain incomplete. |

---

## 7. OWASP Agentic Applications Top 10:2026 crosswalk

| Agentic risk | Status | Assessment |
|---|---|---|
| **ASI01 Agent Goal Hijack** | **Red / Critical** | Initial memory/context can alter the planner’s goal; deterministic policy does not independently derive allowed effects from user intent. |
| **ASI02 Tool Misuse & Exploitation** | **Red** | Grants/sandbox/approval help; safe taxonomy is incomplete and MCP/skills need typed, pinned capabilities. |
| **ASI03 Identity & Privilege Abuse** | **Red** | One bearer inherits all user authority; no scoped delegated identity or object ownership. |
| **ASI04 Agentic Supply Chain** | **Red / Critical** | Production skill/MCP admission gaps and mutable models/toolchains. |
| **ASI05 Unexpected Code Execution** | **Amber/Red** | No shell interpolation and Python is compiled only; malicious MCP runtime/package remains process-execution boundary. |
| **ASI06 Memory & Context Poisoning** | **Red** | Automatic ingest, global vector store, textual trust boundary, mutable consolidation artifact. |
| **ASI07 Insecure Inter-Agent Communication** | **Amber / limited scope** | Subagent is currently a bounded local read helper, not an autonomous peer; future expansion needs identity/signing/provenance. |
| **ASI08 Cascading Failures** | **Red** | Global DB mutex, automation queue, tool hangs, non-atomic checkpoint and fail-open logs amplify failures. |
| **ASI09 Human-Agent Trust Exploitation** | **Red** | UI does not show exact full plan/diff/provenance; model facts can appear authoritative. |
| **ASI10 Rogue Agents** | **Amber** | Iteration bounds and fixed tool enum reduce autonomy; persistent poisoned memory/supply-chain compromise can still produce behavioral drift. |

---

## 8. OWASP API Security Top 10:2023 crosswalk

| API risk | Applicability | Assessment |
|---|---|---|
| **API1 BOLA** | High | Session/checkpoint/run IDs have no principal ownership. |
| **API2 Broken Authentication** | High | Improved macOS proof, but global bearer/non-mac optional/cleartext configurable transport remain. |
| **API3 Object Property Authorization** | Medium | Generic request params permit broad method-specific fields; no per-field capability model. |
| **API4 Resource Consumption** | High | Unbounded server lines/connections, MCP waits, user budgets. |
| **API5 Function Authorization** | High | One token reaches grant, undo, checkpoint, automation, consolidation, and task methods. |
| **API6 Sensitive Business Flows** | High | Automation runs, gateway triggers, grant creation, approval and consolidation need quotas/revision binding. |
| **API7 SSRF** | Medium/High | BYOK endpoint and fal returned URLs; trusted-config boundary is too broad. |
| **API8 Misconfiguration** | High | Cleartext configurable bind, optional auth, custom HTTP parser, broad entitlements. |
| **API9 Inventory Management** | Medium | No protocol version negotiation or formal endpoint/deprecation inventory; docs have drifted historically. |
| **API10 Unsafe API Consumption** | High | Provider bodies/URLs/tool descriptions and model outputs need stronger origin/schema/size controls. |

---

## 9. ASVS 5.0 control-domain assessment

This is not a requirement-by-requirement certification. It identifies whether the build is ready for a
formal ASVS L2 verification campaign.

| Control domain | Readiness | Key evidence/gap |
|---|---|---|
| Architecture/threat model | Partial | Rich roadmaps and this review; no maintained data-flow/threat-model artifact tied to changes. |
| Input validation/business logic | Partial | Strong plan/path validation; server frame limits, prompt provenance, budgets and generic tool schemas incomplete. |
| Authentication | Partial | CSPRNG/proof/token gate; no per-client identity, peer credential, mandatory all-platform auth, or rate limits. |
| Session/token management | Weak | Long-lived global token, no revocation UI/device sessions/rotation cadence. |
| Access control | Weak | Session/object ownership and function roles absent. |
| Cryptography | Weak | Keychain used; plaintext retained data, custom proof and secret argv remain. |
| Error handling/logging | Weak | Raw detail surfaces, incomplete audit, fail-open sinks, no alerting. |
| Data protection/privacy | Weak | Broad plaintext retention, no TTL/encryption/full deletion inventory. |
| Communications | Weak | Local TCP is cleartext and configurable; webhook TLS expected externally but not owned here. |
| Malicious code/supply chain | Weak | No enforced production skill pin, MCP fail-open, no SCA/SBOM/provenance. |
| File/resource handling | Partial | Canonicalization/symlink checks strong; TOCTOU, size limits, metadata and artifact integrity incomplete. |
| API/web services | Partial | Webhook auth/replay improved; custom parser, rate/size limits and endpoint inventory remain. |
| Configuration | Weak | Many security-critical env vars, silent defaults, no signed policy/config profile. |

**Conclusion:** not ready to claim ASVS L2 alignment. A requirement-level test plan should begin only
after SEC-OWASP-01 through SEC-OWASP-13 are remediated.

---

## 10. SecOps build and deployment review

### CI evidence

- PR #51 final branch passed Linux build/unit/golden and Darwin-fast Rust/unit/Swift/DIST-01.
- Full Darwin 51/51 remains unexecuted on the PR path.
- The 42→41 hard-count workflow correction exists in the Arena workspace but could not be pushed
  because the GitHub App lacks workflow scope.
- Passing tests establish compilation/regression confidence, not OWASP control compliance.

### Missing security gates

1. SAST/CodeQL or equivalent.
2. Rust Clippy as blocking security/quality gate.
3. `cargo-audit`/OSV and `cargo-deny` policy.
4. Secret scanning with history and push protection.
5. Dependency update automation and security SLA.
6. SBOM plus VEX.
7. License compliance.
8. Fuzzing for IPC, HTTP, JSONL, planner schemas and path handling.
9. Miri/sanitizers for unsafe/FFI/vector code.
10. DAST/negative tests against live daemon/webhook endpoints.
11. Reproducible/pinned toolchains and models.
12. Signed provenance and release attestation.
13. Installed signed-app E2E and fake-daemon/relay test.
14. Human security review requirement.

### Release pipeline

The release pipeline is not production-grade from an OWASP/SCVS perspective:

- unsigned DMG is the default;
- no Release workflow evidence exists;
- no SBOM/provenance or notarized-only publication policy;
- app resources omit skills, model registry and MCP policy;
- Sparkle key/feed may be unconfigured;
- broad entitlements weaken hardened runtime;
- version input is not strongly validated/escaped for plist semantics;
- package resolution and toolchain are mutable.

---

## 11. Security test plan

### P0 adversarial tests

1. **Skill rug pull:** install benign skill, mutate one byte/template/manifest, verify production
   `ToolRegistry` denies execution by persisted digest.
2. **Missing/malformed MCP policy:** delete/corrupt allowlist and modify Node path; verify zero spawn.
3. **Memory injection:** seed multilingual/encoded/paraphrased indirect instruction in retained
   memory and verify initial plan cannot add any unauthorized effect.
4. **Approval transparency:** verify UI bytes/diff/digest equal the server-stored plan and consumed plan.
5. **Daemon relay:** malicious local proxy relays proof to legitimate alternate-port daemon; client
   must detect wrong peer/process/socket identity and send no token.
6. **BOLA:** attempt checkpoint/undo/consolidation/session operations across two client identities.
7. **Oversize/slow IPC:** >1 MiB line, no newline, slowloris, 10k connections, oversized JSON nesting.
8. **MCP hang/flood:** no response, infinite line, wrong ID stream, huge tools list, child fork/spawn.
9. **Artifact tamper:** modify JSON after consolidation review; apply must deny by digest/ownership.
10. **Crash matrix:** fault every transition in write, checkpoint, automation lease, session log and apply.

### LLM/agent red-team suite

- direct/indirect prompt injection in every language/encoding;
- payload splitting across memory turns;
- malicious evidence/citations and source confusion;
- tool description poisoning and MCP schema drift;
- skill body/frontmatter/template mutation;
- memory namespace collisions and retrieval starvation;
- hidden instruction in source files/tool results;
- approval fatigue and benign-looking dangerous plans;
- token/cost/iteration amplification;
- planner repair induction and cross-call paraphrase;
- misinformation that attempts to alter security policy or permissions.

### API/Webhook suite

- stale/future Slack timestamps, malformed signatures and timing analysis;
- Telegram wrong/missing secret and unauthorized sender/chat;
- duplicate/reordered provider events across restart;
- GitHub delivery replay and signature over raw whitespace/body bytes;
- malformed headers, conflicting content length, invalid UTF-8, chunked encoding, slow body;
- rate and concurrency saturation;
- outbound redirect/DNS-rebinding/private-address tests.

---

## 12. Remediation program

### Phase 0 — Release containment (immediate)

1. Keep product status pre-release/no-go.
2. Enforce loopback-only daemon and mandatory auth on every platform.
3. Keep gateways, automation, external MCP and non-curated skills disabled by default.
4. Do not process sensitive/proprietary workspaces until at-rest protection is implemented.
5. Remove unsigned public release path.
6. Publish `SECURITY.md` and assign a security owner.

### Phase 1 — Agentic policy boundary (week 1)

1. Integrate persisted skill pin/admit into production.
2. Make MCP curated policy mandatory and bundle it.
3. Apply deterministic provenance-aware authorization to initial plans.
4. Expand approval to every mutation/skill/secret/external action.
5. Display and sign exact plan/diff/budget/provenance envelope.
6. Bind automation/gateway grants to immutable config revisions.

### Phase 2 — Identity, API and resource controls (week 2)

1. Unix socket + peer UID; per-client scoped identities.
2. Object/function authorization matrix.
3. Server frame, connection, nesting, rate, budget and timeout limits.
4. Replace custom HTTP servers with maintained framework.
5. Tool process supervisor and DB concurrency isolation.
6. Standard HMAC/asymmetric proof and token revocation/rotation.

### Phase 3 — Data and integrity (week 3)

1. Explicit tenant/session vector metadata and index-time filters.
2. Memory trust/quarantine/TTL/export/delete.
3. At-rest encryption and 0600/0700 creation policy.
4. Integrity-bind consolidation and JSONL.
5. Durable checkpoint/automation state machines.
6. Complete mandatory audit schema and security alerts.

### Phase 4 — SCVS/ASVS release gates (week 4)

1. Pinned toolchains/actions/packages/models.
2. SAST, SCA, secrets, fuzz, Miri/sanitizer, license checks.
3. CycloneDX SBOM, VEX and provenance.
4. Signed/notarized-only installed-app E2E.
5. ASVS 5.0 L2 requirement matrix with evidence IDs.
6. Independent human SecOps approval and external penetration test.

---

## 13. Production acceptance criteria

A SecOps signoff requires all of the following:

- Production cannot execute an unpinned skill or MCP server under any missing/corrupt-config state.
- Untrusted user/memory/tool content cannot independently authorize a new effect.
- Every effect is authorized by deterministic policy and, when impactful, an exact human-reviewed plan.
- Daemon is loopback/Unix-socket only, mutually authenticated, mandatory-auth, rate-limited and peer-scoped.
- Every object/method has a documented authorization rule and cross-principal negative test.
- No sensitive value is passed through argv, unprotected logs, provider error bodies, or unapproved endpoint.
- Persistent sensitive data has encryption, strict modes, inventory, retention and deletion.
- IPC/MCP/webhooks/models/tools have hard size/time/concurrency/cost bounds.
- Memory retrieval is structurally tenant/session-filtered and poisoning-tested.
- Security events are complete, tamper-evident, redacted, retained and alertable.
- Build is reproducible and has passing SAST/SCA/secrets/fuzz/ASVS evidence, SBOM and provenance.
- Release is signed, notarized, resource-complete and tested from the installed artifact.
- Full Darwin canonical and all security E2E tests pass on the exact release commit.

---

## 14. Final SecOps disposition

The current build is materially safer than the baseline and demonstrates good engineering direction.
The immutable approval record, crash reconciliation, webhook authentication, parameterized commands,
workspace path controls, schema-constrained plans, and dedicated benchmark path are meaningful—not
security theater.

The residual OWASP risk is nevertheless unacceptable for production because the agentic supply-chain
and initial-context authorization boundaries are not structurally enforced. Conventional AppSec gaps
then compound that risk: one global principal, cleartext/configurable IPC, plaintext retained data,
unbounded server/tool paths, mutable artifacts, incomplete audit, and an unverified software supply
chain.

**SecOps decision: retain NO-GO.** Permit only constrained local development use under the conditions
at the top of this report. The shortest path to a changed decision is to remediate SEC-OWASP-01 and
SEC-OWASP-02 first, then identity/resource/data-integrity controls, then complete ASVS L2 and SCVS
release evidence.
