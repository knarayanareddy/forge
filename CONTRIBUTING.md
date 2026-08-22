# Contributing

## Required workflow

1. Open an issue or design note for security-boundary changes.
2. Add or update the threat model and tests before implementation.
3. Run formatting, Clippy, unit tests, golden matrix, dependency policy and secret scans.
4. Keep model output outside authorization decisions; document deterministic enforcement.
5. Never weaken a fail-closed path to make a harness green.
6. Obtain at least one independent human approval. Authors may not self-approve security/release changes.

## Security-sensitive areas

Changes to IPC/auth, permissions, approvals, sandbox, MCP, skills, memory, gateways, automation, release workflows, signing, Keychain or cryptography require SecOps review and negative tests.

## Pull request evidence

Include threat actor, assets, trust boundaries, misuse cases, migrations, rollback, test commands, exact CI links and residual risk. Generated media and secrets must not enter Git; use approved artifact storage.

## Style

- Rust: `cargo fmt --all`, Clippy with warnings denied.
- Avoid `unwrap`, silent defaults and discarded security errors in production paths.
- Bound every untrusted input, output, loop, process and network call.
- Use typed schemas and parameterized process/SQL APIs.
