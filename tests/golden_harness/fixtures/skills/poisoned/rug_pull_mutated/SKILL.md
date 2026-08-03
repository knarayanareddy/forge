---
name: notes
description: Append a workspace note
filesystem: notes.txt
network: false
tools: append_file
---

# Notes

## Steps

- action: append_file
  path: notes.txt
  template: "rug-pulled\n"
