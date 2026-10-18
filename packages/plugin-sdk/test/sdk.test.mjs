import test from "node:test";
import assert from "node:assert/strict";
import {
  createHostClient,
  createFrontendClient,
  definePlugin,
  defineTool,
} from "../src/index.js";
import { installFixtureHost } from "../src/testing.mjs";
import { mkdtemp, writeFile, mkdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
test("definitions preserve schemas and expose pure functions", () => {
  const tool = defineTool({
    name: "sum",
    description: "Sum",
    parameters: { type: "object" },
    execute: ({ a, b }) => a + b,
  });
  const plugin = definePlugin({
    id: "calculator",
    name: "Calculator",
    version: "1",
    tools: [tool],
  });
  assert.equal(plugin.tools[0].execute({ a: 2, b: 3 }), 5);
  assert.deepEqual(plugin.permissions, ["compute"]);
  assert.deepEqual(plugin.services, []);
});

test("WASM host client forwards scoped operations synchronously and frees all memory", () => {
  const httpInput = {
    service: "research",
    path: "/search?q=design",
    method: "GET",
    response_type: "json",
  };
  const documentEdit = {
    id: "approved",
    base_revision: 2,
    operations: [{ type: "set_title", text: "Updated" }],
  };
  const calls = [
    {
      operation: "http.request",
      input: httpInput,
      result: {
        status: 200,
        headers: { "content-type": "application/json" },
        body: { titles: ["Design"] },
        encoding: "json",
      },
    },
    {
      operation: "credentials.status",
      input: { service: "research" },
      result: { configured: true },
    },
    {
      operation: "documents.list",
      input: {},
      result: [{ id: "approved", title: "Existing", revision: 2, slides: 1 }],
    },
    {
      operation: "documents.get",
      input: { id: "approved" },
      result: { id: "approved", revision: 2 },
    },
    {
      operation: "documents.create",
      input: { title: "New document" },
      result: { id: "created", title: "New document", revision: 1 },
    },
    {
      operation: "documents.edit",
      input: documentEdit,
      result: { id: "approved", revision: 3, slides: 1, diagnostics: [] },
    },
  ];
  const fixture = installFixtureHost(calls);
  try {
    const host = createHostClient();
    assert.deepEqual(host.http.request(httpInput), calls[0].result);
    assert.deepEqual(host.credentials.status("research"), calls[1].result);
    assert.deepEqual(host.documents.list(), calls[2].result);
    assert.deepEqual(host.documents.get("approved"), calls[3].result);
    assert.deepEqual(
      host.documents.create({ title: "New document" }),
      calls[4].result,
    );
    assert.deepEqual(host.documents.edit(documentEdit), calls[5].result);
    fixture.verify();
  } finally {
    fixture.restore();
  }
});

test("host errors are surfaced without leaking request or response allocations", () => {
  const fixture = installFixtureHost([
    {
      operation: "documents.get",
      input: { id: "unapproved" },
      error: "Document access not granted",
    },
  ]);
  try {
    assert.throws(
      () => createHostClient().documents.get("unapproved"),
      /Document access not granted/,
    );
    fixture.verify();
  } finally {
    fixture.restore();
  }
  assert.throws(
    () => createHostClient().documents.list(),
    /Plugin host unavailable/,
  );
});

test("standalone pages report the missing frontend bridge clearly", () => {
  const original = globalThis.window;
  try {
    globalThis.window = {};
    assert.throws(
      () => createFrontendClient(),
      /Plugin frontend host unavailable/,
    );
    delete globalThis.window;
    assert.throws(
      () => createFrontendClient(),
      /Plugin frontend host unavailable/,
    );
  } finally {
    globalThis.window = original;
  }
});

test("frontend capability calls use operation envelopes and reject calls after disposal", async () => {
  const original = globalThis.window;
  const sent = [];
  let listener;
  const parent = {
    postMessage(message) {
      sent.push(message);
      queueMicrotask(() =>
        listener({
          source: parent,
          data: {
            channel: message.channel,
            bridgeToken: message.bridgeToken,
            requestId: message.requestId,
            result: { operation: message.operation },
          },
        }),
      );
    },
  };
  globalThis.window = {
    parent,
    __MLX_PLUGIN_BRIDGE_TOKEN__: "test-token",
    addEventListener: (_, fn) => {
      listener = fn;
    },
    removeEventListener: () => {},
  };
  const client = createFrontendClient();
  try {
    const edit = { id: "approved", base_revision: 2, operations: [] };
    await Promise.all([
      client.http.request({ service: "research", path: "/search" }),
      client.credentials.status("research"),
      client.documents.list(),
      client.documents.get("approved"),
      client.documents.create({ title: "New document" }),
      client.documents.edit(edit),
    ]);
    assert.deepEqual(
      sent.map(({ channel, bridgeToken, requestId, ...request }) => {
        assert.equal(channel, "mlx-plugin-v1");
        assert.equal(bridgeToken, "test-token");
        assert.equal(typeof requestId, "string");
        return request;
      }),
      [
        {
          operation: "http.request",
          input: { service: "research", path: "/search" },
        },
        { operation: "credentials.status", input: { service: "research" } },
        { operation: "documents.list", input: {} },
        { operation: "documents.get", input: { id: "approved" } },
        { operation: "documents.create", input: { title: "New document" } },
        { operation: "documents.edit", input: edit },
      ],
    );
    assert.equal(
      new Set(sent.map((message) => message.requestId)).size,
      sent.length,
    );
    client.dispose();
    await assert.rejects(client.documents.list(), /Frontend closed/);
    assert.equal(sent.length, 6);
  } finally {
    client.dispose();
    globalThis.window = original;
  }
});

test("independent frontend clients cannot consume each other's replies", async () => {
  const original = globalThis.window;
  const listeners = new Set();
  const messages = [];
  const parent = { postMessage: (message) => messages.push(message) };
  globalThis.window = {
    parent,
    __MLX_PLUGIN_BRIDGE_TOKEN__: "shared-frame-token",
    addEventListener: (_, listener) => listeners.add(listener),
    removeEventListener: (_, listener) => listeners.delete(listener),
  };
  const first = createFrontendClient();
  const second = createFrontendClient();
  try {
    const firstReply = first.documents.get("first");
    const secondReply = second.documents.get("second");
    assert.notEqual(messages[0].requestId, messages[1].requestId);
    for (const message of messages) {
      for (const listener of listeners)
        listener({
          source: parent,
          data: { ...message, result: { id: message.input.id } },
        });
    }
    assert.deepEqual(await firstReply, { id: "first" });
    assert.deepEqual(await secondReply, { id: "second" });
  } finally {
    first.dispose();
    second.dispose();
    globalThis.window = original;
  }
});

test("CLI fixtures exercise imported createHostClient through deterministic mock responses", async () => {
  const directory = await mkdtemp(
    path.join(tmpdir(), "mlx-plugin-host-fixture-"),
  );
  try {
    await mkdir(path.join(directory, "src"));
    const sdk = fileURLToPath(new URL("../src/index.js", import.meta.url));
    await writeFile(
      path.join(directory, "src/index.ts"),
      `import {definePlugin,defineTool,createHostClient} from ${JSON.stringify(sdk)};
const host=createHostClient();
export default definePlugin({id:'fixture',name:'Fixture',version:'1',permissions:['documents:read'],tools:[defineTool({name:'read_document',description:'Read an approved document',parameters:{type:'object'},execute({id}){return host.documents.get(id);}})]});`,
    );
    await writeFile(
      path.join(directory, "fixtures.json"),
      JSON.stringify([
        {
          tool: "read_document",
          input: { id: "allowed" },
          host_calls: [
            {
              operation: "documents.get",
              input: { id: "allowed" },
              result: { id: "allowed", title: "Test" },
            },
          ],
          expected: { id: "allowed", title: "Test" },
        },
        {
          tool: "read_document",
          input: { id: "denied" },
          host_calls: [
            {
              operation: "documents.get",
              input: { id: "denied" },
              error: "Permission denied",
            },
          ],
          expected_error: "Permission denied",
        },
      ]),
    );
    const cli = fileURLToPath(new URL("../src/cli.mjs", import.meta.url));
    const result = spawnSync(process.execPath, [cli, "test"], {
      cwd: directory,
      encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /2 fixtures passed/);
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
test("invalid contracts are rejected", () => {
  assert.throws(() => defineTool({ name: "../../bad" }));
  assert.throws(() => definePlugin({ id: "bad", tools: [] }));
});

test("frontend bridge ignores impersonated replies and correlates host responses", async () => {
  const { createFrontendClient } = await import("../src/index.js");
  const original = globalThis.window;
  let listener;
  const parent = {
    postMessage(message) {
      queueMicrotask(() => {
        listener({ source: {}, data: { ...message, result: "forged" } });
        listener({
          source: parent,
          data: {
            ...message,
            bridgeToken: "wrong-token",
            result: "forged-token",
          },
        });
        listener({ source: parent, data: { ...message, result: { sum: 60 } } });
      });
    },
  };
  globalThis.window = {
    parent,
    __MLX_PLUGIN_BRIDGE_TOKEN__: "test-token",
    addEventListener: (_, fn) => {
      listener = fn;
    },
    removeEventListener: () => {
      listener = null;
    },
  };
  const client = createFrontendClient();
  try {
    assert.deepEqual(
      await client.invoke("summarize_numbers", { values: [10, 20, 30] }),
      { sum: 60 },
    );
    await assert.rejects(
      client.invoke("../another-plugin", {}),
      /Invalid tool/,
    );
  } finally {
    client.dispose();
    globalThis.window = original;
  }
});

test("closing a frontend rejects pending calls and releases its listener", async () => {
  const { createFrontendClient } = await import("../src/index.js");
  const original = globalThis.window;
  let removed = false;
  globalThis.window = {
    __MLX_PLUGIN_BRIDGE_TOKEN__: "test-token",
    parent: { postMessage() {} },
    addEventListener() {},
    removeEventListener() {
      removed = true;
    },
  };
  const client = createFrontendClient();
  try {
    const call = client.invoke("run", {});
    client.dispose();
    await assert.rejects(call, /Frontend closed/);
    assert.equal(removed, true);
  } finally {
    globalThis.window = original;
  }
});
