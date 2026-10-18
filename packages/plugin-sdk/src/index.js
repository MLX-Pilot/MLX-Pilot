/** Definitions are inert until compiled. Runtime calls are always validated by MLX Pilot. */
export function defineTool(definition) {
  if (!/^[a-z0-9_]{1,30}$/.test(definition.name))
    throw new Error("Use a snake_case tool name (1–30 characters).");
  if (
    definition.parameters?.type !== "object" ||
    typeof definition.execute !== "function"
  )
    throw new Error("A tool needs an object schema and execute function.");
  return definition;
}
export function definePlugin(definition) {
  if (!/^[a-z0-9_]{1,30}$/.test(definition.id) || !definition.tools?.length)
    throw new Error("A plugin needs an id and at least one tool.");
  return {
    ...definition,
    api_version: 1,
    permissions: definition.permissions || ["compute"],
    services: definition.services || [],
  };
}

function capabilities(call) {
  return {
    http: { request: (input) => call("http.request", input) },
    credentials: {
      status: (service) => call("credentials.status", { service }),
    },
    documents: {
      list: () => call("documents.list", {}),
      get: (id) => call("documents.get", { id }),
      create: (input) => call("documents.create", input),
      edit: (input) => call("documents.edit", input),
    },
  };
}

/** Synchronous WASM host bridge. Credentials remain in the host's secret store. */
export function createHostClient() {
  return capabilities((operation, input) => {
    const invoke = globalThis.Host?.getFunctions?.().mlx_host_call;
    if (typeof invoke !== "function" || !globalThis.Memory)
      throw new Error("Plugin host unavailable");
    const request = Memory.fromString(JSON.stringify({ operation, input }));
    let response;
    try {
      response = Memory.find(invoke(request.offset));
      const envelope = JSON.parse(response.readString());
      if (typeof envelope.error === "string") throw new Error(envelope.error);
      if (!Object.prototype.hasOwnProperty.call(envelope, "result"))
        throw new Error("Invalid plugin host response");
      return envelope.result;
    } finally {
      response?.free();
      request.free();
    }
  });
}

/** Browser bridge: host verifies iframe identity and scopes calls to this plugin. */
export function createFrontendClient({ timeout = 60000 } = {}) {
  const bridgeToken = globalThis.window?.__MLX_PLUGIN_BRIDGE_TOKEN__;
  if (typeof bridgeToken !== "string" || !bridgeToken)
    throw new Error("Plugin frontend host unavailable");
  const pending = new Map();
  let sequence = 0;
  const clientId =
    globalThis.crypto?.randomUUID?.() ||
    `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  let closed = false;
  const receive = (event) => {
    if (
      event.source !== window.parent ||
      event.data?.channel !== "mlx-plugin-v1" ||
      event.data?.bridgeToken !== bridgeToken
    )
      return;
    const item = pending.get(event.data.requestId);
    if (!item) return;
    pending.delete(event.data.requestId);
    clearTimeout(item.timer);
    if (event.data.error) item.reject(new Error(event.data.error));
    else item.resolve(event.data.result);
  };
  window.addEventListener("message", receive);
  const request = (payload) => {
    if (closed) return Promise.reject(new Error("Frontend closed"));
    const requestId = `${clientId}-${++sequence}`;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(requestId);
        reject(new Error("Plugin request timed out"));
      }, timeout);
      pending.set(requestId, { resolve, reject, timer });
      window.parent.postMessage(
        { channel: "mlx-plugin-v1", bridgeToken, requestId, ...payload },
        "*",
      );
    });
  };
  return {
    ...capabilities((operation, input) => request({ operation, input })),
    invoke(tool, input) {
      if (!/^[a-z0-9_]{1,30}$/.test(tool))
        return Promise.reject(new Error("Invalid tool name"));
      return request({ tool, input });
    },
    dispose() {
      closed = true;
      window.removeEventListener("message", receive);
      for (const item of pending.values()) {
        clearTimeout(item.timer);
        item.reject(new Error("Frontend closed"));
      }
      pending.clear();
    },
  };
}
