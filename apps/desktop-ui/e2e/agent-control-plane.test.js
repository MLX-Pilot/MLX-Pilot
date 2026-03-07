import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { createAgentControlPlaneController } from "../ui/agent-control-plane.js";

function createFixtureDom() {
  const dom = new JSDOM(
    `<!doctype html>
    <html>
      <body>
        <button id="agent-plugins-refresh-btn" type="button">plugins</button>
        <div id="agent-plugins-list"></div>
        <h4 id="agent-plugin-detail-title"></h4>
        <p id="agent-plugin-detail-meta"></p>
        <input id="agent-plugin-config-enabled" type="checkbox" />
        <div id="agent-plugin-config-form"></div>
        <button id="agent-plugin-save-btn" type="button">save-plugin</button>
        <button id="agent-plugin-reset-btn" type="button">reset-plugin</button>
        <p id="agent-plugin-feedback">-</p>

        <select id="agent-tool-profile-select"></select>
        <button id="agent-tool-profile-apply-btn" type="button">apply-profile</button>
        <select id="agent-policy-scope-select">
          <option value="agent">agent</option>
          <option value="global">global</option>
        </select>
        <textarea id="agent-policy-allow-input"></textarea>
        <textarea id="agent-policy-deny-input"></textarea>
        <button id="agent-policy-save-btn" type="button">save-policy</button>
        <button id="agent-policy-reset-btn" type="button">reset-policy</button>
        <p id="agent-policy-feedback">-</p>
        <p id="agent-tool-catalog-summary">-</p>
        <ul id="agent-effective-policy-list"></ul>

        <input id="agent-memory-local-toggle" type="checkbox" />
        <select id="agent-memory-backend-select">
          <option value="local">local</option>
          <option value="sqlite">sqlite</option>
        </select>
        <select id="agent-memory-compression-select">
          <option value="adaptive">adaptive</option>
          <option value="aggressive">aggressive</option>
        </select>
        <button id="agent-memory-save-btn" type="button">save-memory</button>
        <p id="agent-memory-feedback">-</p>

        <button id="agent-budget-refresh-btn" type="button">budget</button>
        <pre id="agent-budget-telemetry">-</pre>
        <input id="agent-max-prompt-input" type="range" value="2200" />
        <p id="agent-max-prompt-value">-</p>
        <input id="agent-max-history-input" type="range" value="14" />
        <p id="agent-max-history-value">-</p>
        <input id="agent-max-tools-input" type="range" value="6" />
        <p id="agent-max-tools-value">-</p>

        <button id="agent-runtime-refresh-btn" type="button">runtime</button>
        <p id="agent-runtime-summary">-</p>
        <p id="agent-runtime-framework-meta">-</p>
        <ul id="agent-runtime-diagnostics-list"></ul>
        <select id="agent-runtime-log-mode">
          <option value="channel">channel</option>
          <option value="plugin">plugin</option>
        </select>
        <select id="agent-runtime-log-source"></select>
        <select id="agent-runtime-log-account"></select>
        <ul id="agent-runtime-log-list"></ul>
      </body>
    </html>`,
    { url: "http://localhost/" },
  );

  const previous = {
    window: globalThis.window,
    document: globalThis.document,
    HTMLElement: globalThis.HTMLElement,
    HTMLInputElement: globalThis.HTMLInputElement,
    HTMLSelectElement: globalThis.HTMLSelectElement,
    HTMLTextAreaElement: globalThis.HTMLTextAreaElement,
    Event: globalThis.Event,
    MouseEvent: globalThis.MouseEvent,
  };

  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.HTMLInputElement = dom.window.HTMLInputElement;
  globalThis.HTMLSelectElement = dom.window.HTMLSelectElement;
  globalThis.HTMLTextAreaElement = dom.window.HTMLTextAreaElement;
  globalThis.Event = dom.window.Event;
  globalThis.MouseEvent = dom.window.MouseEvent;

  function restore() {
    globalThis.window = previous.window;
    globalThis.document = previous.document;
    globalThis.HTMLElement = previous.HTMLElement;
    globalThis.HTMLInputElement = previous.HTMLInputElement;
    globalThis.HTMLSelectElement = previous.HTMLSelectElement;
    globalThis.HTMLTextAreaElement = previous.HTMLTextAreaElement;
    globalThis.Event = previous.Event;
    globalThis.MouseEvent = previous.MouseEvent;
    dom.window.close();
  }

  return { dom, restore };
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

class FakeControlPlaneBackend {
  constructor() {
    this.profile = "coding";
    this.agentConfig = {
      tool_policy: {
        profile: "coding",
        agent_overrides: {
          default: {
            allow: ["read_file", "list_dir"],
            deny: ["exec"],
          },
        },
      },
    };
    this.plugins = [
      {
        id: "memory",
        name: "Memory",
        class: "memory",
        aliases: ["context-memory"],
        capabilities: ["state", "summaries"],
        supports_lazy_load: true,
        docs: { summary: "Persistent memory hooks", help: "Stores local summaries." },
        config_schema: {
          type: "object",
          properties: {
            backend: { type: "string", enum: ["local", "sqlite"] },
            compression_strategy: { type: "string", enum: ["adaptive", "aggressive"] },
            local_enabled: { type: "boolean", description: "Persist local summaries" },
          },
        },
        enabled: true,
        configured: true,
        loaded: false,
        health: "idle",
        errors: [],
        config: { backend: "local", compression_strategy: "adaptive", local_enabled: true },
      },
      {
        id: "auth",
        name: "Auth Plugins",
        class: "auth",
        aliases: ["identity"],
        capabilities: ["credentials"],
        supports_lazy_load: true,
        docs: { summary: "Auth provider bridge", help: "Handles token broker config." },
        config_schema: {
          type: "object",
          properties: {
            endpoint: { type: "string" },
            enabled: { type: "boolean", description: "Route auth through broker" },
          },
        },
        enabled: false,
        configured: false,
        loaded: false,
        health: "disabled",
        errors: [],
        config: {},
      },
    ];
    this.runtimeLogs = [
      {
        timestamp: "2026-03-06T12:00:00Z",
        channel: "telegram",
        account_id: "bot-a",
        action: "probe",
        result: "healthy",
      },
    ];
    this.channels = [
      {
        id: "telegram",
        name: "Telegram",
        accounts: [
          {
            account_id: "bot-a",
            health_state: { status: "healthy" },
          },
        ],
      },
    ];
  }

  async fetchJson(path, options = {}) {
    const method = options.method || "GET";
    const body = options.body ? JSON.parse(options.body) : null;

    if (path === "/agent/plugins" && method === "GET") {
      return clone(this.plugins);
    }

    if (path === "/agent/plugins/enable" && method === "POST") {
      const plugin = this.plugins.find((entry) => entry.id === body.plugin_id);
      plugin.enabled = true;
      plugin.health = plugin.configured ? "idle" : "unknown";
      return clone(plugin);
    }

    if (path === "/agent/plugins/disable" && method === "POST") {
      const plugin = this.plugins.find((entry) => entry.id === body.plugin_id);
      plugin.enabled = false;
      plugin.health = "disabled";
      return clone(plugin);
    }

    if (path === "/agent/plugins/config" && method === "POST") {
      const plugin = this.plugins.find((entry) => entry.id === body.plugin_id);
      plugin.enabled = body.enabled ?? plugin.enabled;
      plugin.config = clone(body.config || {});
      plugin.configured = true;
      plugin.health = plugin.enabled ? "idle" : "disabled";
      return clone(plugin);
    }

    if (path === "/agent/tools/catalog" && method === "GET") {
      return {
        profiles: [
          { id: "minimal", tools: ["read_file"] },
          { id: "coding", tools: ["read_file", "list_dir"] },
          { id: "full", tools: ["read_file", "list_dir", "exec"] },
        ],
        entries: [{ name: "read_file" }, { name: "list_dir" }, { name: "exec" }],
      };
    }

    if (path === "/agent/tools/effective-policy" && method === "GET") {
      return {
        profile: this.profile,
        agent_id: "default",
        entries: [
          {
            name: "read_file",
            section: "filesystem",
            risk: "low",
            description: "Read file",
            implemented: true,
            allowed: true,
            final_rule: `profile:${this.profile}`,
          },
          {
            name: "exec",
            section: "execution",
            risk: "high",
            description: "Exec command",
            implemented: true,
            allowed: this.profile === "full",
            final_rule: this.profile === "full" ? "profile:full" : "agent:deny:exec",
          },
        ],
      };
    }

    if (path === "/agent/tools/profile" && method === "POST") {
      this.profile = body.profile;
      this.agentConfig.tool_policy.profile = body.profile;
      return this.fetchJson("/agent/tools/effective-policy", { method: "GET" });
    }

    if (path === "/agent/tools/allow-deny" && method === "POST") {
      this.agentConfig.tool_policy.agent_overrides.default = {
        allow: clone(body.allow || []),
        deny: clone(body.deny || []),
      };
      return this.fetchJson("/agent/tools/effective-policy", { method: "GET" });
    }

    if (path === "/agent/config" && method === "GET") {
      return clone(this.agentConfig);
    }

    if (path === "/health" && method === "GET") {
      return { status: "ok", provider: "mlx" };
    }

    if (path === "/config" && method === "GET") {
      return { active_agent_framework: "openclaw" };
    }

    if (path === "/openclaw/runtime" && method === "GET") {
      return { service_status: "running", service_state: "ready", rpc_ok: true, pid: 777 };
    }

    if (path === "/agent/channels/status" && method === "GET") {
      return clone(this.channels);
    }

    if (path.startsWith("/agent/channels/logs?") && method === "GET") {
      return clone(this.runtimeLogs);
    }

    if (path === "/agent/context/budget" && method === "GET") {
      return {
        session_id: "sess-1",
        provider_id: "mlx",
        model_id: "qwen2.5",
        model_profile: "small_local",
        tool_profile: this.profile,
        max_prompt_tokens: 2200,
        prompt_tokens_estimate: 1180,
        prompt_tokens_before_compression: 1320,
        history_messages_total: 12,
        history_messages_used: 8,
        summarized_messages: 4,
        summary_entries: 2,
        tools_considered: 6,
        tools_in_prompt: 4,
        critical: false,
        response_style: "concise",
        last_updated: "2026-03-06T12:15:00Z",
      };
    }

    throw new Error(`Unhandled route: ${method} ${path}`);
  }
}

function collectElements(document) {
  return {
    agentPluginsRefreshBtn: document.getElementById("agent-plugins-refresh-btn"),
    agentPluginsList: document.getElementById("agent-plugins-list"),
    agentPluginDetailTitle: document.getElementById("agent-plugin-detail-title"),
    agentPluginDetailMeta: document.getElementById("agent-plugin-detail-meta"),
    agentPluginConfigEnabled: document.getElementById("agent-plugin-config-enabled"),
    agentPluginConfigForm: document.getElementById("agent-plugin-config-form"),
    agentPluginSaveBtn: document.getElementById("agent-plugin-save-btn"),
    agentPluginResetBtn: document.getElementById("agent-plugin-reset-btn"),
    agentPluginFeedback: document.getElementById("agent-plugin-feedback"),
    agentToolProfileSelect: document.getElementById("agent-tool-profile-select"),
    agentToolProfileApplyBtn: document.getElementById("agent-tool-profile-apply-btn"),
    agentPolicyScopeSelect: document.getElementById("agent-policy-scope-select"),
    agentPolicyAllowInput: document.getElementById("agent-policy-allow-input"),
    agentPolicyDenyInput: document.getElementById("agent-policy-deny-input"),
    agentPolicySaveBtn: document.getElementById("agent-policy-save-btn"),
    agentPolicyResetBtn: document.getElementById("agent-policy-reset-btn"),
    agentPolicyFeedback: document.getElementById("agent-policy-feedback"),
    agentToolCatalogSummary: document.getElementById("agent-tool-catalog-summary"),
    agentEffectivePolicyList: document.getElementById("agent-effective-policy-list"),
    agentMemoryLocalToggle: document.getElementById("agent-memory-local-toggle"),
    agentMemoryBackendSelect: document.getElementById("agent-memory-backend-select"),
    agentMemoryCompressionSelect: document.getElementById("agent-memory-compression-select"),
    agentMemorySaveBtn: document.getElementById("agent-memory-save-btn"),
    agentMemoryFeedback: document.getElementById("agent-memory-feedback"),
    agentBudgetRefreshBtn: document.getElementById("agent-budget-refresh-btn"),
    agentBudgetTelemetry: document.getElementById("agent-budget-telemetry"),
    agentMaxPromptInput: document.getElementById("agent-max-prompt-input"),
    agentMaxPromptValue: document.getElementById("agent-max-prompt-value"),
    agentMaxHistoryInput: document.getElementById("agent-max-history-input"),
    agentMaxHistoryValue: document.getElementById("agent-max-history-value"),
    agentMaxToolsInput: document.getElementById("agent-max-tools-input"),
    agentMaxToolsValue: document.getElementById("agent-max-tools-value"),
    agentRuntimeRefreshBtn: document.getElementById("agent-runtime-refresh-btn"),
    agentRuntimeSummary: document.getElementById("agent-runtime-summary"),
    agentRuntimeFrameworkMeta: document.getElementById("agent-runtime-framework-meta"),
    agentRuntimeDiagnosticsList: document.getElementById("agent-runtime-diagnostics-list"),
    agentRuntimeLogMode: document.getElementById("agent-runtime-log-mode"),
    agentRuntimeLogSource: document.getElementById("agent-runtime-log-source"),
    agentRuntimeLogAccount: document.getElementById("agent-runtime-log-account"),
    agentRuntimeLogList: document.getElementById("agent-runtime-log-list"),
  };
}

async function flushUi() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

test("control plane smoke saves memory plugin config, switches profile, and renders runtime health", async () => {
  const { dom, restore } = createFixtureDom();
  const backend = new FakeControlPlaneBackend();
  let statusMessage = "";
  let toolPolicyRefreshes = 0;

  try {
    const controller = createAgentControlPlaneController({
      elements: collectElements(dom.window.document),
      fetchJson: backend.fetchJson.bind(backend),
      onStatus: (message) => {
        statusMessage = message;
      },
      onToolPolicyChanged: async () => {
        toolPolicyRefreshes += 1;
      },
      loadAgentConfig: async () => backend.fetchJson("/agent/config", { method: "GET" }),
    });

    await controller.loadPlugins();
    await controller.loadToolPolicies(await backend.fetchJson("/agent/config", { method: "GET" }));
    await controller.loadRuntimeHealth();

    assert.match(dom.window.document.getElementById("agent-plugins-list").textContent, /Memory/);
    assert.match(dom.window.document.getElementById("agent-runtime-summary").textContent, /plugins ativos 1/);
    assert.match(dom.window.document.getElementById("agent-runtime-log-list").textContent, /telegram:bot-a/);

    dom.window.document.getElementById("agent-memory-local-toggle").checked = false;
    dom.window.document.getElementById("agent-memory-backend-select").value = "sqlite";
    dom.window.document.getElementById("agent-memory-compression-select").value = "aggressive";
    dom.window.document.getElementById("agent-memory-save-btn").click();
    await flushUi();

    assert.equal(backend.plugins.find((entry) => entry.id === "memory").enabled, false);
    assert.equal(backend.plugins.find((entry) => entry.id === "memory").config.backend, "sqlite");
    assert.match(dom.window.document.getElementById("agent-memory-feedback").textContent, /backend sqlite/);

    dom.window.document.getElementById("agent-tool-profile-select").value = "full";
    dom.window.document.getElementById("agent-tool-profile-apply-btn").click();
    await flushUi();

    assert.equal(backend.profile, "full");
    assert.equal(toolPolicyRefreshes, 1);
    assert.match(dom.window.document.getElementById("agent-policy-feedback").textContent, /Profile full aplicado/);
    assert.match(dom.window.document.getElementById("agent-effective-policy-list").textContent, /profile:full/);
    assert.match(statusMessage, /Profile full aplicado/);
  } finally {
    restore();
  }
});
