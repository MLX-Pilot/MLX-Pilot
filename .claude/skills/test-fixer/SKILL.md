---
name: test-fixer
description: Reproduce failing tests or build errors, make the smallest defensible code fix, and rerun targeted verification until the issue is resolved.
metadata:
  claude:
    capabilities:
      fs_read: true
      fs_write: true
      exec: true
---

# Test Fixer

Use this skill when the user wants a failing test, build, or verification step fixed end to end.

## Workflow

1. Capture the exact failing command and error output.
2. Reduce the problem to the smallest reliable reproducer.
3. Identify the root cause before editing.
4. Patch the narrowest correct fix.
5. Rerun the focused test, then the broader relevant suite.

## Guardrails

- Prefer fixing code over weakening assertions.
- Do not silence failures with broad catch blocks or blanket skips unless the user explicitly wants triage-only behavior.
- Preserve existing interfaces unless there is a clear reason to change them.
- If the test is flaky, distinguish flake mitigation from a true product fix.

## Deliverable

Explain the root cause, the fix, what was rerun, and what still was not validated.

