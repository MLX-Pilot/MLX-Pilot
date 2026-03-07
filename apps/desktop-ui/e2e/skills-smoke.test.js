import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { createAgentSkillsController } from "../ui/agent-skills.js";

function createFixtureDom() {
  const dom = new JSDOM(
    `<!doctype html>
    <html>
      <body>
        <select id="agent-node-manager-select">
          <option value="npm">npm</option>
          <option value="pnpm">pnpm</option>
        </select>
        <p id="agent-skills-summary">-</p>
        <ul id="agent-skills-list"></ul>
      </body>
    </html>`,
    { url: "http://localhost/" },
  );

  const previous = {
    window: globalThis.window,
    document: globalThis.document,
    HTMLElement: globalThis.HTMLElement,
    Event: globalThis.Event,
    MouseEvent: globalThis.MouseEvent,
  };

  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Event = dom.window.Event;
  globalThis.MouseEvent = dom.window.MouseEvent;

  function restore() {
    globalThis.window = previous.window;
    globalThis.document = previous.document;
    globalThis.HTMLElement = previous.HTMLElement;
    globalThis.Event = previous.Event;
    globalThis.MouseEvent = previous.MouseEvent;
    dom.window.close();
  }

  return { dom, restore };
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

class FakeSkillsBackend {
  constructor() {
    this.calls = [];
    this.skills = [
      {
        name: "obsidian",
        description: "Vault integration",
        enabled: true,
        active: true,
        eligible: true,
        source: "workspace",
        bundled: false,
        integrity: "ok",
        sha256: null,
        capabilities: ["fs_read"],
        missing: [],
        install_options: [],
        primary_env: null,
        configured_env: [],
        configured_config: [],
        os: ["macos"],
      },
      {
        name: "gog",
        description: "GOG downloads",
        enabled: false,
        active: false,
        eligible: false,
        source: "workspace",
        bundled: false,
        integrity: "ok",
        sha256: null,
        capabilities: ["exec"],
        missing: ["anyBin:gogdl|lgogdownloader"],
        install_options: [
          {
            id: "gog-go",
            kind: "go",
            label: "Install gogdl",
            bins: ["gogdl"],
            os: ["macos"],
          },
        ],
        primary_env: null,
        configured_env: [],
        configured_config: [],
        os: ["macos"],
      },
      {
        name: "summarize",
        description: "Summaries",
        enabled: false,
        active: false,
        eligible: false,
        source: "workspace",
        bundled: false,
        integrity: "ok",
        sha256: null,
        capabilities: ["network"],
        missing: ["env:OPENAI_API_KEY", "config:provider"],
        install_options: [],
        primary_env: "OPENAI_API_KEY",
        configured_env: [],
        configured_config: [],
        os: [],
      },
    ];
  }

  buildSummary() {
    return {
      total: this.skills.length,
      eligible: this.skills.filter((skill) => skill.eligible).length,
      active: this.skills.filter((skill) => skill.active).length,
      missing_dependencies: this.skills.filter((skill) =>
        skill.missing.some((entry) => entry.startsWith("bin:") || entry.startsWith("anyBin:")),
      ).length,
      missing_configuration: this.skills.filter((skill) =>
        skill.missing.some((entry) => entry.startsWith("env:") || entry.startsWith("config:")),
      ).length,
      configure_now: true,
      installable: this.skills.filter((skill) => skill.install_options.length > 0).length,
      node_manager: "npm",
    };
  }

  async fetchJson(path, options = {}) {
    this.calls.push({ path, method: options.method || "GET", body: options.body || null });
    const payload = options.body ? JSON.parse(options.body) : {};

    if (path === "/agent/skills/check" && (options.method || "GET") === "GET") {
      return clone({ summary: this.buildSummary(), skills: this.skills });
    }

    if (path === "/agent/skills/enable" && options.method === "POST") {
      const skill = this.skills.find((entry) => entry.name === payload.skill);
      skill.enabled = true;
      skill.active = Boolean(skill.eligible);
      return { status: "ok" };
    }

    if (path === "/agent/skills/disable" && options.method === "POST") {
      const skill = this.skills.find((entry) => entry.name === payload.skill);
      skill.enabled = false;
      skill.active = false;
      return { status: "ok" };
    }

    if (path === "/agent/skills/install" && options.method === "POST") {
      const skill = this.skills.find((entry) => entry.name === payload.skills[0]);
      skill.eligible = true;
      skill.enabled = true;
      skill.active = true;
      skill.missing = [];
      return {
        node_manager: payload.node_manager,
        results: [
          {
            skill: skill.name,
            installs: [
              {
                id: "gog-go",
                kind: "go",
                label: "Install gogdl",
                ok: true,
                code: 0,
                stdout: "installed",
                stderr: "",
                warnings: [],
              },
            ],
            warnings: [],
          },
        ],
      };
    }

    if (path === "/agent/skills/config" && options.method === "POST") {
      const skill = this.skills.find((entry) => entry.name === payload.skill);
      skill.enabled = true;
      skill.eligible = true;
      skill.active = true;
      skill.missing = [];
      skill.configured_env = Object.keys(payload.env || {});
      skill.configured_config = Object.keys(payload.config || {});
      return clone(skill);
    }

    throw new Error(`Unhandled route: ${options.method || "GET"} ${path}`);
  }
}

function collectElements(document) {
  return {
    agentNodeManagerSelect: document.getElementById("agent-node-manager-select"),
    agentSkillsSummary: document.getElementById("agent-skills-summary"),
    agentSkillsList: document.getElementById("agent-skills-list"),
  };
}

async function flushUi() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

test("skills smoke toggles eligible skill, installs missing dependency, and configures env", async () => {
  const { dom, restore } = createFixtureDom();
  const backend = new FakeSkillsBackend();
  const prompts = [];
  let statusMessage = "";

  try {
    const controller = createAgentSkillsController({
      elements: collectElements(dom.window.document),
      fetchJson: backend.fetchJson.bind(backend),
      promptText: async (payload) => {
        prompts.push(payload);
        if (payload.title.includes("OPENAI_API_KEY")) {
          return "sk-test-secret";
        }
        if (payload.title.includes("provider")) {
          return "openai";
        }
        return "";
      },
      onStatus: (message) => {
        statusMessage = message;
      },
    });

    await controller.loadSkills();

    assert.match(dom.window.document.getElementById("agent-skills-summary").textContent, /1\/3 elegiveis/);
    assert.match(dom.window.document.getElementById("agent-skills-list").textContent, /obsidian/);
    assert.match(dom.window.document.getElementById("agent-skills-list").textContent, /Faltando: anyBin:gogdl\|lgogdownloader/);

    const toggle = dom.window.document.querySelector('input[data-item-name="obsidian"]');
    toggle.checked = false;
    toggle.dispatchEvent(new dom.window.Event("change", { bubbles: true }));
    await flushUi();
    assert.equal(controller.getSkills().find((skill) => skill.name === "obsidian").enabled, false);

    const installButton = Array.from(dom.window.document.querySelectorAll("button"))
      .find((button) => button.textContent === "Install" && button.closest("li").textContent.includes("gog"));
    installButton.click();
    await flushUi();
    assert.equal(controller.getSkills().find((skill) => skill.name === "gog").eligible, true);
    assert.match(statusMessage, /gog: Install gogdl -> ok/);

    const configButton = Array.from(dom.window.document.querySelectorAll("button"))
      .find((button) => button.textContent === "Config" && button.closest("li").textContent.includes("summarize"));
    configButton.click();
    await flushUi();
    const summarize = controller.getSkills().find((skill) => skill.name === "summarize");
    assert.equal(summarize.active, true);
    assert.deepEqual(summarize.configured_env, ["OPENAI_API_KEY"]);
    assert.deepEqual(summarize.configured_config, ["provider"]);

    assert.equal(prompts.length, 2);
    assert.deepEqual(
      backend.calls.map((entry) => `${entry.method} ${entry.path}`),
      [
        "GET /agent/skills/check",
        "POST /agent/skills/disable",
        "GET /agent/skills/check",
        "POST /agent/skills/install",
        "GET /agent/skills/check",
        "POST /agent/skills/config",
        "GET /agent/skills/check",
      ],
    );
  } finally {
    restore();
  }
});
