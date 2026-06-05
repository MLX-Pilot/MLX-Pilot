import test from "node:test";
import assert from "node:assert/strict";

import { fromOpenAIResponse, toOpenAIRequest } from "./openai_chat.ts";
import { parseSyntheticToolCallEnvelope } from "./helpers.ts";
import { resolveProvider } from "./router.ts";
import {
  fromAnthropicResponse,
  toAnthropicRequest,
} from "./anthropic_messages.ts";

test("OpenAI request normalization preserves tool roles and tool defs", () => {
  const payload = toOpenAIRequest({
    model: "gpt-4o-mini",
    messages: [
      { role: "system", content: "system rule" },
      { role: "user", content: "weather?" },
      {
        role: "assistant",
        content: "",
        tool_call: {
          id: "call_1",
          name: "get_weather",
          arguments: { city: "Brasilia" },
        },
      },
      {
        role: "tool",
        content: "",
        tool_result: {
          tool_call_id: "call_1",
          output: { temp_c: 26 },
        },
      },
    ],
    tools: [
      {
        name: "get_weather",
        description: "Fetch weather",
        input_schema: {
          type: "object",
          properties: { city: { type: "string" } },
        },
      },
    ],
  });

  assert.equal(payload.model, "gpt-4o-mini");
  assert.equal(Array.isArray(payload.messages), true);
  assert.equal(Array.isArray(payload.tools), true);
});

test("OpenAI response normalization extracts tool calls", () => {
  const response = fromOpenAIResponse({
    model: "gpt-4o-mini",
    choices: [
      {
        finish_reason: "tool_calls",
        message: {
          role: "assistant",
          content: "",
          tool_calls: [
            {
              id: "call_2",
              type: "function",
              function: {
                name: "get_weather",
                arguments: '{"city":"Brasilia"}',
              },
            },
          ],
        },
      },
    ],
  });

  assert.equal(response.provider, "openai");
  assert.equal(response.messages.length, 1);
  assert.equal(response.messages[0].tool_call?.name, "get_weather");
  assert.deepEqual(response.messages[0].tool_call?.arguments, {
    city: "Brasilia",
  });
});

test("Anthropic request normalization emits tool_result blocks as user content", () => {
  const payload = toAnthropicRequest({
    model: "claude-sonnet-4-5",
    messages: [
      { role: "system", content: "system rule" },
      { role: "user", content: "weather?" },
      {
        role: "tool",
        content: "",
        tool_result: {
          tool_call_id: "toolu_1",
          output: "26 C",
        },
      },
    ],
    tools: [
      {
        name: "get_weather",
        input_schema: { type: "object" },
      },
    ],
  });

  assert.equal(payload.system, "system rule");
  assert.equal(Array.isArray(payload.messages), true);
  const toolResultMessage = payload.messages[1];
  assert.equal(toolResultMessage.role, "user");
  assert.equal(toolResultMessage.content[0].type, "tool_result");
});

test("Anthropic response normalization extracts tool_use blocks", () => {
  const response = fromAnthropicResponse({
    model: "claude-sonnet-4-5",
    stop_reason: "tool_use",
    content: [
      { type: "text", text: "I should check that." },
      {
        type: "tool_use",
        id: "toolu_1",
        name: "get_weather",
        input: { city: "Brasilia" },
      },
    ],
  });

  assert.equal(response.provider, "anthropic");
  assert.equal(response.messages.length, 2);
  assert.equal(response.messages[1].tool_call?.name, "get_weather");
});

test("Synthetic tool envelope parsing is strict", () => {
  const parsed = parseSyntheticToolCallEnvelope(
    'Need tool.\n<tool_call>\n{"name":"get_weather","arguments":{"city":"Brasilia"}}\n</tool_call>',
  );
  assert.equal(parsed.tool_call?.name, "get_weather");
  assert.deepEqual(parsed.tool_call?.arguments, { city: "Brasilia" });

  const malformed = parseSyntheticToolCallEnvelope(
    '<tool_call>\n{"name":42,"arguments":{}}\n</tool_call>',
  );
  assert.equal(malformed.tool_call, null);
});

test("Router prefers explicit hints and modest model heuristics", () => {
  assert.equal(
    resolveProvider({
      model: "whatever",
      messages: [],
      provider_hint: "mlx",
    }),
    "mlx",
  );
  assert.equal(
    resolveProvider({
      model: "claude-sonnet-4-5",
      messages: [],
    }),
    "anthropic",
  );
  assert.equal(
    resolveProvider({
      model: "ggml-org/model.GGUF",
      messages: [],
    }),
    "llamacpp",
  );
  assert.equal(
    resolveProvider({
      model: "qwen3.5:7b",
      messages: [],
    }),
    "ollama",
  );
});
