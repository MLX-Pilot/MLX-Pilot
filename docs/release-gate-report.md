# Release Gate Report

- Generated at: 2026-10-30T21:21:28.863Z
- Final decision: NO-GO
- Repo: .
- Revision: 47d941d398f44d4a99d89c3c3dc87de5313c6e62; dirty: true
- Environment: Node v22.19.0, win32/x64
- Scope: partial (not a complete release gate)

## Executive Summary

- channels_real_validation: NOT_RUN. Blockers: channels_real_validation: not selected by --only plugin_security_smoke
- load_smoke: NOT_RUN. Blockers: load_smoke: not selected by --only plugin_security_smoke
- migration_upgrade_rollback: NOT_RUN. Blockers: migration_upgrade_rollback: not selected by --only plugin_security_smoke
- security_audit_smoke: NOT_RUN. Blockers: security_audit_smoke: not selected by --only plugin_security_smoke
- crash_recovery_smoke: NOT_RUN. Blockers: crash_recovery_smoke: not selected by --only plugin_security_smoke
- release_dry_run: NOT_RUN. Blockers: release_dry_run: not selected by --only plugin_security_smoke
- Plugin broker and bridge security: PASS.

## Objective Criteria

- all_required_blocks_executed_and_passed: fail
- plugin_security_cases_executed_and_passed: pass
- no_release_blocking_security_findings: fail
- no_state_loss_in_migration_or_restart: fail
- no_crash_under_short_load: fail
- pending_channels_tested_with_explicit_result: fail
- dry_run_completed_or_checklist_validated: fail

## Channels

- Channel validation did not pass or was not executed; see block status.

## Load Smoke

- Load smoke did not pass or was not executed; see block status.

## Security Findings

- Security audit did not pass or was not executed; see blockers.

## Plugin Security

- Block: pass
- plugin_document_isolation: PASS
- plugin_workspace_isolation: PASS
- plugin_grant_revocation: PASS
- plugin_credentials_redaction: PASS
- plugin_network_authorization: PASS
- plugin_revision_conflict: PASS
- plugin_wasm_limit: PASS
- plugin_request_quota: PASS
- plugin_response_limit: PASS
- bridge_ui_contract: PASS
- sdk_bridge_contract: PASS

## Blockers

- channels_real_validation: not selected by --only plugin_security_smoke
- load_smoke: not selected by --only plugin_security_smoke
- migration_upgrade_rollback: not selected by --only plugin_security_smoke
- security_audit_smoke: not selected by --only plugin_security_smoke
- crash_recovery_smoke: not selected by --only plugin_security_smoke
- release_dry_run: not selected by --only plugin_security_smoke
- channels_real_validation: not_run (must execute and pass)
- load_smoke: not_run (must execute and pass)
- migration_upgrade_rollback: not_run (must execute and pass)
- security_audit_smoke: not_run (must execute and pass)
- crash_recovery_smoke: not_run (must execute and pass)
- release_dry_run: not_run (must execute and pass)

## Reproduction

```bash
node scripts/release-gate/run-all.mjs --only plugin_security_smoke
```


## Archived evidence

[Logs, fixture and sanitization notes](release-gate/evidence/2026-10-30/README.md). Local paths were normalized for repository portability.
