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
  };
}

/** Browser bridge: host verifies iframe identity and scopes calls to this plugin. */
export function createFrontendClient({ timeout = 15000 } = {}) {
  const pending = new Map();
  let sequence = 0;
  const receive = (event) => {
    if (
      event.source !== window.parent ||
      event.data?.channel !== "mlx-plugin-v1"
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
  return {
    invoke(tool, input) {
      if (!/^[a-z0-9_]{1,30}$/.test(tool))
        return Promise.reject(new Error("Invalid tool name"));
      const requestId = `request-${++sequence}`;
      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(requestId);
          reject(new Error("Plugin request timed out"));
        }, timeout);
        pending.set(requestId, { resolve, reject, timer });
        window.parent.postMessage(
          { channel: "mlx-plugin-v1", requestId, tool, input },
          "*",
        );
      });
    },
    dispose() {
      window.removeEventListener("message", receive);
      for (const item of pending.values()) {
        clearTimeout(item.timer);
        item.reject(new Error("Frontend closed"));
      }
      pending.clear();
    },
  };
}
