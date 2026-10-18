export interface ToolDefinition<T = Record<string, unknown>, R = unknown> {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
  mutating?: boolean;
  /** Synchronous WASM execution. Use createHostClient for declared host capabilities. */
  execute(input: T): R;
}
export type PluginPermission =
  | "compute"
  | "network:request"
  | "credentials:use"
  | "documents:read"
  | "documents:write";
export type HttpMethod =
  | "GET"
  | "POST"
  | "PUT"
  | "PATCH"
  | "DELETE"
  | "HEAD"
  | "OPTIONS";
export interface ServiceDefinition {
  id: string;
  name: string;
  /** HTTPS endpoint, or loopback HTTP requiring separate user authorization. */
  base_url: string;
  methods: HttpMethod[];
  /** The host injects this stored credential. Its value is never returned to plugins. */
  credential?: { id: string; header: string; prefix?: string };
}
export interface PluginDefinition {
  id: string;
  name: string;
  version: string;
  permissions?: PluginPermission[];
  services?: ServiceDefinition[];
  tools: ToolDefinition<any, any>[];
  /** Source paths bundled by the CLI into isolated browser assets. */
  frontend?: { html: string; script?: string; style?: string };
}
export function defineTool<T, R>(
  definition: ToolDefinition<T, R>,
): ToolDefinition<T, R>;
export function definePlugin(definition: PluginDefinition): PluginDefinition;

export interface HttpRequest {
  service: string;
  /** Relative to the service base URL; absolute URLs are not accepted. */
  path: string;
  method?: HttpMethod;
  body?: unknown;
  headers?: Record<string, string>;
  response_type?: "json" | "text" | "base64";
}
export interface HttpResponse<T = unknown> {
  status: number;
  headers: Record<string, string>;
  body: T;
  encoding: "json" | "text" | "base64";
}
export interface PresentationDocument {
  schema_version: number;
  id: string;
  revision: number;
  title: string;
  brief: string;
  theme: Record<string, unknown>;
  slides: Record<string, unknown>[];
  /** Asset metadata only: get never returns image data_url values. */
  assets: Record<string, unknown>[];
  css: string;
  script: string;
}
export interface DocumentSummary {
  id: string;
  title: string;
  revision: number;
  slides: number;
}
export interface DocumentEdit {
  id: string;
  base_revision: number;
  operations: Record<string, unknown>[];
}
export interface DocumentEditResult {
  id: string;
  revision: number;
  slides: number;
  diagnostics: Record<string, unknown>[];
}
export interface HostClient {
  http: { request<T = unknown>(input: HttpRequest): HttpResponse<T> };
  credentials: { status(service: string): { configured: boolean } };
  documents: {
    list(): DocumentSummary[];
    get(id: string): PresentationDocument;
    create(input: { title: string }): PresentationDocument;
    edit(input: DocumentEdit): DocumentEditResult;
  };
}
export interface FrontendClient {
  invoke<T = unknown>(tool: string, input: Record<string, unknown>): Promise<T>;
  http: { request<T = unknown>(input: HttpRequest): Promise<HttpResponse<T>> };
  credentials: { status(service: string): Promise<{ configured: boolean }> };
  documents: {
    list(): Promise<DocumentSummary[]>;
    get(id: string): Promise<PresentationDocument>;
    create(input: { title: string }): Promise<PresentationDocument>;
    edit(input: DocumentEdit): Promise<DocumentEditResult>;
  };
  dispose(): void;
}
export function createHostClient(): HostClient;
export function createFrontendClient(options?: {
  timeout?: number;
}): FrontendClient;
