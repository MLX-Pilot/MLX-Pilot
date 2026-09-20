import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { handleFlows } from './mock-flows.mjs';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const UI_DIR = path.join(__dirname, '..', 'apps', 'desktop-ui', 'ui');
const PORT = 11435;

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'application/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
};

function json(res, code, data) {
  res.writeHead(code, { 'Content-Type': 'application/json', 'Access-Control-Allow-Origin': '*' });
  res.end(JSON.stringify(data));
}

function serveFile(res, filePath) {
  try {
    const content = fs.readFileSync(filePath);
    const ext = path.extname(filePath).toLowerCase();
    res.writeHead(200, { 'Content-Type': MIME[ext] || 'application/octet-stream' });
    res.end(content);
  } catch {
    res.writeHead(404);
    res.end('Not found');
  }
}

// --- Mock data ---

const startupSnapshot = {
  phase: 'ready',
  step: 'ready',
  message: 'Providers locais prontos',
  app_ready: true,
  degraded: false,
  can_cancel: false,
  progress_percent: 100,
  operation_id: 'mock-startup',
  providers: [{ id: 'ollama', status: 'ready', message: 'mock' }],
  error: null,
};

const installedModels = [
  { id: 'ollama::qwen3.5:9b', name: 'qwen3.5:9b [Ollama]', provider: 'ollama', path: 'ollama::qwen3.5:9b', is_available: true, agent_tool_mode: 'tool_ready', agent_recommended: true },
  { id: 'ollama::qwen2.5:7b-instruct-q4_K_M', name: 'qwen2.5:7b [Ollama]', provider: 'ollama', path: 'ollama::qwen2.5:7b-instruct-q4_K_M', is_available: true, agent_tool_mode: 'tool_ready', agent_recommended: false },
  { id: 'ollama::llama3.1:8b', name: 'llama3.1:8b [Ollama]', provider: 'ollama', path: 'ollama::llama3.1:8b', is_available: true, agent_tool_mode: 'chat_only', agent_recommended: false },
  { id: 'mlx-community/Qwen3-4B-4bit', name: 'Qwen3 4B [MLX]', provider: 'mlx', path: 'mlx-community/Qwen3-4B-4bit', is_available: true, agent_tool_mode: 'chat_only', agent_recommended: false },
];

const agentConfig = {
  provider: 'mlx',
  model_id: 'mlx-community/Qwen2.5-14B-Instruct-4bit',
  execution_mode: 'full',
  approval_mode: 'ask',
  default_toolset_id: 'general',
  runtime_variant: 'classic',
};

const sessions = [
  { id: 'sess-001', name: 'Debug login flow', message_count: 12, created_at: new Date().toISOString() },
  { id: 'sess-002', name: 'Refactor API layer', message_count: 8, created_at: new Date(Date.now() - 86400000).toISOString() },
  { id: 'sess-003', name: 'Database migration', message_count: 3, created_at: new Date(Date.now() - 172800000).toISOString() },
];

const tools = [
  { name: 'read', enabled: true, description: 'Read a file from the local filesystem' },
  { name: 'write', enabled: true, description: 'Writes a file to the local filesystem' },
  { name: 'edit', enabled: true, description: 'Performs exact string replacements in files' },
  { name: 'bash', enabled: true, description: 'Executes a given shell command with optional timeout' },
  { name: 'glob', enabled: true, description: 'Fast file pattern matching tool' },
  { name: 'grep', enabled: true, description: 'Fast content search tool that works with any codebase size' },
  { name: 'web_fetch', enabled: false, description: 'Fetches content from a specified URL and processes into markdown' },
  { name: 'web_search', enabled: false, description: 'Searches the web for information' },
  { name: 'task', enabled: true, description: 'Launch a new agent to handle complex, multistep tasks autonomously' },
  { name: 'question', enabled: true, description: 'Use this tool when you need to ask the user questions during execution' },
  { name: 'todo_write', enabled: true, description: 'Create and maintain a structured task list for the current coding session' },
  { name: 'skill', enabled: true, description: 'Load a specialized skill when the task at hand matches one of the skills' },
  { name: 'notebook_edit', enabled: false, description: 'Edit Jupyter notebook cells' },
  { name: 'planning', enabled: false, description: 'Plan multi-step operations before execution' },
  { name: 'memory_access', enabled: true, description: 'Access persistent agent memory across sessions' },
  { name: 'git_commit', enabled: true, description: 'Create and manage git commits' },
  { name: 'github_pr', enabled: false, description: 'Create and manage GitHub Pull Requests' },
  { name: 'image_analyze', enabled: false, description: 'Analyze images with vision models' },
];

const skills = [
  { name: 'code-reviewer', active: true, enabled: true, description: 'Review changed code for correctness, regressions, missing tests' },
  { name: 'test-fixer', active: true, enabled: true, description: 'Reproduce failing tests or build errors and fix them' },
  { name: 'repo-onboarding', active: true, enabled: true, description: 'Understand an unfamiliar repository quickly' },
  { name: 'session-orchestrator', active: false, enabled: false, description: 'Coordinate multi-step work by spawning focused local agent sessions' },
  { name: 'git-worktree-manager', active: false, enabled: false, description: 'Create and manage isolated git worktrees and branches' },
  { name: 'customize-opencode', active: false, enabled: false, description: 'Editing or creating opencode configuration' },
  { name: 'find-skills', active: false, enabled: false, description: 'Helps users discover and install agent skills' },
  { name: 'remotion-best-practices', active: false, enabled: false, description: 'Best practices for Remotion - Video creation in React' },
  { name: 'pdf-extractor', active: true, enabled: true, description: 'Extract text and metadata from PDF files' },
  { name: 'csv-analyzer', active: false, enabled: false, description: 'Parse and analyze CSV data files' },
  { name: 'docker-manager', active: true, enabled: true, description: 'Manage Docker containers and images' },
  { name: 'sql-query-builder', active: false, enabled: false, description: 'Build and optimize SQL queries' },
];

const plugins = [
  { id: 'memory', plugin_id: 'memory', name: 'memory', enabled: true, description: 'Persistent memory backend for agent context' },
  { id: 'code-executor', plugin_id: 'code-executor', name: 'code-executor', enabled: true, description: 'Sandboxed code execution environment' },
  { id: 'web-browser', plugin_id: 'web-browser', name: 'web-browser', enabled: false, description: 'Headless browser for web interactions' },
  { id: 'file-watcher', plugin_id: 'file-watcher', name: 'file-watcher', enabled: true, description: 'Monitor file system changes in real-time' },
  { id: 'notification', plugin_id: 'notification', name: 'notification', enabled: false, description: 'Desktop and mobile push notifications' },
  { id: 'scheduler', plugin_id: 'scheduler', name: 'scheduler', enabled: true, description: 'Schedule periodic agent tasks and checks' },
];

const channels = [
  {
    channel_id: 'slack',
    id: 'slack',
    name: 'Slack',
    accounts: [
      { account_id: 'T07A1B2C3D', id: 'T07A1B2C3D', workspace: 'myteam', active: true, status: 'connected' },
    ],
  },
  {
    channel_id: 'github',
    id: 'github',
    name: 'GitHub',
    accounts: [
      { account_id: 'gh-user-42', id: 'gh-user-42', login: 'devbot', active: true, status: 'connected', scopes: ['repo', 'read:org'] },
    ],
  },
  {
    channel_id: 'webhook',
    id: 'webhook',
    name: 'Webhook',
    accounts: [],
  },
  {
    channel_id: 'discord',
    id: 'discord',
    name: 'Discord',
    accounts: [
      { account_id: 'disc-998877', id: 'disc-998877', guild: 'MLX Server', active: false, status: 'disconnected' },
    ],
  },
];

const auditEntries = [
  { event_type: 'command', tool_name: 'bash', status: 'approved', summary: 'npm install react@latest', timestamp: new Date(Date.now() - 60000).toISOString() },
  { event_type: 'command', tool_name: 'read', status: 'approved', summary: '/src/App.tsx (245 lines)', timestamp: new Date(Date.now() - 120000).toISOString() },
  { event_type: 'command', tool_name: 'edit', status: 'approved', summary: 'Replace function signature in utils.ts', timestamp: new Date(Date.now() - 180000).toISOString() },
  { event_type: 'command', tool_name: 'write', status: 'approved', summary: 'Created tests/feature.test.ts', timestamp: new Date(Date.now() - 240000).toISOString() },
  { event_type: 'command', tool_name: 'bash', status: 'denied', summary: 'rm -rf /var/critical-data', timestamp: new Date(Date.now() - 300000).toISOString() },
  { event_type: 'session', status: 'created', summary: 'Nova sessao iniciada', timestamp: new Date(Date.now() - 360000).toISOString() },
  { event_type: 'plugin', tool_name: null, status: 'enabled', summary: 'Plugin memory ativado', timestamp: new Date(Date.now() - 420000).toISOString() },
  { event_type: 'config', status: 'updated', summary: 'Execution mode alterado para full', timestamp: new Date(Date.now() - 480000).toISOString() },
  { event_type: 'command', tool_name: 'grep', status: 'approved', summary: 'Busca por "useState" em 45 arquivos', timestamp: new Date(Date.now() - 540000).toISOString() },
  { event_type: 'skill', status: 'installed', summary: 'Skill code-reviewer instalada', timestamp: new Date(Date.now() - 600000).toISOString() },
  { event_type: 'channel', tool_name: null, status: 'connected', summary: 'Canal Slack conectado', timestamp: new Date(Date.now() - 660000).toISOString() },
  { event_type: 'command', tool_name: 'glob', status: 'approved', summary: 'Procurar **/*.tsx', timestamp: new Date(Date.now() - 720000).toISOString() },
];

const providerProfiles = [
  { id: 'prof-001', name: 'Local MLX', provider: 'mlx', model_id: 'mlx-community/Qwen2.5-14B-Instruct-4bit', endpoint: '', secret_ref: '', is_default: true },
  { id: 'prof-002', name: 'Local Ollama', provider: 'ollama', model_id: 'llama3.1:8b', endpoint: 'http://localhost:11434', secret_ref: '', is_default: false },
  { id: 'prof-003', name: 'OpenAI Cloud', provider: 'openai', model_id: 'gpt-4o-mini', endpoint: '', secret_ref: 'OPENAI_API_KEY', is_default: false },
  { id: 'prof-004', name: 'Anthropic Cloud', provider: 'anthropic', model_id: 'claude-3.5-sonnet', endpoint: '', secret_ref: 'ANTHROPIC_API_KEY', is_default: false },
];

const environmentVars = [
  { key: 'ANTHROPIC_API_KEY', is_secret: true, present: true, masked: 'sk-ant-***...AB12', value: 'sk-ant-api03-xxxxxxxxxxxxxxxxxxxx-AB12' },
  { key: 'OPENAI_API_KEY', is_secret: true, present: true, masked: 'sk-***...CD34', value: 'sk-proj-xxxxxxxxxxxxxxxxxxxxxxxxxxxx-CD34' },
  { key: 'GROQ_API_KEY', is_secret: true, present: false, masked: '', value: '' },
  { key: 'GEMINI_API_KEY', is_secret: true, present: false, masked: '', value: '' },
  { key: 'NODE_ENV', is_secret: false, present: true, masked: '', value: 'development' },
  { key: 'LOG_LEVEL', is_secret: false, present: true, masked: '', value: 'info' },
];

const daemonConfig = {
  mlx_command: 'python -m mlx_lm.server',
  mlx_prefix_args: '--model',
  mlx_timeout_secs: 30,
  mlx_airllm_threshold_percent: 70,
  mlx_airllm_python_command: 'python3',
  mlx_airllm_runner: 'mlx',
  llamacpp_server_binary: 'llama-server',
  llamacpp_base_url: 'http://localhost:8080',
  llamacpp_context_size: 8192,
  llamacpp_auto_start: true,
  llamacpp_auto_install: false,
};

const toolsCatalog = tools.map(t => ({ ...t, category: 'io', risk: 'low' }));

const effectivePolicy = {
  execution_mode: 'full',
  approval_mode: 'ask',
  default_toolset_id: 'general',
  tools,
};

// --- Server ---

const server = http.createServer((req, res) => {
  const url = new URL(req.url, `http://localhost:${PORT}`);
  const method = req.method.toUpperCase();
  const pathname = url.pathname;

  // CORS headers
  res.setHeader('Access-Control-Allow-Origin', '*');
  res.setHeader('Access-Control-Allow-Methods', 'GET, POST, PUT, DELETE, OPTIONS');
  res.setHeader('Access-Control-Allow-Headers', 'Content-Type, x-channel-protocol-version');

  if (method === 'OPTIONS') {
    res.writeHead(204);
    res.end();
    return;
  }

  // --- Static files ---
  // `/flows` fica de fora: e API, senao viraria busca de arquivo e devolveria
  // 404 em texto puro no lugar do JSON que a aba Workflows espera.
  if (method === 'GET' && !pathname.startsWith('/agent/') && !pathname.startsWith('/health') && !pathname.startsWith('/config') && !pathname.startsWith('/environment') && !pathname.startsWith('/chat') && !pathname.startsWith('/catalog') && !pathname.startsWith('/models') && !pathname.startsWith('/runtime') && pathname !== '/flows' && !pathname.startsWith('/flows/')) {
    let file = pathname === '/' ? 'index.html' : pathname.slice(1);
    file = path.normalize(file).replace(/^(\.\.(\/|\\|$))+/, '');
    serveFile(res, path.join(UI_DIR, file));
    return;
  }

  // --- Read body ---
  let body = '';
  req.on('data', chunk => { body += chunk; });
  req.on('end', () => {
    let payload = null;
    try { payload = body ? JSON.parse(body) : null; } catch {}

    const flowsResponse = handleFlows(pathname, method, payload);
    if (flowsResponse) return json(res, flowsResponse.status, flowsResponse.body);

    switch (pathname) {
      // Health
      case '/health':
        if (method === 'GET') return json(res, 200, { status: 'ok', provider: 'mlx', version: '1.0.0' });
        break;

      // Inicializacao: `app_ready` encerra o polling do splash.
      case '/runtime/startup':
      case '/runtime/startup/retry':
      case '/runtime/startup/cancel':
        return json(res, 200, startupSnapshot);

      // Modelos instalados, usados pelo seletor de modelo e pelo no agent.run
      case '/models':
      case '/models/all':
        if (method === 'GET') return json(res, 200, installedModels);
        break;

      // Daemon config
      case '/config':
        if (method === 'GET') return json(res, 200, daemonConfig);
        if (method === 'POST') return json(res, 200, { ...daemonConfig, ...payload });
        break;

      // Agent config
      case '/agent/config':
        if (method === 'GET') return json(res, 200, agentConfig);
        if (method === 'POST') {
          Object.assign(agentConfig, payload || {});
          return json(res, 200, agentConfig);
        }
        break;

      // Sessions
      case '/agent/sessions':
        if (method === 'GET') return json(res, 200, sessions);
        if (method === 'POST') {
          const s = { id: `sess-${Date.now()}`, name: payload?.name || 'Nova sessao', message_count: 0, created_at: new Date().toISOString() };
          sessions.unshift(s);
          return json(res, 200, s);
        }
        break;

      // Tools
      case '/agent/tools':
        if (method === 'GET') return json(res, 200, tools);
        break;
      case '/agent/tools/catalog':
        if (method === 'GET') return json(res, 200, toolsCatalog);
        break;
      case '/agent/tools/effective-policy':
        if (method === 'GET') return json(res, 200, effectivePolicy);
        break;
      case '/agent/tools/profile':
        if (method === 'POST') return json(res, 200, { ok: true, ...payload });
        break;
      case '/agent/tools/allow-deny':
        if (method === 'POST') return json(res, 200, { ok: true, ...payload });
        break;

      // Skills
      case '/agent/skills/check':
        if (method === 'GET') return json(res, 200, { skills });
        break;
      case '/agent/skills/enable':
      case '/agent/skills/disable':
        if (method === 'POST') {
          const name = payload?.skill;
          const s = skills.find(sk => sk.name === name);
          if (s) { s.active = pathname.includes('enable'); s.enabled = s.active; }
          return json(res, 200, { ok: true });
        }
        break;
      case '/agent/skills/config':
        if (method === 'POST') return json(res, 200, { ok: true, ...payload });
        break;
      case '/agent/skills/install':
        if (method === 'POST') {
          const name = payload?.skill || payload?.name;
          if (name && !skills.some(sk => sk.name === name)) {
            skills.push({ name, active: true, enabled: true, description: `Skill ${name} instalada` });
          }
          return json(res, 200, { ok: true });
        }
        break;

      // Plugins
      case '/agent/plugins':
        if (method === 'GET') return json(res, 200, plugins);
        break;
      case '/agent/plugins/enable':
      case '/agent/plugins/disable':
        if (method === 'POST') {
          const id = payload?.plugin_id;
          const p = plugins.find(pl => (pl.id || pl.plugin_id) === id);
          if (p) p.enabled = pathname.includes('enable');
          return json(res, 200, { ok: true });
        }
        break;
      case '/agent/plugins/config':
        if (method === 'POST') return json(res, 200, { ok: true, enabled: payload?.enabled ?? true, ...payload });
        break;

      // Channels
      case '/agent/channels':
        if (method === 'GET') return json(res, 200, channels);
        break;
      case '/agent/channels/status':
        if (method === 'GET') return json(res, 200, channels.map(ch => ({ channel_id: ch.channel_id, connected: ch.accounts.some(a => a.active) })));
        break;
      case '/agent/channels/logs':
        if (method === 'GET') return json(res, 200, { entries: [] });
        break;
      case '/agent/channels/upsert':
      case '/agent/channels/upsert-account':
        if (method === 'POST') return json(res, 200, { ok: true });
        break;
      case '/agent/channels/remove':
      case '/agent/channels/remove-account':
        if (method === 'POST') return json(res, 200, { ok: true });
        break;
      case '/agent/channels/login':
        if (method === 'POST') return json(res, 200, { ok: true, url: 'https://example.com/oauth' });
        break;
      case '/agent/channels/logout':
        if (method === 'POST') return json(res, 200, { ok: true });
        break;
      case '/agent/channels/probe':
      case '/agent/channels/resolve':
        if (method === 'POST') return json(res, 200, { ok: true, found: true });
        break;
      case '/agent/message/send':
        if (method === 'POST') return json(res, 200, { ok: true, sent: true });
        break;
      case '/agent/channels/status':
        if (method === 'GET') return json(res, 200, channels.map(ch => ({ channel_id: ch.channel_id, connected: ch.accounts.some(a => a.active) })));
        break;

      // Audit
      case '/agent/audit':
        if (method === 'GET') {
          const limit = parseInt(url.searchParams.get('limit') || '30', 10);
          return json(res, 200, { entries: auditEntries.slice(0, limit) });
        }
        break;

      // Provider profiles
      case '/agent/provider-profiles':
        if (method === 'GET') return json(res, 200, providerProfiles);
        if (method === 'POST') {
          const idx = providerProfiles.findIndex(p => p.id === payload?.id);
          if (idx >= 0) {
            Object.assign(providerProfiles[idx], payload || {});
          } else {
            providerProfiles.push({ id: `prof-${Date.now()}`, ...payload });
          }
          return json(res, 200, providerProfiles);
        }
        break;

      // Environment
      case '/environment':
        if (method === 'GET') return json(res, 200, { variables: environmentVars });
        break;

      // Agent run
      case '/agent/run':
        if (method === 'POST') return json(res, 200, { result: 'Operacao executada com sucesso.', ok: true });
        break;

      // Agent context budget
      case '/agent/context/budget':
        if (method === 'GET') return json(res, 200, { budget: 200000, used: 45000, remaining: 155000 });
        break;

      // Chat stream
      case '/chat/stream':
        if (method === 'POST') return json(res, 200, { message: 'Resposta do modelo.', role: 'assistant' });
        break;

      // Agent stream
      case '/agent/stream':
        if (method === 'POST') return json(res, 200, { message: 'Agente executou a operacao.', role: 'assistant' });
        break;

      default:
        // Serve static files that start with /agent/ but don't match endpoints (e.g., /agent/sessions/{id}/export)
        if (method === 'GET' && !pathname.startsWith('/chat') && !pathname.startsWith('/catalog')) {
          let file = pathname.slice(1);
          file = path.normalize(file).replace(/^(\.\.(\/|\\|$))+/, '');
          const fp = path.join(UI_DIR, file);
          if (fs.existsSync(fp)) return serveFile(res, fp);
        }
        json(res, 200, { echo: true, path: pathname, method, payload });
    }
  });
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`Mock daemon em http://127.0.0.1:${PORT}`);
  console.log(`UI servida de: ${UI_DIR}`);
});
