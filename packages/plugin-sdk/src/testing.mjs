import assert from "node:assert/strict";

/** Local fixture transport only. Does not perform network requests or read secrets. */
export function installFixtureHost(calls = []) {
  assert.ok(Array.isArray(calls), "host_calls must be an array");
  const saved = {
    Host: Object.getOwnPropertyDescriptor(globalThis, "Host"),
    Memory: Object.getOwnPropertyDescriptor(globalThis, "Memory"),
  };
  const allocations = new Map();
  let offset = 0n;
  let consumed = 0;
  const allocate = (text) => {
    const pointer = ++offset;
    const handle = {
      offset: pointer,
      readString: () => {
        assert.ok(allocations.has(pointer), "Fixture memory already freed");
        return text;
      },
      free: () => allocations.delete(pointer),
    };
    allocations.set(pointer, handle);
    return handle;
  };
  globalThis.Memory = {
    fromString: allocate,
    find: (pointer) => {
      const memory = allocations.get(pointer);
      assert.ok(memory, "Invalid fixture memory pointer");
      return memory;
    },
  };
  globalThis.Host = {
    getFunctions: () => ({
      mlx_host_call: (pointer) => {
        const request = JSON.parse(
          globalThis.Memory.find(pointer).readString(),
        );
        const expected = calls[consumed++];
        assert.ok(expected, `Unexpected host call: ${request.operation}`);
        assert.deepEqual(request, {
          operation: expected.operation,
          input: expected.input,
        });
        const response = Object.hasOwn(expected, "error")
          ? { error: expected.error }
          : { result: expected.result };
        return allocate(JSON.stringify(response)).offset;
      },
    }),
  };
  return {
    verify() {
      assert.equal(
        consumed,
        calls.length,
        "Not all expected host calls were made",
      );
      assert.equal(
        allocations.size,
        0,
        "Plugin host client leaked fixture memory",
      );
    },
    restore() {
      for (const [key, descriptor] of Object.entries(saved)) {
        if (descriptor) Object.defineProperty(globalThis, key, descriptor);
        else delete globalThis[key];
      }
      allocations.clear();
    },
  };
}
