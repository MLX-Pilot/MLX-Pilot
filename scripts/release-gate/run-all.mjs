#!/usr/bin/env node

import { runChannelsRealValidation } from "./channels-real-validation.mjs";
import { runLoadSmoke } from "./load-smoke.mjs";
import { runMigrationUpgradeRollback } from "./migration-upgrade-rollback.mjs";
import { runSecurityAuditSmoke } from "./security-audit-smoke.mjs";
import { runCrashRecoverySmoke } from "./crash-recovery-smoke.mjs";
import { runReleaseDryRunChecklist } from "./release-dry-run-checklist.mjs";
import {
  isDirectRun,
  printStandaloneResult,
  reportJsonPath,
  reportMarkdownPath,
  runReproductionCommands,
  writeJson,
} from "./_lib.mjs";
import fs from "node:fs/promises";

function blockSummary(block) {
  return {
    id: block.block,
    title: block.title,
    status: block.status,
    blockers: block.blockers || [],
  };
}

function computeDecision(blocks) {
  const byId = Object.fromEntries(blocks.map((block) => [block.block, block]));
  const securityHighFindings =
    byId.security_audit_smoke?.findings?.filter((entry) => entry.severity === "high") || [];
  const migrationOk = byId.migration_upgrade_rollback?.status === "pass";
  const restartOk = byId.crash_recovery_smoke?.status === "pass";
  const loadOk = byId.load_smoke?.status === "pass";
  const channelsOk = byId.channels_real_validation?.status === "pass";
  const dryRunOk = byId.release_dry_run?.status === "pass";

  const go =
    securityHighFindings.length === 0 &&
    migrationOk &&
    restartOk &&
    loadOk &&
    channelsOk &&
    dryRunOk;

  const blockers = blocks.flatMap((block) => block.blockers || []);
  if (securityHighFindings.length > 0) {
    blockers.unshift(
      ...securityHighFindings.map((entry) => `Security ${entry.severity}: ${entry.title}`),
    );
  }

  return {
    status: go ? "GO" : "NO-GO",
    criteria: {
      no_release_blocking_security_findings: securityHighFindings.length === 0,
      no_state_loss_in_migration_or_restart: migrationOk && restartOk,
      no_crash_under_short_load: loadOk,
      pending_channels_tested_with_explicit_result: channelsOk,
      dry_run_completed_or_checklist_validated: dryRunOk,
    },
    blockers: Array.from(new Set(blockers)),
  };
}

function renderMarkdown(report) {
  const blockLines = report.blocks
    .map((block) => {
      const blockerText = block.blockers.length
        ? ` Blockers: ${block.blockers.join(" | ")}`
        : "";
      return `- ${block.title}: ${block.status.toUpperCase()}.${blockerText}`;
    })
    .join("\n");

  const loadBlock = report.results.load_smoke;
  const loadLines = (loadBlock?.observations?.batches || [])
    .map(
      (entry) =>
        `- ${entry.endpoint} @ c=${entry.metrics.concurrency}: p50=${entry.metrics.p50_ms}ms p95=${entry.metrics.p95_ms}ms p99=${entry.metrics.p99_ms}ms errors=${entry.metrics.error_rate} timeouts=${entry.metrics.timeouts}`,
    )
    .join("\n");

  const channelLines = (report.results.channels_real_validation?.results || [])
    .map(
      (entry) =>
        `- ${entry.channel}: ${entry.status}${entry.failed_operation ? ` (${entry.failed_operation}/${entry.canonical_error})` : ""}`,
    )
    .join("\n");

  const securityFindings = (report.results.security_audit_smoke?.findings || [])
    .map(
      (entry) =>
        `- [${entry.severity}] ${entry.area}: ${entry.title}. Action: ${entry.action}`,
    )
    .join("\n") || "- No findings.";

  const criteriaLines = Object.entries(report.decision.criteria)
    .map(([key, value]) => `- ${key}: ${value ? "pass" : "fail"}`)
    .join("\n");

  const blockerLines = report.decision.blockers.length
    ? report.decision.blockers.map((entry) => `- ${entry}`).join("\n")
    : "- Nenhum bloqueador.";

  return `# Release Gate Report

- Generated at: ${report.generated_at}
- Final decision: ${report.decision.status}
- Repo: ${report.repo}

## Executive Summary

${blockLines}

## Objective Criteria

${criteriaLines}

## Channels

${channelLines || "- No bridge/webhook pending channels found."}

## Load Smoke

${loadLines || "- No load batches recorded."}

## Security Findings

${securityFindings}

## Blockers

${blockerLines}

## Reproduction

\`\`\`bash
${report.reproduction_commands.join("\n")}
\`\`\`
`;
}

export async function runAllReleaseGate() {
  const blocks = [];
  for (const runner of [
    runChannelsRealValidation,
    runLoadSmoke,
    runMigrationUpgradeRollback,
    runSecurityAuditSmoke,
    runCrashRecoverySmoke,
    runReleaseDryRunChecklist,
  ]) {
    blocks.push(await runner());
  }

  const decision = computeDecision(blocks);
  const report = {
    generated_at: new Date().toISOString(),
    repo: "/Users/kaike/mlx-ollama-pilot",
    decision,
    blocks: blocks.map(blockSummary),
    results: Object.fromEntries(blocks.map((block) => [block.block, block])),
    reproduction_commands: runReproductionCommands(),
  };

  await writeJson(reportJsonPath, report);
  await fs.writeFile(reportMarkdownPath, `${renderMarkdown(report)}\n`);
  return report;
}

if (isDirectRun(import.meta.url)) {
  runAllReleaseGate()
    .then(printStandaloneResult)
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
