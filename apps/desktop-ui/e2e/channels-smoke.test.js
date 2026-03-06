import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";

import { createAgentChannelsController } from "../ui/agent-channels.js";

const CHANNEL_PROTOCOL_HEADER = "x-channel-protocol-version";

function buildChannel({
  id,
  name,
  protocolFamily,
  accounts,
  defaultAccountId = null,
  ambiguityWarning = null,
}) {
  return {
    id,
    name,
    protocol_family: protocolFamily,
    protocol_version: "v1",
    protocol_schema: { family: protocolFamily, protocol_version: "v1" },
    aliases: [id],
    capabilities: ["probe", "resolve", "send"],
    supports_lazy_load: true,
    docs: {
      summary: `${name} docs`,
      help_url: `https://example.test/${id}`,
      examples: [],
    },
    config_schema: {},
    default_account_id: defaultAccountId,
    ambiguity_warning: ambiguityWarning,
    accounts,
  };
}

function buildAccount(accountId, overrides = {}) {
  return {
    account_id: accountId,
    enabled: overrides.enabled ?? true,
    configured: true,
    is_default: overrides.is_default ?? false,
    credentials_ref: overrides.credentials_ref ?? `vault://${accountId}`,
    metadata: overrides.metadata ?? {},
    routing_defaults: overrides.routing_defaults ?? {},
    health_state: overrides.health_state ?? { status: "healthy" },
    limits: overrides.limits ?? {},
    adapter_config: overrides.adapter_config ?? {},
    session: overrides.session ?? { status: "idle" },
    capabilities: overrides.capabilities ?? ["probe", "resolve", "send"],
  };
}

function createFixtureDom() {
  const dom = new JSDOM(
    `<!doctype html>
    <html>
      <body>
        <button id="agent-channels-refresh-btn" type="button">refresh</button>
        <select id="agent-channel-select"></select>
        <input id="agent-channel-account-id" />
        <textarea id="agent-channel-credentials"></textarea>
        <textarea id="agent-channel-metadata"></textarea>
        <textarea id="agent-channel-routing-defaults"></textarea>
        <input id="agent-channel-enabled" type="checkbox" checked />
        <input id="agent-channel-set-default" type="checkbox" />
        <button id="agent-channel-save-btn" type="button">save</button>
        <button id="agent-channel-clear-btn" type="button">clear</button>
        <p id="agent-channel-form-feedback">-</p>

        <select id="agent-send-channel"></select>
        <select id="agent-send-account"></select>
        <input id="agent-send-target" />
        <input id="agent-send-message" />
        <button id="agent-send-test-btn" type="button">send</button>
        <button id="agent-probe-channel-btn" type="button">probe</button>
        <button id="agent-resolve-target-btn" type="button">resolve</button>
        <p id="agent-channel-action-feedback">-</p>

        <div id="agent-channel-list"></div>

        <button id="agent-channel-logs-refresh-btn" type="button">logs</button>
        <select id="agent-channel-logs-channel"></select>
        <select id="agent-channel-logs-account"></select>
        <ul id="agent-channel-logs-list"></ul>
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
    URLSearchParams: globalThis.URLSearchParams,
  };

  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Event = dom.window.Event;
  globalThis.MouseEvent = dom.window.MouseEvent;
  globalThis.URLSearchParams = dom.window.URLSearchParams;

  function restore() {
    globalThis.window = previous.window;
    globalThis.document = previous.document;
    globalThis.HTMLElement = previous.HTMLElement;
    globalThis.Event = previous.Event;
    globalThis.MouseEvent = previous.MouseEvent;
    globalThis.URLSearchParams = previous.URLSearchParams;
    dom.window.close();
  }

  return { dom, restore };
}

function clone(value) {
  return JSON.parse(JSON.stringify(value));
}

class FakeChannelBackend {
  constructor() {
    this.channels = [
      buildChannel({
        id: "whatsapp",
        name: "WhatsApp",
        protocolFamily: "native_runtime_v1",
        defaultAccountId: "work",
        accounts: [
          buildAccount("work", {
            is_default: true,
            session: { status: "linked" },
            metadata: { profile: "work" },
          }),
          buildAccount("personal", {
            session: { status: "linked" },
            metadata: { profile: "personal" },
          }),
        ],
      }),
      buildChannel({
        id: "signal",
        name: "Signal",
        protocolFamily: "bridge_http_v1",
        ambiguityWarning: "Mais de uma conta ativa sem default_account_id.",
        accounts: [
          buildAccount("ops", { session: { status: "linked" } }),
          buildAccount("sales", { session: { status: "linked" } }),
        ],
      }),
      buildChannel({
        id: "telegram",
        name: "Telegram",
        protocolFamily: "token_bot_v1",
        accounts: [buildAccount("bot-a", { session: { status: "linked" } })],
      }),
    ];
    this.logs = [
      {
        timestamp: "2026-03-06T10:00:00Z",
        channel: "signal",
        account_id: "ops",
        action: "probe",
        result: "error",
        error_code: "permission_error",
        error: "missing chat.write scope",
      },
    ];
  }

  assertProtocol(path, options = {}) {
    if (!path.startsWith("/agent/channels") && path !== "/agent/message/send") {
      return;
    }
    const headers = options.headers || {};
    assert.equal(headers[CHANNEL_PROTOCOL_HEADER], "v1", `missing protocol header for ${path}`);
  }

  findChannel(channelId) {
    const channel = this.channels.find((entry) => entry.id === channelId);
    assert.ok(channel, `channel ${channelId} must exist`);
    return channel;
  }

  findAccount(channelId, accountId) {
    const channel = this.findChannel(channelId);
    const account = channel.accounts.find((entry) => entry.account_id === accountId);
    assert.ok(account, `account ${channelId}:${accountId} must exist`);
    return { channel, account };
  }

  resolveAccount(channelId, requestedAccountId) {
    const channel = this.findChannel(channelId);
    if (requestedAccountId) {
      const account = channel.accounts.find((entry) => entry.account_id === requestedAccountId);
      if (!account) {
        throw new Error(`invalid_request: account_id '${requestedAccountId}' not found`);
      }
      return account;
    }
    if (channel.default_account_id) {
      return channel.accounts.find((entry) => entry.account_id === channel.default_account_id);
    }
    const active = channel.accounts.filter((entry) => entry.enabled);
    if (active.length === 1) {
      return active[0];
    }
    throw new Error(
      `invalid_request: ambiguous account_id; available accounts: ${active.map((entry) => entry.account_id).join(", ")}`,
    );
  }

  appendLog(entry) {
    this.logs.unshift({
      timestamp: "2026-03-06T10:10:00Z",
      protocol_version: "v1",
      ...entry,
    });
  }

  async fetchJson(path, options = {}) {
    this.assertProtocol(path, options);

    if (path === "/agent/channels" && options.method === "GET") {
      return clone(this.channels);
    }

    if (path.startsWith("/agent/channels/logs?") && options.method === "GET") {
      const query = new URL(path, "http://localhost").searchParams;
      const channel = query.get("channel");
      const accountId = query.get("account_id");
      return clone(
        this.logs.filter((entry) => {
          if (channel && entry.channel !== channel) return false;
          if (accountId && entry.account_id !== accountId) return false;
          return true;
        }),
      );
    }

    const payload = options.body ? JSON.parse(options.body) : {};

    if (path === "/agent/channels/upsert-account" && options.method === "POST") {
      const channel = this.findChannel(payload.channel);
      let account = channel.accounts.find((entry) => entry.account_id === payload.account_id);
      if (!account) {
        account = buildAccount(payload.account_id, {
          metadata: payload.metadata || {},
          routing_defaults: payload.routing_defaults || {},
          session: { status: "idle" },
        });
        channel.accounts.push(account);
      }
      account.enabled = payload.enabled ?? true;
      account.metadata = payload.metadata || {};
      account.routing_defaults = payload.routing_defaults || {};
      if (payload.credentials_ref) {
        account.credentials_ref = payload.credentials_ref;
      } else if (payload.credentials) {
        account.credentials_ref = `vault://${payload.channel}/${payload.account_id}`;
      }
      if (payload.set_as_default) {
        channel.default_account_id = payload.account_id;
        channel.accounts.forEach((entry) => {
          entry.is_default = entry.account_id === payload.account_id;
        });
      }
      this.appendLog({
        channel: payload.channel,
        account_id: payload.account_id,
        action: "upsert_account",
        result: "success",
      });
      return {
        channel: payload.channel,
        account_id: payload.account_id,
        status: "saved",
      };
    }

    if (path === "/agent/channels/remove-account" && options.method === "POST") {
      const channel = this.findChannel(payload.channel);
      channel.accounts = channel.accounts.filter((entry) => entry.account_id !== payload.account_id);
      if (channel.default_account_id === payload.account_id) {
        channel.default_account_id = null;
      }
      this.appendLog({
        channel: payload.channel,
        account_id: payload.account_id,
        action: "remove_account",
        result: "success",
      });
      return { status: "removed" };
    }

    if (path === "/agent/channels/login" && options.method === "POST") {
      const { channel, account } = this.findAccount(payload.channel, payload.account_id);
      account.session.status = "linked";
      this.appendLog({
        channel: channel.id,
        account_id: account.account_id,
        action: "login",
        result: "success",
      });
      return {
        channel: channel.id,
        account_id: account.account_id,
        protocol_family: channel.protocol_family,
        protocol_version: "v1",
        status: "linked",
        message: "linked",
        details: {},
      };
    }

    if (path === "/agent/channels/logout" && options.method === "POST") {
      const { channel, account } = this.findAccount(payload.channel, payload.account_id);
      account.session.status = "logged_out";
      this.appendLog({
        channel: channel.id,
        account_id: account.account_id,
        action: "logout",
        result: "success",
      });
      return {
        channel: channel.id,
        account_id: account.account_id,
        protocol_family: channel.protocol_family,
        protocol_version: "v1",
        status: "logged_out",
        message: "logged_out",
        details: {},
      };
    }

    if (path === "/agent/channels/probe" && options.method === "POST") {
      const channel = this.findChannel(payload.channel);
      const accounts = payload.all_accounts
        ? channel.accounts
        : [this.resolveAccount(payload.channel, payload.account_id)];
      const result = accounts.map((account) => ({
        channel: payload.channel,
        account_id: account.account_id,
        protocol_family: channel.protocol_family,
        protocol_version: "v1",
        status: "healthy",
        message: "probe_ok",
        details: {},
      }));
      accounts.forEach((account) => {
        this.appendLog({
          channel: payload.channel,
          account_id: account.account_id,
          action: "probe",
          result: "success",
        });
      });
      return result;
    }

    if (path === "/agent/channels/resolve" && options.method === "POST") {
      const channel = this.findChannel(payload.channel);
      const account = this.resolveAccount(payload.channel, payload.account_id);
      this.appendLog({
        channel: payload.channel,
        account_id: account.account_id,
        action: "resolve",
        result: "success",
      });
      return {
        channel: payload.channel,
        account_id: account.account_id,
        protocol_family: channel.protocol_family,
        protocol_version: "v1",
        requested_target: payload.target,
        resolved_target: `canonical:${payload.target}`,
        status: "resolved",
      };
    }

    if (path === "/agent/message/send" && options.method === "POST") {
      const channel = this.findChannel(payload.channel);
      const account = this.resolveAccount(payload.channel, payload.account_id);
      this.appendLog({
        channel: payload.channel,
        account_id: account.account_id,
        action: "send",
        result: "success",
      });
      return {
        channel: payload.channel,
        account_id: account.account_id,
        protocol_family: channel.protocol_family,
        protocol_version: "v1",
        target: payload.target,
        message_id: `${payload.channel}-${account.account_id}-msg-1`,
        status: "sent",
      };
    }

    throw new Error(`unhandled route: ${options.method || "GET"} ${path}`);
  }
}

function collectElements(document) {
  return {
    agentChannelsRefreshBtn: document.getElementById("agent-channels-refresh-btn"),
    agentChannelSelect: document.getElementById("agent-channel-select"),
    agentChannelAccountIdInput: document.getElementById("agent-channel-account-id"),
    agentChannelCredentialsInput: document.getElementById("agent-channel-credentials"),
    agentChannelMetadataInput: document.getElementById("agent-channel-metadata"),
    agentChannelRoutingDefaultsInput: document.getElementById("agent-channel-routing-defaults"),
    agentChannelEnabledToggle: document.getElementById("agent-channel-enabled"),
    agentChannelSetDefaultToggle: document.getElementById("agent-channel-set-default"),
    agentChannelSaveBtn: document.getElementById("agent-channel-save-btn"),
    agentChannelClearBtn: document.getElementById("agent-channel-clear-btn"),
    agentChannelFormFeedback: document.getElementById("agent-channel-form-feedback"),
    agentSendChannelSelect: document.getElementById("agent-send-channel"),
    agentSendAccountSelect: document.getElementById("agent-send-account"),
    agentSendTargetInput: document.getElementById("agent-send-target"),
    agentSendMessageInput: document.getElementById("agent-send-message"),
    agentSendTestBtn: document.getElementById("agent-send-test-btn"),
    agentProbeChannelBtn: document.getElementById("agent-probe-channel-btn"),
    agentResolveTargetBtn: document.getElementById("agent-resolve-target-btn"),
    agentChannelActionFeedback: document.getElementById("agent-channel-action-feedback"),
    agentChannelList: document.getElementById("agent-channel-list"),
    agentChannelLogsRefreshBtn: document.getElementById("agent-channel-logs-refresh-btn"),
    agentChannelLogsChannelSelect: document.getElementById("agent-channel-logs-channel"),
    agentChannelLogsAccountSelect: document.getElementById("agent-channel-logs-account"),
    agentChannelLogsList: document.getElementById("agent-channel-logs-list"),
  };
}

async function flushUi() {
  await new Promise((resolve) => setTimeout(resolve, 0));
  await new Promise((resolve) => setTimeout(resolve, 0));
}

test("channels smoke renders catalog, supports add/default, and shows logs", async () => {
  const { dom, restore } = createFixtureDom();
  const backend = new FakeChannelBackend();

  try {
    const controller = createAgentChannelsController({
      elements: collectElements(dom.window.document),
      fetchJson: backend.fetchJson.bind(backend),
      promptText: async () => null,
      confirmAction: async () => true,
    });

    await controller.loadChannels();

    const listText = dom.window.document.getElementById("agent-channel-list").textContent;
    assert.match(listText, /WhatsApp/);
    assert.match(listText, /Signal/);
    assert.match(listText, /family: native_runtime_v1/);
    assert.match(listText, /Mais de uma conta ativa sem default_account_id/);

    const logsText = dom.window.document.getElementById("agent-channel-logs-list").textContent;
    assert.match(logsText, /permission_error/);
    assert.match(logsText, /signal:ops/);

    const channelSelect = dom.window.document.getElementById("agent-channel-select");
    channelSelect.value = "telegram";
    dom.window.document.getElementById("agent-channel-account-id").value = "bot-b";
    dom.window.document.getElementById("agent-channel-credentials").value = '{"token":"123:abc"}';
    dom.window.document.getElementById("agent-channel-metadata").value = '{"workspace":"ops"}';
    dom.window.document.getElementById("agent-channel-routing-defaults").value = '{"target":"@alerts"}';
    dom.window.document.getElementById("agent-channel-set-default").checked = true;
    dom.window.document.getElementById("agent-channel-save-btn").click();
    await flushUi();

    const formFeedback = dom.window.document.getElementById("agent-channel-form-feedback").textContent;
    assert.match(formFeedback, /Conta telegram:bot-b salva/);

    const telegramSection = dom.window.document.getElementById("agent-channel-list").textContent;
    assert.match(telegramSection, /bot-b/);
    assert.match(telegramSection, /\(default\)/);
  } finally {
    restore();
  }
});

test("channels smoke exercises login, probe, resolve, send, logout, and ambiguity handling", async () => {
  const { dom, restore } = createFixtureDom();
  const backend = new FakeChannelBackend();
  const promptCalls = [];

  try {
    const controller = createAgentChannelsController({
      elements: collectElements(dom.window.document),
      fetchJson: backend.fetchJson.bind(backend),
      promptText: async (payload) => {
        promptCalls.push(payload);
        return "work-renamed";
      },
      confirmAction: async () => true,
    });

    await controller.loadChannels();

    const loginButton = dom.window.document.querySelector('[data-channel-action="login"][data-channel="whatsapp"][data-account="work"]');
    loginButton.click();
    await flushUi();
    assert.match(dom.window.document.getElementById("agent-channel-action-feedback").textContent, /whatsapp:work • linked/);

    dom.window.document.getElementById("agent-send-channel").value = "whatsapp";
    dom.window.document.getElementById("agent-send-channel").dispatchEvent(new dom.window.Event("change", { bubbles: true }));
    dom.window.document.getElementById("agent-send-account").value = "personal";
    dom.window.document.getElementById("agent-send-target").value = "@client";
    dom.window.document.getElementById("agent-send-message").value = "hello";

    dom.window.document.getElementById("agent-probe-channel-btn").click();
    await flushUi();
    assert.match(dom.window.document.getElementById("agent-channel-action-feedback").textContent, /personal:healthy/);

    dom.window.document.getElementById("agent-resolve-target-btn").click();
    await flushUi();
    assert.match(dom.window.document.getElementById("agent-channel-action-feedback").textContent, /canonical:@client/);

    dom.window.document.getElementById("agent-send-test-btn").click();
    await flushUi();
    assert.match(dom.window.document.getElementById("agent-channel-action-feedback").textContent, /Mensagem enviada via whatsapp:personal/);

    const renameButton = dom.window.document.querySelector('[data-channel-action="rename"][data-channel="whatsapp"][data-account="work"]');
    renameButton.click();
    await flushUi();
    assert.equal(promptCalls.length, 1);
    assert.match(dom.window.document.getElementById("agent-channel-list").textContent, /work-renamed/);

    const logoutButton = dom.window.document.querySelector('[data-channel-action="logout"][data-channel="whatsapp"][data-account="personal"]');
    logoutButton.click();
    await flushUi();
    assert.match(dom.window.document.getElementById("agent-channel-action-feedback").textContent, /whatsapp:personal • logged_out/);

    dom.window.document.getElementById("agent-send-channel").value = "signal";
    dom.window.document.getElementById("agent-send-channel").dispatchEvent(new dom.window.Event("change", { bubbles: true }));
    dom.window.document.getElementById("agent-send-account").value = "";
    dom.window.document.getElementById("agent-send-target").value = "#ops";
    dom.window.document.getElementById("agent-send-message").value = "ambiguous";
    dom.window.document.getElementById("agent-send-test-btn").click();
    await flushUi();
    assert.match(dom.window.document.getElementById("agent-channel-action-feedback").textContent, /Falha no envio: invalid_request: ambiguous account_id/);

    await controller.loadLogs();
    const logsText = dom.window.document.getElementById("agent-channel-logs-list").textContent;
    assert.match(logsText, /send/);
    assert.match(logsText, /whatsapp:personal/);
  } finally {
    restore();
  }
});
