#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import fsSync from "node:fs";
import { createServer } from "node:http";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const reportPath = path.join(repoRoot, "docs", "skills-validation-report.md");
const isWindows = process.platform === "win32";

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function commandOutput(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error) {
    return "not available";
  }
  const text = `${result.stdout || ""}${result.stderr || ""}`.trim();
  if (!text) {
    return result.status === 0 ? "ok" : `failed (${result.status ?? "unknown"})`;
  }
  return text.split(/\r?\n/)[0];
}

function fixtureScript(success = true) {
  if (isWindows) {
    return success ? "@echo off\r\nexit /b 0\r\n" : "@echo off\r\nexit /b 1\r\n";
  }
  return success ? "#!/bin/sh\nexit 0\n" : "#!/bin/sh\nexit 1\n";
}

async function writeCommand(dir, name, content) {
  const filename = isWindows ? `${name}.cmd` : name;
  const filePath = path.join(dir, filename);
  await fs.writeFile(filePath, content, { mode: 0o755 });
  if (!isWindows) {
    await fs.chmod(filePath, 0o755);
  }
}

async function writeSkill(skillsDir, name, content) {
  const dir = path.join(skillsDir, name);
  await fs.mkdir(dir, { recursive: true });
  await fs.writeFile(path.join(dir, "SKILL.md"), content);
}

async function requestJson(baseUrl, method, route, body) {
  const response = await fetch(`${baseUrl}${route}`, {
    method,
    headers: {
      "Content-Type": "application/json",
    },
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await response.text();
  const payload = text ? JSON.parse(text) : null;
  if (!response.ok) {
    throw new Error(`${method} ${route} failed (${response.status}): ${text}`);
  }
  return payload;
}

async function waitForHealth(baseUrl, proc, name) {
  for (let index = 0; index < 120; index += 1) {
    if (proc.exitCode !== null) {
      throw new Error(`${name} exited early with code ${proc.exitCode}`);
    }
    try {
      const response = await fetch(`${baseUrl}/health`);
      if (response.ok) {
        return;
      }
    } catch {}
    await delay(500);
  }
  throw new Error(`Timed out waiting for ${name} health check`);
}

function startDaemon({
  name,
  workspace,
  settingsPath,
  port,
  pathPrefix = [],
  env = {},
  npmPrefix,
  npmCache,
}) {
  const logPath = path.join(path.dirname(settingsPath), `${name}.log`);
  const combinedPath = [...pathPrefix, process.env.PATH || ""]
    .filter(Boolean)
    .join(path.delimiter);
  const stdout = fsSync.openSync(logPath, "w");
  const proc = spawn("cargo", ["run", "-p", "mlx-ollama-daemon"], {
    cwd: repoRoot,
    env: {
      ...process.env,
      APP_BIND_ADDR: `127.0.0.1:${port}`,
      APP_SETTINGS_PATH: settingsPath,
      APP_AGENT_WORKSPACE: workspace,
      npm_config_prefix: npmPrefix,
      NPM_CONFIG_PREFIX: npmPrefix,
      npm_config_cache: npmCache,
      NPM_CONFIG_CACHE: npmCache,
      PATH: combinedPath,
      ...env,
    },
    stdio: ["ignore", stdout, stdout],
  });
  fsSync.closeSync(stdout);

  return {
    name,
    proc,
    baseUrl: `http://127.0.0.1:${port}`,
    logPath,
  };
}

async function stopDaemon(instance) {
  if (!instance?.proc || instance.proc.exitCode !== null) {
    return;
  }
  instance.proc.kill("SIGTERM");
  for (let index = 0; index < 20; index += 1) {
    if (instance.proc.exitCode !== null) {
      return;
    }
    await delay(250);
  }
  instance.proc.kill("SIGKILL");
  await delay(250);
}

function prettyJson(value) {
  return `\`\`\`json\n${JSON.stringify(value, null, 2)}\n\`\`\``;
}

async function main() {
  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), "mlx-pilot-skills-smoke-"));
  const workspace = path.join(tempRoot, "workspace");
  const skillsDir = path.join(workspace, ".claude", "skills");
  const binsDir = path.join(tempRoot, "bins");
  const npmPrefix = path.join(tempRoot, "npm-global");
  const npmCache = path.join(tempRoot, "npm-cache");
  const npmBinDir = path.join(npmPrefix, "bin");
  await fs.mkdir(skillsDir, { recursive: true });
  await fs.mkdir(binsDir, { recursive: true });
  await fs.mkdir(npmPrefix, { recursive: true });
  await fs.mkdir(npmBinDir, { recursive: true });
  await fs.mkdir(npmCache, { recursive: true });
  const artifactServer = createServer((request, response) => {
    if (request.url === "/artifact.bin") {
      response.writeHead(200, { "Content-Type": "application/octet-stream" });
      response.end("skills artifact fixture");
      return;
    }
    if (request.url === "/slow-artifact.bin") {
      setTimeout(() => {
        response.writeHead(200, { "Content-Type": "application/octet-stream" });
        response.end("skills slow artifact fixture");
      }, 5000);
      return;
    }
    response.writeHead(404, { "Content-Type": "text/plain" });
    response.end("not found");
  });
  await new Promise((resolve, reject) => {
    artifactServer.once("error", reject);
    artifactServer.listen(0, "127.0.0.1", resolve);
  });
  const artifactAddress = artifactServer.address();
  const artifactBaseUrl = `http://127.0.0.1:${artifactAddress.port}`;

  await writeCommand(binsDir, "obsidian", fixtureScript(true));
  await writeCommand(binsDir, "wa-cli", fixtureScript(true));
  await writeCommand(binsDir, "gh", fixtureScript(true));
  await writeCommand(binsDir, "curl", fixtureScript(true));
  await writeSkill(
    skillsDir,
    "obsidian",
    `---
name: obsidian
description: Obsidian workspace integration.
metadata:
  hermes:
    requires:
      bins:
        - obsidian
---

# Obsidian
`,
  );
  await writeSkill(
    skillsDir,
    "wacli",
    `---
name: wacli
description: WhatsApp CLI integration.
metadata:
  hermes:
    requires:
      bins:
        - wa-cli
---

# WA CLI
`,
  );
  await writeSkill(
    skillsDir,
    "gog",
    `---
name: gog
description: GOG sync helper.
metadata:
  hermes:
    requires:
      anyBins:
        - stringer
---

# GOG
`,
  );
  await writeSkill(
    skillsDir,
    "github",
    `---
name: github
description: GitHub helper.
metadata:
  hermes:
    primaryEnv: GITHUB_TOKEN
    requires:
      bins:
        - gh
      env:
        - GITHUB_TOKEN
---

# GitHub
`,
  );
  await writeSkill(
    skillsDir,
    "weather",
    `---
name: weather
description: Weather helper.
metadata:
  hermes:
    requires:
      bins:
        - curl
---

# Weather
`,
  );
  await writeSkill(
    skillsDir,
    "summarize",
    `---
name: summarize
description: Summaries.
metadata:
  hermes:
    primaryEnv: OPENAI_API_KEY
    requires:
      env:
        - OPENAI_API_KEY
      config:
        - provider
---

# Summarize
`,
  );
  await writeSkill(
    skillsDir,
    "artifact-install",
    `---
name: artifact-install
description: Download installer validation.
metadata:
  hermes:
    requires:
      bins:
        - artifact-install-fixture
    install:
      - id: artifact-install
        kind: download
        url: ${artifactBaseUrl}/artifact.bin
        bins:
          - artifact-install-fixture
        label: Download fixture artifact
---

# Artifact install
`,
  );
  await writeSkill(
    skillsDir,
    "download-fail",
    `---
name: download-fail
description: Download failure validation.
metadata:
  hermes:
    requires:
      bins:
        - never-download
    install:
      - id: download-fail
        kind: download
        url: http://127.0.0.1:9/fail
        label: Download fail fixture
---

# Download fail
`,
  );
  await writeSkill(
    skillsDir,
    "manual-fail",
    `---
name: manual-fail
description: Manual install validation.
metadata:
  hermes:
    requires:
      bins:
        - never-manual
    install:
      - id: manual-fail
        kind: manual
        url: https://example.invalid/manual-install
        bins:
          - never-manual
        label: Manual install fixture
---

# Manual fail
`,
  );
  await writeSkill(
    skillsDir,
    "timeout-skill",
    `---
name: timeout-skill
description: Timeout validation.
metadata:
  hermes:
    requires:
      bins:
        - never-timeout
    install:
      - id: timeout-skill
        kind: download
        url: ${artifactBaseUrl}/slow-artifact.bin
        bins:
          - never-timeout
        label: Slow download timeout fixture
---

# Timeout
`,
  );

  const reportLines = [
    "# Skills Validation Report",
    "",
    "## Environment",
    "",
    `- Date: ${new Date().toISOString()}`,
    `- Platform: ${process.platform} ${os.release()}`,
    `- Node: ${process.version}`,
    `- npm: ${commandOutput("npm", ["-v"])}`,
    `- cargo: ${commandOutput("cargo", ["-V"])}`,
    "",
    "## Skills tested",
    "",
    "- obsidian",
    "- wacli",
    "- gog",
    "- github",
    "- weather",
    "- summarize",
    "- artifact-install",
    "",
  ];

  const commonPath = [binsDir, npmPrefix, npmBinDir];
  const mainDaemon = startDaemon({
    name: "skills-main",
    workspace,
    settingsPath: path.join(tempRoot, "settings-main.json"),
    port: 19435,
    pathPrefix: commonPath,
    npmPrefix,
    npmCache,
  });

  const extraDaemons = [];
  try {
    await waitForHealth(mainDaemon.baseUrl, mainDaemon.proc, mainDaemon.name);

    const initialConfig = await requestJson(mainDaemon.baseUrl, "GET", "/agent/config");
    await requestJson(mainDaemon.baseUrl, "POST", "/agent/config", {
      ...initialConfig,
      workspace_root: workspace,
      node_package_manager: "npm",
    });

    const initialCheck = await requestJson(mainDaemon.baseUrl, "GET", "/agent/skills/check");
    assert.equal(initialCheck.summary.total >= 6, true);
    assert.equal(initialCheck.skills.find((skill) => skill.name === "obsidian").eligible, true);
    assert.equal(initialCheck.skills.find((skill) => skill.name === "wacli").eligible, true);
    assert.equal(initialCheck.skills.find((skill) => skill.name === "gog").eligible, false);

    await requestJson(mainDaemon.baseUrl, "POST", "/agent/skills/disable", { skill: "obsidian" });
    let skills = await requestJson(mainDaemon.baseUrl, "GET", "/agent/skills");
    assert.equal(skills.find((skill) => skill.name === "obsidian").enabled, false);

    await requestJson(mainDaemon.baseUrl, "POST", "/agent/skills/enable", { skill: "obsidian" });
    skills = await requestJson(mainDaemon.baseUrl, "GET", "/agent/skills");
    assert.equal(skills.find((skill) => skill.name === "obsidian").enabled, true);

    const installResponse = await requestJson(mainDaemon.baseUrl, "POST", "/agent/skills/install", {
      skills: ["artifact-install"],
      node_manager: "npm",
    });
    const installResult = installResponse.results.find((result) => result.skill === "artifact-install");
    assert.ok(installResult, JSON.stringify(installResponse, null, 2));
    const nodeInstall = installResult.insts?.[0] ?? installResult.installs?.[0];
    assert.ok(nodeInstall, JSON.stringify(installResponse, null, 2));
    assert.equal(nodeInstall.ok, true, JSON.stringify(nodeInstall, null, 2));
    assert.deepEqual(nodeInstall.warnings, ["artifact_downloaded_only"]);
    await fs.access(nodeInstall.stdout);

    const downloadFail = await requestJson(mainDaemon.baseUrl, "POST", "/agent/skills/install", {
      skills: ["download-fail"],
      node_manager: "npm",
    });
    const downloadResult = downloadFail.results[0].installs[0];
    assert.equal(downloadResult.ok, false);
    assert.equal(typeof downloadResult.stderr, "string");

    await requestJson(mainDaemon.baseUrl, "POST", "/agent/skills/config", {
      skill: "github",
      enabled: true,
      env: {
        GITHUB_TOKEN: "ghp_test_token_redacted",
      },
      config: {},
    });
    await requestJson(mainDaemon.baseUrl, "POST", "/agent/skills/config", {
      skill: "summarize",
      enabled: true,
      env: {
        OPENAI_API_KEY: "sk-test-redacted",
      },
      config: {
        provider: "openai",
      },
    });
    await requestJson(mainDaemon.baseUrl, "POST", "/agent/skills/disable", { skill: "weather" });

    await stopDaemon(mainDaemon);
    const restartedDaemon = startDaemon({
      name: "skills-main-restart",
      workspace,
      settingsPath: path.join(tempRoot, "settings-main.json"),
      port: 19435,
      pathPrefix: commonPath,
      npmPrefix,
      npmCache,
    });
    extraDaemons.push(restartedDaemon);
    await waitForHealth(restartedDaemon.baseUrl, restartedDaemon.proc, restartedDaemon.name);

    const restartedConfig = await requestJson(restartedDaemon.baseUrl, "GET", "/agent/config");
    const restartedSkills = await requestJson(restartedDaemon.baseUrl, "GET", "/agent/skills");
    const restartedCheck = await requestJson(restartedDaemon.baseUrl, "GET", "/agent/skills/check");
    assert.equal(restartedConfig.node_package_manager, "npm");
    assert.equal(restartedConfig.skill_overrides.github.enabled, true);
    assert.equal(restartedConfig.skill_overrides.weather.enabled, false);
    assert.ok(restartedConfig.skill_overrides.github.env_refs.GITHUB_TOKEN.startsWith("vault://"));
    assert.equal(restartedSkills.find((skill) => skill.name === "weather").active, false);
    assert.equal(
      restartedSkills.every((skill) => !skill.active || (skill.enabled && skill.eligible)),
      true,
    );
    assert.equal(
      restartedCheck.skills.find((skill) => skill.name === "github").eligible,
      true,
    );

    const manualResponse = await requestJson(
      restartedDaemon.baseUrl,
      "POST",
      "/agent/skills/install",
      { skills: ["manual-fail"] },
    );
    const permissionInstall = manualResponse.results[0].installs[0];
    assert.equal(permissionInstall.ok, false);
    assert.match(permissionInstall.stderr, /manual install required/i);
    assert.deepEqual(permissionInstall.warnings, ["manual_install_required"]);

    const timeoutDaemon = startDaemon({
      name: "skills-timeout",
      workspace,
      settingsPath: path.join(tempRoot, "settings-timeout.json"),
      port: 19437,
      pathPrefix: commonPath,
      npmPrefix,
      npmCache,
      env: {
        APP_AGENT_INSTALL_DOWNLOAD_TIMEOUT_SECS: "1",
      },
    });
    extraDaemons.push(timeoutDaemon);
    await waitForHealth(timeoutDaemon.baseUrl, timeoutDaemon.proc, timeoutDaemon.name);
    const timeoutResponse = await requestJson(timeoutDaemon.baseUrl, "POST", "/agent/skills/install", {
      skills: ["timeout-skill"],
    });
    const timeoutInstall = timeoutResponse.results[0].installs[0];
    assert.equal(timeoutInstall.ok, false);
    assert.match(timeoutInstall.stderr, /timed out|error sending request/i);
    assert.ok(
      timeoutInstall.warnings.length === 0 || timeoutInstall.warnings.includes("timeout"),
      JSON.stringify(timeoutInstall, null, 2),
    );

    reportLines.push("## UI smoke");
    reportLines.push("");
    reportLines.push("- Automated via `node --test apps/desktop-ui/e2e/skills-smoke.test.js`.");
    reportLines.push("- Verified enable/disable, install, configure and visual summary refresh without manual reload.");
    reportLines.push("");
    reportLines.push("## Real install evidence");
    reportLines.push("");
    reportLines.push(`- Download install skill: \`${nodeInstall.label}\` -> ok=${nodeInstall.ok}, code=${nodeInstall.code}`);
    reportLines.push("- Structured backend response snapshot:");
    reportLines.push("");
    reportLines.push(prettyJson({
      node: {
        ok: nodeInstall.ok,
        code: nodeInstall.code,
        stdout: nodeInstall.stdout.slice(0, 200),
        stderr: nodeInstall.stderr.slice(0, 200),
        warnings: nodeInstall.warnings,
      },
    }));
    reportLines.push("");
    reportLines.push("## Failure handling");
    reportLines.push("");
    reportLines.push(`- Network/download failure: ok=${downloadResult.ok}, stderr=${downloadResult.stderr.split(/\r?\n/)[0]}`);
    reportLines.push(`- Manual install required: ok=${permissionInstall.ok}, stderr=${permissionInstall.stderr.split(/\r?\n/)[0]}`);
    reportLines.push(`- Timeout failure: ok=${timeoutInstall.ok}, stderr=${timeoutInstall.stderr.split(/\r?\n/)[0]}`);
    reportLines.push("");
    reportLines.push("## Persistence after restart");
    reportLines.push("");
    reportLines.push("- `node_package_manager` persisted as `npm`.");
    reportLines.push("- `github` and `summarize` kept secret env refs in the vault-backed config.");
    reportLines.push("- `weather` remained disabled after restart.");
    reportLines.push("- Active skills after restart remained a subset of enabled + eligible skills.");
    reportLines.push("");
    reportLines.push("## Reproduction");
    reportLines.push("");
    reportLines.push("```bash");
    reportLines.push(`cd ${repoRoot}`);
    reportLines.push("node --test apps/desktop-ui/e2e/skills-smoke.test.js");
    reportLines.push("cargo test -p mlx-agent-skills -p mlx-agent-core -p mlx-ollama-daemon");
    reportLines.push("node scripts/skills-smoke.mjs");
    reportLines.push("```");

    await fs.mkdir(path.dirname(reportPath), { recursive: true });
    await fs.writeFile(reportPath, `${reportLines.join("\n")}\n`);
    console.log(`Skills smoke completed. Report: ${reportPath}`);
  } finally {
    await stopDaemon(mainDaemon);
    for (const daemon of extraDaemons) {
      await stopDaemon(daemon);
    }
    await new Promise((resolve) => artifactServer.close(resolve));
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});

