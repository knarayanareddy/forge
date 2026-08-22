# Critical Remediation Program

- **Baseline review:** [`INDUSTRY_REVIEW_2026-08-20.md`](./INDUSTRY_REVIEW_2026-08-20.md)
- **Target branch:** `arena/01a0214e-forge`
- **Program status:** critical implementation complete; Linux and Darwin-fast CI green on PR #51
- **Validated revision:** `d22f77a` — Linux 2m58s, Darwin-fast/Swift 3m25s ([run 32427747747](https://github.com/knarayanareddy/forge/actions/runs/32427747747))
- **Release posture:** no-go until the workflow-scope blocker is cleared and the full Darwin canonical gate passes

## Coordinated expert workstreams

| Lead persona | Critical finding | Implementation | Acceptance evidence |
|---|---|---|---|
| Protocol / Swift lead | CRIT-01: hanging IPC | Unary and approval events are terminal in Swift; socket send loops until complete; read/write timeouts and 1 MiB bounds added; EOF is handled explicitly. | Swift build plus real daemon-client integration tests for every event type. |
| Authorization lead | CRIT-02: approval not bound to plan | Added DB-backed, expiring, single-use `approval_id`; server stores serialized normalized plan/session/workspace/prompt and consumes it once; `approved:true` is rejected; approval follow-up never replans. | Unit test for session/workspace binding and replay; TCP test rejects boolean bypass; macOS approval E2E. |
| Data-integrity lead | CRIT-03: false crash recovery | Write journal now records intended content; startup reconciles target as not-started/applied/ambiguous; applied crash window preserves inverse; ambiguous state fails startup. | Unit fault-state tests plus RES-01 regression. |
| Tool-safety lead | CRIT-04: destructive implicit Git | `git_init` always requires approval, refuses an existing repo, validates branch first, creates no README, and uses an empty initial commit. | PERM-02 Git case and GIT-01 preservation of a user-owned README. |
| Gateway-security lead | CRIT-05: unauthenticated/replayable gateway | Telegram secret is mandatory; Slack HMAC/timestamp verification added; Discord production route fails closed until Ed25519 support exists; sender allowlist and provider event replay table added; Telegram long poll also enforces sender/replay; PR webhooks use GitHub HMAC plus delivery replay IDs. | HMAC RFC vector, automation valid/invalid/replay tests, gateway sender/replay test, GATE regressions. |
| Platform-security lead | CRIT-06: weak/one-sided daemon auth | Replaced timestamp/PID token with versioned 256-bit OS-CSPRNG token and legacy rotation; atomic mode-0600 fallback; nonce-bound server proof verified with CryptoKit before client trust. | Token/proof unit tests and TCP proof test; fake-daemon macOS integration test still required. |
| CI/evaluation lead | CRIT-07: impossible canonical gate | Darwin hard expectation corrected from 42 to registry truth 41; Darwin harness exits nonzero when any selected canonical task fails. | Exact-commit canonical 51/51 (41 hard / 10 soft), repeated twice. |
| Product-routing lead | CRIT-08: UI does not reach agent | Added explicit Agent/Chat UI mode; Agent routes ordinary text directly to NL planning via protocol `execution_mode`; Chat remains completion-only; production output/context defaults raised to 1024/8192; TTFT uses a dedicated 16/512 benchmark path; complex profile routing is now used for classified requests. | SwiftUI E2E: natural-language file task → immutable approval → verified write; Chat response exceeds 16 tokens; ROUT regression. |

## Defense-in-depth fixes included

- Replaced potentially misaligned SQLite blob-to-`&[f32]` cast with explicit float decoding.
- Replaced hard-coded graph-recency date with current UTC epoch-day calculation.
- Made injection-finding truncation Unicode-safe to avoid panic/database-mutex poisoning.
- Model stream errors are terminal and no longer followed by `done` or successful partial-memory ingest.
- Every NL file write is now instructed to include `verify_contains`; unrelated Python lint is no longer a universal write-completion requirement.

## Migration strategy

All new state is additive and created through `Database::init_schema`:

- `pending_approvals`
- `gateway_events`
- `automation_webhook_events`

Existing legacy daemon auth tokens are rotated when the new daemon first starts. Existing gateway/webhook routes fail closed until mandatory secrets and sender IDs are configured. Existing Discord webhook configuration is intentionally disabled rather than accepted without Ed25519 verification.

## Required environment for gateway operation

- Telegram webhook secret: `AETHER_TELEGRAM_WEBHOOK_SECRET` or `AETHER_GATEWAY_WEBHOOK_SECRET_<CHANNEL>`
- Slack signing secret: `AETHER_SLACK_SIGNING_SECRET` or `AETHER_SLACK_SIGNING_SECRET_<CHANNEL>`
- Authorized provider sender: `AETHER_GATEWAY_ALLOWED_SENDER_<CHANNEL>`
- Discord: disabled pending an Ed25519 implementation and public-key configuration

Channel suffixes are uppercase alphanumeric with every other character replaced by `_`.

## Validation evidence and remaining gate

PR #51 independently compiled and tested the exact committed branch on both supported CI tiers:

- Linux build, unit tests, and Linux golden matrix: **PASS**
- Darwin Rust build/unit tests, Swift build, and DIST-01 smoke: **PASS**
- Static shell/Python/JSON/TOML, MCP allowlist, docs scoreboard, and whitespace checks: **PASS**
- Full Darwin 51/51: **PENDING**. The one-line 42→41 workflow correction exists in the Arena
  workspace but GitHub rejected that workflow-file update because the connected App lacks
  `workflows` permission. This is an authentication/scope blocker, not a code-test failure.

## Mandatory validation sequence

1. `cargo fmt --all -- --check`
2. `cargo test --workspace --locked`
3. `cargo clippy --workspace --all-targets -- -D warnings`
4. `bash scripts/scan-mcp-allowlist.sh`
5. `bash scripts/check-doc-scoreboard.sh`
6. Linux golden matrix with named expected outcomes
7. Darwin unit tests and Swift build
8. Swift↔daemon integration suite covering all unary terminals, timeout, partial write, approval, and fake daemon
9. Darwin full harness 51/51 with 41 hard / 10 soft
10. Repeat canonical run to detect model/evaluation flake

## Exit criteria

The critical program is complete only when:

- no client call can block beyond its configured timeout;
- `approved:true` without a server record is rejected;
- an approval ID cannot be replayed or moved across session/workspace;
- crash-after-write remains undoable and ambiguous recovery refuses startup;
- `git_init` leaves all pre-existing workspace files unchanged;
- unsigned, stale, unauthorized, or duplicate gateway events execute zero tools;
- a fake process on the daemon port cannot pass the client identity challenge;
- ordinary Agent-mode UI text reaches planning, while Chat mode remains explicitly non-tooling;
- exact-commit Linux, Darwin fast, Swift, and canonical harness checks are green.
