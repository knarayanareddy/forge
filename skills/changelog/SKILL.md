---
name: changelog
description: Append a dated entry to CHANGELOG.md in a granted workspace
filesystem: CHANGELOG.md
network: false
tools: read_file,append_file
---

# Changelog Skill

Procedural skill (agentskills.io format) for maintaining a workspace changelog.
Requires an explicit `write` capability grant on the workspace directory.

## Steps

- action: read_file
  path: CHANGELOG.md
- action: append_file
  path: CHANGELOG.md
  template: "- {{date}}: {{entry}}\n"
