---
name: session-orchestrator
description: Coordinate multi-step work by spawning focused local agent sessions, delegating isolated subtasks, and consolidating the results back into the current session.
metadata:
  claude:
    capabilities:
      fs_read: true
---

# Session Orchestrator

Use this skill when the task is large enough to benefit from parallel exploration, isolated investigation, or staged execution.

## Workflow

1. Split the request into two to four independent subtasks with explicit deliverables.
2. Create one child session per subtask with `sessions_spawn`.
3. Send each child a concise brief with:
   - the objective
   - the file or subsystem scope
   - the success criteria
   - whether it may edit files or should stay read-only
4. Track progress with `sessions_status`.
5. Pull the useful output back with `sessions_history`.
6. Consolidate the findings in the parent session and resolve conflicts before editing shared files.

## Good patterns

- Use `memory_search` and `memory_get` before spawning children when prior work may already answer part of the request.
- Keep child scopes narrow so they do not fight over the same files.
- Treat child output as evidence, not ground truth. Re-check the final files before merging conclusions.

## Avoid

- Spawning sessions for small single-file changes.
- Letting multiple sessions modify the same hot path at once.
- Returning raw child transcripts when a synthesized conclusion is possible.

