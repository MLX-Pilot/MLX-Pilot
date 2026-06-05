import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { TextDecoder, TextEncoder } from "node:util";
import { JSDOM } from "jsdom";

const indexHtml = await readFile(new URL("../ui/index.html", import.meta.url), "utf8");
const appJs = await readFile(new URL("../ui/app.js", import.meta.url), "utf8");

function jsonResponse(data, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    async text() {
      return data == null ? "" : JSON.stringify(data);
    },
    async json() {
      return data;
    },
  };
}

function streamingResponse(lines, status = 200) {
  const encoder = new TextEncoder();
  const chunks = lines.map((line) => encoder.encode(`${line}\n`));
  let index = 0;
  return {
    ok: status >= 200 && status < 300,
    status,
    body: {
      getReader() {
        return {
          async read() {
            if (index >= chunks.length) return { done: true, value: undefined };
            return { done: false, value: chunks[index++] };
          },
        };
      },
    },
    async text() {
      return lines.join("\n");
    },
    async json() {
      return JSON.parse(lines[lines.length - 1] || "{}");
    },
  };
}

async function flush(count = 4) {
  for (let index = 0; index < count; index += 1) {
    await new Promise((resolve) => setImmediate(resolve));
  }
}

function createFixture({
  modelsResponse,
  cachedModels,
  cachedCurrentModel,
  agentConfigResponse,
  environmentResponseHidden,
  environmentResponseRevealed,
} = {}) {
  let sessions = [
    { id: "sess-1", name: "Operacao", message_count: 3 },
    { id: "sess-2", name: "Channels", message_count: 1 },
  ];
  let nextSession = 3;
  const fetchCalls = [];
  const hiddenEnvironment = environmentResponseHidden ?? { variables: [] };
  const revealedEnvironment = environmentResponseRevealed ?? hiddenEnvironment;

  const dom = new JSDOM(indexHtml, {
    url: "http://localhost/",
    runScripts: "outside-only",
    pretendToBeVisual: true,
  });

  const { window } = dom;
  const { document } = window;
  const nativeCalls = [];

  window.TextEncoder = TextEncoder;
  window.TextDecoder = TextDecoder;

  window.__MLX_PILOT_DAEMON_URL__ = "http://127.0.0.1:11436";
  window.localStorage.setItem("mlxPilotDaemonUrl", "http://127.0.0.1:11435");
  if (cachedModels) {
    window.localStorage.setItem("mlxPilotModelCache", JSON.stringify(cachedModels));
  }
  if (cachedCurrentModel) {
    window.localStorage.setItem("mlxPilotCurrentModel", cachedCurrentModel);
  }

  Object.defineProperty(window.HTMLCanvasElement.prototype, "getContext", {
    configurable: true,
    value() {
      return {
        beginPath() {},
        moveTo() {},
        lineTo() {},
        stroke() {},
        arc() {},
        fill() {},
        clearRect() {},
      };
    },
  });

  window.requestAnimationFrame = () => 1;
  window.cancelAnimationFrame = () => {};
  window.setTimeout = (callback) => {
    callback();
    return 1;
  };
  window.clearTimeout = () => {};
  window.alert = () => {};
  window.confirm = () => true;
  window.prompt = () => "slack";
  window.open = () => {};
  window.__TAURI__ = {
    core: {
      async invoke(command, args = {}) {
        nativeCalls.push({ command, args });
        if (command === "desktop_runtime_info") {
          return {
            daemon_url: "http://127.0.0.1:11436",
            embedded_daemon_enabled: true,
            log_path: "C:/Users/kaike/AppData/Local/MLX Pilot/logs/desktop.log",
            pid: 4242,
          };
        }
        if (command === "desktop_log_snapshot") {
          return {
            path: "C:/Users/kaike/AppData/Local/MLX Pilot/logs/desktop.log",
            entries: [
              "1778200000000 [INFO] desktop shell starting",
              "1778200001000 [INFO] embedded daemon binding to http://127.0.0.1:11436",
            ],
          };
        }
        if (command === "desktop_log_append" || command === "desktop_log_clear") {
          return null;
        }
        throw new Error(`Unhandled native command: ${command}`);
      },
    },
  };

  Object.defineProperty(window.navigator, "clipboard", {
    configurable: true,
    value: {
      async writeText() {},
    },
  });

  window.fetch = async (url, options = {}) => {
    const requestUrl = new URL(url, "http://localhost/");
    const path = `${requestUrl.pathname}${requestUrl.search}`;
    const method = options.method || "GET";
    const body = options.body ? JSON.parse(options.body) : null;

    fetchCalls.push({ method, path, body, url: requestUrl.toString() });

    if (path === "/health") {
      return jsonResponse({ status: "ok", provider: "ollama" });
    }

    if (path === "/config") {
      return jsonResponse({
        models_dir: "G:/models",
      });
    }

    if (path === "/models") {
      return jsonResponse(modelsResponse ?? [
        {
          id: "ollama::qwen3.5:9b",
          name: "qwen3.5:9b [Ollama]",
          provider: "ollama",
          is_available: true,
          agent_tool_mode: "tool_ready",
          agent_recommended: true,
        },
        {
          id: "ollama::deepseek-r1:8b",
          name: "deepseek-r1:8b [Ollama]",
          provider: "ollama",
          is_available: true,
          agent_tool_mode: "chat_only",
        },
        {
          id: "mlx-community/Qwen3-4B-4bit",
          name: "Qwen3 4B [MLX]",
          provider: "mlx",
          is_available: true,
          agent_tool_mode: "chat_only",
        },
      ]);
    }

    if (path === "/agent/config" && method === "GET") {
      return jsonResponse(agentConfigResponse ?? {
        provider: "ollama",
        model_id: "qwen3.5:9b",
        execution_mode: "full",
        approval_mode: "ask",
        provider_profiles: [
          {
            id: "ollama-local",
            provider: "ollama",
            model_id: "qwen3.5:9b",
            base_url: "http://127.0.0.1:11434",
            api_key_ref: null,
            custom_headers: {},
            runtime_variant: "classic",
          },
        ],
      });
    }

    if (path === "/agent/config" && method === "POST") {
      return jsonResponse(body);
    }

    if (path === "/agent/sessions" && method === "GET") {
      return jsonResponse(sessions);
    }

    if (path === "/agent/sessions" && method === "POST") {
      const created = {
        id: `sess-${nextSession}`,
        name: body?.name || `Sessao ${nextSession}`,
        message_count: 0,
      };
      nextSession += 1;
      sessions = [created, ...sessions];
      return jsonResponse(created);
    }

    if (path === "/agent/plugins") {
      return jsonResponse([
        { id: "memory", enabled: true, description: "Persistencia local" },
        { id: "auth", enabled: false, description: "Broker de identidade" },
      ]);
    }

    if (path === "/agent/plugins/enable" || path === "/agent/plugins/disable") {
      return jsonResponse({});
    }

    if (path === "/agent/skills/check") {
      return jsonResponse({
        skills: [
          { name: "planner", active: true },
          { name: "channels", enabled: true },
          { name: "browser", active: false },
        ],
      });
    }

    if (path === "/agent/skills/enable" || path === "/agent/skills/disable") {
      return jsonResponse({});
    }

    if (path === "/agent/tools") {
      return jsonResponse([
        { name: "read_file", enabled: true },
        { name: "list_dir", enabled: true },
        { name: "glob", enabled: true },
        { name: "grep", enabled: true },
        { name: "exec", enabled: false },
      ]);
    }

    if (path === "/agent/channels") {
      return jsonResponse([
        { channel_id: "whatsapp", accounts: [{ account_id: "ops", status: "connected" }] },
        { channel_id: "slack", accounts: [] },
      ]);
    }

    if (path === "/agent/channels/upsert" || path === "/agent/channels/remove") {
      return jsonResponse({});
    }

    if (path === "/agent/audit?limit=30") {
      return jsonResponse({
        entries: [
          {
            event_type: "tool_call",
            tool_name: "read_file",
            summary: "Resumo de auditoria",
            timestamp: "2026-04-15T12:00:00Z",
          },
        ],
      });
    }

    if (path === "/environment?reveal=false") {
      return jsonResponse(hiddenEnvironment);
    }

    if (path === "/environment?reveal=true") {
      return jsonResponse(revealedEnvironment);
    }

    if (path === "/environment" && method === "POST") {
      return jsonResponse(hiddenEnvironment);
    }

    if (path === "/web/brave/search" && method === "POST") {
      return jsonResponse({
        query: body?.query || "",
        key_source: "env_file",
        results: [
          {
            title: "MLX Pilot release notes",
            url: "https://example.test/mlx-pilot",
            description: "Resultado atual usado para validar contexto de busca web.",
          },
        ],
      });
    }

    if (path === "/chat/stream" && method === "POST") {
      return streamingResponse([
        JSON.stringify({ event: "status", status: "thinking" }),
        JSON.stringify({ event: "thinking_delta", delta: "Mapeando estado inicial..." }),
        JSON.stringify({ event: "answer_delta", delta: "<think>Consolidando contexto interno.</think>\n## Diagnostico\n- Runtime conectado\n- Cache aquecido\n\n```js\nconsole.log('ok');\n```" }),
        JSON.stringify({ event: "done", status: "completed", total_tokens: 64, latency_ms: 320 }),
      ]);
    }

    if (path === "/chat" && method === "POST") {
      return jsonResponse({
        message: {
          content: "<think>Consolidando contexto interno.</think>\n## Diagnostico\n- Runtime conectado",
        },
        usage: {
          prompt_tokens: 30,
          completion_tokens: 10,
          total_tokens: 40,
        },
        latency_ms: 320,
      });
    }

    if (path === "/agent/stream" && method === "POST") {
      return streamingResponse([
        JSON.stringify({ event: "status", status: "thinking", session_id: body?.session_id || "sess-1" }),
        JSON.stringify({ event: "thinking_delta", delta: "Planejando...", session_id: body?.session_id || "sess-1" }),
        JSON.stringify({ event: "tool_call_started", tool: "read_file", session_id: body?.session_id || "sess-1" }),
        JSON.stringify({ event: "tool_call_completed", tool: "read_file", message: "ok", session_id: body?.session_id || "sess-1" }),
        JSON.stringify({ event: "answer_delta", delta: "<think>Validando politica final.</think>\n## Resposta do agent\n- Ajuste aplicado", session_id: body?.session_id || "sess-1" }),
        JSON.stringify({ event: "done", status: "completed", session_id: body?.session_id || "sess-1", total_tokens: 128, latency_ms: 900 }),
      ]);
    }

    if (path === "/agent/run" && method === "POST") {
      return jsonResponse({
        session_id: body?.session_id || "sess-1",
        final_response: "<think>Validando politica final.</think>\n## Resposta do agent\n- Ajuste aplicado",
        total_tokens: 128,
        latency_ms: 900,
      });
    }

    throw new Error(`Unhandled request: ${method} ${path}`);
  };

  window.eval(appJs);

  return {
    window,
    document,
    fetchCalls,
    nativeCalls,
    cleanup() {
      dom.window.close();
    },
  };
}

test("agent workspace boots with live summary and toggles config tab", async () => {
  const fixture = createFixture();

  try {
    await flush(10);

    assert.ok(fixture.fetchCalls.some((entry) => entry.url.startsWith("http://127.0.0.1:11436/")));
    assert.equal(fixture.document.getElementById("agent-daemon-status")?.textContent, "Online");
    assert.equal(fixture.document.getElementById("agent-session-count")?.textContent, "2");
    assert.equal(fixture.document.getElementById("agent-plugin-count")?.textContent, "1");
    assert.equal(fixture.document.getElementById("agent-skill-count")?.textContent, "2");
    assert.equal(fixture.document.getElementById("agent-channel-count")?.textContent, "2");
    assert.equal(fixture.document.getElementById("agent-audit-count")?.textContent, "1");

    fixture.document.querySelector('.tab[data-panel="agent"]')?.click();
    fixture.document.querySelector('.agent-view-tab[data-agent-view="config"]')?.click();
    await flush(2);

    assert.ok(fixture.document.getElementById("agent-view-config")?.classList.contains("active"));
    assert.equal(fixture.document.getElementById("agent-view-config")?.style.display, "block");
  } finally {
    fixture.cleanup();
  }
});

test("agent workspace shortcuts render deterministic diagnostics", async () => {
  const fixture = createFixture();

  try {
    await flush();

    fixture.document.querySelector(".agent-prompt-card")?.click();
    const input = fixture.document.getElementById("agent-command-input");
    assert.match(input?.value || "", /Revise a configuracao atual do agent/i);

    fixture.document.getElementById("agent-send-btn")?.click();
    await flush();

    assert.ok(!fixture.fetchCalls.some((entry) => entry.path === "/agent/stream"));
    assert.ok(fixture.fetchCalls.some((entry) => entry.path === "/agent/config"));
    assert.ok(fixture.fetchCalls.some((entry) => entry.path === "/agent/tools"));
    assert.match(fixture.document.getElementById("agent-chat-messages")?.textContent || "", /Runtime e politicas/);
    assert.match(fixture.document.getElementById("agent-chat-messages")?.textContent || "", /Tools ativas/);
    assert.match(fixture.document.getElementById("console-feed")?.textContent || "", /agent-shortcut/);
  } finally {
    fixture.cleanup();
  }
});

test("agent workspace freeform submits runs and creates sessions", async () => {
  const fixture = createFixture();

  try {
    await flush();

    const input = fixture.document.getElementById("agent-command-input");
    input.value = "Liste os arquivos do workspace.";
    input.dispatchEvent(new fixture.window.Event("input", { bubbles: true }));
    fixture.document.getElementById("agent-send-btn")?.click();
    await flush();

    const runCall = fixture.fetchCalls.find((entry) => entry.path === "/agent/stream");
    assert.ok(runCall);
    assert.equal(runCall.body.model_id, "ollama::qwen3.5:9b");
    assert.match(fixture.document.getElementById("agent-chat-messages")?.textContent || "", /Resposta do agent/);
    assert.match(fixture.document.getElementById("agent-chat-messages")?.textContent || "", /read_file/);
    assert.match(fixture.document.querySelector("#agent-chat-messages .thinking-content")?.textContent || "", /Validando politica final/);
    assert.match(fixture.document.getElementById("console-feed")?.textContent || "", /agent-tool/);
    const agentAssistantHtml = fixture.document.querySelector("#agent-chat-messages .assistant-message .msg-content")?.innerHTML || "";
    assert.match(agentAssistantHtml, /<h2>Resposta do agent<\/h2>/);
    assert.match(agentAssistantHtml, /<li>Ajuste aplicado<\/li>/);

    fixture.document.getElementById("btn-new-session")?.click();
    await flush();

    assert.equal(fixture.document.getElementById("agent-session-count")?.textContent, "3");
    assert.equal(fixture.document.getElementById("btn-export-session")?.disabled, false);
  } finally {
    fixture.cleanup();
  }
});

test("chat stream shows thinking and renders markdown output", async () => {
  const fixture = createFixture();

  try {
    await flush();

    const input = fixture.document.getElementById("chat-input");
    input.value = "Diagnostique o runtime atual";
    fixture.document.getElementById("send-btn")?.click();
    await flush(6);

    const streamCall = fixture.fetchCalls.find((entry) => entry.path === "/chat/stream");
    assert.ok(streamCall);
    assert.ok(fixture.fetchCalls.some((entry) => entry.path === "/web/brave/search"));
    assert.equal(streamCall.body.messages[0].role, "system");
    assert.match(streamCall.body.messages[0].content, /Contexto de busca web recente/);
    assert.match(streamCall.body.messages[0].content, /MLX Pilot release notes/);

    const thinkingText = fixture.document.querySelector("#chat-messages .assistant-message .thinking-content")?.textContent || "";
    const answerHtml = fixture.document.querySelector("#chat-messages .assistant-message .msg-content")?.innerHTML || "";

    assert.match(thinkingText, /Mapeando estado inicial/);
    assert.match(thinkingText, /Consolidando contexto interno/);
    assert.match(answerHtml, /<h2>Diagnostico<\/h2>/);
    assert.match(answerHtml, /<li>Runtime conectado<\/li>/);
    assert.match(answerHtml, /code-block/);
  } finally {
    fixture.cleanup();
  }
});

test("workspace preserves cached model shell when no installed model is returned", async () => {
  const fixture = createFixture({
    modelsResponse: [],
    cachedModels: [
      {
        id: "cached/qwen-local",
        name: "Qwen Local",
        provider: "mlx",
        is_available: true,
      },
    ],
    cachedCurrentModel: "cached/qwen-local",
  });

  try {
    await flush();

    assert.equal(fixture.document.getElementById("current-model")?.textContent, "Qwen Local");
    assert.match(fixture.document.getElementById("installed-count")?.textContent || "", /modelo/);
  } finally {
    fixture.cleanup();
  }
});

test("agent filtra modelos chat-only e repara para qwen3.5:9b", async () => {
  const fixture = createFixture({
    agentConfigResponse: {
      provider: "ollama",
      model_id: "deepseek-r1:8b",
      execution_mode: "full",
      approval_mode: "ask",
    },
  });

  try {
    await flush(12);

    fixture.document.querySelector('.tab[data-panel="agent"]')?.click();
    await flush(4);

    const menu = fixture.document.getElementById("model-menu");
    fixture.document.getElementById("model-picker-btn")?.click();
    await flush(2);

    assert.match(fixture.document.getElementById("current-model")?.textContent || "", /qwen3\.5:9b/i);
    assert.ok(menu?.textContent.includes("Tool-ready"));
    assert.ok(!menu?.textContent.includes("deepseek-r1:8b [Ollama]"));
    assert.ok(fixture.fetchCalls.some((entry) =>
      entry.path === "/agent/config"
      && entry.method === "POST"
      && entry.body?.model_id === "qwen3.5:9b"
    ));
  } finally {
    fixture.cleanup();
  }
});

test("agent provider selector agrupa local como MLX-Pilot e mostra cloud apenas com secret configurado", async () => {
  const fixture = createFixture({
    agentConfigResponse: {
      provider: "ollama",
      model_id: "qwen3.5:9b",
      execution_mode: "full",
      approval_mode: "ask",
      provider_profiles: [
        {
          id: "ollama-local",
          provider: "ollama",
          model_id: "qwen3.5:9b",
          base_url: "http://127.0.0.1:11434",
          api_key_ref: null,
          custom_headers: {},
          runtime_variant: "classic",
        },
        {
          id: "openai-prod",
          provider: "openai",
          model_id: "gpt-4o-mini",
          base_url: "https://api.openai.com/v1",
          api_key_ref: null,
          custom_headers: {},
          runtime_variant: "classic",
        },
        {
          id: "anthropic-prod",
          provider: "anthropic",
          model_id: "claude-3.5-sonnet",
          base_url: "",
          api_key_ref: null,
          custom_headers: {},
          runtime_variant: "classic",
        },
      ],
    },
    environmentResponseHidden: {
      variables: [
        {
          key: "OPENAI_API_KEY",
          value: "",
          masked: "sk-****abcd",
          source: "env_file",
          present: true,
          is_secret: true,
        },
        {
          key: "ANTHROPIC_API_KEY",
          value: "",
          masked: "-",
          source: "catalog",
          present: false,
          is_secret: true,
        },
      ],
    },
  });

  try {
    await flush(12);

    fixture.document.querySelector('.tab[data-panel="agent"]')?.click();
    fixture.document.querySelector('.agent-view-tab[data-agent-view="config"]')?.click();
    await flush(3);

    const providerSelect = fixture.document.getElementById("agent-provider-select");
    const options = Array.from(providerSelect?.options || []).map((option) => option.textContent);
    assert.deepEqual(options, ["MLX-Pilot (local)", "OpenAI (gpt-4o-mini)"]);

    providerSelect.value = "cloud:openai";
    providerSelect.dispatchEvent(new fixture.window.Event("change", { bubbles: true }));
    await flush(4);

    const saveCall = fixture.fetchCalls.find((entry) =>
      entry.path === "/agent/config"
      && entry.method === "POST"
      && entry.body?.provider === "openai"
    );
    assert.ok(saveCall);
    assert.equal(saveCall.body.model_id, "gpt-4o-mini");
    assert.match(fixture.document.getElementById("agent-provider-pill")?.textContent || "", /OpenAI/);
  } finally {
    fixture.cleanup();
  }
});

test("agent provider profiles podem ser editados e salvos pela aba do agent", async () => {
  const fixture = createFixture({
    agentConfigResponse: {
      provider: "ollama",
      model_id: "qwen3.5:9b",
      execution_mode: "full",
      approval_mode: "ask",
      provider_profiles: [
        {
          id: "ollama-local",
          provider: "ollama",
          model_id: "qwen3.5:9b",
          base_url: "http://127.0.0.1:11434",
          api_key_ref: null,
          custom_headers: {},
          runtime_variant: "classic",
        },
        {
          id: "openai-prod",
          provider: "openai",
          model_id: "gpt-4o-mini",
          base_url: "https://api.openai.com/v1",
          api_key_ref: null,
          custom_headers: {},
          runtime_variant: "classic",
        },
      ],
    },
  });

  try {
    await flush(12);

    fixture.document.querySelector('.tab[data-panel="agent"]')?.click();
    fixture.document.querySelector('.agent-view-tab[data-agent-view="config"]')?.click();
    await flush(3);

    const profileRow = fixture.document.querySelector('[data-profile-id="openai-prod"]');
    const modelInput = profileRow?.querySelector('[data-field="model_id"]');
    const secretRefInput = profileRow?.querySelector('[data-field="api_key_ref"]');
    const runtimeSelect = profileRow?.querySelector('[data-field="runtime_variant"]');

    modelInput.value = "gpt-4.1-mini";
    modelInput.dispatchEvent(new fixture.window.Event("input", { bubbles: true }));
    secretRefInput.value = "OPENAI_API_KEY";
    secretRefInput.dispatchEvent(new fixture.window.Event("input", { bubbles: true }));
    runtimeSelect.value = "hermes_inspired";
    runtimeSelect.dispatchEvent(new fixture.window.Event("change", { bubbles: true }));

    fixture.document.getElementById("agent-save-provider-profiles")?.click();
    await flush(4);

    const saveCall = fixture.fetchCalls.find((entry) =>
      entry.path === "/agent/config"
      && entry.method === "POST"
      && Array.isArray(entry.body?.provider_profiles)
      && entry.body.provider_profiles.some((profile) => profile.id === "openai-prod" && profile.model_id === "gpt-4.1-mini")
    );
    assert.ok(saveCall);

    const savedProfile = saveCall.body.provider_profiles.find((profile) => profile.id === "openai-prod");
    assert.equal(savedProfile.api_key_ref, "OPENAI_API_KEY");
    assert.equal(savedProfile.runtime_variant, "hermes_inspired");

    const providerOptions = Array.from(fixture.document.getElementById("agent-provider-select")?.options || []).map((option) => option.textContent);
    assert.ok(providerOptions.includes("OpenAI (gpt-4.1-mini)"));
  } finally {
    fixture.cleanup();
  }
});

test("agent provider profile pode ser ativado direto na configuracao", async () => {
  const fixture = createFixture({
    agentConfigResponse: {
      provider: "ollama",
      model_id: "qwen3.5:9b",
      execution_mode: "full",
      approval_mode: "ask",
      provider_profiles: [
        {
          id: "ollama-local",
          provider: "ollama",
          model_id: "qwen3.5:9b",
          base_url: "http://127.0.0.1:11434",
          api_key_ref: null,
          custom_headers: {},
          runtime_variant: "classic",
        },
        {
          id: "openai-prod",
          provider: "openai",
          model_id: "gpt-4o-mini",
          base_url: "https://api.openai.com/v1",
          api_key_ref: "OPENAI_API_KEY",
          custom_headers: {},
          runtime_variant: "classic",
        },
      ],
    },
  });

  try {
    await flush(12);

    fixture.document.querySelector('.tab[data-panel="agent"]')?.click();
    fixture.document.querySelector('.agent-view-tab[data-agent-view="config"]')?.click();
    await flush(3);

    fixture.document.querySelector('[data-profile-id="openai-prod"] [data-action="use"]')?.click();
    await flush(4);

    const useCall = fixture.fetchCalls.find((entry) =>
      entry.path === "/agent/config"
      && entry.method === "POST"
      && entry.body?.provider_profile_id === "openai-prod"
      && entry.body?.provider === "openai"
    );
    assert.ok(useCall);
    assert.equal(useCall.body.model_id, "gpt-4o-mini");
    assert.match(fixture.document.getElementById("agent-provider-pill")?.textContent || "", /OpenAI/);
  } finally {
    fixture.cleanup();
  }
});

test("environment reveal mostra secret salvo e nao depende de campo vazio", async () => {
  const fixture = createFixture({
    environmentResponseHidden: {
      variables: [
        {
          key: "OPENAI_API_KEY",
          value: "",
          masked: "sk-****1234",
          source: "env_file",
          present: true,
          is_secret: true,
        },
      ],
    },
    environmentResponseRevealed: {
      variables: [
        {
          key: "OPENAI_API_KEY",
          value: "sk-live-123456",
          masked: "sk-****1234",
          source: "env_file",
          present: true,
          is_secret: true,
        },
      ],
    },
  });

  try {
    await flush(6);

    const revealButton = fixture.document.querySelector("#env-table .reveal-btn");
    const secretInput = fixture.document.querySelector("#env-table .env-val");
    assert.equal(secretInput?.value, "sk-****1234");
    assert.equal(secretInput?.type, "password");

    revealButton?.click();
    await flush(3);

    assert.equal(secretInput?.value, "sk-live-123456");
    assert.equal(secretInput?.type, "text");
    assert.equal(revealButton?.textContent, "Ocultar");

    revealButton?.click();
    await flush(2);

    assert.equal(secretInput?.type, "password");
    assert.equal(secretInput?.value, "sk-****1234");
  } finally {
    fixture.cleanup();
  }
});

test("sidebar global aparece apenas no chat e some nas outras abas", async () => {
  const fixture = createFixture();

  try {
    await flush();

    assert.equal(fixture.document.getElementById("app")?.classList.contains("chat-sidebar-visible"), true);

    fixture.document.querySelector('.tab[data-panel="agent"]')?.click();
    await flush(2);
    assert.equal(fixture.document.getElementById("app")?.classList.contains("chat-sidebar-visible"), false);

    fixture.document.querySelector('.tab[data-panel="chat"]')?.click();
    await flush(2);
    assert.equal(fixture.document.getElementById("app")?.classList.contains("chat-sidebar-visible"), true);
  } finally {
    fixture.cleanup();
  }
});

test("console mostra diagnostico nativo e permite limpar logs", async () => {
  const fixture = createFixture();

  try {
    await flush(8);

    fixture.document.querySelector('.tab[data-panel="console"]')?.click();
    await flush(4);

    assert.equal(fixture.document.getElementById("panel-console")?.classList.contains("active"), true);
    assert.equal(fixture.document.getElementById("console-health")?.textContent, "Online");
    assert.match(fixture.document.getElementById("console-process")?.textContent || "", /PID 4242/);
    assert.match(fixture.document.getElementById("console-log-path")?.textContent || "", /desktop\.log/);
    assert.match(fixture.document.getElementById("console-feed")?.textContent || "", /desktop shell starting/);
    assert.ok(fixture.nativeCalls.some((entry) => entry.command === "desktop_runtime_info"));
    assert.ok(fixture.nativeCalls.some((entry) => entry.command === "desktop_log_snapshot"));

    fixture.document.getElementById("clear-console")?.click();
    await flush(2);

    assert.ok(fixture.nativeCalls.some((entry) => entry.command === "desktop_log_clear"));
    assert.match(fixture.document.getElementById("console-feed")?.textContent || "", /Console limpo/);
  } finally {
    fixture.cleanup();
  }
});

test("chat canoniza modelos legados decorados antes de chamar o backend", async () => {
  const fixture = createFixture({
    modelsResponse: [
      {
        id: "ollama::dolphin3:8b",
        name: "dolphin3:8b [Ollama]",
        provider: "ollama",
        is_available: true,
      },
    ],
    cachedCurrentModel: "dolphin3:8b [Ollama]",
  });

  try {
    await flush();

    const input = fixture.document.getElementById("chat-input");
    input.value = "Mostre o estado do runtime";
    fixture.document.getElementById("send-btn")?.click();
    await flush(6);

    const streamCall = fixture.fetchCalls.find((entry) => entry.path === "/chat/stream");
    assert.ok(streamCall);
    assert.equal(streamCall.body.model_id, "ollama::dolphin3:8b");
  } finally {
    fixture.cleanup();
  }
});
