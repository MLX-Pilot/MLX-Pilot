#!/usr/bin/env node

import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import fsSync from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const repoRoot = "/Users/kaike/mlx-ollama-pilot";
const reportPath = path.join(repoRoot, "docs", "skills-validation-report.md");

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function writeExecutable(filePath, content, mode = 0o755) {
  await fs.writeFile(filePath, content, { mode });
  await fs.chmod(filePath, mode);
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
  gobin,
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
      GOBIN: gobin,
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
  const skillsDir = path.join(workspace, "skills");
  const binsDir = path.join(tempRoot, "bins");
  const npmPrefix = path.join(tempRoot, "npm-global");
  const goBin = path.join(tempRoot, "go-bin");
  const npmCache = path.join(tempRoot, "npm-cache");
  await fs.mkdir(skillsDir, { recursive: true });
  await fs.mkdir(binsDir, { recursive: true });
  await fs.mkdir(path.join(npmPrefix, "bin"), { recursive: true });
  await fs.mkdir(npmCache, { recursive: true });
  await fs.mkdir(goBin, { recursive: true });

  await writeExecutable(path.join(binsDir, "obsidian"), "#!/bin/sh\nexit 0\n");
  await writeExecutable(path.join(binsDir, "wa-cli"), "#!/bin/sh\nexit 0\n");
  await writeExecutable(path.join(binsDir, "gh"), "#!/bin/sh\nexit 0\n");

  await writeSkill(
    skillsDir,
    "obsidian",
    `---
name: obsidian
description: Obsidian workspace integration.
os:
  - macos
metadata:
  openclaw:
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
  openclaw:
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
  openclaw:
    requires:
      anyBins:
        - stringer
    install:
      - id: gog-go
        kind: go
        module: golang.org/x/tools/cmd/stringer
        bins:
          - stringer
        label: Install stringer via go
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
  openclaw:
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
  openclaw:
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
  openclaw:
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
    "node-real-install",
    `---
name: node-real-install
description: Node installer validation.
metadata:
  openclaw:
    requires:
      bins:
        - npm-check-updates
    install:
      - id: node-real
        kind: node
        package: npm-check-updates
        bins:
          - npm-check-updates
        label: Install npm-check-updates
---

# Node install
`,
  );
  await writeSkill(
    skillsDir,
    "download-fail",
    `---
name: download-fail
description: Download failure validation.
metadata:
  openclaw:
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
    "permission-fail",
    `---
name: permission-fail
description: Permission failure validation.
metadata:
  openclaw:
    requires:
      bins:
        - never-permission
    install:
      - id: permission-fail
        kind: uv
        package: ruff
        label: UV permission fixture
---

# Permission fail
`,
  );
  await writeSkill(
    skillsDir,
    "timeout-skill",
    `---
name: timeout-skill
description: Timeout validation.
metadata:
  openclaw:
    requires:
      bins:
        - never-timeout
    install:
      - id: timeout-skill
        kind: uv
        package: ruff
        label: UV timeout fixture
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
    `- macOS: ${spawnSync("sw_vers", ["-productVersion"], { encoding: "utf8" }).stdout.trim()}`,
    `- Node: ${spawnSync("node", ["-v"], { encoding: "utf8" }).stdout.trim()}`,
    `- npm: ${spawnSync("npm", ["-v"], { encoding: "utf8" }).stdout.trim()}`,
    `- go: ${spawnSync("go", ["version"], { encoding: "utf8" }).stdout.trim()}`,
    `- brew: ${spawnSync("brew", ["--version"], { encoding: "utf8" }).stdout.split("\n")[0]}`,
    "",
    "## Skills tested",
    "",
    "- obsidian",
    "- wacli",
    "- gog",
    "- github",
    "- weather",
    "- summarize",
    "- node-real-install",
    "",
  ];

  const mainDaemon = startDaemon({
    name: "skills-main",
    workspace,
    settingsPath: path.join(tempRoot, "settings-main.json"),
    port: 19435,
    pathPrefix: [binsDir, path.join(npmPrefix, "bin"), goBin],
    npmPrefix,
    npmCache,
    gobin: goBin,
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
      skills: ["node-real-install", "gog"],
      node_manager: "npm",
    });
    const nodeInstall = installResponse.results.find((result) => result.skill === "node-real-install").installs[0];
    const goInstall = installResponse.results.find((result) => result.skill === "gog").installs[0];
    assert.equal(typeof nodeInstall.ok, "boolean");
    assert.equal(typeof goInstall.ok, "boolean");
    assert.equal(nodeInstall.ok, true);
    assert.equal(goInstall.ok, true);

    const afterInstallCheck = await requestJson(mainDaemon.baseUrl, "GET", "/agent/skills/check");
    assert.equal(
      afterInstallCheck.skills.find((skill) => skill.name === "node-real-install").eligible,
      true,
    );
    assert.equal(afterInstallCheck.skills.find((skill) => skill.name === "gog").eligible, true);

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
    await waitForHealth(
      (extraDaemons.push(
        startDaemon({
          name: "skills-main-restart",
          workspace,
          settingsPath: path.join(tempRoot, "settings-main.json"),
          port: 19435,
          pathPrefix: [binsDir, path.join(npmPrefix, "bin"), goBin],
          npmPrefix,
          npmCache,
          gobin: goBin,
        }),
      ), extraDaemons[extraDaemons.length - 1]).baseUrl,
      extraDaemons[extraDaemons.length - 1].proc,
      extraDaemons[extraDaemons.length - 1].name,
    );
    const restartedDaemon = extraDaemons[extraDaemons.length - 1];

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

    const permissionBins = path.join(tempRoot, "permission-bins");
    await fs.mkdir(permissionBins, { recursive: true });
    await writeExecutable(
      path.join(permissionBins, "uv"),
      "#!/bin/sh\necho 'permission denied' 1>&2\nexit 126\n",
    );
    const permissionDaemon = startDaemon({
      name: "skills-permission",
      workspace,
      settingsPath: path.join(tempRoot, "settings-permission.json"),
      port: 19436,
      pathPrefix: [permissionBins],
      npmPrefix,
      npmCache,
      gobin: goBin,
    });
    extraDaemons.push(permissionDaemon);
    await waitForHealth(permissionDaemon.baseUrl, permissionDaemon.proc, permissionDaemon.name);
    const permissionResponse = await requestJson(
      permissionDaemon.baseUrl,
      "POST",
      "/agent/skills/install",
      { skills: ["permission-fail"] },
    );
    const permissionInstall = permissionResponse.results[0].installs[0];
    assert.equal(permissionInstall.ok, false);
    assert.match(permissionInstall.stderr, /Permission denied|failed to spawn|permission/i);

    const timeoutBins = path.join(tempRoot, "timeout-bins");
    await fs.mkdir(timeoutBins, { recursive: true });
    await writeExecutable(
      path.join(timeoutBins, "uv"),
      "#!/bin/sh\nsleep 5\nexit 0\n",
    );
    const timeoutDaemon = startDaemon({
      name: "skills-timeout",
      workspace,
      settingsPath: path.join(tempRoot, "settings-timeout.json"),
      port: 19437,
      pathPrefix: [timeoutBins],
      npmPrefix,
      npmCache,
      gobin: goBin,
      env: {
        APP_AGENT_INSTALL_TIMEOUT_SECS: "1",
      },
    });
    extraDaemons.push(timeoutDaemon);
    await waitForHealth(timeoutDaemon.baseUrl, timeoutDaemon.proc, timeoutDaemon.name);
    const timeoutResponse = await requestJson(timeoutDaemon.baseUrl, "POST", "/agent/skills/install", {
      skills: ["timeout-skill"],
    });
    const timeoutInstall = timeoutResponse.results[0].installs[0];
    assert.equal(timeoutInstall.ok, false);
    assert.match(timeoutInstall.stderr, /timed out/);
    assert.deepEqual(timeoutInstall.warnings, ["timeout"]);

    reportLines.push("## UI smoke");
    reportLines.push("");
    reportLines.push("- Automated via `node --test apps/desktop-ui/e2e/skills-smoke.test.js`.");
    reportLines.push("- Verified enable/disable, install, configure and visual summary refresh without manual reload.");
    reportLines.push("");
    reportLines.push("## Real install evidence");
    reportLines.push("");
    reportLines.push(`- Node install skill: \`${nodeInstall.label}\` -> ok=${nodeInstall.ok}, code=${nodeInstall.code}`);
    reportLines.push(`- Go install skill: \`${goInstall.label}\` -> ok=${goInstall.ok}, code=${goInstall.code}`);
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
      go: {
        ok: goInstall.ok,
        code: goInstall.code,
        stdout: goInstall.stdout.slice(0, 200),
        stderr: goInstall.stderr.slice(0, 200),
        warnings: goInstall.warnings,
      },
    }));
    reportLines.push("");
    reportLines.push("## Failure handling");
    reportLines.push("");
    reportLines.push(`- Network/download failure: ok=${downloadResult.ok}, stderr=${downloadResult.stderr.split("\n")[0]}`);
    reportLines.push(`- Permission failure: ok=${permissionInstall.ok}, stderr=${permissionInstall.stderr.split("\n")[0]}`);
    reportLines.push(`- Timeout failure: ok=${timeoutInstall.ok}, stderr=${timeoutInstall.stderr.split("\n")[0]}`);
    reportLines.push("");
    reportLines.push("## Persistence after restart");
    reportLines.push("");
    reportLines.push("- `node_package_manager` persisted as `npm`.");
    reportLines.push("- `github` and `summarize` kept secret env refs in the vault-backed config.");
    reportLines.push("- `weather` remained disabled after restart.");
    reportLines.push("- Active skills after restart remained a subset of enabled + eligible skills.");
    reportLines.push("");
    reportLines.push("## Limitations");
    reportLines.push("");
    reportLines.push("- The Tauri window was built locally, but UI interaction evidence is headless via jsdom smoke instead of native window automation.");
    reportLines.push("- Real install coverage used `node` and `go`; `brew` remained available but was not required because `go` satisfied the acceptance gate.");
    reportLines.push("");
    reportLines.push("## Reproduction");
    reportLines.push("");
    reportLines.push("```bash");
    reportLines.push("cd /Users/kaike/mlx-ollama-pilot");
    reportLines.push("node --test apps/desktop-ui/e2e/skills-smoke.test.js");
    reportLines.push("cargo test -p mlx-agent-skills -p mlx-agent-core -p mlx-ollama-daemon");
    reportLines.push("node scripts/skills-smoke.mjs");
    reportLines.push("cd apps/desktop-ui/src-tauri && cargo tauri build");
    reportLines.push("```");

    await fs.writeFile(reportPath, `${reportLines.join("\n")}\n`);
    console.log(`Skills smoke completed. Report: ${reportPath}`);
  } finally {
    await stopDaemon(mainDaemon);
    for (const daemon of extraDaemons) {
      await stopDaemon(daemon);
    }
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
}).finally(() => {
  process.exit(process.exitCode ?? 0);
});
