---
name: git-worktree-manager
description: Create and manage isolated git worktrees and branches for implementation, review, or experiments without disturbing the current workspace.
metadata:
  claude:
    capabilities:
      fs_read: true
      fs_write: true
      exec: true
---

# Git Worktree Manager

Use this skill when the task benefits from an isolated branch or a parallel working tree.

## Workflow

1. Inspect the current branch and working tree state.
2. Choose a clear branch name that reflects the task.
3. Create the worktree with non-interactive git commands.
4. Confirm the new path, branch, and upstream state before doing any edits.

## Guardrails

- Never discard or overwrite unrelated user changes.
- Prefer `git worktree add` over copying directories manually.
- Prefer non-interactive git commands.
- Keep the user informed about the exact path and branch created.

## When to use

- risky refactors
- parallel implementation tracks
- review or reproduction branches
- temporary isolation from a dirty main workspace

