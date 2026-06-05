import {
  executeJsonRequest,
  flattenTextContent,
  generateToolCallId,
  getTransport,
  isRecord,
} from "./helpers.ts";
import type {
  UniAdapter,
  UniMessage,
  UniRequest,
  UniResponse,
  UniToolDef,
} from "./types.ts";

type OpenAITool = {
  type: "function";
  function: {
    name: string;
    description?: string;
    parameters: Record<string, unknown>;
  };
};

type OpenAIMessage = {
  role: "system" | "user" | "assistant" | "tool";
  content: string | null;
  tool_call_id?: string;
  tool_calls?: Array<{
    id?: string;
    type?: "function";
    function?: {
      name?: string;
      arguments?: string;
    };
  }>;
};

export function toOpenAIRequest(request: UniRequest): Record<string, unknown> {
  const messages: OpenAIMessage[] = request.messages.map((message) => {
    if (message.role === "tool") {
      const result = message.tool_result;
      return {
        role: "tool",
        content:
          typeof result?.output === "string"
            ? result.output
            : JSON.stringify(result?.output ?? message.content),
        tool_call_id: result?.tool_call_id,
      };
    }

    if (message.role === "assistant" && message.tool_call) {
      return {
        role: "assistant",
        content: message.content || "",
        tool_calls: [
          {
            id: message.tool_call.id,
            type: "function",
            function: {
              name: message.tool_call.name,
              arguments: JSON.stringify(message.tool_call.arguments ?? {}),
            },
          },
        ],
      };
    }

    return {
      role: message.role,
      content: message.content,
    };
  });

  const tools = request.tools?.map(toOpenAITool);

  return {
    model: request.model,
    messages,
    tools,
    tool_choice: tools?.length ? "auto" : undefined,
    temperature: request.temperature,
    max_tokens: request.max_tokens,
    stream: false,
  };
}

export function fromOpenAIResponse(raw: unknown): UniResponse {
  if (!isRecord(raw) || !Array.isArray(raw.choices) || raw.choices.length === 0) {
    throw new Error("Invalid OpenAI response");
  }

  const choice = raw.choices[0];
  if (!isRecord(choice) || !isRecord(choice.message)) {
    throw new Error("Invalid OpenAI response choice");
  }

  const message = choice.message;
  const messages: UniMessage[] = [];
  const content = flattenTextContent(message.content);

  if (content) {
    messages.push({
      role: "assistant",
      content,
    });
  }

  if (Array.isArray(message.tool_calls)) {
    for (const entry of message.tool_calls) {
      if (!isRecord(entry) || !isRecord(entry.function) || typeof entry.function.name !== "string") {
        continue;
      }

      let parsedArguments: unknown = {};
      const rawArguments =
        typeof entry.function.arguments === "string" ? entry.function.arguments : "{}";
      try {
        parsedArguments = JSON.parse(rawArguments);
      } catch {
        parsedArguments = rawArguments;
      }

      messages.push({
        role: "assistant",
        content: "",
        tool_call: {
          id:
            typeof entry.id === "string" && entry.id
              ? entry.id
              : generateToolCallId("openai"),
          name: entry.function.name,
          arguments: parsedArguments,
        },
      });
    }
  }

  if (!messages.length) {
    messages.push({
      role: "assistant",
      content: "",
    });
  }

  return {
    model: typeof raw.model === "string" ? raw.model : "unknown",
    provider: "openai",
    messages,
    raw,
    stop_reason:
      typeof choice.finish_reason === "string" ? choice.finish_reason : undefined,
  };
}

export async function executeOpenAI(request: UniRequest): Promise<UniResponse> {
  const transport = getTransport(request, "openai", {
    base_url: process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1",
    api_key: process.env.OPENAI_API_KEY,
  });
  const payload = toOpenAIRequest(request);
  const raw = await executeJsonRequest(
    request,
    "openai",
    "/chat/completions",
    payload,
    transport,
  );
  return fromOpenAIResponse(raw);
}

function toOpenAITool(tool: UniToolDef): OpenAITool {
  return {
    type: "function",
    function: {
      name: tool.name,
      description: tool.description,
      parameters: tool.input_schema,
    },
  };
}

export const openAIChatAdapter: UniAdapter = {
  provider: "openai",
  chat: executeOpenAI,
};
