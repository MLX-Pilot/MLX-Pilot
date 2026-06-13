/* ============================================================
   MLX PILOT — Orbital Command
   Fully functional frontend with backend API integration
   ============================================================ */

'use strict';

// === auto-imports (generated — do not edit) ===
import { api, nativeInvoke } from './js/core/api.js';
import { esc, fmtBytes } from './js/core/dom.js';
import { AGENT_LOCAL_PROVIDER_CHOICE, AGENT_PROVIDER_PROFILE_TYPES, CLOUD_PROVIDER_DEFAULTS, CURRENT_MODEL_KEY, DAEMON_READY_EVENT, DEFAULT_DAEMON_URL, MIN_SPLASH_MS, MODEL_CACHE_KEY, readStorage, state } from './js/core/state.js';
import { loadAudit, loadChannels, loadPlugins, loadSkills, loadTools } from './js/features/agent.js';
import { loadSessions } from './js/features/chat.js';
import { loadConsoleSnapshot, loadEnvironment, renderConsole } from './js/features/console.js';
import { loadModels, renderInstalledModels, renderModelPicker, selectModel } from './js/features/models.js';
import { activateAgentProviderProfile, loadAgentConfig, loadDaemonConfig } from './js/features/settings.js';
// === end auto-imports ===


  const originalConsole = {
    log: console.log.bind(console),
    info: console.info.bind(console),
    warn: console.warn.bind(console),
    error: console.error.bind(console),
  };

  function stringifyConsoleArg(value) {
    if (value instanceof Error) return value.stack || value.message;
    if (typeof value === 'string') return value;
    try {
      return JSON.stringify(value);
    } catch {
      return String(value);
    }
  }

  export function pushConsoleEntry(level, source, message) {
    const entry = {
      time: new Date().toISOString(),
      level: String(level || 'info').toLowerCase(),
      source: source || 'ui',
      message: String(message || '').replace(/\s+/g, ' ').trim(),
    };
    state.consoleEntries.push(entry);
    if (state.consoleEntries.length > 300) state.consoleEntries.shift();
    void nativeInvoke('desktop_log_append', {
      level: entry.level,
      message: `${entry.source}: ${entry.message}`,
    }).catch(() => {});
    renderConsole();
  }

  ['log', 'info', 'warn', 'error'].forEach((level) => {
    console[level] = (...args) => {
      originalConsole[level](...args);
      pushConsoleEntry(level === 'log' ? 'info' : level, 'ui', args.map(stringifyConsoleArg).join(' '));
    };
  });

  window.addEventListener('error', (event) => {
    pushConsoleEntry('error', 'window', `${event.message || 'Erro sem mensagem'} ${event.filename || ''}:${event.lineno || 0}`);
  });

  window.addEventListener('unhandledrejection', (event) => {
    pushConsoleEntry('error', 'promise', stringifyConsoleArg(event.reason || 'Promise rejeitada sem motivo'));
  });

  function stripModelDecoration(value) {
    return String(value || '')
      .trim()
      .replace(/\s+\[(Ollama|MLX|llama\.cpp)\]$/i, '')
      .trim();
  }

  export function humanizeModelLabel(value) {
    return stripModelDecoration(value).replace(/^(ollama|mlx|llama)::/i, '').trim();
  }

  function normalizeProviderId(provider) {
    const normalized = String(provider || '').trim().toLowerCase();
    if (!normalized) return '';
    if (normalized === AGENT_LOCAL_PROVIDER_CHOICE || normalized === 'mlx-pilot') return 'mlx-pilot';
    if (normalized.includes('ollama')) return 'ollama';
    if (normalized === 'mlx' || normalized.includes('mlx')) return 'mlx';
    if (normalized.includes('llama')) return 'llamacpp';
    if (normalized.includes('anthropic')) return 'anthropic';
    if (normalized.includes('openrouter')) return 'openrouter';
    if (normalized.includes('deepseek')) return 'deepseek';
    if (normalized.includes('groq')) return 'groq';
    if (normalized.includes('gemini') || normalized.includes('google')) return 'gemini';
    if (normalized.includes('zai') || normalized.includes('zhipu')) return 'zai';
    if (normalized.includes('perplexity')) return 'perplexity';
    if (normalized.includes('openai')) return 'openai';
    if (normalized === 'local' || normalized === 'auto') return normalized;
    return normalized;
  }

  export function isLocalProvider(provider) {
    const normalized = normalizeProviderId(provider);
    return normalized === 'mlx-pilot'
      || normalized === 'ollama'
      || normalized === 'mlx'
      || normalized === 'llamacpp'
      || normalized === 'local'
      || normalized === 'auto';
  }

  function providerDisplayName(provider) {
    const normalized = normalizeProviderId(provider);
    if (isLocalProvider(normalized)) return 'MLX-Pilot';
    if (normalized === 'openai') return 'OpenAI';
    if (normalized === 'anthropic') return 'Anthropic';
    if (normalized === 'openrouter') return 'OpenRouter';
    if (normalized === 'deepseek') return 'DeepSeek';
    if (normalized === 'groq') return 'Groq';
    if (normalized === 'gemini') return 'Gemini';
    if (normalized === 'zai') return 'ZAI';
    if (normalized === 'perplexity') return 'Perplexity';
    return normalized ? normalized.charAt(0).toUpperCase() + normalized.slice(1) : 'MLX-Pilot';
  }

  function providerPrefix(provider) {
    const normalized = String(provider || '').trim().toLowerCase();
    if (normalized.includes('ollama')) return 'ollama::';
    if (normalized === 'mlx' || normalized.includes('mlx')) return 'mlx::';
    if (normalized.includes('llama')) return 'llama::';
    return '';
  }

  export function inferModelProvider(modelId, fallback = '') {
    const raw = String(modelId || '').trim().toLowerCase();
    const fallbackPrefix = providerPrefix(fallback);
    if (raw.startsWith('ollama::') || fallbackPrefix === 'ollama::') return 'ollama';
    if (raw.startsWith('mlx::') || fallbackPrefix === 'mlx::') return 'mlx';
    if (raw.startsWith('llama::') || fallbackPrefix === 'llama::') return 'llamacpp';
    return fallback || state.agentConfig?.provider || state.provider || 'configured';
  }

  export function resolveModelId(candidate, provider = '') {
    const raw = stripModelDecoration(candidate);
    if (!raw) return '';

    const exact = state.models.find(model =>
      model.id === raw
      || model.name === raw
      || stripModelDecoration(model.id) === raw
      || stripModelDecoration(model.name) === raw
    );
    if (exact) return exact.id;

    if (!raw.includes('::')) {
      const suffixMatch = state.models.find(model => model.id.endsWith(`::${raw}`));
      if (suffixMatch) return suffixMatch.id;
    }

    const prefix = providerPrefix(provider);
    if (prefix && !raw.startsWith(prefix) && !raw.includes('::') && !raw.includes('/') && !raw.includes('\\')) {
      return `${prefix}${raw}`;
    }

    return raw;
  }

  export function activeModelId() {
    return resolveModelId(state.currentModel || state.agentConfig?.model_id || '', state.agentConfig?.provider);
  }

  export function activeAgentModelId() {
    return resolveModelId(state.agentConfig?.model_id || state.currentModel || '', state.agentConfig?.provider)
      || activeModelId();
  }

  function currentPanelId() {
    return state.activePanel || document.querySelector('.tab.active')?.dataset.panel || 'chat';
  }

  export function isAgentPanelActive() {
    return currentPanelId() === 'agent';
  }

  export function modelCapabilityMode(model) {
    if (!model) return 'unknown';

    const explicit = String(model.agent_tool_mode || '').trim().toLowerCase();
    if (explicit) return explicit;

    const provider = String(model.provider || inferModelProvider(model.id, '')).trim().toLowerCase();
    const label = `${model.id || ''} ${model.name || ''}`.toLowerCase();
    if (provider !== 'ollama') return 'chat_only';
    if (/(embed|embedding|nomic-embed|mxbai-embed|qwen3-vl|vision|-vl\b)/.test(label)) return 'chat_only';
    if (/(deepseek-r1|dolphin3|dolphin-mixtral|mythomax)/.test(label)) return 'chat_only';
    if (/(llama3\.1|qwen2\.5|qwen2\.5-coder|qwen3:8b|qwen3:14b|qwen3\.5:9b)/.test(label)) return 'tool_ready';
    return 'unknown';
  }

  export function modelCapabilityReason(model) {
    if (model?.agent_tool_reason) return model.agent_tool_reason;
    const mode = modelCapabilityMode(model);
    if (mode === 'tool_ready') return 'Compatível com tool calling no Agent.';
    if (mode === 'chat_only') return 'Indicado para chat simples neste runtime.';
    return 'Compatibilidade ainda não validada para uso com tools.';
  }

  export function isToolReadyModel(model) {
    return modelCapabilityMode(model) === 'tool_ready';
  }

  export function recommendedAgentModelId() {
    const isLocal = model => isLocalProvider(model.provider || inferModelProvider(model.id, ''));
    const preferred = state.models.find(model =>
      isToolReadyModel(model)
      && isLocal(model)
      && (model.agent_recommended || /qwen3\.5:9b/i.test(`${model.id || ''} ${model.name || ''}`))
    );
    if (preferred) return preferred.id;
    const toolReady = state.models.find(model => isToolReadyModel(model) && isLocal(model));
    if (toolReady) return toolReady.id;
    // No tool-ready model (e.g. local llama.cpp): fall back to the first available local model.
    return state.models.find(isLocal)?.id || '';
  }

  function configuredEnvironmentKeys() {
    return new Set(
      (state.environmentVars || [])
        .filter(variable => variable && variable.present)
        .map(variable => String(variable.key || '').trim().toUpperCase())
        .filter(Boolean),
    );
  }

  export function profileHasConfiguredSecret(profile) {
    if (!profile) return false;
    if (isLocalProvider(profile.provider)) return true;
    if (String(profile.api_key_ref || '').trim()) return true;
    const secretKeys = CLOUD_PROVIDER_DEFAULTS[normalizeProviderId(profile.provider)]?.secretKeys || [];
    const configuredKeys = configuredEnvironmentKeys();
    return secretKeys.some((key) => configuredKeys.has(key));
  }

  export function defaultCloudModelForProvider(provider) {
    const normalized = normalizeProviderId(provider);
    if (
      state.agentConfig
      && normalizeProviderId(state.agentConfig.provider) === normalized
      && String(state.agentConfig.model_id || '').trim()
    ) {
      return state.agentConfig.model_id;
    }
    return CLOUD_PROVIDER_DEFAULTS[normalized]?.modelId || '';
  }

  function slugifyProfileId(value) {
    return String(value || '')
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '');
  }

  export function createProviderProfileDraft(seed = {}) {
    const provider = normalizeProviderId(seed.provider || 'openai') || 'openai';
    const fallbackId = `${provider}-${Date.now()}`;
    return {
      id: String(seed.id || slugifyProfileId(seed.description) || fallbackId),
      description: String(seed.description || ''),
      provider,
      model_id: String(seed.model_id || defaultCloudModelForProvider(provider) || ''),
      base_url: String(seed.base_url || ''),
      api_key_ref: seed.api_key_ref != null ? String(seed.api_key_ref) : '',
      runtime_variant: String(seed.runtime_variant || state.agentConfig?.runtime_variant || 'classic'),
      custom_headers: seed.custom_headers || {},
    };
  }

  function syncProviderProfileRow(row) {
    if (!row) return;
    const providerInput = row.querySelector('[data-field="provider"]');
    const modelInput = row.querySelector('[data-field="model_id"]');
    const baseUrlInput = row.querySelector('[data-field="base_url"]');
    const secretInput = row.querySelector('[data-field="api_key_ref"]');
    const statusNode = row.querySelector('.agent-provider-profile-status');
    const useButton = row.querySelector('[data-action="use"]');
    const provider = normalizeProviderId(providerInput?.value || 'openai') || 'openai';
    const profile = {
      provider,
      api_key_ref: secretInput?.value || '',
    };
    const isLocal = isLocalProvider(provider);

    if (modelInput && !String(modelInput.value || '').trim()) {
      modelInput.value = defaultCloudModelForProvider(provider) || '';
    }
    if (baseUrlInput && !String(baseUrlInput.value || '').trim() && provider === 'ollama') {
      baseUrlInput.placeholder = 'http://127.0.0.1:11434';
    } else if (baseUrlInput) {
      baseUrlInput.placeholder = provider === 'openai' ? 'https://api.openai.com/v1' : 'Opcional';
    }

    if (statusNode) {
      if (isLocal) {
        statusNode.textContent = 'Local';
      } else if (profileHasConfiguredSecret(profile)) {
        statusNode.textContent = 'Secret pronto';
      } else {
        statusNode.textContent = 'Secret pendente';
      }
    }
    if (useButton) {
      useButton.disabled = !isLocal && !profileHasConfiguredSecret(profile);
    }
  }

  export function renderAgentProviderProfiles() {
    const list = document.getElementById('agent-provider-profile-list');
    const note = document.getElementById('agent-provider-profiles-note');
    if (!list) return;

    const profiles = Array.isArray(state.agentConfig?.provider_profiles)
      ? state.agentConfig.provider_profiles
      : [];
    const activeProfileId = String(state.agentConfig?.provider_profile_id || '').trim();

    list.innerHTML = '';
    if (profiles.length === 0) {
      list.innerHTML = '<div class="agent-empty-copy">Nenhum profile salvo ainda</div>';
    } else {
      profiles.forEach((profile) => {
        const draft = createProviderProfileDraft(profile);
        const row = document.createElement('div');
        row.className = 'plugin-item';
        row.dataset.profileId = draft.id;
        const providerOptions = AGENT_PROVIDER_PROFILE_TYPES
          .map((option) => `<option value="${esc(option.value)}"${option.value === draft.provider ? ' selected' : ''}>${esc(option.label)}</option>`)
          .join('');
        const isActive = draft.id === activeProfileId;
        row.innerHTML = `
          <div class="plugin-info" style="width:100%">
            <div style="display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;margin-bottom:10px">
              <span class="plugin-name">${esc(draft.id)}</span>
              <div style="display:flex;align-items:center;gap:8px;flex-wrap:wrap">
                <span class="agent-meta-pill">${isActive ? 'Ativo' : 'Salvo'}</span>
                <span class="agent-meta-pill agent-provider-profile-status">${isLocalProvider(draft.provider) ? 'Local' : (profileHasConfiguredSecret(draft) ? 'Secret pronto' : 'Secret pendente')}</span>
                <button class="action-btn" type="button" data-action="use"${!isLocalProvider(draft.provider) && !profileHasConfiguredSecret(draft) ? ' disabled' : ''}>Usar no Agent</button>
                <button class="action-btn danger" type="button" data-action="remove">Remover</button>
              </div>
            </div>
            <div class="config-fields">
              <div class="config-field">
                <label>Profile ID</label>
                <input type="text" class="input" data-field="id" value="${esc(draft.id)}" placeholder="openai-prod" />
              </div>
              <div class="config-field">
                <label>Provider</label>
                <select class="input" data-field="provider">${providerOptions}</select>
              </div>
              <div class="config-field">
                <label>Model ID</label>
                <input type="text" class="input" data-field="model_id" value="${esc(draft.model_id)}" placeholder="gpt-4o-mini" />
              </div>
              <div class="config-field">
                <label>Base URL</label>
                <input type="text" class="input" data-field="base_url" value="${esc(draft.base_url)}" placeholder="Opcional" />
              </div>
              <div class="config-field">
                <label>Secret Ref</label>
                <input type="text" class="input" data-field="api_key_ref" value="${esc(draft.api_key_ref || '')}" placeholder="OPENAI_API_KEY" />
              </div>
              <div class="config-field">
                <label>Runtime</label>
                <select class="input" data-field="runtime_variant">
                  <option value="classic"${draft.runtime_variant === 'classic' ? ' selected' : ''}>classic</option>
                  <option value="hermes_inspired"${draft.runtime_variant === 'hermes_inspired' ? ' selected' : ''}>hermes_inspired</option>
                </select>
              </div>
              <div class="config-field" style="grid-column:1 / -1">
                <label>Description</label>
                <input type="text" class="input" data-field="description" value="${esc(draft.description)}" placeholder="Production cloud profile" />
              </div>
            </div>
          </div>`;
        list.appendChild(row);
        row.querySelectorAll('[data-field]').forEach((input) => {
          input.addEventListener('input', () => syncProviderProfileRow(row));
          input.addEventListener('change', () => syncProviderProfileRow(row));
        });
      });
    }

    list.querySelectorAll('[data-action="remove"]').forEach((button) => {
      button.addEventListener('click', () => {
        button.closest('[data-profile-id]')?.remove();
        if (!list.children.length) {
          list.innerHTML = '<div class="agent-empty-copy">Nenhum profile salvo ainda</div>';
        }
      });
    });

    list.querySelectorAll('[data-action="use"]').forEach((button) => {
      button.addEventListener('click', async () => {
        const profileId = button.closest('[data-profile-id]')?.dataset.profileId;
        if (!profileId) return;
        await activateAgentProviderProfile(profileId);
      });
    });

    list.querySelectorAll('[data-profile-id]').forEach((row) => syncProviderProfileRow(row));

    if (note) {
      const active = profiles.find((profile) => profile.id === activeProfileId);
      note.textContent = active
        ? `Profile ativo: ${active.id} (${providerDisplayName(active.provider)}).`
        : 'Edite provider, modelo, endpoint e referencia de secret. Use o profile desejado para tornar esse backend ativo no Agent.';
    }
  }

  export function readAgentProviderProfilesFromDom() {
    const rows = Array.from(document.querySelectorAll('#agent-provider-profile-list [data-profile-id]'));
    const existingProfiles = new Map(
      (state.agentConfig?.provider_profiles || []).map((profile) => [String(profile.id || ''), profile]),
    );
    const profiles = [];
    const seenIds = new Set();

    rows.forEach((row, index) => {
      const provider = normalizeProviderId(row.querySelector('[data-field="provider"]')?.value || 'openai') || 'openai';
      const inputId = row.querySelector('[data-field="id"]')?.value || '';
      const id = slugifyProfileId(inputId) || `${provider}-${index + 1}`;
      if (seenIds.has(id)) {
        throw new Error(`Profile ID duplicado: ${id}`);
      }
      seenIds.add(id);
      const previous = existingProfiles.get(String(row.dataset.profileId || '')) || existingProfiles.get(id) || {};
      profiles.push({
        ...previous,
        id,
        description: String(row.querySelector('[data-field="description"]')?.value || '').trim(),
        provider,
        model_id: String(row.querySelector('[data-field="model_id"]')?.value || '').trim() || defaultCloudModelForProvider(provider),
        base_url: String(row.querySelector('[data-field="base_url"]')?.value || '').trim(),
        api_key_ref: String(row.querySelector('[data-field="api_key_ref"]')?.value || '').trim() || null,
        runtime_variant: String(row.querySelector('[data-field="runtime_variant"]')?.value || 'classic').trim() || 'classic',
        custom_headers: previous.custom_headers || {},
      });
    });

    return profiles;
  }

  function buildAgentProviderOptions() {
    const profiles = Array.isArray(state.agentConfig?.provider_profiles)
      ? state.agentConfig.provider_profiles
      : [];
    const options = [];
    const localProfiles = profiles.filter((profile) => isLocalProvider(profile.provider));
    const preferredLocalProfile =
      localProfiles.find((profile) => normalizeProviderId(profile.provider) === normalizeProviderId(state.agentConfig?.provider))
      || localProfiles[0]
      || null;
    const localModelId = resolveModelId(
      isLocalProvider(state.agentConfig?.provider)
        ? state.agentConfig?.model_id
        : (recommendedAgentModelId() || state.currentModel || preferredLocalProfile?.model_id || ''),
      preferredLocalProfile?.provider || state.agentConfig?.provider || 'ollama',
    );

    options.push({
      value: AGENT_LOCAL_PROVIDER_CHOICE,
      label: 'MLX-Pilot',
      provider: 'mlx-pilot',
      kind: 'local',
      profileId: preferredLocalProfile?.id || null,
      modelId: localModelId,
      description: 'Modelos locais agrupados no runtime MLX-Pilot.',
    });

    const selectedProfileId = String(state.agentConfig?.provider_profile_id || '').trim();
    const groupedProfiles = new Map();
    profiles
      .filter((profile) => !isLocalProvider(profile.provider) && profileHasConfiguredSecret(profile))
      .forEach((profile) => {
        const providerId = normalizeProviderId(profile.provider);
        const existing = groupedProfiles.get(providerId);
        if (!existing || existing.id !== selectedProfileId) {
          groupedProfiles.set(providerId, profile);
        }
        if (profile.id === selectedProfileId) {
          groupedProfiles.set(providerId, profile);
        }
      });

    groupedProfiles.forEach((profile, providerId) => {
      options.push({
        value: `cloud:${providerId}`,
        label: providerDisplayName(providerId),
        provider: providerId,
        kind: 'cloud',
        profileId: profile.id || null,
        modelId: profile.model_id || defaultCloudModelForProvider(providerId),
        description: profile.model_id || providerDisplayName(providerId),
      });
    });

    Object.entries(CLOUD_PROVIDER_DEFAULTS).forEach(([providerId, descriptor]) => {
      if (groupedProfiles.has(providerId)) return;
      const configuredKeys = configuredEnvironmentKeys();
      const hasSecret = descriptor.secretKeys.some((key) => configuredKeys.has(key));
      const usingGlobalAgentSecret =
        normalizeProviderId(state.agentConfig?.provider) === providerId
        && Boolean(String(state.agentConfig?.api_key_ref || state.agentConfig?.api_key || '').trim());
      if (!hasSecret && !usingGlobalAgentSecret) return;
      options.push({
        value: `cloud:${providerId}`,
        label: descriptor.label,
        provider: providerId,
        kind: 'cloud',
        profileId: null,
        modelId: defaultCloudModelForProvider(providerId),
        description: 'Secret configurado no ambiente local.',
      });
    });

    state.agentProviderOptions = options;
    return options;
  }

  function currentAgentProviderChoiceValue() {
    const options = state.agentProviderOptions.length ? state.agentProviderOptions : buildAgentProviderOptions();
    if (isLocalProvider(state.agentConfig?.provider)) {
      return AGENT_LOCAL_PROVIDER_CHOICE;
    }
    const providerId = normalizeProviderId(state.agentConfig?.provider);
    const selectedProfileId = String(state.agentConfig?.provider_profile_id || '').trim();
    const matched =
      options.find((option) => option.profileId && option.profileId === selectedProfileId)
      || options.find((option) => option.provider === providerId);
    return matched?.value || AGENT_LOCAL_PROVIDER_CHOICE;
  }

  function selectedAgentProviderOption() {
    const select = document.getElementById('agent-provider-select');
    const choiceValue = select?.value || currentAgentProviderChoiceValue();
    const options = state.agentProviderOptions.length ? state.agentProviderOptions : buildAgentProviderOptions();
    return options.find((option) => option.value === choiceValue) || options[0] || null;
  }

  export function renderAgentProviderSelector() {
    const select = document.getElementById('agent-provider-select');
    const note = document.getElementById('agent-provider-note');
    if (!select) return;

    const options = buildAgentProviderOptions();
    const currentValue = currentAgentProviderChoiceValue();

    select.innerHTML = '';
    options.forEach((option) => {
      const node = document.createElement('option');
      node.value = option.value;
      node.textContent = option.kind === 'local'
        ? `${option.label} (local)`
        : `${option.label} (${String(option.modelId || 'modelo padrao')})`;
      select.appendChild(node);
    });

    if (options.some((option) => option.value === currentValue)) {
      select.value = currentValue;
    } else if (options[0]) {
      select.value = options[0].value;
    }

    const selected = selectedAgentProviderOption();
    if (!note) return;
    if (!selected) {
      note.textContent = 'Selecione um provider para o Agent.';
    } else if (selected.kind === 'local') {
      note.textContent = 'Modelos locais aparecem como MLX-Pilot. O modelo do Agent continua vindo do seletor de modelos.';
    } else {
      note.textContent = `${selected.label} so aparece quando ja existe secret configurado. Modelo padrao: ${String(selected.modelId || '-')}.`;
    }
  }

  export function visibleModelsForCurrentPanel() {
    if (!isAgentPanelActive()) return state.models;
    const providerOption = selectedAgentProviderOption();
    if (providerOption?.kind === 'cloud') {
      const cloudModelId = resolveModelId(
        providerOption.modelId || state.agentConfig?.model_id || '',
        providerOption.provider,
      );
      if (!cloudModelId) return [];
      const current = state.models.find((model) => model.id === cloudModelId);
      return current ? [current] : [];
    }
    // Show every local model — tool-readiness is surfaced as a badge, never used to hide models.
    return state.models.filter((model) =>
      isLocalProvider(model.provider || inferModelProvider(model.id, ''))
    );
  }

  export function capabilityBadge(mode) {
    if (mode === 'tool_ready') return { label: 'Tool-ready', tone: 'tool-ready' };
    if (mode === 'chat_only') return { label: 'Chat-only', tone: 'chat-only' };
    return { label: 'Verificar', tone: 'unknown' };
  }

  export function agentConfigModelId(modelId, provider = '') {
    const resolvedId = resolveModelId(modelId, provider);
    const inferredProvider = inferModelProvider(resolvedId || modelId, provider);
    const raw = stripModelDecoration(resolvedId || modelId);
    if (inferredProvider === 'ollama') return raw.replace(/^ollama::/i, '');
    if (inferredProvider === 'mlx') return raw.replace(/^mlx::/i, '');
    if (inferredProvider === 'llamacpp') return raw.replace(/^llama::/i, '');
    return raw;
  }

  export async function persistAgentModelSelection(modelId) {
    const resolvedId = resolveModelId(modelId, state.agentConfig?.provider);
    if (!resolvedId) return false;

    const model = state.models.find(entry => entry.id === resolvedId);
    const provider = inferModelProvider(resolvedId, model?.provider || state.agentConfig?.provider || 'llamacpp');
    const payload = {
      ...(state.agentConfig || {}),
      provider,
      model_id: agentConfigModelId(resolvedId, provider),
    };

    if (
      state.agentConfig
      && state.agentConfig.provider === payload.provider
      && state.agentConfig.model_id === payload.model_id
    ) {
      return true;
    }

    try {
      const saved = await api('/agent/config', { method: 'POST', body: JSON.stringify(payload) });
      state.agentConfig = saved || payload;
      return true;
    } catch (error) {
      console.error('Agent model save failed:', error);
      return false;
    } finally {
      renderAgentProviderSelector();
      hydrateModelShell();
      updateAgentWorkspaceSummary();
    }
  }

  export async function ensureAgentCompatibleModel({ persist = false } = {}) {
    const providerOption = selectedAgentProviderOption();
    if (providerOption?.kind === 'cloud') {
      if (providerOption.modelId) {
        ensureVisibleModel(providerOption.modelId, providerOption.provider);
      }
      return true;
    }

    const current = state.models.find(model => model.id === activeAgentModelId());
    if (current && isLocalProvider(current.provider || inferModelProvider(current.id, ''))) return true;

    const fallbackId = recommendedAgentModelId();
    if (!fallbackId) return false;

    selectModel(fallbackId);
    if (persist) await persistAgentModelSelection(fallbackId);
    return true;
  }

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
    const label = online ? 'Online' : degraded ? 'Degradado' : starting ? 'Iniciando' : 'Offline';
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
