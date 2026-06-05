import { randomUUID } from "node:crypto";

import type {
  UniMessage,
  UniProvider,
  UniProviderTransport,
  UniRequest,
  UniToolCall,
  UniToolDef,
  UniToolResult,
  UniTransportExecutionContext,
} from "./types.ts";

type LocalChatMessage = {
  role: "system" | "user" | "assistant" | "tool";
  content: string;
};

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function flattenTextContent(content: unknown): string {
  if (typeof content === "string") {
    return content;
  }

  if (Array.isArray(content)) {
    return content
      .map((part) => {
        if (typeof part === "string") {
          return part;
        }
        if (!isRecord(part)) {
          return "";
        }
        if (typeof part.text === "string") {
          return part.text;
        }
        if (typeof part.content === "string") {
          return part.content;
        }
        return "";
      })
      .filter(Boolean)
      .join("\n");
  }

  if (isRecord(content)) {
    if (typeof content.text === "string") {
      return content.text;
    }
    if (typeof content.content === "string") {
      return content.content;
    }
  }

  return "";
}

export function extractSystemMessages(messages: UniMessage[]): {
  system: string[];
  rest: UniMessage[];
} {
  const system: string[] = [];
  const rest: UniMessage[] = [];

  for (const message of messages) {
    if (message.role === "system") {
      if (message.content.trim()) {
        system.push(message.content.trim());
      }
      continue;
    }
    rest.push(message);
  }

  return { system, rest };
}

export function generateToolCallId(prefix = "toolcall"): string {
  try {
    return `${prefix}_${randomUUID()}`;
  } catch {
    return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 10)}`;
  }
}

export function stripProviderPrefix(model: string): {
  model: string;
  provider?: UniProvider;
} {
  const trimmed = model.trim();
  const match = /^(anthropic|openai|ollama|mlx|llamacpp)::(.+)$/i.exec(trimmed);
  if (!match) {
    return { model: trimmed };
  }
  return {
    provider: match[1].toLowerCase() as UniProvider,
    model: match[2].trim(),
  };
}

export function ensureProviderPrefixedModel(provider: UniProvider, model: string): string {
  const stripped = stripProviderPrefix(model);
  return `${provider}::${stripped.model}`;
}

export function stableJson(value: unknown): string {
  return JSON.stringify(value ?? null);
}

export function serializeSyntheticToolCall(call: UniToolCall): string {
  return `<tool_call>\n${stableJson({ name: call.name, arguments: call.arguments })}\n</tool_call>`;
}

export function serializeSyntheticToolResult(result: UniToolResult): string {
  return `<tool_result>\n${stableJson({
    tool_call_id: result.tool_call_id,
    output: result.output,
    is_error: Boolean(result.is_error),
  })}\n</tool_result>`;
}

export function buildSyntheticToolPrompt(tools: UniToolDef[] | undefined): string | null {
  if (!tools?.length) {
    return null;
  }

  const renderedTools = tools
    .map((tool) =>
      stableJson({
        name: tool.name,
        description: tool.description ?? "",
        input_schema: tool.input_schema,
      }),
    )
    .join("\n");

  return [
    "Tools are available.",
    "If a tool is required, respond with exactly one tool envelope and no prose around it.",
    "Use this format only:",
    "<tool_call>",
    '{"name":"tool_name","arguments":{"key":"value"}}',
    "</tool_call>",
    "If no tool is required, answer normally.",
    "Available tools:",
    renderedTools,
  ].join("\n");
}

export function parseSyntheticToolCallEnvelope(text: string): {
  content: string;
  tool_call: UniToolCall | null;
} {
  const match = /<tool_call>\s*([\s\S]*?)\s*<\/tool_call>/i.exec(text);
  if (!match) {
    return { content: text, tool_call: null };
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(match[1]);
  } catch {
    return { content: text, tool_call: null };
  }

  if (!isRecord(parsed) || typeof parsed.name !== "string") {
    return { content: text, tool_call: null };
  }

  const content = text.replace(match[0], "").trim();
  return {
    content,
    tool_call: {
      id: generateToolCallId("synthetic"),
      name: parsed.name,
      arguments: parsed.arguments ?? {},
    },
  };
}

export function toSyntheticLocalMessages(request: UniRequest): LocalChatMessage[] {
  const { system, rest } = extractSystemMessages(request.messages);
  const syntheticPrompt = buildSyntheticToolPrompt(request.tools);
  const messages: LocalChatMessage[] = [];

  const systemText = [system.join("\n\n"), syntheticPrompt]
    .filter((value) => value && value.trim())
    .join("\n\n");
  if (systemText) {
    messages.push({ role: "system", content: systemText });
  }

  for (const message of rest) {
    if (message.role === "tool") {
      const result =
        message.tool_result ??
        ({
          tool_call_id: generateToolCallId("unknown"),
          output: message.content,
        } satisfies UniToolResult);
      messages.push({
        role: "user",
        content: serializeSyntheticToolResult(result),
      });
      continue;
    }

    let content = message.content ?? "";
    if (message.tool_call) {
      content = [content, serializeSyntheticToolCall(message.tool_call)]
        .filter((value) => value.trim())
        .join("\n");
    }
    if (message.tool_result) {
      content = [content, serializeSyntheticToolResult(message.tool_result)]
        .filter((value) => value.trim())
        .join("\n");
    }

    messages.push({
      role: message.role,
      content,
    });
  }

  return messages;
}

export function getTransport(
  request: UniRequest,
  provider: UniProvider,
  defaults: UniProviderTransport,
): UniProviderTransport {
  return {
    ...defaults,
    ...(request.transport?.[provider] ?? {}),
    headers: {
      ...(defaults.headers ?? {}),
      ...(request.transport?.[provider]?.headers ?? {}),
    },
  };
}

export async function executeJsonRequest(
  request: UniRequest,
  provider: UniProvider,
  path: string,
  payload: unknown,
  transport: UniProviderTransport,
  init?: { method?: string; extra_headers?: Record<string, string> },
): Promise<unknown> {
  if (transport.executor) {
    const context: UniTransportExecutionContext = {
      provider,
      path,
      request,
      payload,
    };
    return transport.executor(payload, context);
  }

  const baseUrl = transport.base_url?.trim();
  if (!baseUrl) {
    throw new Error(`No transport configured for provider '${provider}'`);
  }

  const url = joinUrl(baseUrl, path);
  const headers: Record<string, string> = {
    "content-type": "application/json",
    ...(transport.headers ?? {}),
    ...(init?.extra_headers ?? {}),
  };

  if (transport.api_key && !headers.authorization) {
    headers.authorization = `Bearer ${transport.api_key}`;
  }

  const controller = new AbortController();
  const timeoutMs = transport.timeout_ms ?? 60_000;
  const timer = setTimeout(() => controller.abort(), timeoutMs);

  try {
    const response = await fetch(url, {
      method: init?.method ?? "POST",
      headers,
      body: JSON.stringify(payload),
      signal: controller.signal,
    });
    if (!response.ok) {
      const text = await response.text();
      throw new Error(`${provider} HTTP ${response.status}: ${text}`);
    }
    return response.json();
  } finally {
    clearTimeout(timer);
  }
}

export function joinUrl(baseUrl: string, path: string): string {
  const trimmedBase = baseUrl.replace(/\/+$/, "");
  const trimmedPath = path.replace(/^\/+/, "");
  return `${trimmedBase}/${trimmedPath}`;
}

export function looksLikeAnthropicModel(model: string): boolean {
  return /^claude/i.test(model.trim());
}

export function looksLikeOpenAIModel(model: string): boolean {
  return /^(gpt|o1|o3|o4|text-embedding|gpt-oss)/i.test(model.trim());
}

export function looksLikeLlamaCppModel(model: string): boolean {
  const value = model.trim().toLowerCase();
  return value.endsWith(".gguf") || value.includes("gguf");
}

export function looksLikeOllamaModel(model: string): boolean {
  const value = model.trim();
  return value.includes(":") || /^(llama|qwen|gemma|mistral|phi|deepseek|codestral)/i.test(value);
}

export function isAppleSiliconHost(): boolean {
  return process.platform === "darwin" && process.arch === "arm64";
}
