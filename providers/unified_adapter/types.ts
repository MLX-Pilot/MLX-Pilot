export type UniProvider = "anthropic" | "openai" | "ollama" | "mlx" | "llamacpp";

export type UniRole = "system" | "user" | "assistant" | "tool";

export type UniToolDef = {
  name: string;
  description?: string;
  input_schema: Record<string, unknown>;
};

export type UniToolCall = {
  id: string;
  name: string;
  arguments: unknown;
};

export type UniToolResult = {
  tool_call_id: string;
  output: unknown;
  is_error?: boolean;
};

export type UniMessage = {
  role: UniRole;
  content: string;
  tool_call?: UniToolCall | null;
  tool_result?: UniToolResult | null;
};

export type UniTransportExecutionContext = {
  provider: UniProvider;
  path: string;
  request: UniRequest;
  payload: unknown;
};

export type UniTransportExecutor = (
  payload: unknown,
  context: UniTransportExecutionContext,
) => Promise<unknown>;

export type UniProviderTransport = {
  base_url?: string;
  api_key?: string;
  headers?: Record<string, string>;
  timeout_ms?: number;
  native_tool_calling?: boolean;
  anthropic_version?: string;
  executor?: UniTransportExecutor;
};

export type UniRequest = {
  model: string;
  messages: UniMessage[];
  tools?: UniToolDef[];
  temperature?: number;
  max_tokens?: number;
  provider_hint?: UniProvider;
  metadata?: Record<string, unknown>;
  transport?: Partial<Record<UniProvider, UniProviderTransport>>;
};

export type UniResponse = {
  model: string;
  provider: UniProvider;
  messages: UniMessage[];
  raw?: unknown;
  stop_reason?: string;
};

export type UniAdapter = {
  readonly provider: UniProvider;
  chat(request: UniRequest): Promise<UniResponse>;
};
