# forge — phased remediation plan

Planned work only; nothing here was executed. Findings refer to [review](REVIEW.md). Preserve existing security thresholds unless an owner approves a documented, evidence-based change. Paths labeled proposed do not yet exist; all commands below are future checks, not claims of availability or success.

## Dependency map and phase gates

| Phase | Task | Prerequisites | Exit evidence |
|---|---|---|---|
| 0: baseline | FG-T1 | Owner authorizes log and isolated-runner access | Reproduction record and gate contract |
| 1: defects | FG-T2, FG-T3 | T1; T3 also needs local-client trust decision | Negative gate and IPC tests |
| 2: boundaries | FG-T4, FG-T5 | T1 and T3; T5 also needs T4 threat model | MCP and supply-chain evidence |
| 3: experience and operations | FG-T6 | T2–T5 and owner-approved release resources | Recovery, UI and release evidence |
| 4: acceptance | FG-T7 | T1–T6 | Independent evidence review |

Parallel tasks within a phase must not be treated as completed merely because another task passed. If platform support is undecided, security and release acceptance stay blocked.

## Phase 0 — reproduce and define the contract

**FG-T1 — baseline and historical failure triage** (FG-1, FG-3). Observed paths: `.github/workflows/ci.yml`, `README.md`, `tests/golden_harness/src/`; prerequisite: read-only access to the three recorded Actions runs and approved isolated Linux/macOS runners.
1. Retrieve each run's job, step, logs and SHA; record missing or expired evidence separately, and classify observed failures without guessing.
2. Trace harness result emission and hard/soft case definitions to source; compare them with workflow and README claims, including the Darwin PR/full split and model timing setting.
3. With owner approval, reproduce on isolated runners; record exact revision, toolchains, model identities, OS and full outputs. If reproduction is blocked, record the blocker instead of substituting a different SHA.

**Output:** baseline and approved case-level gate contract. **Verify:** compare each conclusion with its full log and source; distinguish pinned historical CI from any new run. **DoD:** [ ] causes evidenced or explicitly unknown; [ ] expected cases derived; [ ] environment and new run SHA recorded.

## Phase 1 — correct observed defects

**FG-T2 — faithful CI gates** (FG-1, FG-2). Prerequisite: T1 contract. Observed `.github/workflows/ci.yml`; proposed workflow-gate fixtures.
1. Align assertions and messages with the approved case classifications, without dropping required cases or lowering thresholds.
2. Capture harness and logging process statuses independently; reject missing, malformed, duplicate or incomplete summaries and required-case failures.
3. Add fixtures for valid output, low/missing counts and an apparently passing summary followed by a nonzero harness exit; cover Linux and Darwin paths separately.

**Output:** workflow change and regression fixtures. **Verify:** deliberately restore status loss or an incorrect hard count; relevant tests must fail, then pass when restored. **DoD:** [ ] process failure propagated; [ ] required cases checked; [ ] mutation caught.

**FG-T3 — enforce IPC identity and grant authority** (FG-5, FG-6). Prerequisites: T1 and owner decision on supported platforms and whether a token-bearing local client may grant workspace access. Observed `crates/aether-daemon/src/{main,server}.rs` and `tests/ipc_auth.rs`.
1. Map startup token creation, address binding, token delivery and all privileged IPC methods for each supported platform; document any explicitly isolated development mode.
2. Require a valid configured identity in supported production modes; design an owner-approved authorization/consent boundary for workspace grants and replace misleading audit provenance with evidence-backed identity.
3. Add wire-level tests for absent, empty, wrong and valid token, unauthenticated ping, grant denial and approved grant; assert no database grant is inserted on denial.

**Output:** platform policy, startup and authorization changes, and tests. **Verify:** in isolated environments, deliberately bypass the auth or grant check and confirm denial tests fail; inspect actual audit records without recording tokens. **DoD:** [ ] unsupported or insecure startup rejected as specified; [ ] unauthorized grant has no side effect; [ ] provenance distinguishes authenticated client from verified UI consent.

## Phase 2 — secure the MCP and data boundaries

**FG-T4 — exercise the live execution boundary** (preventive security requirement; FG-5/6). Prerequisites: T1, T3. Observed `crates/aether-mcp/src/runtime.rs`, `crates/aether-daemon/src/server.rs`; proposed threat-model and integration-test paths.
1. Trace each live daemon/tool entry through model output, argument validation, permissions, MCP allowlist, sandbox and result handling; identify whether direct spawn helpers are reachable from production callers.
2. Define expected behavior for poisoned tool results, unauthorized filesystem/network access, missing sandbox, webhook/gateway authorization, secret echo and credential-to-destination binding; include data retention and backup sensitivity.
3. Build end-to-end negative tests against the actual supported production path, using synthetic secrets and isolated services; check logs, errors and stored records for leakage.

**Output:** call-graph threat model and executable security cases. **Verify:** disable each relevant guard in a controlled test build and require its corresponding test to fail; inspect actual subprocess and storage behavior. **DoD:** [ ] bypass paths resolved or accepted by owner; [ ] denial and redaction observed; [ ] negative controls detected.

**FG-T5 — stable MCP pins and dependency integrity** (FG-7; preventive supply-chain requirement). Prerequisites: T1, T4. Observed `crates/aether-mcp/src/runtime.rs`, `.github/workflows/ci.yml`, `Cargo.lock`; proposed pin migration notes.
1. Establish whether tools hash semantics intentionally include server order; inventory existing stored pins and pin-generation consumers before choosing a canonical representation.
2. Test reordered equivalent tool lists, changed descriptions, duplicate names and pending pins; if canonicalizing, provide an explicit compatible migration and preserve detection of changed tools.
3. Replace the floating global MCP package with an owner-reviewed version and integrity/reproducibility strategy; inventory action revisions, Rust lockfile use and model identities.

**Output:** documented pin contract, migration and dependency inventory. **Verify:** reordered-list behavior matches the approved contract, changed-tool and tampered dependency tests fail closed; reproduce the resolved versions on a clean runner. **DoD:** [ ] pin compatibility assessed; [ ] meaningful changes rejected; [ ] dependency identities recorded.

## Phase 3 — release, recovery and desktop UX

**FG-T6 — prove release and recovery** (FG-4; preventive UX/operations). Prerequisites: T2–T5, owner-approved signing accounts, macOS machine, storage and distribution policy. Observed `.github/workflows/release.yml`, `macos/AetherForgeApp/`, `scripts/`; proposed release and recovery runbooks.
1. Separate unsigned internal artifacts from public releases; define approval, provenance, Developer ID, notarization and update-signing requirements, and verify current scripts before relying on them.
2. On an authorized Mac, build and independently verify the exact signed/notarized DMG, install it, observe the running build identity, then rehearse a rollback and database backup/restore including format compatibility.
3. Exercise keyboard and VoiceOver navigation, workspace consent, denied permissions and failure recovery; record defects and retest fixes with a reviewer.

**Output:** release evidence, tested recovery and accessibility report. **Verify:** the public gate rejects unsigned or tampered artifacts; compare restored data with known test data and document measured recovery. **DoD:** [ ] artifact identity and notarization observed; [ ] rollback and restore demonstrated; [ ] accessibility issues resolved or explicitly accepted.

## Phase 4 — independent acceptance

**FG-T7 — evidence-based acceptance** (preventive requirement). Prerequisites: T1–T6. Proposed acceptance record; observed workflows and test paths above.
1. Have a reviewer independent of implementation inspect the task evidence, exact commit and release artifact hashes, and unresolved owner decisions.
2. Re-run approved negative controls and inspect logs, UI and restored data; ensure failures are real assertions rather than mirrored implementation assumptions.
3. Record acceptance or block release with assigned follow-ups; retain evidence and any approved exceptions without presenting test count as coverage proof.

**Output:** independent acceptance record. **Verify:** trace each checklist item to observed evidence for the same release revision. **DoD:** [ ] controls independently observed; [ ] blockers owned; [ ] approval or rejection recorded.

## Production acceptance, approvals and rollback

- [ ] Pinned historical failures investigated; new CI evidence identifies its own SHA.
- [ ] IPC authorization, sandbox, injection and secret negative controls observed on supported platforms.
- [ ] Gate and dependency integrity established; owner-approved exceptions documented.
- [ ] Signed/notarized artifact, desktop accessibility and restore drill independently verified.

Owner approval is required for platform/security policy, threshold changes, model downloads, accounts, signing, distribution and retention. Publish only an immutable reviewed artifact; on failed verification stop distribution, preserve evidence, revert to the last independently verified signed build and restore a tested compatible backup if needed. If no such build or backup exists, halt rather than assert rollback succeeded.
