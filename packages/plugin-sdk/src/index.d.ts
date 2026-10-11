export interface ToolDefinition<T = Record<string, unknown>, R = unknown> {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  mutating?: boolean;
  /** Pure, synchronous computation. No Node, filesystem, network or DOM APIs. */
  execute(input: T): R;
}
export interface PluginDefinition {
  id: string;
  name: string;
  version: string;
  permissions?: string[];
  tools: ToolDefinition<any, any>[];
  /** Source paths bundled by the CLI into isolated browser assets. */
  frontend?: { html: string; script?: string; style?: string };
}
export function defineTool<T, R>(
  definition: ToolDefinition<T, R>,
): ToolDefinition<T, R>;
export function definePlugin(definition: PluginDefinition): PluginDefinition;

export function createFrontendClient(options?: { timeout?: number }): {
  invoke<T = unknown>(tool: string, input: Record<string, unknown>): Promise<T>;
  dispose(): void;
};
