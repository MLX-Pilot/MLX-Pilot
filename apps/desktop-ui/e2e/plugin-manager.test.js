import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { JSDOM } from "jsdom";

const source = await readFile(new URL("../ui/plugin-manager.js", import.meta.url), "utf8");
const copy = (value) => JSON.parse(JSON.stringify(value));
async function until(predicate) {
  for (let turn = 0; turn < 30; turn++) {
    if (predicate()) return;
    await new Promise(setImmediate);
  }
  assert.ok(predicate(), "The UI did not finish its pending operation");
}

function fixture({ enabled = false, put } = {}) {
  const dom = new JSDOM('<!doctype html><button data-panel="plugins">Plugins</button><button data-panel="presentations">Apresentações</button><div id="plugin-manager"></div><div id="presentation-studio"></div>', {
    url: "http://localhost/", runScripts: "outside-only",
  });
  const { window } = dom;
  window.HTMLElement.prototype.scrollIntoView = () => {};
  window.confirm = () => true;
  const plugin = {
    manifest: {
      id: "research", name: "Pesquisa <segura>", version: "1.0.0",
      permissions: ["network:request", "credentials:use", "documents:read", "documents:write"],
      tools: [{ name: "summarize" }],
      services: [{ id: "api", name: 'API <img src=x onerror="alert(1)">', base_url: "https://example.com/v1", methods: ["GET", "POST"], credential: { id: "api-token", header: "Authorization", prefix: "Bearer " } }],
    },
    grants: ["network:request", "credentials:use", "documents:read", "documents:write"],
    enabled, has_frontend: true, bundled: false,
  };
  let configuration = {
    grants: [...plugin.grants],
    services: { api: { enabled: true, base_url: "https://example.com/v1", allow_private_network: false, credential_set: true } },
    documents: { ids: ["first"], allow_create: false },
    available_documents: [
      { id: "first", title: "Planejamento <estratégico>", revision: 3, slides: 5 },
      { id: "second", title: "Pesquisa", revision: 1, slides: 2 },
    ],
  };
  const calls = [];
  const response = (value, ok = true, status = 200) => ({ ok, status, json: async () => copy(value) });
  window.fetch = async (url, options = {}) => {
    const path = new URL(url).pathname;
    const body = options.body ? JSON.parse(options.body) : undefined;
    const call = { path, method: options.method || "GET", body };
    calls.push(call);
    if (path === "/agent/wasm-plugins") return response([plugin]);
    if (path.endsWith("/configuration")) {
      if (call.method === "PUT") {
        if (put) await put(body);
        plugin.grants = [...body.grants];
        configuration = {
          ...configuration, grants: [...body.grants], documents: copy(body.documents),
          services: Object.fromEntries(Object.entries(body.services).map(([id, service]) => [id, {
            enabled: service.enabled, base_url: service.base_url,
            allow_private_network: service.allow_private_network,
            credential_set: service.clear_credential ? false : !!service.credential || configuration.services[id].credential_set,
          }])),
        };
      }
      return response(configuration);
    }
    if (path.endsWith("/frontend")) return response({ html: "<h1>Pesquisa</h1>", css: "", javascript: "" });
    if (path.endsWith("/host")) return response({ accepted: body.operation });
    if (path.endsWith("/tools/summarize")) return response({ sum: 6 });
    throw new Error(`Unexpected request: ${path}`);
  };
  window.eval(source);
  const $ = (selector) => window.document.querySelector(selector);
  const ready = () => until(() => $("[data-configure]"));
  async function configure() {
    await ready();
    $("[data-configure]").click();
    await until(() => $("#pm-configuration-form") && !$("#plugin-manager").hasAttribute("aria-busy"));
  }
  const submit = () => $("#pm-configuration-form").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
  return { window, dom, $, calls, ready, configure, submit, configuration: () => configuration };
}

test("disabled plugins can configure individual permissions, services and document access", async () => {
  const f = fixture();
  try {
    await f.configure();
    assert.equal(f.$("[data-open]").disabled, true);
    assert.equal(f.$("[data-configure]").disabled, false);
    assert.equal(f.$("#pm-configuration-title").textContent, "Configurar Pesquisa <segura>");
    assert.equal(f.$("#pm-configuration img"), null);
    assert.match(f.$(".pm-service h3").textContent, /<img/);
    assert.equal(f.$("[data-service-secret]").type, "password");
    assert.equal(f.$("[data-service-secret]").value, "");
    f.window.document.querySelectorAll('input[name="grant"]').forEach((checkbox) => { checkbox.checked = false; });
    f.$("[data-service-url]").value = "http://127.0.0.1:3000/api";
    f.$("[data-service-private]").checked = true;
    f.$("[data-service-enabled]").checked = false;
    f.$("[data-service-secret]").value = "private-write-only-token";
    f.$('input[name="document"][value="first"]').checked = false;
    f.$('input[name="document"][value="second"]').checked = true;
    f.$("#pm-allow-create").checked = true;
    f.submit();
    await until(() => /Configuração salva/.test(f.$("#pm-configuration-status")?.textContent || "") && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const saved = f.calls.find((call) => call.method === "PUT").body;
    assert.deepEqual(saved.grants, []);
    assert.deepEqual(saved.services.api, { enabled: false, base_url: "http://127.0.0.1:3000/api", allow_private_network: true, credential: "private-write-only-token" });
    assert.deepEqual(saved.documents, { ids: ["second"], allow_create: true });
    assert.equal(f.$("[data-service-secret]").value, "");
    assert.ok(!f.$("#plugin-manager").innerHTML.includes("private-write-only-token"));
    assert.equal(f.configuration().services.api.credential_set, true);
  } finally { f.dom.window.close(); }
});

test("saving without a replacement leaves credentials untouched and explicit removal clears them", async () => {
  const f = fixture();
  try {
    await f.configure();
    f.submit();
    await until(() => f.calls.some((call) => call.method === "PUT") && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const first = f.calls.find((call) => call.method === "PUT").body.services.api;
    assert.equal(Object.hasOwn(first, "credential"), false);
    assert.equal(Object.hasOwn(first, "clear_credential"), false);
    f.$("[data-service-secret]").value = "discard-this-replacement";
    f.$("[data-service-clear]").checked = true;
    f.$("[data-service-clear]").dispatchEvent(new f.window.Event("change"));
    assert.equal(f.$("[data-service-secret]").value, "");
    assert.equal(f.$("[data-service-secret]").disabled, true);
    f.submit();
    await until(() => f.calls.filter((call) => call.method === "PUT").length === 2 && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const second = f.calls.filter((call) => call.method === "PUT")[1].body.services.api;
    assert.equal(second.clear_credential, true);
    assert.equal(Object.hasOwn(second, "credential"), false);
    assert.equal(f.configuration().services.api.credential_set, false);
  } finally { f.dom.window.close(); }
});

test("pending writes lock configuration controls and reject duplicate submissions", async () => {
  let resolve;
  const waiting = new Promise((done) => { resolve = done; });
  const f = fixture({ put: () => waiting });
  try {
    await f.configure();
    f.submit();
    f.submit();
    assert.equal(f.$("#pm-save-configuration").disabled, true);
    assert.equal(f.$("[data-service-url]").disabled, true);
    assert.equal(f.calls.filter((call) => call.method === "PUT").length, 1);
    resolve();
    await until(() => !f.$("#plugin-manager").hasAttribute("aria-busy"));
    assert.equal(f.$("#pm-save-configuration").disabled, false);
    assert.equal(f.$("[data-service-url]").disabled, false);
  } finally { resolve(); f.dom.window.close(); }
});

test("write errors cannot echo supplied secrets into status messages", async () => {
  const f = fixture({ put: async (body) => { throw new Error(`Rejected ${body.services.api.credential}`); } });
  try {
    await f.configure();
    f.$("[data-service-secret]").value = "private-secret";
    f.submit();
    await until(() => !f.$("#plugin-manager").hasAttribute("aria-busy"));
    assert.ok(!f.$("#pm-status").textContent.includes("private-secret"));
    assert.ok(!f.$("#pm-configuration-status").textContent.includes("private-secret"));
    assert.equal(f.$("[data-service-secret]").value, "");
    assert.equal(f.$("#pm-save-configuration").disabled, false);
  } finally { f.dom.window.close(); }
});

test("installation review shows service destinations, methods and credential delivery before consent", async () => {
  const f = fixture();
  try {
    await f.ready();
    const pkg = {
      manifest: {
        id: "new-plugin", name: "Nova pesquisa", permissions: ["network:request", "credentials:use"],
        services: [{ id: "search", name: "Pesquisa externa", base_url: "https://search.example.com/v1", methods: ["GET", "POST"], credential: { id: "search-key", header: "Authorization", prefix: "Bearer " } }],
      }, wasm_base64: "AA==",
    };
    const file = { size: 1024, text: async () => JSON.stringify(pkg) };
    Object.defineProperty(f.$("#pm-file"), "files", { configurable: true, value: [file] });
    f.$("#pm-file").dispatchEvent(new f.window.Event("change"));
    await until(() => !f.$("#pm-review").hidden && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const review = f.$("#pm-review").textContent;
    for (const detail of ["Pesquisa externa", "https://search.example.com/v1", "GET, POST", "Authorization", "Bearer", "search-key", "Configurar"])
      assert.ok(review.includes(detail), `Review should explain ${detail}`);
    assert.equal(f.$("#pm-review").querySelectorAll("input:checked").length, 0);
    f.$("#pm-install").click();
    await until(() => /Conceda as permissões/.test(f.$("#pm-status").textContent));
    assert.ok(!f.calls.some((call) => call.method === "POST"));
  } finally { f.dom.window.close(); }
});

test("saving reloads an open authorized frontend and closes it after revocation", async () => {
  const f = fixture({ enabled: true });
  try {
    await f.ready();
    f.$("[data-open]").click();
    await until(() => f.$("#pm-frame iframe") && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const firstFrame = f.$("#pm-frame iframe");
    await f.configure();
    f.submit();
    await until(() => f.calls.filter((call) => call.path.endsWith("/frontend")).length === 2 && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    assert.notEqual(f.$("#pm-frame iframe"), firstFrame);
    assert.equal(firstFrame.isConnected, false);
    f.$('input[name="grant"]').checked = false;
    f.submit();
    await until(() => f.calls.filter((call) => call.method === "PUT").length === 2 && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    assert.equal(f.$("#pm-frame iframe"), null);
    assert.equal(f.$("[data-open]").disabled, true);
    assert.equal(f.$("[data-configure]").disabled, false);
    assert.equal(f.$(".pm-badge").textContent, "Permissões revogadas");
    assert.equal(f.calls.filter((call) => call.path.endsWith("/frontend")).length, 2);
  } finally { f.dom.window.close(); }
});

test("the isolated bridge accepts only scoped broker operations and preserves tool calls", async () => {
  const f = fixture({ enabled: true });
  try {
    await f.ready();
    f.$("[data-open]").click();
    await until(() => f.$("#pm-frame iframe") && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const frame = f.$("#pm-frame iframe");
    assert.equal(frame.getAttribute("sandbox"), "allow-scripts");
    assert.match(frame.srcdoc, /connect-src 'none'/);
    assert.ok(!frame.getAttribute("sandbox").includes("allow-same-origin"));
    const replies = [];
    frame.contentWindow.postMessage = (message) => replies.push(copy(message));
    const bridgeToken = JSON.parse(frame.srcdoc.match(/__MLX_PLUGIN_BRIDGE_TOKEN__=("[^"]+")/)[1]);
    const send = (data, source = frame.contentWindow) => f.window.dispatchEvent(new f.window.MessageEvent("message", { source, data: { channel: "mlx-plugin-v1", bridgeToken, ...data } }));
    const operations = ["http.request", "credentials.status", "documents.list", "documents.get", "documents.create", "documents.edit"];
    for (const [index, operation] of operations.entries()) send({ requestId: `host-${index}`, operation, input: { service: "api" } });
    send({ requestId: "old-tool", tool: "summarize", input: { values: [1, 2, 3] } });
    await until(() => replies.length === 7);
    assert.ok(replies.every((reply) => reply.bridgeToken === bridgeToken));
    assert.equal(f.calls.filter((call) => call.path.endsWith("/research/host")).length, 6);
    assert.deepEqual(replies.find((reply) => reply.requestId === "old-tool").result, { sum: 6 });
    const before = f.calls.length;
    send({ requestId: "not-allowed", operation: "credentials.get", input: {} });
    send({ requestId: "ambiguous", operation: "documents.list", tool: "summarize", input: {} });
    send({ requestId: "forged-window", operation: "documents.list", input: {} }, f.window);
    send({ requestId: "other-document", operation: "documents.list", input: {}, bridgeToken: "unknown-document-token" });
    send({ requestId: "too-big", operation: "documents.list", input: "x".repeat(1024 * 1024 + 1) });
    await until(() => replies.length === 10);
    assert.equal(f.calls.length, before);
    assert.ok(replies.find((reply) => reply.requestId === "not-allowed").error);
    assert.ok(replies.find((reply) => reply.requestId === "ambiguous").error);
    assert.ok(!replies.some((reply) => reply.requestId === "forged-window"));
    assert.ok(!replies.some((reply) => reply.requestId === "other-document"));
    assert.match(replies.find((reply) => reply.requestId === "too-big").error, /grande/);
  } finally { f.dom.window.close(); }
});

test("navigation disconnects the bridge and reopening issues a new document token", async () => {
  const f = fixture({ enabled: true });
  try {
    await f.ready();
    f.$("[data-open]").click();
    await until(() => f.$("#pm-frame iframe") && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const frame = f.$("#pm-frame iframe");
    const token = JSON.parse(frame.srcdoc.match(/__MLX_PLUGIN_BRIDGE_TOKEN__=("[^"]+")/)[1]);
    const replies = [];
    frame.contentWindow.postMessage = (message) => replies.push(copy(message));
    f.window.dispatchEvent(new f.window.MessageEvent("message", { source: frame.contentWindow, data: { channel: "mlx-plugin-v1", requestId: "legacy", tool: "summarize", input: {} } }));
    assert.match(replies[0].error, /SDK atualizado/);
    assert.equal(Object.hasOwn(replies[0], "bridgeToken"), false);
    assert.ok(!f.calls.some((call) => call.path.endsWith("/tools/summarize")));
    frame.dispatchEvent(new f.window.Event("load"));
    if (frame.isConnected) frame.dispatchEvent(new f.window.Event("load"));
    assert.equal(f.$("#pm-frame iframe"), null);
    assert.match(f.$("#pm-status").textContent, /mudou de página/);
    f.$("[data-open]").click();
    await until(() => f.$("#pm-frame iframe") && !f.$("#plugin-manager").hasAttribute("aria-busy"));
    const next = f.$("#pm-frame iframe");
    const nextToken = JSON.parse(next.srcdoc.match(/__MLX_PLUGIN_BRIDGE_TOKEN__=("[^"]+")/)[1]);
    assert.notEqual(nextToken, token);
    const before = f.calls.length;
    f.window.dispatchEvent(new f.window.MessageEvent("message", { source: next.contentWindow, data: { channel: "mlx-plugin-v1", requestId: "old-token", bridgeToken: token, operation: "documents.list", input: {} } }));
    await new Promise(setImmediate);
    assert.equal(f.calls.length, before);
  } finally { f.dom.window.close(); }
});
