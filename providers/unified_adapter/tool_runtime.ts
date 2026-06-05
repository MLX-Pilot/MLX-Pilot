import type { UniToolCall, UniToolDef, UniToolResult } from "./types.ts";

export type ToolRuntime = (definition: UniToolDef, args: unknown) => Promise<unknown>;

export async function runTool(
  call: UniToolCall,
  registry: Record<string, ToolRuntime>,
  defs?: Record<string, UniToolDef>,
): Promise<UniToolResult> {
  const runtime = registry[call.name];
  if (!runtime) {
    return {
      tool_call_id: call.id,
      output: { error: `Tool not found: ${call.name}` },
      is_error: true,
    };
  }

  try {
    const definition = defs?.[call.name] ?? {
      name: call.name,
      input_schema: {},
    };
    const output = await runtime(definition, call.arguments);
    return {
      tool_call_id: call.id,
      output,
    };
  } catch (error) {
    return {
      tool_call_id: call.id,
      output: {
        error: error instanceof Error ? error.message : String(error),
      },
      is_error: true,
    };
  }
}
