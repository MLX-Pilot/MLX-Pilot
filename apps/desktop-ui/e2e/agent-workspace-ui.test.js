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
  catalogModelsResponse,
  initialDownloads,
  startupResponse,
  useRealTimers = false,
} = {}) {
  let sessions = [
    { id: "sess-1", name: "Operacao", message_count: 3 },
    { id: "sess-2", name: "Channels", message_count: 1 },
  ];
  let nextSession = 3;
  const fetchCalls = [];
  const hiddenEnvironment = environmentResponseHidden ?? { variables: [] };
  const revealedEnvironment = environmentResponseRevealed ?? hiddenEnvironment;
  let downloads = Array.isArray(initialDownloads) ? [...initialDownloads] : [];

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
  if (useRealTimers) {
    window.setTimeout = globalThis.setTimeout;
    window.clearTimeout = globalThis.clearTimeout;
  } else {
    window.setTimeout = (callback) => {
      callback();
      return 1;
    };
    window.clearTimeout = () => {};
  }
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
      return jsonResponse({ status: "ok", provider: "auto", provider_ready: true, degraded: false });
    }

    if (path === "/runtime/startup" || path === "/runtime/startup/retry" || path === "/runtime/startup/cancel") {
      return jsonResponse(startupResponse ?? {
        phase: "ready",
        step: "ready",
        message: "Pronto",
        progress_percent: null,
        bytes_downloaded: 0,
        bytes_total: null,
        bytes_per_second: null,
        can_cancel: false,
        app_ready: true,
        degraded: false,
        operation_id: "startup-test",
        providers: [
          { provider: "ollama", phase: "ready", ready: true, applicable: true, message: "Ollama pronto" },
          { provider: "llamacpp", phase: "ready", ready: true, applicable: true, message: "llama.cpp pronto" },
          { provider: "mlx", phase: "unsupported", ready: false, applicable: false, message: "Nao aplicavel" },
        ],
        error: null,
      });
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

    if (requestUrl.pathname === "/catalog/models") {
      return jsonResponse(catalogModelsResponse ?? []);
    }

    if (path === "/catalog/downloads" && method === "GET") {
      return jsonResponse(downloads);
    }

    if (path === "/catalog/downloads" && method === "POST") {
      const job = {
        id: "dl-test-1",
        source: body?.source || "huggingface",
        model_id: body?.model_id || "unknown/model",
        destination: "G:/models/test",
        status: "running",
        progress_percent: 37,
        bytes_downloaded: 370,
        bytes_total: 1000,
        total_files: 2,
        completed_files: 0,
        current_file: "model.gguf",
        can_cancel: true,
      };
      downloads = [job, ...downloads.filter((entry) => entry.id !== job.id)];
      return jsonResponse(job);
    }

    if (/^\/catalog\/downloads\/[^/]+\/cancel$/.test(path) && method === "POST") {
      const jobId = path.split("/")[3];
      const current = downloads.find((entry) => entry.id === jobId);
      const cancelled = {
        ...current,
        status: "cancelled",
        can_cancel: false,
        error: "cancelado pelo usuario",
      };
      downloads = downloads.map((entry) => entry.id === jobId ? cancelled : entry);
      return jsonResponse(cancelled);
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
            base_url: "",
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

    if (path === "/flows/node-types") {
      return jsonResponse({
        tools: ["read_file", "exec"],
        nodes: [
          {
            kind: "trigger.manual",
            label: "Gatilho manual",
            group: "Gatilhos",
            description: "Inicia o fluxo sob demanda.",
            color: "#f2b84b",
            glyph: "GO",
            inputs: [],
            outputs: ["main"],
            defaults: { sample: "" },
            fields: [{ key: "sample", label: "Payload de teste", kind: "json" }],
          },
          {
            kind: "data.set",
            label: "Editar campos",
            group: "Dados",
            description: "Define campos em cada item.",
            color: "#2ec4b6",
            glyph: "SET",
            inputs: ["main"],
            outputs: ["main"],
            defaults: { assignments: [], keep_only_set: false },
            fields: [
              { key: "assignments", label: "Campos", kind: "json", required: true },
              { key: "keep_only_set", label: "Descartar os outros campos", kind: "boolean" },
            ],
          },
          {
            kind: "flow.if",
            label: "Condicional",
            group: "Fluxo",
            description: "Divide os itens em duas saidas.",
            color: "#b38cff",
            glyph: "IF",
            inputs: ["main"],
            outputs: ["true", "false"],
            defaults: { condition: "{{ $json.ok }}" },
            fields: [{ key: "condition", label: "Condicao", kind: "expression", required: true }],
          },
          {
            kind: "debug.log",
            label: "Log",
            group: "Dados",
            description: "Registra uma mensagem e repassa os itens.",
            color: "#8d99ae",
            glyph: "LOG",
            inputs: ["main"],
            outputs: ["main"],
            defaults: { message: "{{ $json }}" },
            fields: [{ key: "message", label: "Mensagem", kind: "expression" }],
          },
          {
            kind: "agent.run",
            label: "Agente MLX Pilot",
            group: "MLX Pilot",
            description: "Envia um prompt ao agente local.",
            color: "#00d4ff",
            glyph: "AI",
            inputs: ["main"],
            outputs: ["main"],
            defaults: {
              message: "Resuma: {{ $json.texto }}",
              system_prompt: "",
              provider: "",
              model_id: "",
              provider_profile_id: "",
              base_url: "",
              temperature: null,
              max_iterations: 1,
              tools: [],
              output_key: "response",
              parse_json: false,
              keep_input: true,
            },
            fields: [
              { key: "provider", label: "Provedor", kind: "select", options_source: "agent_providers" },
              { key: "model_id", label: "Modelo", kind: "select", options_source: "agent_models" },
              { key: "message", label: "Mensagem", kind: "textarea", required: true },
              { key: "tools", label: "Ferramentas liberadas", kind: "json" },
              { key: "max_iterations", label: "Iteracoes maximas", kind: "number" },
              { key: "output_key", label: "Campo de saida", kind: "text", advanced: true },
              { key: "base_url", label: "Base URL", kind: "text", advanced: true },
            ],
          },
          {
            kind: "tool.call",
            label: "Ferramenta MLX Pilot",
            group: "MLX Pilot",
            description: "Roda uma ferramenta do agente.",
            color: "#7bdff2",
            glyph: "TL",
            inputs: ["main"],
            outputs: ["main"],
            defaults: { tool: "read_file", params: {}, read_only: true, workspace_root: "" },
            fields: [
              { key: "tool", label: "Ferramenta", kind: "select", options_source: "flow_tools", required: true },
              { key: "params", label: "Parametros", kind: "json" },
              { key: "workspace_root", label: "Raiz do workspace", kind: "text", advanced: true },
            ],
          },
        ],
      });
    }

    if (path === "/flows" && method === "GET") {
      return jsonResponse({
        flows: [
          {
            id: "flow-1",
            name: "Fluxo salvo",
            active: false,
            node_count: 2,
            edge_count: 1,
            trigger_kinds: ["trigger.manual"],
          },
        ],
      });
    }

    if (path === "/flows" && method === "POST") {
      return jsonResponse({
        flow: { ...body, id: body?.id || "flow-1" },
        validation: { valid: true, issues: [] },
      });
    }

    if (path === "/flows/flow-1" && method === "GET") {
      return jsonResponse({
        schema: "mlxflow.v1",
        id: "flow-1",
        name: "Fluxo salvo",
        active: false,
        nodes: [
          { id: "manual-1", name: "Gatilho manual", kind: "trigger.manual", parameters: {}, position: { x: 280, y: 240 } },
        ],
        edges: [],
        settings: { timeout_secs: 300, max_parallel: 8, save_runs: true },
      });
    }

    if (path.startsWith("/flows/flow-1/runs")) {
      return jsonResponse({ runs: [] });
    }

    if (path === "/flows/flow-1/run" && method === "POST") {
      return jsonResponse({
        id: "run-1",
        flow_id: "flow-1",
        flow_name: "Fluxo salvo",
        status: "success",
        trigger: "manual",
        started_at: new Date().toISOString(),
        duration_ms: 12,
        output: [{ ok: true }],
        nodes: [
          {
            node_id: "manual-1",
            node_name: "Gatilho manual",
            kind: "trigger.manual",
            status: "success",
            duration_ms: 3,
            attempts: 1,
            input_count: 1,
            output_count: 1,
            output: {},
            logs: [],
          },
        ],
      });
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

test("catalog shows download percentage and supports cancellation", async () => {
  const fixture = createFixture({
    useRealTimers: true,
    catalogModelsResponse: [
      {
        source: "huggingface",
        model_id: "acme/model-7b",
        name: "model-7b",
        author: "acme",
        downloads: 1200,
        likes: 42,
        size_bytes: 1000,
      },
    ],
  });

  try {
    await flush(8);

    fixture.document.querySelector('.tab[data-panel="discover"]')?.click();
    await flush(5);
    fixture.document.querySelector(".download-btn")?.click();
    await flush(5);

    const progress = fixture.document.querySelector(".download-progress-track");
    assert.equal(progress?.getAttribute("aria-valuenow"), "37");
    assert.match(fixture.document.getElementById("catalog-download-list")?.textContent || "", /Baixando 37%/);
    assert.equal(fixture.document.querySelector(".download-btn")?.disabled, true);

    fixture.document.querySelector(".download-cancel-btn")?.click();
    await flush(5);

    assert.ok(fixture.fetchCalls.some((entry) =>
      entry.path === "/catalog/downloads/dl-test-1/cancel"
      && entry.method === "POST"
    ));
    assert.match(fixture.document.getElementById("catalog-download-list")?.textContent || "", /Cancelado 37%/);
    assert.equal(fixture.document.querySelector(".download-cancel-btn"), null);
  } finally {
    fixture.cleanup();
  }
});

test("agent mantem modelo local chat-only visivel e selecionado", async () => {
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

    assert.match(fixture.document.getElementById("current-model")?.textContent || "", /deepseek-r1:8b/i);
    assert.ok(menu?.textContent.includes("Tool-ready"));
    assert.ok(menu?.textContent.includes("deepseek-r1:8b [Ollama]"));
    assert.ok(menu?.textContent.includes("Chat-only"));
    assert.ok(!fixture.fetchCalls.some((entry) =>
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

test("editor de fluxos monta o grafo e salva no formato nativo", async () => {
  const fixture = createFixture();

  try {
    await flush(8);
    fixture.document.querySelector('.tab[data-panel="workflows"]')?.click();
    await flush(6);

    // A paleta vem do catalogo publicado pelo daemon.
    assert.ok(fixture.document.querySelector('[data-flow-node-kind="data.set"]'));
    assert.equal(fixture.document.querySelectorAll("#flow-nodes .workflow-node").length, 1);

    fixture.document.querySelector('[data-flow-node-kind="data.set"]')?.click();
    await flush(2);
    assert.equal(fixture.document.querySelectorAll("#flow-nodes .workflow-node").length, 2);
    assert.match(fixture.document.getElementById("flow-inspector")?.textContent || "", /Editar campos/);

    fixture.document.getElementById("flow-save-btn")?.click();
    await flush(6);

    const saveCall = fixture.fetchCalls.find((entry) => entry.path === "/flows" && entry.method === "POST");
    assert.ok(saveCall, "deveria ter chamado POST /flows");
    assert.equal(saveCall.body.schema, "mlxflow.v1");
    assert.equal(saveCall.body.nodes.length, 2);
    assert.equal(saveCall.body.nodes[1].kind, "data.set");
    assert.equal(fixture.document.getElementById("flow-save-state")?.textContent, "Salvo");
    // Executar so libera com o fluxo gravado.
    assert.equal(fixture.document.getElementById("flow-run-btn")?.disabled, false);
  } finally {
    fixture.cleanup();
  }
});

test("no de agente nasce com o provedor e o modelo ativos do MLX Pilot", async () => {
  const fixture = createFixture();

  try {
    await flush(8);
    fixture.document.querySelector('.tab[data-panel="workflows"]')?.click();
    await flush(8);

    fixture.document.querySelector('[data-flow-node-kind="agent.run"]')?.click();
    await flush(3);

    const provider = fixture.document.querySelector('[data-field-key="provider"]');
    const model = fixture.document.querySelector('[data-field-key="model_id"]');
    assert.ok(provider, "campo provedor deveria ser um select");
    assert.equal(provider.tagName, "SELECT");
    assert.equal(model.tagName, "SELECT");

    // Herda o que esta ativo no resto do app (agent config: ollama / qwen3.5:9b).
    assert.equal(provider.value, "ollama");
    assert.match(model.value, /qwen3\.5:9b/);

    // Agrupado em Local / Cloud.
    const groups = [...provider.querySelectorAll("optgroup")].map((group) => group.label);
    assert.ok(groups.includes("Local (MLX Pilot)"), `grupos: ${groups.join(",")}`);

    // Base URL e herdada: nao aparece entre os campos principais.
    const mainFields = [...fixture.document.querySelectorAll("#flow-inspector .settings-field [data-field-key]")]
      .filter((field) => !field.closest(".workflow-inspector-advanced"))
      .map((field) => field.dataset.fieldKey);
    assert.ok(!mainFields.includes("base_url"), `campos visiveis: ${mainFields.join(",")}`);
  } finally {
    fixture.cleanup();
  }
});

test("escolher outro modelo sincroniza o provedor do no de agente", async () => {
  const fixture = createFixture();

  try {
    await flush(8);
    fixture.document.querySelector('.tab[data-panel="workflows"]')?.click();
    await flush(8);
    fixture.document.querySelector('[data-flow-node-kind="agent.run"]')?.click();
    await flush(3);

    const model = fixture.document.querySelector('[data-field-key="model_id"]');
    // Todos os modelos locais instalados aparecem no grupo local.
    const localGroup = [...model.querySelectorAll("optgroup")].find((g) => g.label === "Local (MLX Pilot)");
    const values = [...localGroup.querySelectorAll("option")].map((option) => option.value);
    assert.equal(values.length, 3, `modelos locais: ${values.join(",")}`);

    // Trocar para um modelo MLX deve mudar o provedor junto.
    model.value = "mlx-community/Qwen3-4B-4bit";
    model.dispatchEvent(new fixture.window.Event("change", { bubbles: true }));
    await flush(3);

    const provider = fixture.document.querySelector('[data-field-key="provider"]');
    assert.equal(provider.value, "mlx");
  } finally {
    fixture.cleanup();
  }
});

test("arrastar da porta de saida ate outro no cria a conexao", async () => {
  const fixture = createFixture();

  try {
    await flush(8);
    fixture.document.querySelector('.tab[data-panel="workflows"]')?.click();
    await flush(8);

    // Dois nos desconectados: adiciona o segundo numa posicao explicita para
    // evitar a conexao automatica.
    fixture.document.querySelector('[data-flow-node-kind="debug.log"]')?.click();
    await flush(2);
    const nodes = [...fixture.document.querySelectorAll("#flow-nodes .workflow-node")];
    assert.equal(nodes.length, 2);

    const before = fixture.document.querySelectorAll("#flow-connections .workflow-connection-group").length;

    const source = nodes[0];
    const outputPort = source.querySelector('[data-port="output"]');
    const targetPort = nodes[1].querySelector('[data-port="input"]');
    const pointer = (type, target, x) => target.dispatchEvent(
      new fixture.window.MouseEvent(type, { bubbles: true, button: 0, clientX: x, clientY: 0 }),
    );

    pointer("pointerdown", outputPort, 0);
    pointer("pointermove", fixture.document, 80);
    pointer("pointerup", targetPort, 80);
    await flush(3);

    const after = fixture.document.querySelectorAll("#flow-connections .workflow-connection-group").length;
    assert.ok(after > before, `conexoes antes=${before} depois=${after}`);
  } finally {
    fixture.cleanup();
  }
});

test("clicar na saida e depois na entrada tambem conecta", async () => {
  const fixture = createFixture();

  try {
    await flush(8);
    fixture.document.querySelector('.tab[data-panel="workflows"]')?.click();
    await flush(8);
    fixture.document.querySelector('[data-flow-node-kind="debug.log"]')?.click();
    await flush(2);

    const click = (element) => {
      element.dispatchEvent(new fixture.window.MouseEvent("pointerdown", { bubbles: true, button: 0, clientX: 0, clientY: 0 }));
      element.dispatchEvent(new fixture.window.MouseEvent("pointerup", { bubbles: true, button: 0, clientX: 0, clientY: 0 }));
      element.dispatchEvent(new fixture.window.MouseEvent("click", { bubbles: true, button: 0 }));
    };

    let nodes = [...fixture.document.querySelectorAll("#flow-nodes .workflow-node")];
    click(nodes[0].querySelector('[data-port="output"]'));
    await flush(2);

    nodes = [...fixture.document.querySelectorAll("#flow-nodes .workflow-node")];
    click(nodes[1].querySelector('[data-port="input"]'));
    await flush(3);

    assert.equal(fixture.document.querySelectorAll("#flow-connections .workflow-connection-group").length, 1);
  } finally {
    fixture.cleanup();
  }
});

test("condicional expoe as duas portas de saida no canvas", async () => {
  const fixture = createFixture();

  try {
    await flush(8);
    fixture.document.querySelector('.tab[data-panel="workflows"]')?.click();
    await flush(6);

    fixture.document.querySelector('[data-flow-node-kind="flow.if"]')?.click();
    await flush(2);

    const ports = [...fixture.document.querySelectorAll("#flow-nodes .workflow-node-port.output")]
      .map((port) => port.dataset.portName);
    assert.ok(ports.includes("true"), `portas encontradas: ${ports.join(",")}`);
    assert.ok(ports.includes("false"), `portas encontradas: ${ports.join(",")}`);
  } finally {
    fixture.cleanup();
  }
});

test("executar um fluxo salvo mostra o resultado por no", async () => {
  const fixture = createFixture();

  try {
    await flush(8);
    fixture.document.querySelector('.tab[data-panel="workflows"]')?.click();
    await flush(6);

    fixture.document.querySelector('[data-flow-open="flow-1"]')?.click();
    await flush(6);

    fixture.document.getElementById("flow-run-btn")?.click();
    await flush(6);

    const runCall = fixture.fetchCalls.find((entry) => entry.path === "/flows/flow-1/run");
    assert.ok(runCall, "deveria ter chamado POST /flows/{id}/run");

    const result = fixture.document.getElementById("flow-run-result");
    assert.equal(result?.hidden, false);
    assert.match(result?.textContent || "", /Sucesso/);
    assert.match(result?.textContent || "", /Gatilho manual/);
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

test("startup renders real download telemetry and cancellation", async () => {
  const fixture = createFixture({
    startupResponse: {
      phase: "cancelled",
      step: "cancelled",
      message: "Operacao cancelada com seguranca",
      progress_percent: 42,
      bytes_downloaded: 420,
      bytes_total: 1000,
      bytes_per_second: 100,
      can_cancel: true,
      app_ready: true,
      degraded: true,
      operation_id: "startup-cancel",
      providers: [
        {
          provider: "ollama",
          phase: "cancelled",
          ready: false,
          applicable: true,
          message: "Cancelado",
        },
        {
          provider: "llamacpp",
          phase: "ready",
          ready: true,
          applicable: true,
          message: "llama.cpp pronto",
        },
      ],
      error: "operacao cancelada pelo usuario",
    },
  });

  try {
    await flush(8);
    assert.equal(fixture.document.getElementById("startup-progress")?.getAttribute("aria-valuenow"), "42");
    assert.match(fixture.document.getElementById("startup-meta")?.textContent || "", /42%/);
    fixture.document.getElementById("startup-cancel")?.click();
    await flush(3);
    assert.ok(fixture.fetchCalls.some((entry) =>
      entry.path === "/runtime/startup/cancel" && entry.method === "POST"
    ));
  } finally {
    fixture.cleanup();
  }
});

test("degraded Ollama blocks only models routed to Ollama", async () => {
  const fixture = createFixture({
    startupResponse: {
      phase: "degraded",
      step: "degraded",
      message: "Aplicacao pronta em modo degradado",
      progress_percent: null,
      bytes_downloaded: 0,
      bytes_total: null,
      bytes_per_second: null,
      can_cancel: false,
      app_ready: true,
      degraded: true,
      operation_id: "startup-degraded",
      providers: [
        {
          provider: "ollama",
          phase: "failed",
          ready: false,
          applicable: true,
          message: "GPU nao detectada",
          error: "Ollama iniciou somente em CPU",
        },
        {
          provider: "llamacpp",
          phase: "ready",
          ready: true,
          applicable: true,
          message: "llama.cpp pronto",
        },
        {
          provider: "mlx",
          phase: "unsupported",
          ready: false,
          applicable: false,
          message: "Nao aplicavel",
        },
      ],
      error: "Ollama iniciou somente em CPU",
    },
  });

  try {
    await flush(10);
    const input = fixture.document.getElementById("chat-input");
    input.value = "Ola";
    fixture.document.getElementById("send-btn")?.click();
    await flush(3);
    assert.ok(!fixture.fetchCalls.some((entry) => entry.path === "/chat/stream"));
    assert.match(fixture.document.getElementById("chat-messages")?.textContent || "", /Ollama nao esta pronto/);
    assert.equal(fixture.document.getElementById("agent-daemon-status")?.textContent, "Degradado");
  } finally {
    fixture.cleanup();
  }
});
