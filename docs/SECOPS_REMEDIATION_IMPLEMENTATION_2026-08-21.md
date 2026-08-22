# SecOps Remediation Implementation Evidence

This document records the implementation response to all 20 findings in `OWASP_SECOPS_REVIEW_2026-08-21.md`. Status is **implemented, pending independent CI validation**.

1. **Skill/MCP admission:** persisted `installed_skills`, signed-source digest registry, production pre-execution admission, mandatory curated MCP policy, pinned overrides, 30s/1MiB MCP response boundary.
2. **Context/approval:** initial recalled-memory induction check; every file write and skill execution is risky; exact plan approval required.
3. **Principal ownership:** session owner registry; principal-bound task, grant, checkpoint, undo, consolidation and automation paths.
4. **IPC authentication:** loopback enforcement, mandatory non-macOS token, HMAC-SHA256 challenge proof.
5. **Resource controls:** bounded/timed IPC, connection semaphore, caller budget caps, MCP timeout, webhook timeout/body/UTF-8 limits.
6. **Data lifecycle:** 0700/0600 storage, 30-day configurable purge, session logs and persistent memory opt-in, redacted/limited retained outputs.
7. **BYOK/secrets:** HTTPS endpoint validation and explicit custom endpoint enrollment; Keyring/Security.framework write path; token fallback disabled by default.
8. **Consolidation integrity:** canonical JSON/Markdown and SHA-256 persisted in DB; apply uses pinned bytes and validates run/session/node ownership; UI displays pinned Markdown/hash.
9. **Delegated grants:** automation/gateway grant config hashes bind consent to the exact current registered revision.
10. **Audit:** approval/auth/fs decisions audited; session-log sink fails task status; daemon logs retained locally rather than discarded.
11. **Memory isolation:** explicit semantic-memory session column; SQL-scoped lexical/vector retrieval; scoped ingest.
12. **SCVS build:** toolchain/package pinning, format/Clippy/RustSec/cargo-deny gates, Dependabot, action SHA pins, locked builds, CycloneDX release SBOM.
13. **Least privilege:** dangerous JIT/unsigned-memory/library-validation entitlements removed; Linux tools fail closed unless isolated CI explicitly opts in; Darwin cross-user reads denied.
14. **Third-party URLs:** fal status/result/media origins constrained to HTTPS fal domains, redirects disabled; BYOK origin enrollment enforced.
15. **Exceptional conditions:** automation leases/recovery, checkpoint operation state ledger, atomic transcript/fork writes, mutex poison recovery, schema migrations.
16. **Fact integrity:** extracted graph evidence must be an exact source span; inferred facts remain distinguishable.
17. **Output handling:** recursive MCP path argument validation, verified tool inventory membership, 1MiB protocol bound, provider error body caps.
18. **Informed approval:** plan digest binds session/workspace/plan/budgets; IPC and Swift UI show exact canonical plan, digest and expiry; lifecycle audited.
19. **Session log integrity:** collision-resistant hashed filenames, strict session/sequence checks, HMAC chain, 0600 files and atomic rewrite.
20. **Governance:** Apache-2.0 license, SECURITY policy/SLAs, CONTRIBUTING secure-review rules, CODEOWNERS, threat model and remediation ledger.

## Remaining validation gates

- Linux build/unit/golden under the new default-deny Linux policy.
- Darwin Rust/unit/Swift/DIST-01.
- Full Darwin 51/51 canonical.
- Installed signed-app fake-daemon, approval, skill/MCP rug-pull, memory-poisoning and artifact-tamper E2E.
- Workflow changes require GitHub App `workflows` permission before they can be pushed.
