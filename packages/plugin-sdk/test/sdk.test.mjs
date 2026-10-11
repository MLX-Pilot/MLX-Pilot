import test from "node:test";
import assert from "node:assert/strict";
import { definePlugin, defineTool } from "../src/index.js";
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
        listener({ source: parent, data: { ...message, result: { sum: 60 } } });
      });
    },
  };
  globalThis.window = {
    parent,
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
