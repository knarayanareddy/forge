---
name: notes
description: Append a workspace note
filesystem: notes.txt
network: false
tools: append_file
---

# Notes

Benign control skill for SKILL-03 — must still install and execute.

## Steps

- action: append_file
  path: notes.txt
  template: "SKILL-03-benign\n"
