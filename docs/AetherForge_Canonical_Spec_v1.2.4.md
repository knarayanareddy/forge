# AetherForge: Canonical Production Specification (v1.2.4 - Spec/Code Parity Patch)

**Date**: 2026-07-24  
**Status**: Parity patch over v1.2.3 (Darwin-verified implementation truth)  
**Target Platform**: Apple Silicon Mac (macOS 15+ Sequoia / macOS 16)  

---

## 1. Executive Summary & v1.2.4 Patch Notes

v1.2.4 resolves **spec ↔ code drift** identified in independent audits. It does **not** expand MVP scope — it documents what the harness actually ships and verifies on Darwin.

1. **Seatbelt profile canonical = allow-default deny-list** — The working Darwin profile uses `(allow default)` with targeted denies (network, `/etc` reads, writes outside `WORKSPACE_PATH`). Pure `(deny default)` profiles abort on macOS 26.x (`SIGABRT` exit 134) without extensive system allows. v1.2.3 §5 deny-default profile is retained as a **future hardening target**, not current canonical.
2. **Apple Silicon MCP paths** — Node MCP servers use `/opt/homebrew/bin/node` and `/opt/homebrew/lib/node_modules/...` (not Intel `/usr/local`).
3. **Platform scoreboard** — Darwin is canonical for FS-02 (Seatbelt) and MEM-01 (Ollama). Linux CI must fail closed when `sandbox-exec` or Ollama are absent.
4. **SKILL-01 procedural skills** — `skills/*/SKILL.md` loader (agentskills.io frontmatter + `## Steps`) registered in `procedural_skills` and executed with grant checks.

---

## 2. Golden Harness Scoreboard (v1.2.4)

| Task | Darwin (canonical) | Linux CI |
|------|-------------------|----------|
| FS-01 | PASS | PASS |
| FS-02 | PASS | FAIL (`sandbox-exec required`) |
| GIT-01 | PASS | RED |
| CODE-01 | PASS | RED |
| MCP-01 | PASS | PASS |
| MEM-01 | PASS (Ollama + `all-minilm`) | FAIL (Ollama offline) |
| SKILL-01 | PASS | PASS |
| SAFE-01 | PASS | PASS |
| ROUT-01 | PASS (Ollama chat model) | RED |
| RES-01 | PASS | PASS |

**Darwin baseline (v1.2.4 phase patch): 10 / 10** — all golden tasks green on Darwin when Ollama + chat model are available.

### v1.2.4 phase patch (2026-07-24)

- **Permissions:** path canonicalization rejects `..`; subpath grants inherit from parent workspace paths
- **Memory:** sqlite-vec KNN `MATCH ... k=N` with linear cosine fallback; `Database` uses `Arc<Mutex<Connection>>`
- **ROUT-01:** `OllamaProvider` + `ModelRouter` wired to `/api/chat`; fail-closed when Ollama offline
- **GIT-01 / CODE-01:** grant-checked git init/commit/branch; Python syntax lint via `py_compile`
- **LoopEngine:** `StubLoopEngine` trait stub in `aether-core`
- **macOS:** minimal `macos/AetherForgeApp/` SwiftUI shell + `BookmarkManager` skeleton

---

## 3. Canonical Seatbelt Profile (`profiles/sandbox_tool.sb`)

The **shipped and verified** profile (Darwin MVP):

```scheme
;; AetherForge Tool Sandbox Profile (sandbox_tool.sb)
(version 1)
(allow default)

(deny network*)

(deny file-write*
    (require-not (subpath (param "WORKSPACE_PATH"))))

(deny file-read*
    (subpath "/etc")
    (subpath "/private/etc"))
```

**Rationale:** macOS Sequoia+ requires dyld cache, mach-lookup, sysctl, and system library access for basic process execution. Allow-default with workspace-scoped write denial and `/etc` read denial provides verified isolation on Darwin while `(deny default)` without 100+ literal allows aborts.

**Future hardening:** migrate toward literal binary allowlist once a complete deny-default profile is validated on target macOS versions.

---

## 4. Curated MCP Allowlist (`mcp_allowlist.json`)

Apple Silicon paths (Phase 1):

```json
{
  "servers": [
    {
      "name": "filesystem",
      "command": "/opt/homebrew/bin/node",
      "args": ["/opt/homebrew/lib/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"],
      "sha256_pin": "PENDING_VERIFIED_DIGEST_TO_BE_PINNED_AT_BUILD"
    },
    {
      "name": "sqlite",
      "command": "/opt/homebrew/bin/node",
      "args": ["/opt/homebrew/lib/node_modules/@modelcontextprotocol/server-sqlite/dist/index.js"],
      "sha256_pin": "PENDING_VERIFIED_DIGEST_TO_BE_PINNED_AT_BUILD"
    }
  ]
}
```

Runtime rule: `PENDING*` pins fail closed until replaced with verified digests at build time.

---

## 5. Procedural Skills (`skills/`)

Skills follow agentskills.io-style YAML frontmatter and a `## Steps` section:

```markdown
---
name: changelog
description: Append a dated entry to CHANGELOG.md in a granted workspace
---

## Steps

- action: read_file
  path: CHANGELOG.md
- action: append_file
  path: CHANGELOG.md
  template: "- {{date}}: {{entry}}\n"
```

Loader: `aether-skills::SkillLoader`  
Executor: `aether-skills::SkillExecutor` (grant-checked via `PermissionManager`)  
Persistence: `procedural_skills` table (`success_count`, `failure_count`)

---

## 6. Explicit Backlog (Not in v1.2.4 phase patch)

- GraphRAG / temporal knowledge graph
- OpenClaw-style gateway (Slack/Telegram/Discord)
- Full Loop engine (`/loop`, `/goal`, maker-checker) beyond stub
- Direct MLX / llama.cpp / Hugging Face model runtime
- macOS installable app (DMG, notarization, full SwiftUI product shell)
- BYOK Keychain credential storage

---

## 7. Relationship to v1.2.3

v1.2.3 remains the comprehensive engineering specification (DDL, embeddings, bookmarks, undo journal). **v1.2.4 supersedes v1.2.3 only for:**

- §5 Seatbelt profile body (allow-default canonical)
- §6 MCP allowlist paths
- Platform scoring expectations
- SKILL-01 procedural skill contract

All other v1.2.3 sections remain authoritative until a future version explicitly patches them.
