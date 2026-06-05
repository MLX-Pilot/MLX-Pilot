import {
  executeJsonRequest,
  generateToolCallId,
  getTransport,
  isRecord,
  parseSyntheticToolCallEnvelope,
  toSyntheticLocalMessages,
} from "./helpers.ts";
import { fromOpenAIResponse, toOpenAIRequest } from "./openai_chat.ts";
import type { UniAdapter, UniMessage, UniRequest, UniResponse } from "./types.ts";

export function toLlamaCppRequest(request: UniRequest): Record<string, unknown> {
  const transport = getTransport(request, "llamacpp", {});
  if (transport.native_tool_calling) {
    return {
      ...toOpenAIRequest(request),
      parse_tool_calls: true,
      parallel_tool_calls: false,
    };
  }

  return {
    model: request.model,
    messages: toSyntheticLocalMessages(request),
    temperature: request.temperature,
    max_tokens: request.max_tokens,
  };
}

export function fromLlamaCppResponse(raw: unknown, nativeToolCalling: boolean): UniResponse {
  if (nativeToolCalling) {
    const response = fromOpenAIResponse(raw);
    return {
      ...response,
      provider: "llamacpp",
    };
  }

  if (!isRecord(raw) || !Array.isArray(raw.choices) || raw.choices.length === 0) {
    throw new Error("Invalid llama.cpp response");
  }

  const choice = raw.choices[0];
  if (!isRecord(choice) || !isRecord(choice.message)) {
    throw new Error("Invalid llama.cpp response choice");
  }

  const content = typeof choice.message.content === "string" ? choice.message.content : "";
  const parsed = parseSyntheticToolCallEnvelope(content);
  const messages: UniMessage[] = [];
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
      tool_call: {
        id: parsed.tool_call.id || generateToolCallId("llamacpp"),
        name: parsed.tool_call.name,
        arguments: parsed.tool_call.arguments,
      },
    });
  }
  if (!messages.length) {
    messages.push({
      role: "assistant",
      content,
    });
  }

  return {
    model: typeof raw.model === "string" ? raw.model : "unknown",
    provider: "llamacpp",
    messages,
    raw,
    stop_reason: typeof choice.finish_reason === "string" ? choice.finish_reason : undefined,
  };
}

export async function executeLlamaCpp(request: UniRequest): Promise<UniResponse> {
  const transport = getTransport(request, "llamacpp", {
    base_url: process.env.LLAMACPP_BASE_URL ?? "http://127.0.0.1:11439/v1",
    api_key: process.env.LLAMACPP_API_KEY ?? "sk-no-key-required",
  });
  const payload = toLlamaCppRequest(request);
  const raw = await executeJsonRequest(
    request,
    "llamacpp",
    "/chat/completions",
    payload,
    transport,
  );
  return fromLlamaCppResponse(raw, Boolean(transport.native_tool_calling));
}

export const llamacppAdapter: UniAdapter = {
  provider: "llamacpp",
  chat: executeLlamaCpp,
};
