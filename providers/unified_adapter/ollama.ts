import {
  executeJsonRequest,
  flattenTextContent,
  generateToolCallId,
  getTransport,
  isRecord,
  parseSyntheticToolCallEnvelope,
  toSyntheticLocalMessages,
} from "./helpers.ts";
import type { UniAdapter, UniMessage, UniRequest, UniResponse, UniToolDef } from "./types.ts";

export function toOllamaRequest(request: UniRequest): Record<string, unknown> {
  const transport = getTransport(request, "ollama", {});
  const nativeTools = transport.native_tool_calling !== false;

  if (!nativeTools && request.tools?.length) {
    return {
      model: request.model,
      messages: toSyntheticLocalMessages(request),
      stream: false,
      options: {
        temperature: request.temperature,
        num_predict: request.max_tokens,
      },
    };
  }

  return {
    model: request.model,
    messages: request.messages.map((message) => {
      if (message.role === "tool") {
        return {
          role: "tool",
          content:
            typeof message.tool_result?.output === "string"
              ? message.tool_result.output
              : JSON.stringify(message.tool_result?.output ?? message.content),
        };
      }

      return {
        role: message.role,
        content: message.content,
      };
    }),
    tools: request.tools?.map(toOllamaTool),
    stream: false,
    options: {
      temperature: request.temperature,
      num_predict: request.max_tokens,
    },
  };
}

export function fromOllamaResponse(raw: unknown): UniResponse {
  if (!isRecord(raw) || !isRecord(raw.message)) {
    throw new Error("Invalid Ollama response");
  }

  const message = raw.message;
  const messages: UniMessage[] = [];
  const content = flattenTextContent(message.content);

  if (Array.isArray(message.tool_calls) && message.tool_calls.length > 0) {
    if (content.trim()) {
      messages.push({
        role: "assistant",
        content: content.trim(),
      });
    }

    for (const entry of message.tool_calls) {
      if (!isRecord(entry) || !isRecord(entry.function) || typeof entry.function.name !== "string") {
        continue;
      }
      messages.push({
        role: "assistant",
        content: "",
        tool_call: {
          id: generateToolCallId("ollama"),
          name: entry.function.name,
          arguments: entry.function.arguments ?? {},
        },
      });
    }
  } else {
    const parsed = parseSyntheticToolCallEnvelope(content);
    if (parsed.content) {
      messages.push({
        role: "assistant",
        content: parsed.content,
      });
    }
    if (parsed.tool_call) {
      messages.push({
        role: "assistant",
        content: "",
        tool_call: parsed.tool_call,
      });
    }
    if (!parsed.content && !parsed.tool_call) {
      messages.push({
        role: "assistant",
        content,
      });
    }
  }

  return {
    model: typeof raw.model === "string" ? raw.model : "unknown",
    provider: "ollama",
    messages,
    raw,
    stop_reason: typeof raw.done_reason === "string" ? raw.done_reason : undefined,
  };
}

export async function executeOllama(request: UniRequest): Promise<UniResponse> {
  const transport = getTransport(request, "ollama", {
    base_url: process.env.OLLAMA_BASE_URL ?? "http://127.0.0.1:11434",
  });
  const payload = toOllamaRequest(request);
  const raw = await executeJsonRequest(request, "ollama", "/api/chat", payload, transport);
  return fromOllamaResponse(raw);
}

function toOllamaTool(tool: UniToolDef): Record<string, unknown> {
  return {
    type: "function",
    function: {
      name: tool.name,
      description: tool.description,
      parameters: tool.input_schema,
    },
  };
}

export const ollamaAdapter: UniAdapter = {
  provider: "ollama",
  chat: executeOllama,
};
