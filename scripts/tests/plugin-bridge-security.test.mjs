import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { BRIDGE_SECURITY_CASES, evaluateBridgeTestOutput } from "../release-gate/plugin-bridge-security.mjs";

const required = "essential bridge regression";

async function nodeSuite(t, source) {
  const directory = await mkdtemp(path.join(tmpdir(), "mlx-bridge-gate-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const file = path.join(directory, "suite.test.mjs");
  await writeFile(file, `import test from 'node:test';\n${source}\n`);
  const env = { ...process.env };
  delete env.NODE_TEST_CONTEXT;
  const result = spawnSync(process.execPath, ["--test", "--test-reporter=tap", file], {
    env, encoding: "utf8", timeout: 10_000, windowsHide: true,
  });
  assert.ifError(result.error);
  assert.match(result.stdout, /^TAP version 13\r?\n/);
  return { stdout: result.stdout, stderr: result.stderr, exitCode: result.status, signal: result.signal };
}

test("declares the UI and SDK contract case IDs", () => {
  assert.deepEqual(BRIDGE_SECURITY_CASES, ["bridge_ui_contract", "sdk_bridge_contract"]);
});

test("accepts completed Node TAP evidence with the required passing regression", async (t) => {
  const result = await nodeSuite(t, `test('${required}', () => {});`);
  const assessment = evaluateBridgeTestOutput(result, [required]);
  assert.equal(assessment.status, "pass", JSON.stringify(assessment.problems));
  assert.equal(assessment.counts.tests, 1);
  assert.deepEqual(assessment.executedTests, [required]);
});

test("a passing suite cannot substitute unrelated tests for the required regression", async (t) => {
  const result = await nodeSuite(t, "test('unrelated', () => {});");
  const assessment = evaluateBridgeTestOutput(result, [required]);
  assert.equal(assessment.status, "fail");
  assert.ok(assessment.problems.some((problem) => problem.includes("Required regression")));
});

for (const [label, body] of [
  ["failed", `test('${required}', () => { throw new Error('expected failure'); });`],
  ["skipped", `test('${required}', { skip: true }, () => {});`],
  ["TODO", `test.todo('${required}');`],
  ["empty", ""],
]) {
  test(`rejects ${label} real Node test output`, async (t) => {
    const result = await nodeSuite(t, body);
    const assessment = evaluateBridgeTestOutput(result, [required]);
    assert.equal(assessment.status, "fail");
    assert.ok(assessment.problems.length > 0);
  });
}

test("rejects skipped unrelated tests even when the required regression passed", async (t) => {
  const result = await nodeSuite(t, `test('${required}', () => {}); test.skip('other', () => {});`);
  assert.equal(evaluateBridgeTestOutput(result, [required]).status, "fail");
});

test("missing suite execution fails instead of becoming an empty success", () => {
  const assessment = evaluateBridgeTestOutput({ stdout: "", exitCode: null, error: "Suite unavailable: ENOENT" }, [required]);
  assert.equal(assessment.status, "fail");
  assert.ok(assessment.problems.includes("Suite unavailable: ENOENT"));
});

test("standalone runner exits nonzero and records both unavailable suites", async (t) => {
  const directory = await mkdtemp(path.join(tmpdir(), "mlx-bridge-missing-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const script = path.join(directory, "scripts", "release-gate", "plugin-bridge-security.mjs");
  await mkdir(path.dirname(script), { recursive: true });
  await copyFile(new URL("../release-gate/plugin-bridge-security.mjs", import.meta.url), script);
  const result = spawnSync(process.execPath, [script], {
    encoding: "utf8", timeout: 10_000, windowsHide: true,
  });
  assert.ifError(result.error);
  assert.equal(result.status, 1);
  const report = JSON.parse(result.stdout);
  assert.equal(report.status, "fail");
  assert.deepEqual(report.cases.map(({ id }) => id), BRIDGE_SECURITY_CASES);
  for (const entry of report.cases) {
    assert.equal(entry.status, "fail");
    assert.equal(entry.evidence.exitCode, null);
    assert.ok(entry.evidence.problems.some((problem) => problem.startsWith("Suite unavailable:")));
  }
});

test("process errors, timeouts, signals and truncation override passing TAP", async (t) => {
  const result = await nodeSuite(t, `test('${required}', () => {});`);
  for (const failure of [
    { exitCode: 1 },
    { signal: "SIGTERM" },
    { timedOut: true },
    { truncated: true },
    { error: "Could not execute Node suite" },
  ]) assert.equal(evaluateBridgeTestOutput({ ...result, ...failure }, [required]).status, "fail");
});

test("incomplete or contradictory TAP cannot pass from an exit-zero process", async (t) => {
  const result = await nodeSuite(t, `test('${required}', () => {});`);
  for (const stdout of [
    result.stdout.replace(/^# tests \d+\r?\n/m, ""),
    result.stdout.replace(/^1\.\.1$/m, "1..2"),
    result.stdout.replace(/^# pass 1$/m, "# pass 2"),
    result.stdout.replace(/^ok 1 - /m, "ok 2 - "),
    `${result.stdout}\n# fail 0\n`,
    `${result.stdout}\nBail out! suite aborted\n`,
  ]) assert.equal(evaluateBridgeTestOutput({ ...result, stdout }, [required]).status, "fail");
});
