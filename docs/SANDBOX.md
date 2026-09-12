# Production Tool Sandbox

Phase 8.0c routes the current production tool surface through
`aether_sandbox::ProductionSandbox`.

## Boundary

The daemon remains trusted so it can own IPC, SQLite, grants, and model routing. On macOS, every
agent-controlled file operation or subprocess is delegated to a child wrapped by
`/usr/bin/sandbox-exec`:

| Surface | Sandboxed operation |
|---------|---------------------|
| `fs_write` | `/usr/bin/tee` with content over stdin |
| `fs_read`, `verify_contains`, `python_lint_file` | `/bin/cat` |
| `python_lint`, `python_lint_file` | `python3 -m py_compile` with source under workspace `.aether-tmp`. `python_lint` compiles source quoted in the plan; `python_lint_file` (CHECK-02) first `/bin/cat`s the artifact the plan wrote, then compiles that — same boundary, different source of bytes |
| `git_init` | README write plus every `git` child |
| `mcp_call` | verified/pinned MCP server process, retaining stdio JSON-RPC |
| skill read/append | `/bin/cat` / `/usr/bin/tee -a` |
| gateway response artifact | `/usr/bin/tee` via `journal_file_write`, so the reply is snapshot + undo-journaled like any agent write (GATE-03) |

No operation uses `sh -c`; user content is never interpolated into a shell command.

## Invariants

1. Capability grants are checked before sandbox dispatch.
2. Target paths reject `..`, absolute workspace escapes, and escaping symlinks.
3. Darwin fails closed when `sandbox-exec` or the profile is missing.
4. Child environments are cleared. Only a minimal executable path, workspace-scoped
   `HOME`/`TMPDIR`, UTC timezone, and fixed local git identity are present.
5. `profiles/sandbox_tool.sb` denies all network and writes outside `WORKSPACE_PATH`.
6. The profile is bundled at `Contents/Resources/profiles/sandbox_tool.sb`; development resolves
   the repository profile, and `AETHER_SANDBOX_PROFILE` is an explicit override.

## Platform behavior

- **Darwin:** Seatbelt is mandatory for production tool children.
- **Linux CI:** the same path checks and environment scrubbing apply, but commands run natively.
  Linux isolation (bubblewrap/Landlock) remains a separate, explicit future slice.

The profile remains allow-default with explicit network/write/read denials because the canonical
macOS toolchain needs system frameworks and runtimes. This is not a deny-default container and
must not be described as one.

## Verification

`SB-01` is the Darwin hard gate:

- executes file read/write/verify, Python lint, and git through `ToolRegistry`;
- confirms parent secrets are absent from a child environment;
- confirms outbound HTTPS fails;
- confirms `/etc/passwd` cannot be read through the production file API.

Existing MCP-01, SKILL-01, LOOP-01, AUTO-01, CHECK-01, and GATE-01 are regression checks over the
same production dispatch paths.

