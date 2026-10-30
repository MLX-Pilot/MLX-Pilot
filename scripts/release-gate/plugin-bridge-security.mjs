#!/usr/bin/env node

// These are DOM/jsdom and Node SDK contracts, not real-browser E2E coverage.
import { spawn } from "node:child_process";
import { mkdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = fileURLToPath(new URL("../../", import.meta.url));
const execution = "DOM/jsdom UI and Node SDK contract suites; not real-browser E2E";
const suites = [
  {
    id: "bridge_ui_contract",
    file: "apps/desktop-ui/e2e/plugin-manager.test.js",
    requiredTests: [
      "the isolated bridge accepts only scoped broker operations and preserves tool calls",
      "navigation disconnects the bridge and reopening issues a new document token",
      "saving reloads an open authorized frontend and closes it after revocation",
    ],
  },
  {
    id: "sdk_bridge_contract",
    file: "packages/plugin-sdk/test/sdk.test.mjs",
    requiredTests: [
      "frontend bridge ignores impersonated replies and correlates host responses",
      "independent frontend clients cannot consume each other's replies",
      "frontend capability calls use operation envelopes and reject calls after disposal",
      "closing a frontend rejects pending calls and releases its listener",
    ],
  },
];

export const BRIDGE_SECURITY_CASES = Object.freeze(suites.map(({ id }) => id));

// Validate the Node TAP reporter's completed, flat suites, including the named
// regressions. Exit zero alone also accepts empty, skipped or filtered suites.
export function evaluateBridgeTestOutput(result, requiredTests = []) {
  const problems = [];
  const stdout = result.stdout || "";
  const lines = stdout.split(/\r?\n/);
  const counts = {};
  const countNames = ["tests", "pass", "fail", "cancelled", "skipped", "todo"];
  for (const name of countNames) {
    const matches = lines.filter((line) => new RegExp(`^# ${name} \\d+$`).test(line));
    if (matches.length !== 1) problems.push(`Missing or repeated TAP ${name} count`);
    else counts[name] = Number(matches[0].split(" ")[2]);
  }
  const records = lines.flatMap((line) => {
    const match = /^(ok|not ok) (\d+) - (.*)$/.exec(line);
    if (!match) return [];
    const directive = /\s+#\s+(SKIP|TODO)\b/i.exec(match[3]);
    return [{
      ok: match[1] === "ok",
      number: Number(match[2]),
      name: directive ? match[3].slice(0, directive.index) : match[3],
      directive: directive?.[1].toUpperCase(),
    }];
  });
  const plans = lines.flatMap((line) => /^1\.\.(\d+)$/.exec(line)?.slice(1).map(Number) || []);
  if (lines[0] !== "TAP version 13") problems.push("Missing Node TAP header");
  if (plans.length !== 1 || plans[0] !== records.length) problems.push("Incomplete TAP plan/results");
  if (records.some((record, index) => record.number !== index + 1)) problems.push("Invalid TAP result sequence");
  if (counts.tests !== records.length || counts.pass !== counts.tests) problems.push("TAP results do not match passed test counts");
  if (!(counts.tests > 0)) problems.push("No tests executed");
  for (const name of ["fail", "cancelled", "skipped", "todo"]) {
    if (counts[name] !== 0) problems.push(`TAP ${name} must be zero`);
  }
  if (records.some((record) => !record.ok || record.directive)) problems.push("A test failed, skipped or remained TODO");
  if (lines.some((line) => /^\s*Bail out!/i.test(line))) problems.push("TAP execution bailed out");
  if (result.exitCode !== 0) problems.push(`Node exited with ${result.exitCode ?? "no exit code"}`);
  if (result.signal) problems.push(`Node terminated with ${result.signal}`);
  if (result.error) problems.push(result.error);
  if (result.timedOut) problems.push("Suite execution timed out");
  if (result.truncated) problems.push("Suite output exceeded the evidence limit");

  const executedTests = records.filter((record) => record.ok && !record.directive).map(({ name }) => name);
  for (const name of requiredTests) {
    if (!executedTests.includes(name)) problems.push(`Required regression did not pass: ${name}`);
  }
  return { status: problems.length ? "fail" : "pass", counts, executedTests, problems };
}

function executeSuite(args) {
  return new Promise((resolve) => {
    const result = { stdout: "", stderr: "", exitCode: null, signal: null };
    let finished = false;
    let bytes = 0;
    // A caller may itself run under node --test. Its internal child marker
    // would switch this independent reporter to the test-runner IPC format.
    const env = { ...process.env };
    delete env.NODE_TEST_CONTEXT;
    const child = spawn(process.execPath, args, {
      cwd: repoRoot,
      env,
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
    });
    const finish = () => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      resolve(result);
    };
    const abort = () => {
      child.kill("SIGKILL");
      child.stdout.destroy();
      child.stderr.destroy();
      finish();
    };
    const timer = setTimeout(() => {
      result.timedOut = true;
      abort();
    }, 120_000);
    for (const stream of ["stdout", "stderr"]) {
      child[stream].setEncoding("utf8").on("data", (chunk) => {
        if (finished) return;
        bytes += Buffer.byteLength(chunk);
        if (bytes > 4 * 1024 * 1024) {
          result.truncated = true;
          abort();
        } else result[stream] += chunk;
      });
    }
    child.on("error", (error) => {
      result.error = `Could not execute Node suite: ${error.message}`;
      finish();
    });
    child.on("close", (code, signal) => {
      if (finished) return;
      result.exitCode = code;
      result.signal = signal;
      finish();
    });
  });
}

export async function runPluginBridgeSecurity({ outputDir } = {}) {
  const cases = [];
  for (const suite of suites) {
    const args = ["--test", "--test-reporter=tap", suite.file];
    const startedAt = new Date().toISOString();
    let result;
    try {
      if (!(await stat(path.join(repoRoot, suite.file))).isFile()) throw new Error("Suite is not a file");
      result = await executeSuite(args);
    } catch (error) {
      result = { stdout: "", stderr: "", exitCode: null, error: `Suite unavailable: ${error.message}` };
    }
    const assessment = evaluateBridgeTestOutput(result, suite.requiredTests);
    const evidence = {
      execution,
      suite: suite.file,
      command: [process.execPath, ...args],
      startedAt,
      finishedAt: new Date().toISOString(),
      exitCode: result.exitCode,
      signal: result.signal || null,
      requiredTests: suite.requiredTests,
      counts: assessment.counts,
      executedTests: assessment.executedTests,
      problems: assessment.problems,
    };
    if (outputDir) {
      try {
        await mkdir(outputDir, { recursive: true });
        evidence.stdoutPath = path.resolve(outputDir, `${suite.id}.tap`);
        evidence.stderrPath = path.resolve(outputDir, `${suite.id}.stderr.log`);
        await writeFile(evidence.stdoutPath, result.stdout, "utf8");
        await writeFile(evidence.stderrPath, result.stderr, "utf8");
      } catch (error) {
        assessment.status = "fail";
        evidence.problems.push(`Could not persist suite evidence: ${error.message}`);
      }
    }
    cases.push({ id: suite.id, status: assessment.status, evidence });
  }
  return { status: cases.every(({ status }) => status === "pass") ? "pass" : "fail", execution, cases };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const result = await runPluginBridgeSecurity({ outputDir: process.argv[2] });
  console.log(JSON.stringify(result, null, 2));
  process.exitCode = result.status === "pass" ? 0 : 1;
}
