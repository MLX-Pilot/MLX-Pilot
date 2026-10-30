#!/usr/bin/env node

import { runChannelsRealValidation } from "./channels-real-validation.mjs";
import { runLoadSmoke } from "./load-smoke.mjs";
import { runMigrationUpgradeRollback } from "./migration-upgrade-rollback.mjs";
import { runSecurityAuditSmoke } from "./security-audit-smoke.mjs";
import { runCrashRecoverySmoke } from "./crash-recovery-smoke.mjs";
import { runReleaseDryRunChecklist } from "./release-dry-run-checklist.mjs";
import { runPluginSecuritySmoke, REQUIRED_PLUGIN_SECURITY_CASES } from "./plugin-security-smoke.mjs";
import {
  isDirectRun,
  printStandaloneResult,
  reportJsonPath,
  repoRoot,
  runReproductionCommands,
  writeJson,
} from "./_lib.mjs";
import fs from "node:fs/promises";
import path from "node:path";
import { execFileSync } from "node:child_process";

const defaultRunners = {
  channels_real_validation: runChannelsRealValidation,
  load_smoke: runLoadSmoke,
  migration_upgrade_rollback: runMigrationUpgradeRollback,
  security_audit_smoke: runSecurityAuditSmoke,
  crash_recovery_smoke: runCrashRecoverySmoke,
  release_dry_run: runReleaseDryRunChecklist,
  plugin_security_smoke: runPluginSecuritySmoke,
};
export const REQUIRED_BLOCK_IDS = Object.keys(defaultRunners);

function repositoryRevision() {
  const options = { cwd: repoRoot, encoding: "utf8", windowsHide: true, timeout: 5000, stdio: ["ignore", "pipe", "pipe"] };
  try {
    return {
      commit: execFileSync("git", ["rev-parse", "HEAD"], options).trim(),
      dirty: Boolean(execFileSync("git", ["status", "--porcelain"], options).trim()),
    };
  } catch {
    return { commit: null, dirty: null, error: "Git revision could not be read" };
  }
}

function blockSummary(block) {
  return {
    id: block.block,
    title: block.title,
    status: block.status,
    executed: block.executed,
    blockers: block.blockers || [],
  };
}

export function computeDecision(blocks) {
  const blockers = blocks.flatMap((block) => block.blockers || []);
  for (const block of blocks) {
    if (!REQUIRED_BLOCK_IDS.includes(block.block)) blockers.push(`Unexpected release gate block: ${block.block}`);
  }
  const matches = (id) => blocks.filter((block) => block.block === id);
  const passed = (id) => matches(id).length === 1 && matches(id)[0].status === "pass" && matches(id)[0].executed !== false;
  for (const id of REQUIRED_BLOCK_IDS) {
    const entries = matches(id);
    if (entries.length !== 1) blockers.push(`${id}: expected one result, found ${entries.length}`);
    else if (!passed(id)) blockers.push(`${id}: ${entries[0].status} (must execute and pass)`);
  }

  const plugin = matches("plugin_security_smoke")[0];
  const cases = Array.isArray(plugin?.cases) ? plugin.cases : [];
  const seenCases = new Set();
  let casesOk = REQUIRED_PLUGIN_SECURITY_CASES.length > 0 && cases.length > 0;
  for (const entry of cases) {
    if (!entry?.id || seenCases.has(entry.id) || entry.status !== "pass" || entry.executed === false) {
      casesOk = false;
      blockers.push(`Plugin case ${entry?.id || "<missing id>"}: duplicate, invalid or not passed (${entry?.status})`);
    }
    seenCases.add(entry?.id);
  }
  for (const id of REQUIRED_PLUGIN_SECURITY_CASES) {
    if (!seenCases.has(id)) {
      casesOk = false;
      blockers.push(`Plugin case ${id}: not executed`);
    }
  }
  if (!cases.length) blockers.push("Plugin security: no cases executed");
  if (!REQUIRED_PLUGIN_SECURITY_CASES.length) blockers.push("Plugin security: required case list is empty");

  const securityHighFindings = blocks.flatMap((block) => block.findings || [])
    .filter((entry) => ["high", "critical"].includes(entry.severity));
  blockers.push(...securityHighFindings.map((entry) => `Security ${entry.severity}: ${entry.title}`));
  const allBlocksPassed = REQUIRED_BLOCK_IDS.every(passed) && blocks.every((block) => block.status === "pass" && block.executed !== false);
  const pluginOk = passed("plugin_security_smoke") && casesOk;
  const go = allBlocksPassed && pluginOk && securityHighFindings.length === 0 && blockers.length === 0;

  return {
    status: go ? "GO" : "NO-GO",
    criteria: {
      all_required_blocks_executed_and_passed: allBlocksPassed,
      plugin_security_cases_executed_and_passed: pluginOk,
      no_release_blocking_security_findings: passed("security_audit_smoke") && securityHighFindings.length === 0,
      no_state_loss_in_migration_or_restart: passed("migration_upgrade_rollback") && passed("crash_recovery_smoke"),
      no_crash_under_short_load: passed("load_smoke"),
      pending_channels_tested_with_explicit_result: passed("channels_real_validation"),
      dry_run_completed_or_checklist_validated: passed("release_dry_run"),
    },
    blockers: Array.from(new Set(blockers)),
  };
}

export function renderMarkdown(report) {
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
    .join("\n") || (report.results.security_audit_smoke?.status === "pass"
      ? "- No findings in the completed security audit."
      : "- Security audit did not pass or was not executed; see blockers.");

  const plugin = report.results.plugin_security_smoke;
  const pluginCases = Array.isArray(plugin?.cases) ? plugin.cases : [];
  const pluginLines = REQUIRED_PLUGIN_SECURITY_CASES.map((id) => {
    const entries = pluginCases.filter((entry) => entry?.id === id);
    if (entries.length !== 1) return `- ${id}: ${entries.length ? "DUPLICATE" : "NOT_RUN"}`;
    const entry = entries[0];
    return `- ${id}: ${String(entry.status || "invalid").toUpperCase()}${entry.reason || entry.error ? ` — ${entry.reason || entry.error}` : ""}`;
  }).join("\n");

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
- Revision: ${report.revision.commit || "unavailable"}; dirty: ${report.revision.dirty ?? "unknown"}${report.revision.error ? ` (${report.revision.error})` : ""}
- Environment: Node ${report.environment.node}, ${report.environment.platform}/${report.environment.architecture}
- Scope: ${report.scope}${report.scope === "partial" ? " (not a complete release gate)" : ""}

## Executive Summary

${blockLines}

## Objective Criteria

${criteriaLines}

## Channels

${channelLines || (report.results.channels_real_validation?.status === "pass"
  ? "- Completed channel validation returned no pending channel results."
  : "- Channel validation did not pass or was not executed; see block status.")}

## Load Smoke

${loadLines || (loadBlock?.status === "pass"
  ? "- Completed load smoke returned no batches."
  : "- Load smoke did not pass or was not executed; see block status.")}

## Security Findings

${securityFindings}

## Plugin Security

- Block: ${plugin?.status || "not_run"}
${pluginLines || "- No required cases registered."}

## Blockers

${blockerLines}

## Reproduction

\`\`\`bash
${report.reproduction_commands.join("\n")}
\`\`\`
`;
}

export async function runAllReleaseGate({ runners = defaultRunners, only = null, outputDir = process.env.RELEASE_GATE_OUTPUT_DIR } = {}) {
  if (only !== null && !REQUIRED_BLOCK_IDS.includes(only)) throw new Error(`Unknown release gate block: ${only}`);
  const destination = path.resolve(outputDir || path.dirname(reportJsonPath));
  const revision = repositoryRevision();
  const blocks = [];
  for (const id of REQUIRED_BLOCK_IDS) {
    if ((only && only !== id) || typeof runners[id] !== "function") {
      blocks.push({ block: id, title: id, status: "not_run", executed: false, blockers: [
        `${id}: ${only && only !== id ? `not selected by --only ${only}` : "runner unavailable"}`,
      ] });
      continue;
    }
    try {
      const result = await runners[id]({ outputDir: destination });
      if (!result || result.block !== id || typeof result.status !== "string") throw new Error(`Invalid result for ${id}`);
      blocks.push({ ...result, title: result.title || id, executed: result.executed !== false });
    } catch (error) {
      blocks.push({ block: id, title: id, status: "fail", executed: false,
        blockers: [`${id}: ${error?.message || String(error)}`], error: error?.message || String(error) });
    }
  }

  const decision = computeDecision(blocks);
  const report = {
    generated_at: new Date().toISOString(),
    repo: ".",
    revision,
    environment: { node: process.version, platform: process.platform, architecture: process.arch },
    scope: only ? "partial" : "full",
    selected_blocks: only ? [only] : [...REQUIRED_BLOCK_IDS],
    decision,
    blocks: blocks.map(blockSummary),
    results: Object.fromEntries(blocks.map((block) => [block.block, block])),
    reproduction_commands: only ? [`node scripts/release-gate/run-all.mjs --only ${only}`] : runReproductionCommands(),
  };

  await writeJson(path.join(destination, "release-gate-report.json"), report);
  await fs.writeFile(path.join(destination, "release-gate-report.md"), `${renderMarkdown(report)}\n`);
  return report;
}

export async function runReleaseGateCli(args, { runners = defaultRunners } = {}) {
  const options = { runners };
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!["--only", "--output-dir"].includes(flag) || !value || value.startsWith("--")) {
      throw new Error("Usage: node scripts/release-gate/run-all.mjs [--only <block>] [--output-dir <directory>]");
    }
    options[flag === "--only" ? "only" : "outputDir"] = value;
  }
  const report = await runAllReleaseGate(options);
  printStandaloneResult(report);
  return report;
}

if (isDirectRun(import.meta.url)) {
  runReleaseGateCli(process.argv.slice(2))
    .catch((error) => {
      process.stderr.write(`${error.stack || error.message || String(error)}\n`);
      process.exitCode = 1;
    });
}
