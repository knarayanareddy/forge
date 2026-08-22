# SecOps Remediation Ledger — OWASP Findings 1–20

**Source:** `docs/OWASP_SECOPS_REVIEW_2026-08-21.md`  
**Method:** one finding at a time; fail-closed implementation + migration + regression evidence  
**Release posture:** NO-GO until all rows are implemented and independently validated

| # | Finding | Status | Primary evidence |
|---:|---|---|---|
| 01 | Production skill/MCP admission fails open | Implemented | Persisted skill pins; mandatory curated MCP policy; bounded MCP protocol |
| 02 | Memory/context steers initial plan outside approval | Implemented | Provenance-aware initial-plan policy; all mutations/skills/secrets approved |
| 03 | One bearer principal / no object ownership | Implemented | Principal-bound session/object registry and method authorization |
| 04 | IPC cleartext/configurable/relayable | Implemented | Loopback enforcement, mandatory auth, standard HMAC proof |
| 05 | Resource consumption insufficiently bounded | Implemented | IPC/HTTP/MCP/tool/model limits and timeouts |
| 06 | Sensitive retained data lifecycle | Implemented | 0700/0600 storage, redaction, retention/purge controls |
| 07 | BYOK and secret redirect/exposure | Implemented | HTTPS endpoint enrollment; no secret argv/fallback by default |
| 08 | Consolidation artifact not integrity-bound | Implemented | Canonical DB-pinned review bytes/hash and ownership checks |
| 09 | Automation/gateway grants mutable | Implemented | Grant bound to immutable config digest/revision |
| 10 | Audit/alerting incomplete/fail-open | Implemented | Mandatory security-event schema and complete decision audit |
| 11 | Memory isolation/poisoning not structural | Implemented | Explicit semantic-memory session column and scoped retrieval |
| 12 | CI/release below SCVS baseline | Implemented | Locked builds, SAST/SCA/secrets/SBOM/provenance workflows |
| 13 | Sandbox/entitlements exceed least privilege | Implemented | Hardened entitlements; Linux fail-closed; tighter Darwin reads |
| 14 | Third-party API URLs trusted broadly | Implemented | Scheme/origin/address/redirect validation |
| 15 | Exceptional-condition consistency | Implemented | Leases, atomic transcript/checkpoint state, fail-closed config |
| 16 | Misinformation/fact integrity | Implemented | Exact evidence-span validation and confidence policy |
| 17 | Output validation incomplete | Implemented | Typed MCP args and bounded/redacted outputs |
| 18 | Approval not fully informed/observable | Implemented | Full canonical plan/diff/digest/budget in IPC/UI/audit |
| 19 | Session-log collision/integrity | Implemented | SHA-256 filenames, strict records, MAC chain, atomic writes |
| 20 | Security governance absent | Implemented | SECURITY, CONTRIBUTING, CODEOWNERS, threat model, response SLAs |

## Validation gate per row

A row reaches **Implemented** only when code/config and regression tests exist. It reaches **Validated** only after Linux build/unit/harness and Darwin-fast/Swift CI pass on the exact revision. Full Darwin canonical and installed-artifact security E2E remain final program gates.
