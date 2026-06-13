/* MLX Pilot — App state & configuration (core).
 *
 * The single mutable `state` object shared across modules (imported by
 * reference, never reassigned), the provider/config constants, and the
 * localStorage read helpers. No DOM, no API calls.
 */

export const DEFAULT_DAEMON_URL = 'http://127.0.0.1:11435';
export const DAEMON_READY_EVENT = 'mlx-pilot-daemon-ready';
export const API_DEFAULT_TIMEOUT_MS = 8000;
export const API_SLOW_TIMEOUT_MS = 20000;
export const MIN_SPLASH_MS = 480;
export const MODEL_CACHE_KEY = 'mlxPilotModelCache';
export const CURRENT_MODEL_KEY = 'mlxPilotCurrentModel';
export const AGENT_LOCAL_PROVIDER_CHOICE = 'mlx-pilot-local';
export const CLOUD_PROVIDER_DEFAULTS = {
  anthropic: {
    label: 'Anthropic',
    modelId: 'claude-3.5-sonnet',
    secretKeys: ['ANTHROPIC_API_KEY'],
  },
  openai: {
    label: 'OpenAI',
    modelId: 'gpt-4o-mini',
    secretKeys: ['OPENAI_API_KEY'],
  },
  openrouter: {
    label: 'OpenRouter',
    modelId: 'openai/gpt-4o-mini',
    secretKeys: ['OPENROUTER_API_KEY'],
  },
  deepseek: {
    label: 'DeepSeek',
    modelId: 'deepseek-v4-flash',
    secretKeys: ['DEEPSEEK_API_KEY'],
  },
  groq: {
    label: 'Groq',
    modelId: 'llama-3.3-70b-versatile',
    secretKeys: ['GROQ_API_KEY'],
  },
  gemini: {
    label: 'Gemini',
    modelId: 'gemini-2.0-flash',
    secretKeys: ['GEMINI_API_KEY', 'GOOGLE_API_KEY'],
  },
  zai: {
    label: 'ZAI',
    modelId: 'glm-4.5',
    secretKeys: ['ZAI_API_KEY'],
  },
  perplexity: {
    label: 'Perplexity',
    modelId: 'sonar-pro',
    secretKeys: ['PERPLEXITY_API_KEY'],
  },
};
export const AGENT_PROVIDER_PROFILE_TYPES = [
  { value: 'ollama', label: 'Ollama (local)' },
  { value: 'mlx', label: 'MLX (local)' },
  { value: 'llamacpp', label: 'llama.cpp (local)' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'anthropic', label: 'Anthropic' },
  { value: 'openrouter', label: 'OpenRouter' },
  { value: 'deepseek', label: 'DeepSeek' },
  { value: 'groq', label: 'Groq' },
  { value: 'gemini', label: 'Gemini' },
  { value: 'zai', label: 'ZAI' },
  { value: 'perplexity', label: 'Perplexity' },
];

export function readStorage(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function readJsonStorage(key, fallback) {
  const raw = readStorage(key);
  if (!raw) return fallback;
  try {
    return JSON.parse(raw);
  } catch {
    return fallback;
  }
}

export const cachedModels = readJsonStorage(MODEL_CACHE_KEY, []);
export const cachedCurrentModel = readStorage(CURRENT_MODEL_KEY);

// -- State --------------------------------------------------
export const state = {
  daemonUrl: window.__MLX_PILOT_DAEMON_URL__ || readStorage('mlxPilotDaemonUrl') || DEFAULT_DAEMON_URL,
  models: Array.isArray(cachedModels) ? cachedModels : [],
  installedModels: [],
  modelGroups: [],
  modelsLoaded: Array.isArray(cachedModels) && cachedModels.length > 0,
  modelsLoading: false,
  modelsStale: true,
  modelsPromise: null,
  currentModel: cachedCurrentModel || null,
  messages: [],
  isStreaming: false,
  streamController: null,
  webSearchEnabled: false,
  airllmEnabled: false,
  healthOk: false,
  provider: '',
  runtimeStartup: null,
  daemonConfig: null,
  catalogModels: [],
  downloads: [],
  downloadsLoading: false,
  downloadRefreshTimer: null,
  agentConfig: null,
  agentSessions: [],
  currentSessionId: null,
  auditEntries: [],
  auditFilter: 'all',
  plugins: [],
  skills: [],
  tools: [],
  channels: [],
  environmentVars: [],
  agentProviderOptions: [],
  consoleEntries: [],
  desktopLogEntries: [],
  desktopRuntimeInfo: null,
  pendingAgentShortcut: null,
  activeDiscoverTab: 'catalog',
  activePanel: 'chat',
};
