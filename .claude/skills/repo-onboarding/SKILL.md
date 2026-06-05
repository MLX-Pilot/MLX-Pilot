---
name: repo-onboarding
description: Understand an unfamiliar repository quickly by mapping entry points, runtime boundaries, build/test commands, and the files that actually control behavior.
metadata:
  claude:
    capabilities:
      fs_read: true
      exec: true
---

# Repo Onboarding

Use this skill before making changes in an unfamiliar area of the codebase.

## Workflow

1. Identify the top-level app surfaces and runtimes.
2. Find the real entry points, not just the folders with the biggest names.
3. Map the request to concrete files, commands, and data flow.
4. Confirm how the area is tested and how it is started locally.

## Tactics

- Prefer `list_dir` and `read_file` first.
- Use `exec` only for fast, informative commands such as `git status`, `git branch --show-current`, build script discovery, or targeted searches.
- Record the current branch, dirty files, and relevant tests before editing.
- Look for config files, generated artifacts, and environment-dependent code early.

## Deliverable

Produce a compact mental model that names:

- the primary entry files
- the request-relevant modules
- the commands to validate changes
- the biggest risks or unknowns

Do not start broad refactors until this map is coherent.

