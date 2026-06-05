import {
  chat,
  runTool,
  type UniProvider,
  type UniRequest,
  type UniToolDef,
} from "../providers/unified_adapter/index.ts";

const weatherTool: UniToolDef = {
  name: "get_weather",
  description: "Return the local weather for a city.",
  input_schema: {
    type: "object",
    properties: {
      city: { type: "string" },
    },
    required: ["city"],
  },
};

const toolRegistry = {
  async get_weather(_definition: UniToolDef, args: unknown) {
    const city =
      typeof args === "object" &&
      args !== null &&
      "city" in args &&
      typeof (args as { city?: unknown }).city === "string"
        ? (args as { city: string }).city
        : "unknown";
    return {
      city,
      condition: "clear",
      temperature_c: 26,
    };
  },
};

function scriptedLocalTransport(provider: UniProvider) {
  return async (payload: unknown) => {
    if (
      typeof payload === "object" &&
      payload !== null &&
      "messages" in payload &&
      Array.isArray((payload as { messages: Array<{ content?: string }> }).messages)
    ) {
      const messages = (payload as { messages: Array<{ content?: string }> }).messages;
      const sawToolResult = messages.some(
        (message) =>
          typeof message.content === "string" &&
          message.content.includes("<tool_result>"),
      );

      if (!sawToolResult) {
        return {
          model: "qwen3.5:7b",
          message: {
            role: "assistant",
            content:
              '<tool_call>\n{"name":"get_weather","arguments":{"city":"Brasilia"}}\n</tool_call>',
          },
          done_reason: "tool_call",
        };
      }

      return {
        model: "qwen3.5:7b",
        message: {
          role: "assistant",
          content: "The weather in Brasilia is clear and 26 C.",
        },
        done_reason: "stop",
      };
    }

    throw new Error(`Unexpected payload for ${provider}`);
  };
}

async function main() {
  const baseRequest: UniRequest = {
    model: "qwen3.5:7b",
    provider_hint: "ollama",
    tools: [weatherTool],
    messages: [
      {
        role: "system",
        content: "You are a compact assistant. Use tools when needed.",
      },
      {
        role: "user",
        content: "What is the weather in Brasilia?",
      },
    ],
    transport: {
      ollama: {
        native_tool_calling: false,
        executor: scriptedLocalTransport("ollama"),
      },
    },
  };

  const firstResponse = await chat(baseRequest);
  const toolMessage = firstResponse.messages.find((message) => message.tool_call);
  if (!toolMessage?.tool_call) {
    throw new Error("Expected a tool call in the first response");
  }

  const toolResult = await runTool(toolMessage.tool_call, toolRegistry, {
    [weatherTool.name]: weatherTool,
  });

  const followUp = await chat({
    ...baseRequest,
    messages: [
      ...baseRequest.messages,
      ...firstResponse.messages,
      {
        role: "tool",
        content: "",
        tool_result: toolResult,
      },
    ],
  });

  const finalText = followUp.messages
    .map((message) => message.content)
    .filter(Boolean)
    .join("\n");

  console.log("Initial tool call:", toolMessage.tool_call);
  console.log("Tool result:", toolResult);
  console.log("Final assistant response:", finalText);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
