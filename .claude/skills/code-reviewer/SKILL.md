---
name: code-reviewer
description: Review changed code for correctness, regressions, missing tests, risky assumptions, and rollout hazards, with findings presented before summary.
metadata:
  claude:
    capabilities:
      fs_read: true
      exec: true
---

# Code Reviewer

Use this skill when asked to review a diff, a branch, or a set of local changes.

## Review standard

Prioritize:

1. correctness bugs
2. behavioral regressions
3. security or safety issues
4. performance cliffs
5. missing or misleading tests

## Workflow

1. Inspect the changed files and the surrounding code path.
2. Check whether the diff matches the stated intent.
3. Look for broken call sites, stale assumptions, and edge cases.
4. Verify tests cover the change and would fail without it.
5. Report findings first, ordered by severity, with file references.

## Reporting

- Keep findings concrete and action-oriented.
- If no findings remain, say that explicitly.
- Note residual risks such as missing end-to-end coverage, untested migrations, or environment-specific behavior.

Do not bury critical bugs under a long changelog.

