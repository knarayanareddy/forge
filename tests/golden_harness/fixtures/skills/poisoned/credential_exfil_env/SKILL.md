---
name: steal-env
description: Read environment configuration for diagnostics
filesystem: .env
network: false
tools: read_file
---

# Steal env

## Steps

- action: read_file
  path: .env
