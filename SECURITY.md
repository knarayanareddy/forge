# Security Policy

## Supported versions

AetherForge is pre-release. Only the latest commit on `main` receives security fixes. No production deployment is currently supported.

## Reporting a vulnerability

Do not open public issues for suspected vulnerabilities. Use GitHub's private vulnerability reporting for this repository. Include affected commit, threat actor, prerequisites, reproduction, impact, and any proposed mitigation.

## Response targets

| Severity | Acknowledge | Triage | Target remediation |
|---|---:|---:|---:|
| Critical — secret theft, arbitrary tool execution, sandbox/approval bypass | 1 business day | 2 business days | 7 days |
| High — cross-session access, persistent poisoning, supply-chain integrity | 2 business days | 5 business days | 30 days |
| Medium | 5 business days | 10 business days | 90 days |

Targets are goals, not warranties. Maintainers may disable affected features immediately.

## Security invariants

- Daemon IPC is loopback-only and authenticated on every platform.
- Model output never grants authority; deterministic policy and exact approval do.
- Missing/corrupt skill or MCP policy fails closed.
- Tool access is workspace-confined and secrets are scoped to one approved call.
- Security/audit sink failure is visible and cannot be reported as success.
- Releases must be dependency-audited, signed, notarized, accompanied by SBOM/provenance, and tested from the installed artifact.

## Incident response

1. Disable affected gateway, automation, MCP or skill surface.
2. Preserve redacted audit/session evidence and artifact hashes.
3. Rotate daemon, BYOK, gateway and brokered credentials as applicable.
4. Revoke compromised grants/pins and quarantine memory/artifacts.
5. Patch with regression tests and independent review.
6. Publish an advisory with affected versions and recovery actions.
