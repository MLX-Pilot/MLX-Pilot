import {
  ensureProviderPrefixedModel,
  executeJsonRequest,
  getTransport,
  isRecord,
  parseSyntheticToolCallEnvelope,
  toSyntheticLocalMessages,
} from "./helpers.ts";
import type { UniAdapter, UniMessage, UniRequest, UniResponse } from "./types.ts";

export function toMlxRequest(request: UniRequest): Record<string, unknown> {
  return {
    model_id: ensureProviderPrefixedModel("mlx", request.model),
    messages: toSyntheticLocalMessages(request).map((message) => ({
      role: message.role,
      content: message.content,
    })),
    options: {
      temperature: request.temperature,
      max_tokens: request.max_tokens,
    },
  };
}

export function fromMlxResponse(raw: unknown): UniResponse {
  if (!isRecord(raw) || !isRecord(raw.message) || typeof raw.message.content !== "string") {
    throw new Error("Invalid MLX response");
  }

  const parsed = parseSyntheticToolCallEnvelope(raw.message.content);
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
      tool_call: parsed.tool_call,
    });
  }
  if (!messages.length) {
    messages.push({
      role: "assistant",
      content: raw.message.content,
    });
  }

  return {
    model: typeof raw.model_id === "string" ? raw.model_id : "unknown",
    provider: "mlx",
    messages,
    raw,
  };
}

export async function executeMlx(request: UniRequest): Promise<UniResponse> {
  const transport = getTransport(request, "mlx", {
    base_url: process.env.MLX_PILOT_DAEMON_URL ?? "http://127.0.0.1:11435",
  });
  const payload = toMlxRequest(request);
  const raw = await executeJsonRequest(request, "mlx", "/chat", payload, transport);
  return fromMlxResponse(raw);
}

export const mlxAdapter: UniAdapter = {
  provider: "mlx",
  chat: executeMlx,
};
