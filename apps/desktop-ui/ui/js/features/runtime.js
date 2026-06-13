/* MLX Pilot — Runtime bootstrap & shell (feature/bootstrap).
 *
 * Splash reveal, runtime/provider startup status, daemon connection
 * resolution, the boot sequence + startApp() kickoff, and the topbar/sidebar
 * status, model-label and workspace-summary helpers. Runs the boot side
 * effects (startApp) at module load.
 */

// === auto-imports (generated — do not edit) ===
import { pushConsoleEntry } from '../../app.js';
import { api } from '../core/api.js';
import { esc, fmtBytes } from '../core/dom.js';
import { CURRENT_MODEL_KEY, DAEMON_READY_EVENT, DEFAULT_DAEMON_URL, MIN_SPLASH_MS, MODEL_CACHE_KEY, readStorage, state } from '../core/state.js';
import { loadAudit, loadChannels, loadPlugins, loadSkills, loadTools } from './agent.js';
import { loadSessions } from './chat.js';
import { loadConsoleSnapshot, loadEnvironment } from './console.js';
import { loadModels, renderInstalledModels, renderModelPicker } from './models.js';
import { activeAgentModelId, activeModelId, humanizeModelLabel, inferModelProvider, isLocalProvider, providerDisplayName, renderAgentProviderSelector, resolveModelId, selectedAgentProviderOption } from './providers.js';
import { loadAgentConfig, loadDaemonConfig } from './settings.js';
// === end auto-imports ===

  // -- API ----------------------------------------------------
  // -- Splash -------------------------------------------------
  const splash = document.getElementById('splash');
  const appEl = document.getElementById('app');
  const splashStartedAt = performance.now();

  function revealApp() {
    const remaining = Math.max(0, MIN_SPLASH_MS - (performance.now() - splashStartedAt));
    setTimeout(() => {
      if (!splash || !appEl) return;
      splash.classList.add('fade-out');
      appEl.classList.remove('hidden');
      setTimeout(() => { splash.style.display = 'none'; }, 450);
    }, remaining);
  }

  function runtimeProviderStatus(modelId, fallbackProvider = '') {
    const provider = inferModelProvider(modelId, fallbackProvider);
    const normalized = provider === 'llama' || provider === 'llama.cpp' ? 'llamacpp' : provider;
    return state.runtimeStartup?.providers?.find(entry => entry.provider === normalized) || null;
  }

  export function ensureRuntimeReadyForModel(modelId, fallbackProvider = '') {
    const provider = inferModelProvider(modelId, fallbackProvider);
    if (!isLocalProvider(provider)) return;
    const runtime = runtimeProviderStatus(modelId, fallbackProvider);
    if (!runtime || runtime.ready) return;
    const cause = runtime.error || runtime.message || 'provider local indisponivel';
    throw new Error(`${startupProviderLabel(runtime.provider)} nao esta pronto: ${cause}`);
  }

  function startupProviderLabel(provider) {
    return {
      ollama: 'Ollama',
      llamacpp: 'llama.cpp',
      mlx: 'MLX',
    }[provider] || provider;
  }

  function renderStartupStatus(snapshot) {
    if (!snapshot) return;
    state.runtimeStartup = snapshot;
    const message = document.getElementById('startup-message');
    const meta = document.getElementById('startup-meta');
    const progress = document.getElementById('startup-progress');
    const fill = document.getElementById('startup-progress-fill');
    const providers = document.getElementById('startup-providers');
    const cancel = document.getElementById('startup-cancel');
    const retry = document.getElementById('startup-retry');
    const technical = document.getElementById('startup-technical');
    const percent = Number(snapshot.progress_percent);
    const determinate = snapshot.progress_percent != null && Number.isFinite(percent);

    if (message) message.textContent = snapshot.message || 'Preparando providers locais';
    if (progress && fill) {
      progress.classList.toggle('indeterminate', !determinate);
      if (determinate) {
        const normalized = Math.max(0, Math.min(100, percent));
        fill.style.width = `${normalized}%`;
        fill.style.transform = 'none';
        progress.setAttribute('aria-valuenow', String(Math.round(normalized)));
      } else {
        fill.style.width = '';
        fill.style.transform = '';
        progress.removeAttribute('aria-valuenow');
      }
    }
    if (meta) {
      if (determinate && snapshot.bytes_total) {
        const speed = snapshot.bytes_per_second ? ` · ${fmtBytes(snapshot.bytes_per_second)}/s` : '';
        meta.textContent = `${Math.round(percent)}% · ${fmtBytes(snapshot.bytes_downloaded)} de ${fmtBytes(snapshot.bytes_total)}${speed}`;
      } else {
        meta.textContent = snapshot.step ? snapshot.step.replaceAll('_', ' ') : 'Verificando';
      }
    }
    if (providers) {
      providers.innerHTML = (snapshot.providers || []).map(provider => `
        <span class="startup-provider ${esc(provider.phase)}" title="${esc(provider.error || provider.message || '')}">
          ${esc(startupProviderLabel(provider.provider))}: ${provider.ready ? 'pronto' : provider.phase === 'unsupported' ? 'nao aplicavel' : esc(provider.phase)}
        </span>
      `).join('');
    }
    if (cancel) cancel.hidden = !snapshot.can_cancel;
    if (retry) retry.hidden = !['failed', 'cancelled', 'degraded'].includes(snapshot.phase);
    if (technical) {
      technical.textContent = JSON.stringify({
        phase: snapshot.phase,
        operation_id: snapshot.operation_id,
        providers: snapshot.providers,
        error: snapshot.error,
      }, null, 2);
    }
  }

  async function waitForRuntimeStartup() {
    let consecutiveErrors = 0;
    while (true) {
      try {
        const snapshot = await api('/runtime/startup', { timeoutMs: 5000 });
        consecutiveErrors = 0;
        renderStartupStatus(snapshot);
        if (snapshot?.app_ready || ['failed', 'cancelled'].includes(snapshot?.phase)) {
          return snapshot;
        }
      } catch (error) {
        consecutiveErrors += 1;
        if (consecutiveErrors >= 4) {
          const failed = {
            phase: 'failed',
            step: 'failed',
            message: 'Falha ao consultar a inicializacao',
            app_ready: true,
            degraded: true,
            can_cancel: false,
            providers: [],
            error: error.message,
          };
          renderStartupStatus(failed);
          return failed;
        }
      }
      await new Promise(resolve => setTimeout(resolve, 450));
    }
  }

  document.getElementById('startup-cancel')?.addEventListener('click', async () => {
    try {
      renderStartupStatus(await api('/runtime/startup/cancel', { method: 'POST' }));
    } catch (error) {
      pushConsoleEntry('error', 'startup', error.message);
    }
  });

  document.getElementById('startup-retry')?.addEventListener('click', async () => {
    const retry = document.getElementById('startup-retry');
    if (retry) retry.hidden = true;
    try {
      renderStartupStatus(await api('/runtime/startup/retry', { method: 'POST' }));
      const snapshot = await waitForRuntimeStartup();
      updateStatusBadge(snapshot?.phase === 'ready', snapshot?.phase);
      if (snapshot?.app_ready) revealApp();
    } catch (error) {
      pushConsoleEntry('error', 'startup', error.message);
    }
  });

  function updateSidebarDaemonUrl(label) {
    const sidebarUrl = document.getElementById('sidebar-daemon-url');
    if (!sidebarUrl) return;

    if (label) {
      sidebarUrl.textContent = label;
      return;
    }

    if (!state.daemonUrl) {
      sidebarUrl.textContent = 'Daemon desconectado';
      return;
    }

    sidebarUrl.textContent = `Daemon ${state.daemonUrl.replace(/^https?:\/\//, '')}`;
  }

  function updateSidebarConnectionStatus(online) {
    const dot = document.querySelector('.connection-status .status-dot');
    if (!dot) return;
    dot.classList.toggle('online', online);
    dot.classList.toggle('offline', !online);
  }

  export function syncShellLayout(target) {
    if (!appEl) return;
    appEl.dataset.activePanel = target;
    appEl.classList.toggle('chat-sidebar-visible', target === 'chat');
  }

  export function saveModelCache() {
    try {
      localStorage.setItem(MODEL_CACHE_KEY, JSON.stringify(state.models));
      const resolvedModel = activeModelId();
      if (resolvedModel) {
        localStorage.setItem(CURRENT_MODEL_KEY, resolvedModel);
      }
    } catch {
      /* ignore storage errors */
    }
  }

  export function ensureVisibleModel(modelId, provider) {
    const normalizedId = resolveModelId(modelId, provider);
    if (!normalizedId) return;

    const displayName = humanizeModelLabel(modelId) || normalizedId;

    if (state.models.some(model => model.id === normalizedId)) return;

    state.models = [
      ...state.models,
      {
        id: normalizedId,
        name: displayName,
        provider: inferModelProvider(normalizedId, provider),
        path: normalizedId,
        is_available: false,
        agent_tool_mode: null,
        agent_tool_reason: null,
        agent_recommended: false,
      },
    ];
  }

  export function hydrateModelShell() {
    const configuredModel = resolveModelId(state.agentConfig?.model_id || '', state.agentConfig?.provider);
    if (configuredModel) {
      ensureVisibleModel(configuredModel, state.agentConfig?.provider);
    }
    if (!state.currentModel && configuredModel && isLocalProvider(state.agentConfig?.provider)) {
      state.currentModel = configuredModel;
    }

    renderAgentProviderSelector();
    renderModelPicker();
    const currentModelId = activeModelId();
    if (currentModelId) {
      const currentLabel = state.models.find(model => model.id === currentModelId);
      const nameEl = document.getElementById('current-model');
      if (nameEl) nameEl.textContent = currentLabel ? (currentLabel.name || currentLabel.id) : humanizeModelLabel(currentModelId);
    }
    renderInstalledModels();
    updateAgentWorkspaceSummary();
  }

  function waitForInjectedDaemonUrl(timeoutMs = 900) {
    if (window.__MLX_PILOT_DAEMON_URL__) {
      return Promise.resolve(window.__MLX_PILOT_DAEMON_URL__);
    }

    return new Promise(resolve => {
      let settled = false;
      let timer = null;

      const handler = (event) => {
        if (settled) return;
        settled = true;
        window.removeEventListener(DAEMON_READY_EVENT, handler);
        if (timer) clearTimeout(timer);
        resolve(event?.detail?.url || window.__MLX_PILOT_DAEMON_URL__ || null);
      };

      window.addEventListener(DAEMON_READY_EVENT, handler);
      timer = setTimeout(() => {
        if (settled) return;
        settled = true;
        window.removeEventListener(DAEMON_READY_EVENT, handler);
        resolve(window.__MLX_PILOT_DAEMON_URL__ || null);
      }, timeoutMs);
    });
  }

  export async function probeDaemon(url, timeoutMs = 1200) {
    if (!url) return null;

    try {
      const controller = new AbortController();
      const timeoutId = setTimeout(() => controller.abort(), timeoutMs);
      const response = await fetch(url + '/health', {
        method: 'GET',
        headers: { Accept: 'application/json' },
        signal: controller.signal,
      });
      clearTimeout(timeoutId);

      if (!response.ok) return null;
      const text = await response.text();
      if (!text) return null;
      return JSON.parse(text);
    } catch {
      return null;
    }
  }

  function daemonCandidates(...urls) {
    return [...new Set(urls.filter(Boolean).map(url => url.trim()).filter(Boolean))];
  }

  async function resolveDaemonConnection() {
    const injectedUrl = await waitForInjectedDaemonUrl();
    const candidates = daemonCandidates(
      injectedUrl,
      window.__MLX_PILOT_DAEMON_URL__,
      readStorage('mlxPilotDaemonUrl'),
      state.daemonUrl,
      DEFAULT_DAEMON_URL
    );

    for (const candidate of candidates) {
      const health = await probeDaemon(candidate);
      if (!health) continue;

      state.daemonUrl = candidate;
      state.healthOk = health?.status === 'ok';
      state.provider = health?.provider || 'auto';
      localStorage.setItem('mlxPilotDaemonUrl', candidate);
      updateSidebarDaemonUrl();
      updateStatusBadge(state.healthOk, health?.status);
      pushConsoleEntry('info', 'daemon', `Conectado em ${candidate}`);
      return health;
    }

    state.daemonUrl = candidates[0] || DEFAULT_DAEMON_URL;
    state.healthOk = false;
    state.provider = '';
    updateSidebarDaemonUrl(`Daemon ${state.daemonUrl.replace(/^https?:\/\//, '')} indisponivel`);
    updateStatusBadge(false, 'offline');
    pushConsoleEntry('warn', 'daemon', `Nenhum daemon respondeu. Tentativas: ${candidates.join(', ')}`);
    return null;
  }

  export async function bootSequence() {
    const health = await resolveDaemonConnection();
    if (!health) {
      renderStartupStatus({
        phase: 'failed',
        step: 'failed',
        message: 'Daemon local indisponivel',
        app_ready: true,
        degraded: true,
        providers: [],
        error: 'Nenhum endpoint do daemon respondeu.',
      });
      revealApp();
      return;
    }

    const startup = await waitForRuntimeStartup();
    state.healthOk = startup?.phase === 'ready';
    updateStatusBadge(state.healthOk, startup?.phase);
    revealApp();

    await Promise.allSettled([
      loadDaemonConfig(),
      loadAgentConfig(),
      loadSessions(),
    ]);

    void Promise.allSettled([
      loadModels({ force: true }),
      loadPlugins(),
      loadSkills(),
      loadTools(),
      loadChannels(),
      loadAudit(),
      loadEnvironment(),
      loadConsoleSnapshot(),
    ]);
  }

  async function startApp() {
    syncShellLayout(document.querySelector('.tab.active')?.dataset.panel || 'chat');
    hydrateModelShell();
    await bootSequence();
  }

  void startApp();

  function updateStatusBadge(online, phase = online ? 'ready' : 'offline') {
    const degraded = phase === 'degraded';
    const starting = phase === 'starting'
      || ['checking', 'downloading', 'installing', 'updating', 'validating'].includes(phase);
    const label = online ? 'Online' : degraded ? 'Online limitado' : starting ? 'Iniciando' : 'Offline';
    const badge = document.getElementById('status-badge');
    if (badge) {
      badge.innerHTML = `<span class="badge-dot ${online ? 'online' : 'offline'}"></span><span>${label}</span>`;
      badge.style.background = online ? 'var(--green-soft)' : degraded || starting ? 'var(--amber-soft)' : 'var(--rose-soft)';
      badge.style.color = online ? 'var(--green)' : degraded || starting ? 'var(--amber)' : 'var(--rose)';
    }

    const runtimeBadge = document.getElementById('agent-daemon-status');
    if (runtimeBadge) {
      runtimeBadge.textContent = label;
      runtimeBadge.classList.toggle('status-online', online);
      runtimeBadge.classList.toggle('status-offline', !online);
    }

    updateSidebarConnectionStatus(online);
    updateAgentWorkspaceSummary();
  }


  function setText(id, value) {
    const el = document.getElementById(id);
    if (el) el.textContent = value;
  }

  function currentModelLabel() {
    const selectedId = activeModelId();
    const selected = state.models.find(m => m.id === selectedId);
    if (selected) return selected.name || selected.id;
    return humanizeModelLabel(selectedId || state.currentModel || state.agentConfig?.model_id || '') || 'Nenhum modelo selecionado';
  }

  function currentAgentModelLabel() {
    const selectedId = activeAgentModelId();
    const selected = state.models.find((model) => model.id === selectedId);
    if (selected) return selected.name || selected.id;
    return humanizeModelLabel(selectedId || state.agentConfig?.model_id || state.currentModel || '') || 'Nenhum modelo selecionado';
  }

  function currentProviderLabel() {
    const selected = selectedAgentProviderOption();
    if (selected?.kind === 'local') return 'MLX-Pilot';
    if (selected?.label) return selected.label;
    return providerDisplayName(state.agentConfig?.provider || state.provider || 'auto');
  }

  function enabledSkillsCount() {
    return state.skills.filter(skill => skill.active || skill.enabled).length;
  }

  function enabledToolsCount() {
    return state.tools.filter(tool => tool.enabled !== false).length;
  }

  function enabledPluginsCount() {
    return state.plugins.filter(plugin => plugin.enabled).length;
  }

  export function renderAgentChatEmptyState() {
    const box = document.getElementById('agent-chat-messages');
    if (!box) return;
    box.innerHTML = `
      <div class="agent-chat-empty">
        <div class="agent-chat-empty-icon">
          <svg viewBox="0 0 48 48" width="32" height="32" fill="none" stroke="currentColor" stroke-width="1.5">
            <rect x="8" y="8" width="32" height="24" rx="8" />
            <path d="M16 18h16M16 24h10M20 32l-4 8" />
          </svg>
        </div>
        <h3>Converse com o Agent</h3>
        <p>Use a lista lateral para trocar de sessao e mantenha a conversa operacional no painel principal.</p>
      </div>`;
  }

  export function ensureAgentChatReady() {
    const box = document.getElementById('agent-chat-messages');
    if (!box) return null;
    if (box.querySelector('.agent-chat-empty')) box.innerHTML = '';
    return box;
  }

  export function resizeTextArea(el, maxHeight = 160) {
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, maxHeight) + 'px';
  }

  export function updateAgentWorkspaceSummary() {
    const currentSession = state.agentSessions.find(session => session.id === state.currentSessionId) || null;
    const execMode = state.agentConfig?.execution_mode || 'full';
    const approvalMode = state.agentConfig?.approval_mode || 'ask';
    const modelLabel = currentAgentModelLabel();
    const providerLabel = currentProviderLabel();

    setText('agent-session-count', String(state.agentSessions.length));
    setText('agent-provider-pill', `Provider ${providerLabel}`);
    setText('agent-model-pill', `Model ${modelLabel}`);
    setText('agent-exec-pill', `Exec ${execMode}`);
    setText('agent-approval-pill', `Approval ${approvalMode}`);
    setText('agent-current-session', currentSession ? (currentSession.name || `Sessao ${currentSession.id?.substring(0, 6) || '?'}`) : 'Nenhuma sessao ativa');
    setText('agent-current-session-meta', currentSession ? `${currentSession.message_count || 0} msg${(currentSession.message_count || 0) === 1 ? '' : 's'} nesta sessao` : 'Crie uma sessao ou use uma existente na lista lateral.');
    setText('agent-current-model', modelLabel);
    setText('agent-current-provider', `Provider ${providerLabel}`);
    setText('agent-current-execution', `Exec ${execMode}`);
    setText('agent-current-approval', `Approval ${approvalMode}`);
    setText('agent-composer-provider', `Provider: ${providerLabel}`);
    setText('agent-composer-model', `Model: ${modelLabel}`);
    setText('agent-composer-policy', `Exec/Approval: ${execMode} / ${approvalMode}`);
    setText('agent-channel-count', String(state.channels.length));
    setText('agent-plugin-count', String(enabledPluginsCount()));
    setText('agent-skill-count', String(enabledSkillsCount()));
    setText('agent-tool-count', String(enabledToolsCount()));
    setText('agent-audit-count', String(state.auditEntries.length));

    const exportBtn = document.getElementById('btn-export-session');
    if (exportBtn) exportBtn.disabled = !state.currentSessionId;
  }
