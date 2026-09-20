// Rotas de /flows para o mock daemon.
//
// Espelha a forma que `crates/daemon/src/flows.rs` devolve, com um store em
// memoria, para a aba Workflows funcionar sem compilar o Rust. Nao executa
// nada de verdade: `run` produz um registro plausivel para exercitar o painel
// de execucao.

const NODE_CATALOG = [
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
    kind: "trigger.webhook",
    label: "Webhook",
    group: "Gatilhos",
    description: "Recebe uma chamada HTTP no proprio daemon.",
    color: "#e76f51",
    glyph: "WH",
    inputs: [],
    outputs: ["main"],
    defaults: { path: "meu-fluxo", method: "POST", response_mode: "last_node" },
    fields: [
      { key: "path", label: "Caminho", kind: "text", required: true },
      {
        key: "method",
        label: "Metodo",
        kind: "select",
        options: [
          { value: "POST", label: "POST" },
          { value: "GET", label: "GET" },
        ],
      },
      {
        key: "response_mode",
        label: "Resposta",
        kind: "select",
        options: [
          { value: "last_node", label: "Devolver a saida do fluxo" },
          { value: "immediate", label: "Responder na hora" },
        ],
      },
    ],
  },
  {
    kind: "trigger.schedule",
    label: "Agenda",
    group: "Gatilhos",
    description: "Executa o fluxo em uma expressao cron.",
    color: "#ff8fab",
    glyph: "CR",
    inputs: [],
    outputs: ["main"],
    defaults: { cron: "0 */5 * * * *" },
    fields: [{ key: "cron", label: "Expressao cron", kind: "text", required: true }],
  },
  {
    kind: "http.request",
    label: "Requisicao HTTP",
    group: "Acoes",
    description: "Chama uma URL e devolve status e corpo.",
    color: "#53a9ff",
    glyph: "HTTP",
    inputs: ["main"],
    outputs: ["main"],
    defaults: {
      method: "GET",
      url: "https://example.com",
      headers: {},
      query: {},
      body_type: "none",
      body: "",
      timeout_secs: 30,
      response_format: "auto",
      fail_on_error_status: true,
    },
    fields: [
      {
        key: "method",
        label: "Metodo",
        kind: "select",
        options: ["GET", "POST", "PUT", "PATCH", "DELETE"].map((value) => ({ value, label: value })),
      },
      { key: "url", label: "URL", kind: "text", required: true },
      { key: "headers", label: "Cabecalhos", kind: "json" },
      { key: "query", label: "Parametros de query", kind: "json" },
      {
        key: "body_type",
        label: "Tipo do corpo",
        kind: "select",
        options: [
          { value: "none", label: "Sem corpo" },
          { value: "json", label: "JSON" },
          { value: "text", label: "Texto" },
        ],
      },
      { key: "body", label: "Corpo", kind: "json" },
      { key: "timeout_secs", label: "Timeout (s)", kind: "number", advanced: true },
      { key: "response_format", label: "Formato da resposta", kind: "text", advanced: true },
      { key: "fail_on_error_status", label: "Falhar em 4xx/5xx", kind: "boolean", advanced: true },
    ],
  },
  {
    kind: "data.set",
    label: "Editar campos",
    group: "Dados",
    description: "Define campos em cada item, com expressoes.",
    color: "#2ec4b6",
    glyph: "SET",
    inputs: ["main"],
    outputs: ["main"],
    defaults: { assignments: [{ name: "campo", value: "{{ $json.valor }}" }], keep_only_set: false },
    fields: [
      { key: "assignments", label: "Campos", kind: "json", required: true },
      { key: "keep_only_set", label: "Descartar os outros campos", kind: "boolean" },
    ],
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
    kind: "flow.if",
    label: "Condicional",
    group: "Fluxo",
    description: "Envia cada item para a saida verdadeira ou falsa.",
    color: "#b38cff",
    glyph: "IF",
    inputs: ["main"],
    outputs: ["true", "false"],
    defaults: { condition: "{{ $json.status == 200 }}" },
    fields: [{ key: "condition", label: "Condicao", kind: "expression", required: true }],
  },
  {
    kind: "flow.merge",
    label: "Juntar",
    group: "Fluxo",
    description: "Reune os itens de varios ramos.",
    color: "#4cc9f0",
    glyph: "MRG",
    inputs: ["main"],
    outputs: ["main"],
    defaults: { mode: "append" },
    fields: [
      {
        key: "mode",
        label: "Modo",
        kind: "select",
        options: [
          { value: "append", label: "Todos os itens" },
          { value: "first", label: "Somente o primeiro" },
          { value: "last", label: "Somente o ultimo" },
        ],
      },
    ],
  },
  {
    kind: "agent.run",
    label: "Agente MLX Pilot",
    group: "MLX Pilot",
    description: "Envia um prompt ao agente local e devolve a resposta.",
    color: "#00d4ff",
    glyph: "AI",
    inputs: ["main"],
    outputs: ["main"],
    defaults: {
      message: "Resuma em uma frase: {{ $json.texto }}",
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
      { key: "system_prompt", label: "Prompt de sistema", kind: "textarea" },
      { key: "tools", label: "Ferramentas liberadas", kind: "json" },
      { key: "max_iterations", label: "Iteracoes maximas", kind: "number" },
      { key: "parse_json", label: "Interpretar como JSON", kind: "boolean" },
      { key: "output_key", label: "Campo de saida", kind: "text", advanced: true },
      { key: "keep_input", label: "Manter os campos de entrada", kind: "boolean", advanced: true },
      { key: "base_url", label: "Base URL", kind: "text", advanced: true },
      { key: "temperature", label: "Temperatura", kind: "number", advanced: true },
    ],
  },
  {
    kind: "tool.call",
    label: "Ferramenta MLX Pilot",
    group: "MLX Pilot",
    description: "Roda uma ferramenta do agente no sandbox.",
    color: "#7bdff2",
    glyph: "TL",
    inputs: ["main"],
    outputs: ["main"],
    defaults: {
      tool: "read_file",
      params: {},
      read_only: true,
      workspace_root: "",
      output_key: "tool_output",
      parse_json: false,
      keep_input: true,
    },
    fields: [
      { key: "tool", label: "Ferramenta", kind: "select", options_source: "flow_tools", required: true },
      { key: "params", label: "Parametros", kind: "json" },
      { key: "read_only", label: "Somente leitura", kind: "boolean" },
      { key: "parse_json", label: "Interpretar como JSON", kind: "boolean" },
      { key: "output_key", label: "Campo de saida", kind: "text", advanced: true },
      { key: "keep_input", label: "Manter os campos de entrada", kind: "boolean", advanced: true },
      { key: "workspace_root", label: "Raiz do workspace", kind: "text", advanced: true },
    ],
  },
  {
    kind: "mcp.call",
    label: "Servidor MCP",
    group: "MLX Pilot",
    description: "Executa uma ferramenta de um servidor MCP.",
    color: "#c77dff",
    glyph: "MCP",
    inputs: ["main"],
    outputs: ["main"],
    defaults: {
      server: "",
      tool: "",
      arguments: {},
      output_key: "mcp_output",
      parse_json: false,
      keep_input: true,
    },
    fields: [
      { key: "server", label: "Servidor", kind: "select", options_source: "mcp_servers", required: true },
      { key: "tool", label: "Ferramenta", kind: "select", options_source: "mcp_tools", required: true },
      { key: "arguments", label: "Argumentos", kind: "json" },
      { key: "parse_json", label: "Interpretar como JSON", kind: "boolean" },
      { key: "output_key", label: "Campo de saida", kind: "text", advanced: true },
      { key: "keep_input", label: "Manter os campos de entrada", kind: "boolean", advanced: true },
    ],
  },
];

const TOOLS = ["edit_file", "exec", "glob", "grep", "list_dir", "read_file", "write_file"];

const KNOWN_KINDS = new Set(NODE_CATALOG.map((node) => node.kind));

const flows = new Map();
const runs = new Map();
let mcpServers = [
  {
    name: "eco-teste",
    command: "node",
    args: ["./scripts/mcp-echo-server.mjs"],
    env: {},
    enabled: true,
    description: "servidor de verificacao",
    tools: [
      { name: "eco", description: "Devolve o texto em maiusculas", input_schema: {} },
      { name: "somar", description: "Soma dois numeros", input_schema: {} },
    ],
  },
];

function uid() {
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === "x" ? r : (r & 0x3) | 0x8).toString(16);
  });
}

// Mesmas regras do `graph::validate`, no que a UI consegue perceber.
function validate(flow) {
  const issues = [];
  if (!String(flow?.name || "").trim()) {
    issues.push({ severity: "error", code: "flow_name_required", message: "O fluxo precisa de um nome.", node_ids: [] });
  }
  const nodes = Array.isArray(flow?.nodes) ? flow.nodes : [];
  if (!nodes.length) {
    issues.push({ severity: "error", code: "flow_empty", message: "O fluxo precisa de pelo menos um no.", node_ids: [] });
    return { valid: false, issues };
  }

  const names = new Set();
  for (const node of nodes) {
    if (!KNOWN_KINDS.has(node.kind)) {
      issues.push({
        severity: "error",
        code: "node_kind_unknown",
        message: `O no "${node.name}" usa um tipo desconhecido: "${node.kind}".`,
        node_ids: [node.id],
      });
    }
    if (names.has(node.name)) {
      issues.push({
        severity: "error",
        code: "node_name_duplicated",
        message: `Nome de no repetido: "${node.name}".`,
        node_ids: [node.id],
      });
    }
    names.add(node.name);
  }

  const ids = new Set(nodes.map((node) => node.id));
  for (const edge of flow.edges || []) {
    if (!ids.has(edge.from) || !ids.has(edge.to)) {
      issues.push({
        severity: "error",
        code: "edge_target_unknown",
        message: "Conexao aponta para um no inexistente.",
        node_ids: [],
      });
    }
  }

  if (!nodes.some((node) => !node.disabled && String(node.kind).startsWith("trigger."))) {
    issues.push({
      severity: "warning",
      code: "flow_without_trigger",
      message: "O fluxo nao tem nenhum gatilho habilitado.",
      node_ids: [],
    });
  }

  return { valid: issues.every((issue) => issue.severity !== "error"), issues };
}

// Registro de execucao no formato que o painel espera: saida sempre por porta.
function fakeRun(flow, payload) {
  const startedAt = new Date().toISOString();
  const items = payload === null || payload === undefined ? [{}] : Array.isArray(payload) ? payload : [payload];
  const preview = { main: { items, total: items.length, truncated: false } };

  const nodes = (flow.nodes || []).map((node) => ({
    node_id: node.id,
    node_name: node.name,
    kind: node.kind,
    status: node.disabled ? "disabled" : "success",
    started_at: startedAt,
    finished_at: startedAt,
    duration_ms: 3,
    attempts: node.disabled ? 0 : 1,
    input_count: items.length,
    output_count: items.length,
    output: preview,
    logs: node.kind === "debug.log" ? ["execucao simulada pelo mock"] : [],
  }));

  return {
    id: uid(),
    flow_id: flow.id,
    flow_name: flow.name,
    status: "success",
    trigger: "manual",
    started_at: startedAt,
    finished_at: startedAt,
    duration_ms: 12,
    nodes,
    output: items,
  };
}

/// Devolve `{ status, body }` para uma rota de /flows, ou `null` quando o
/// caminho nao pertence a este modulo.
export function handleFlows(pathname, method, payload) {
  if (pathname !== "/flows" && !pathname.startsWith("/flows/")) return null;
  const segments = pathname.split("/").filter(Boolean);

  if (pathname === "/flows/node-types" && method === "GET") {
    return { status: 200, body: { nodes: NODE_CATALOG, tools: TOOLS, mcp_servers: mcpServers } };
  }

  if (pathname === "/flows/mcp/servers") {
    if (method === "GET") return { status: 200, body: { servers: mcpServers } };
    if (method === "POST") {
      const name = String(payload?.name || "").trim();
      if (!name || !String(payload?.command || "").trim()) {
        return { status: 400, body: { error: "mcp_server_invalid", details: "informe nome e comando" } };
      }
      mcpServers = mcpServers.filter((server) => server.name.toLowerCase() !== name.toLowerCase());
      mcpServers.push({ tools: [], ...payload, name });
      mcpServers.sort((a, b) => a.name.localeCompare(b.name));
      return { status: 200, body: { servers: mcpServers } };
    }
  }

  if (segments[1] === "mcp" && segments[2] === "servers" && segments[3]) {
    const name = decodeURIComponent(segments[3]);
    if (segments[4] === "probe" && method === "POST") {
      const server = mcpServers.find((item) => item.name.toLowerCase() === name.toLowerCase());
      if (!server) {
        return { status: 502, body: { name, reachable: false, tools: [], error: `nao existe servidor MCP chamado \`${name}\`` } };
      }
      server.tools = server.tools?.length
        ? server.tools
        : [{ name: "exemplo", description: "ferramenta simulada pelo mock", input_schema: {} }];
      return { status: 200, body: { ...server, reachable: true } };
    }
    if (method === "DELETE") {
      const before = mcpServers.length;
      mcpServers = mcpServers.filter((item) => item.name.toLowerCase() !== name.toLowerCase());
      return mcpServers.length === before
        ? { status: 404, body: { error: "mcp_server_not_found", details: name } }
        : { status: 200, body: { deleted: true, name } };
    }
  }

  if (pathname === "/flows/validate" && method === "POST") {
    return { status: 200, body: validate(payload) };
  }

  if (pathname === "/flows/import/n8n" && method === "POST") {
    const source = payload?.workflow && typeof payload.workflow === "object" ? payload.workflow : payload;
    if (!Array.isArray(source?.nodes)) {
      return { status: 400, body: { error: "n8n_import_failed", details: "o workflow precisa ter uma lista `nodes`" } };
    }
    const flow = {
      schema: "mlxflow.v1",
      id: "",
      name: source.name || "Workflow importado",
      description: "Importado de um workflow do n8n.",
      active: false,
      nodes: source.nodes.map((node, index) => ({
        id: node.id || uid(),
        name: node.name || `No ${index + 1}`,
        kind: node.type === "n8n-nodes-base.manualTrigger" ? "trigger.manual" : "debug.log",
        parameters: node.type === "n8n-nodes-base.manualTrigger" ? { sample: "" } : { message: "{{ $json }}" },
        position: { x: 240 + index * 260, y: 200 },
        disabled: false,
        on_error: "stop",
      })),
      edges: [],
      settings: { timeout_secs: 300, max_parallel: 8, save_runs: true },
    };
    return {
      status: 200,
      body: {
        flow,
        warnings: ["conversao simplificada: o mock nao traduz parametros"],
        unsupported_kinds: [],
        validation: validate(flow),
      },
    };
  }

  if (pathname === "/flows/runs" && method === "GET") {
    return { status: 200, body: { runs: [...runs.values()].map(summary) } };
  }

  if (segments[1] === "runs" && segments[2] && method === "GET") {
    const run = runs.get(decodeURIComponent(segments[2]));
    return run
      ? { status: 200, body: run }
      : { status: 404, body: { error: "run_not_found", details: segments[2] } };
  }

  if (pathname === "/flows") {
    if (method === "GET") {
      return { status: 200, body: { flows: [...flows.values()].map(summary) } };
    }
    if (method === "POST") {
      const report = validate(payload);
      if (!report.valid) {
        return { status: 400, body: { error: "flow_invalid", details: report.issues.map((i) => i.message).join("; "), validation: report } };
      }
      const now = new Date().toISOString();
      const existing = flows.get(payload.id);
      const flow = {
        ...payload,
        schema: "mlxflow.v1",
        id: payload.id || uid(),
        created_at: existing?.created_at || now,
        updated_at: now,
      };
      flows.set(flow.id, flow);
      return { status: 200, body: { flow, validation: report } };
    }
  }

  const id = segments[1] ? decodeURIComponent(segments[1]) : "";
  const flow = flows.get(id);

  if (segments[2] === "run" && method === "POST") {
    if (!flow) return { status: 404, body: { error: "flow_not_found", details: id } };
    const record = fakeRun(flow, payload?.payload ?? null);
    runs.set(record.id, record);
    return { status: 200, body: record };
  }

  if (segments[2] === "runs" && method === "GET") {
    return { status: 200, body: { runs: [...runs.values()].filter((run) => run.flow_id === id).map(summary) } };
  }

  if (segments.length === 2) {
    if (method === "GET") {
      return flow ? { status: 200, body: flow } : { status: 404, body: { error: "flow_not_found", details: id } };
    }
    if (method === "DELETE") {
      return flows.delete(id)
        ? { status: 200, body: { deleted: true, id } }
        : { status: 404, body: { error: "flow_not_found", details: id } };
    }
  }

  return { status: 404, body: { error: "not_found", details: pathname } };
}

function summary(value) {
  if (value.nodes && value.edges) {
    return {
      id: value.id,
      name: value.name,
      description: value.description,
      active: Boolean(value.active),
      node_count: value.nodes.length,
      edge_count: value.edges.length,
      trigger_kinds: value.nodes.filter((n) => String(n.kind).startsWith("trigger.")).map((n) => n.kind),
      created_at: value.created_at,
      updated_at: value.updated_at,
    };
  }
  return {
    id: value.id,
    flow_id: value.flow_id,
    flow_name: value.flow_name,
    status: value.status,
    trigger: value.trigger,
    started_at: value.started_at,
    finished_at: value.finished_at,
    duration_ms: value.duration_ms,
    node_count: value.nodes.length,
  };
}
