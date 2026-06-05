import {
  isAppleSiliconHost,
  looksLikeAnthropicModel,
  looksLikeLlamaCppModel,
  looksLikeOllamaModel,
  looksLikeOpenAIModel,
  stripProviderPrefix,
} from "./helpers.ts";
import { anthropicMessagesAdapter } from "./anthropic_messages.ts";
import { llamacppAdapter } from "./llamacpp.ts";
import { mlxAdapter } from "./mlx.ts";
import { ollamaAdapter } from "./ollama.ts";
import { openAIChatAdapter } from "./openai_chat.ts";
import type { UniAdapter, UniProvider, UniRequest, UniResponse } from "./types.ts";

const ADAPTERS: Record<UniProvider, UniAdapter> = {
  anthropic: anthropicMessagesAdapter,
  openai: openAIChatAdapter,
  ollama: ollamaAdapter,
  mlx: mlxAdapter,
  llamacpp: llamacppAdapter,
};

export function resolveProvider(request: UniRequest): UniProvider {
  if (request.provider_hint) {
    return request.provider_hint;
  }

  const prefixed = stripProviderPrefix(request.model);
  if (prefixed.provider) {
    return prefixed.provider;
  }

  const configured = configuredProvider();
  if (configured) {
    return configured;
  }

  if (looksLikeAnthropicModel(request.model)) {
    return "anthropic";
  }

  if (looksLikeOpenAIModel(request.model)) {
    return "openai";
  }

  if (looksLikeLlamaCppModel(request.model)) {
    return "llamacpp";
  }

  if (looksLikeOllamaModel(request.model)) {
    return "ollama";
  }

  if (
    isAppleSiliconHost() &&
    (request.transport?.mlx?.executor ||
      request.transport?.mlx?.base_url ||
      process.env.MLX_PILOT_DAEMON_URL)
  ) {
    return "mlx";
  }

  if (
    request.transport?.llamacpp?.executor ||
    request.transport?.llamacpp?.base_url ||
    process.env.LLAMACPP_BASE_URL
  ) {
    return "llamacpp";
  }

  if (
    request.transport?.ollama?.executor ||
    request.transport?.ollama?.base_url ||
    process.env.OLLAMA_BASE_URL
  ) {
    return "ollama";
  }

  throw new Error(
    `Unable to infer provider for model '${request.model}'. Set provider_hint explicitly.`,
  );
}

export async function chat(request: UniRequest): Promise<UniResponse> {
  const provider = resolveProvider(request);
  const adapter = ADAPTERS[provider];
  const normalizedRequest =
    provider === request.provider_hint
      ? request
      : {
          ...request,
          provider_hint: provider,
        };
  return adapter.chat(normalizedRequest);
}

function configuredProvider(): UniProvider | null {
  const raw = (
    process.env.MLX_PILOT_UNIFIED_PROVIDER ??
    process.env.APP_PROVIDER_MODE ??
    ""
  )
    .trim()
    .toLowerCase();

  if (
    raw === "mlx" ||
    raw === "ollama" ||
    raw === "llamacpp" ||
    raw === "anthropic" ||
    raw === "openai"
  ) {
    return raw;
  }

  return null;
}
