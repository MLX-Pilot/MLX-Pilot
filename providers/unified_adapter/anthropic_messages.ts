import {
  executeJsonRequest,
  extractSystemMessages,
  getTransport,
  isRecord,
} from "./helpers.ts";
import type { UniAdapter, UniMessage, UniRequest, UniResponse } from "./types.ts";

type AnthropicBlock =
  | { type: "text"; text: string }
  | { type: "tool_use"; id: string; name: string; input: unknown }
  | { type: "tool_result"; tool_use_id: string; content: string; is_error?: boolean };

export function toAnthropicRequest(request: UniRequest): Record<string, unknown> {
  const { system, rest } = extractSystemMessages(request.messages);
  const messages = rest.map((message) => {
    if (message.role === "tool") {
      const result = message.tool_result;
      return {
        role: "user",
        content: [
          {
            type: "tool_result",
            tool_use_id: result?.tool_call_id ?? "",
            content:
              typeof result?.output === "string"
                ? result.output
                : JSON.stringify(result?.output ?? message.content),
            is_error: Boolean(result?.is_error),
          },
        ],
      };
    }

    const content: AnthropicBlock[] = [];
    if (message.content.trim()) {
      content.push({ type: "text", text: message.content });
    }
    if (message.tool_call) {
      content.push({
        type: "tool_use",
        id: message.tool_call.id,
        name: message.tool_call.name,
        input: message.tool_call.arguments ?? {},
      });
    }

    return {
      role: message.role,
      content: content.length ? content : [{ type: "text", text: "" }],
    };
  });

  return {
    model: request.model,
    max_tokens: request.max_tokens ?? 1024,
    temperature: request.temperature,
    system: system.join("\n\n") || undefined,
    tools: request.tools?.map((tool) => ({
      name: tool.name,
      description: tool.description,
      input_schema: tool.input_schema,
    })),
    messages,
  };
}

export function fromAnthropicResponse(raw: unknown): UniResponse {
  if (!isRecord(raw) || !Array.isArray(raw.content)) {
    throw new Error("Invalid Anthropic response");
  }

  const messages: UniMessage[] = [];
  let bufferedText: string[] = [];

  for (const block of raw.content) {
    if (!isRecord(block) || typeof block.type !== "string") {
      continue;
    }

    if (block.type === "text" && typeof block.text === "string") {
      bufferedText.push(block.text);
      continue;
    }

    if (bufferedText.length) {
      messages.push({
        role: "assistant",
        content: bufferedText.join("\n").trim(),
      });
      bufferedText = [];
    }

    if (
      block.type === "tool_use" &&
      typeof block.id === "string" &&
      typeof block.name === "string"
    ) {
      messages.push({
        role: "assistant",
        content: "",
        tool_call: {
          id: block.id,
          name: block.name,
          arguments: block.input ?? {},
        },
      });
    }
  }

  if (bufferedText.length || !messages.length) {
    messages.push({
      role: "assistant",
      content: bufferedText.join("\n").trim(),
    });
  }

  return {
    model: typeof raw.model === "string" ? raw.model : "unknown",
    provider: "anthropic",
    messages,
    raw,
    stop_reason:
      typeof raw.stop_reason === "string" ? raw.stop_reason : undefined,
  };
}

export async function executeAnthropic(request: UniRequest): Promise<UniResponse> {
  const transport = getTransport(request, "anthropic", {
    base_url: process.env.ANTHROPIC_BASE_URL ?? "https://api.anthropic.com/v1",
    api_key: process.env.ANTHROPIC_API_KEY,
    anthropic_version: process.env.ANTHROPIC_VERSION ?? "2023-06-01",
  });
  const payload = toAnthropicRequest(request);
  const raw = await executeJsonRequest(request, "anthropic", "/messages", payload, transport, {
    extra_headers: {
      "anthropic-version": transport.anthropic_version ?? "2023-06-01",
      "x-api-key": transport.api_key ?? "",
    },
  });
  return fromAnthropicResponse(raw);
}

export const anthropicMessagesAdapter: UniAdapter = {
  provider: "anthropic",
  chat: executeAnthropic,
};
