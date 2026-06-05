/* ============================================================
   MLX PILOT — Orbital Command
   Fully functional frontend with backend API integration
   ============================================================ */

(function () {
  'use strict';

  const DEFAULT_DAEMON_URL = 'http://127.0.0.1:11435';
  const DAEMON_READY_EVENT = 'mlx-pilot-daemon-ready';
  const API_DEFAULT_TIMEOUT_MS = 8000;
  const API_SLOW_TIMEOUT_MS = 20000;
  const MIN_SPLASH_MS = 480;
  const MODEL_CACHE_KEY = 'mlxPilotModelCache';
  const CURRENT_MODEL_KEY = 'mlxPilotCurrentModel';
  const AGENT_LOCAL_PROVIDER_CHOICE = 'mlx-pilot-local';
  const CLOUD_PROVIDER_DEFAULTS = {
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
      modelId: 'deepseek-chat',
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
  const AGENT_PROVIDER_PROFILE_TYPES = [
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

  function readStorage(key) {
    try {
      return localStorage.getItem(key);
    } catch {
      return null;
    }
  }

  function readJsonStorage(key, fallback) {
    const raw = readStorage(key);
    if (!raw) return fallback;
    try {
      return JSON.parse(raw);
    } catch {
      return fallback;
    }
  }

  const cachedModels = readJsonStorage(MODEL_CACHE_KEY, []);
  const cachedCurrentModel = readStorage(CURRENT_MODEL_KEY);

  // -- State --------------------------------------------------
  const state = {
    daemonUrl: window.__MLX_PILOT_DAEMON_URL__ || readStorage('mlxPilotDaemonUrl') || DEFAULT_DAEMON_URL,
    models: Array.isArray(cachedModels) ? cachedModels : [],
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
    daemonConfig: null,
    catalogModels: [],
    downloads: [],
    agentConfig: null,
    agentSessions: [],
    currentSessionId: null,
    auditEntries: [],
    plugins: [],
    skills: [],
    tools: [],
    channels: [],
    environmentVars: [],
    agentProviderOptions: [],
    activeDiscoverTab: 'catalog',
    activePanel: 'chat',
  };

  function stripModelDecoration(value) {
    return String(value || '')
      .trim()
      .replace(/\s+\[(Ollama|MLX|llama\.cpp)\]$/i, '')
      .trim();
  }

  function humanizeModelLabel(value) {
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

  function isLocalProvider(provider) {
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

  function inferModelProvider(modelId, fallback = '') {
    const raw = String(modelId || '').trim().toLowerCase();
    const fallbackPrefix = providerPrefix(fallback);
    if (raw.startsWith('ollama::') || fallbackPrefix === 'ollama::') return 'ollama';
    if (raw.startsWith('mlx::') || fallbackPrefix === 'mlx::') return 'mlx';
    if (raw.startsWith('llama::') || fallbackPrefix === 'llama::') return 'llamacpp';
    return fallback || state.agentConfig?.provider || state.provider || 'configured';
  }

  function resolveModelId(candidate, provider = '') {
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

  function activeModelId() {
    return resolveModelId(state.currentModel || state.agentConfig?.model_id || '', state.agentConfig?.provider);
  }

  function activeAgentModelId() {
    return resolveModelId(state.agentConfig?.model_id || state.currentModel || '', state.agentConfig?.provider)
      || activeModelId();
  }

  function currentPanelId() {
    return state.activePanel || document.querySelector('.tab.active')?.dataset.panel || 'chat';
  }

  function isAgentPanelActive() {
    return currentPanelId() === 'agent';
  }

  function modelCapabilityMode(model) {
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

  function modelCapabilityReason(model) {
    if (model?.agent_tool_reason) return model.agent_tool_reason;
    const mode = modelCapabilityMode(model);
    if (mode === 'tool_ready') return 'Compatível com tool calling no Agent.';
    if (mode === 'chat_only') return 'Indicado para chat simples neste runtime.';
    return 'Compatibilidade ainda não validada para uso com tools.';
  }

  function isToolReadyModel(model) {
    return modelCapabilityMode(model) === 'tool_ready';
  }

  function recommendedAgentModelId() {
    const preferred = state.models.find(model =>
      isToolReadyModel(model)
      && isLocalProvider(model.provider || inferModelProvider(model.id, ''))
      && (model.agent_recommended || /qwen3\.5:9b/i.test(`${model.id || ''} ${model.name || ''}`))
    );
    if (preferred) return preferred.id;
    return state.models.find(model =>
      isToolReadyModel(model) && isLocalProvider(model.provider || inferModelProvider(model.id, ''))
    )?.id || '';
  }

  function configuredEnvironmentKeys() {
    return new Set(
      (state.environmentVars || [])
        .filter(variable => variable && variable.present)
        .map(variable => String(variable.key || '').trim().toUpperCase())
        .filter(Boolean),
    );
  }

  function profileHasConfiguredSecret(profile) {
    if (!profile) return false;
    if (isLocalProvider(profile.provider)) return true;
    if (String(profile.api_key_ref || '').trim()) return true;
    const secretKeys = CLOUD_PROVIDER_DEFAULTS[normalizeProviderId(profile.provider)]?.secretKeys || [];
    const configuredKeys = configuredEnvironmentKeys();
    return secretKeys.some((key) => configuredKeys.has(key));
  }

  function defaultCloudModelForProvider(provider) {
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

  function createProviderProfileDraft(seed = {}) {
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

  function renderAgentProviderProfiles() {
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

  function readAgentProviderProfilesFromDom() {
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

  function renderAgentProviderSelector() {
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

  function visibleModelsForCurrentPanel() {
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
    return state.models.filter((model) =>
      isLocalProvider(model.provider || inferModelProvider(model.id, '')) && isToolReadyModel(model)
    );
  }

  function capabilityBadge(mode) {
    if (mode === 'tool_ready') return { label: 'Tool-ready', tone: 'tool-ready' };
    if (mode === 'chat_only') return { label: 'Chat-only', tone: 'chat-only' };
    return { label: 'Verificar', tone: 'unknown' };
  }

  function agentConfigModelId(modelId, provider = '') {
    const resolvedId = resolveModelId(modelId, provider);
    const inferredProvider = inferModelProvider(resolvedId || modelId, provider);
    const raw = stripModelDecoration(resolvedId || modelId);
    if (inferredProvider === 'ollama') return raw.replace(/^ollama::/i, '');
    if (inferredProvider === 'mlx') return raw.replace(/^mlx::/i, '');
    if (inferredProvider === 'llamacpp') return raw.replace(/^llama::/i, '');
    return raw;
  }

  async function persistAgentModelSelection(modelId) {
    const resolvedId = resolveModelId(modelId, state.agentConfig?.provider);
    if (!resolvedId) return false;

    const model = state.models.find(entry => entry.id === resolvedId);
    const provider = inferModelProvider(resolvedId, model?.provider || state.agentConfig?.provider || 'ollama');
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

  async function ensureAgentCompatibleModel({ persist = false } = {}) {
    const providerOption = selectedAgentProviderOption();
    if (providerOption?.kind === 'cloud') {
      if (providerOption.modelId) {
        ensureVisibleModel(providerOption.modelId, providerOption.provider);
      }
      return true;
    }

    const current = state.models.find(model => model.id === activeAgentModelId());
    if (current && isToolReadyModel(current)) return true;

    const fallbackId = recommendedAgentModelId();
    if (!fallbackId) return false;

    selectModel(fallbackId);
    if (persist) await persistAgentModelSelection(fallbackId);
    return true;
  }

  // -- API ----------------------------------------------------
  async function api(path, opts = {}) {
    const url = (state.daemonUrl || DEFAULT_DAEMON_URL) + path;
    const inferredTimeoutMs =
      path.startsWith('/chat')
      || path.startsWith('/agent/run')
      || path.startsWith('/catalog/downloads')
        ? 120000
        : API_DEFAULT_TIMEOUT_MS;
    const { timeoutMs = inferredTimeoutMs, headers: requestHeaders = {}, ...fetchOpts } = opts;
    const headers = { ...requestHeaders };

    if (fetchOpts.body != null && !Object.keys(headers).some(key => key.toLowerCase() === 'content-type')) {
      headers['Content-Type'] = 'application/json';
    }

    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), timeoutMs);

    let res;
    try {
      res = await fetch(url, {
        ...fetchOpts,
        headers,
        signal: controller.signal,
      });
    } catch (error) {
      if (error?.name === 'AbortError') {
        throw new Error(`Tempo limite ao acessar ${path}`);
      }
      throw error;
    } finally {
      clearTimeout(timeoutId);
    }

    if (res.status === 204 || res.status === 205) return null;
    if (!res.ok) {
      let msg = `HTTP ${res.status}`;
      try {
        const body = await res.json();
        if (body.error) msg = body.error_code ? `${body.error_code}: ${body.error}` : body.error;
      } catch { /* ok */ }
      throw new Error(msg);
    }
    const text = await res.text();
    if (!text) return null;
    try { return JSON.parse(text); } catch { return { message: text }; }
  }

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

  function syncShellLayout(target) {
    if (!appEl) return;
    appEl.dataset.activePanel = target;
    appEl.classList.toggle('chat-sidebar-visible', target === 'chat');
  }

  function saveModelCache() {
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

  function ensureVisibleModel(modelId, provider) {
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

  function hydrateModelShell() {
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

  async function probeDaemon(url, timeoutMs = 1200) {
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
      updateStatusBadge(state.healthOk);
      return health;
    }

    state.daemonUrl = candidates[0] || DEFAULT_DAEMON_URL;
    state.healthOk = false;
    state.provider = '';
    updateSidebarDaemonUrl(`Daemon ${state.daemonUrl.replace(/^https?:\/\//, '')} indisponivel`);
    updateStatusBadge(false);
    return null;
  }

  async function bootSequence() {
    const health = await resolveDaemonConnection();
    if (!health) return;

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
    ]);
  }

  async function startApp() {
    syncShellLayout(document.querySelector('.tab.active')?.dataset.panel || 'chat');
    hydrateModelShell();
    await bootSequence();
    revealApp();
  }

  void startApp();

  function updateStatusBadge(online) {
    const badge = document.getElementById('status-badge');
    if (badge) {
      badge.innerHTML = online
        ? '<span class="badge-dot online"></span><span>Online</span>'
        : '<span class="badge-dot offline"></span><span>Offline</span>';
      badge.style.background = online ? 'var(--green-soft)' : 'var(--rose-soft)';
      badge.style.color = online ? 'var(--green)' : 'var(--rose)';
    }

    const runtimeBadge = document.getElementById('agent-daemon-status');
    if (runtimeBadge) {
      runtimeBadge.textContent = online ? 'Online' : 'Offline';
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

  function renderAgentChatEmptyState() {
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

  function ensureAgentChatReady() {
    const box = document.getElementById('agent-chat-messages');
    if (!box) return null;
    if (box.querySelector('.agent-chat-empty')) box.innerHTML = '';
    return box;
  }

  function resizeTextArea(el, maxHeight = 160) {
    if (!el) return;
    el.style.height = 'auto';
    el.style.height = Math.min(el.scrollHeight, maxHeight) + 'px';
  }

  function updateAgentWorkspaceSummary() {
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

  // -- Daemon Config (/config) --------------------------------
  async function loadDaemonConfig() {
    try {
      const config = await api('/config');
      state.daemonConfig = config;
      populateSettings(config);
    } catch (e) {
      console.error('Config load failed:', e);
    }
  }

  function populateSettings(c) {
    if (!c) return;
    const set = (id, val) => { const el = document.getElementById(id); if (el && val != null) el.value = val; };
    const setCheck = (id, val) => { const el = document.getElementById(id); if (el) el.checked = !!val; };

    set('set-mlx-cmd', c.mlx_command);
    set('set-mlx-prefix', c.mlx_prefix_args);
    set('set-mlx-timeout', c.mlx_timeout_secs);
    set('set-llamacpp-binary', c.llamacpp_server_binary);
    set('set-llamacpp-url', c.llamacpp_base_url);
    set('set-llamacpp-ctx', c.llamacpp_context_size);
    setCheck('set-llamacpp-autostart', c.llamacpp_auto_start);
    setCheck('set-llamacpp-autoinstall', c.llamacpp_auto_install);

    const threshold = c.mlx_airllm_threshold_percent ?? 70;
    set('set-airllm-threshold', threshold);
    const tv = document.getElementById('set-airllm-threshold-val');
    if (tv) tv.textContent = threshold + '%';
    set('set-airllm-python', c.mlx_airllm_python_command);
    set('set-airllm-runner', c.mlx_airllm_runner);
  }

  async function saveDaemonConfig() {
    try {
      // Gather from settings inputs
      const c = state.daemonConfig || {};
      const get = (id) => { const el = document.getElementById(id); return el ? el.value : undefined; };
      const getNum = (id) => { const v = get(id); return v != null && v !== '' ? Number(v) : undefined; };
      const getCheck = (id) => { const el = document.getElementById(id); return el ? el.checked : undefined; };
      const fw = document.querySelector('input[name="settings-framework"]:checked');
      if (get('set-mlx-cmd')) c.mlx_command = get('set-mlx-cmd');
      if (get('set-mlx-prefix')) c.mlx_prefix_args = get('set-mlx-prefix');
      if (getNum('set-mlx-timeout')) c.mlx_timeout_secs = getNum('set-mlx-timeout');
      if (get('set-llamacpp-binary')) c.llamacpp_server_binary = get('set-llamacpp-binary');
      if (get('set-llamacpp-url')) c.llamacpp_base_url = get('set-llamacpp-url');
      if (getNum('set-llamacpp-ctx')) c.llamacpp_context_size = getNum('set-llamacpp-ctx');
      c.llamacpp_auto_start = getCheck('set-llamacpp-autostart');
      c.llamacpp_auto_install = getCheck('set-llamacpp-autoinstall');
      if (getNum('set-airllm-threshold')) c.mlx_airllm_threshold_percent = getNum('set-airllm-threshold');
      if (get('set-airllm-python')) c.mlx_airllm_python_command = get('set-airllm-python');
      if (get('set-airllm-runner')) c.mlx_airllm_runner = get('set-airllm-runner');

      await api('/config', { method: 'POST', body: JSON.stringify(c) });
      state.daemonConfig = c;
      return true;
    } catch (e) {
      console.error('Save config failed:', e);
      return false;
    }
  }

  // -- Agent Config (/agent/config) ---------------------------
  async function loadAgentConfig() {
    try {
      const config = await api('/agent/config');
      state.agentConfig = config;
      const configuredModel = resolveModelId(config?.model_id, config?.provider);
      if (configuredModel) {
        ensureVisibleModel(configuredModel, config.provider);
        if (!state.currentModel && isLocalProvider(config?.provider)) state.currentModel = configuredModel;
      }
      populateAgentPolicy(config);
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
      hydrateModelShell();
      if (configuredModel) {
        const configuredEntry = state.models.find(model => model.id === configuredModel);
        if (configuredEntry && isLocalProvider(config?.provider) && !isToolReadyModel(configuredEntry)) {
          void ensureAgentCompatibleModel({ persist: true });
        }
      }
      updateAgentWorkspaceSummary();
    } catch (e) {
      console.error('Agent config load failed:', e);
    }
  }

  function populateAgentPolicy(config) {
    if (!config) return;
    // Set execution mode radio
    const execVal = config.execution_mode || 'full';
    const execRadio = document.querySelector(`input[name="exec"][value="${execVal}"]`);
    if (execRadio) { execRadio.checked = true; execRadio.dispatchEvent(new Event('change')); }

    // Set approval mode radio
    const appVal = config.approval_mode || 'ask';
    const appRadio = document.querySelector(`input[name="approval"][value="${appVal}"]`);
    if (appRadio) { appRadio.checked = true; appRadio.dispatchEvent(new Event('change')); }
  }

  async function saveAgentPolicy() {
    try {
      const exec = document.querySelector('input[name="exec"]:checked');
      const app = document.querySelector('input[name="approval"]:checked');
      const payload = {
        ...(state.agentConfig || {}),
        execution_mode: exec?.value || 'full',
        approval_mode: app?.value || 'ask',
      };
      const res = await api('/agent/config', { method: 'POST', body: JSON.stringify(payload) });
      state.agentConfig = res || payload;
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
      updateAgentWorkspaceSummary();
      return true;
    } catch (e) {
      console.error('Save agent policy failed:', e);
      return false;
    }
  }

  async function saveAgentProviderSelection(choiceValue) {
    const option = (state.agentProviderOptions || []).find((entry) => entry.value === choiceValue);
    if (!option || !state.agentConfig) return false;

    try {
      const payload = { ...(state.agentConfig || {}) };
      if (option.kind === 'local') {
        let localModelId = resolveModelId(
          state.currentModel || payload.model_id || option.modelId || recommendedAgentModelId(),
          payload.provider,
        );
        let localModel = state.models.find((model) => model.id === localModelId);
        if (
          !localModel
          || !isLocalProvider(localModel.provider || inferModelProvider(localModel.id, ''))
          || !isToolReadyModel(localModel)
        ) {
          localModelId = recommendedAgentModelId()
            || option.modelId
            || state.models.find((model) =>
              isLocalProvider(model.provider || inferModelProvider(model.id, ''))
            )?.id
            || '';
          localModel = state.models.find((model) => model.id === localModelId);
        }
        if (!localModelId) return false;

        const provider = inferModelProvider(
          localModelId,
          localModel?.provider || payload.provider || state.agentConfig?.provider || 'ollama',
        );
        payload.provider = provider;
        payload.model_id = agentConfigModelId(localModelId, provider);
        payload.provider_profile_id = option.profileId || '';
        state.currentModel = localModelId;
      } else {
        payload.provider = option.provider;
        payload.model_id = agentConfigModelId(
          option.modelId || defaultCloudModelForProvider(option.provider),
          option.provider,
        );
        payload.provider_profile_id = option.profileId || '';
        ensureVisibleModel(payload.model_id, payload.provider);
      }

      const saved = await api('/agent/config', { method: 'POST', body: JSON.stringify(payload) });
      state.agentConfig = saved || payload;
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
      hydrateModelShell();
      updateAgentWorkspaceSummary();
      return true;
    } catch (error) {
      console.error('Agent provider save failed:', error);
      return false;
    }
  }

  async function activateAgentProviderProfile(profileId) {
    const profile = (state.agentConfig?.provider_profiles || []).find((entry) => String(entry.id || '') === String(profileId || ''));
    if (!profile || !state.agentConfig) return false;
    if (!isLocalProvider(profile.provider) && !profileHasConfiguredSecret(profile)) {
      alert('Configure o secret desse provider antes de ativar o profile no Agent.');
      return false;
    }

    try {
      const payload = {
        ...(state.agentConfig || {}),
        provider: profile.provider,
        provider_profile_id: profile.id,
        model_id: agentConfigModelId(profile.model_id, profile.provider),
      };
      if (String(profile.base_url || '').trim()) payload.base_url = profile.base_url;
      if (profile.api_key_ref) payload.api_key_ref = profile.api_key_ref;
      if (profile.runtime_variant) payload.runtime_variant = profile.runtime_variant;

      if (isLocalProvider(profile.provider)) {
        const resolvedLocalModel = resolveModelId(profile.model_id, profile.provider);
        if (resolvedLocalModel) {
          state.currentModel = resolvedLocalModel;
          ensureVisibleModel(resolvedLocalModel, profile.provider);
        }
      } else {
        ensureVisibleModel(profile.model_id, profile.provider);
      }

      const saved = await api('/agent/config', { method: 'POST', body: JSON.stringify(payload) });
      state.agentConfig = saved || payload;
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
      hydrateModelShell();
      updateAgentWorkspaceSummary();
      return true;
    } catch (error) {
      console.error('Provider profile activation failed:', error);
      return false;
    }
  }

  async function saveAgentProviderProfiles() {
    if (!state.agentConfig) return false;

    try {
      const profiles = readAgentProviderProfilesFromDom();
      const payload = {
        ...(state.agentConfig || {}),
        provider_profiles: profiles,
      };
      const activeProfileStillExists = profiles.some((profile) => profile.id === payload.provider_profile_id);
      if (!activeProfileStillExists) {
        payload.provider_profile_id = '';
      }

      const saved = await api('/agent/config', { method: 'POST', body: JSON.stringify(payload) });
      state.agentConfig = saved || payload;
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
      hydrateModelShell();
      updateAgentWorkspaceSummary();

      const btn = document.getElementById('agent-save-provider-profiles');
      if (btn) {
        btn.textContent = 'Profiles salvos!';
        setTimeout(() => {
          btn.textContent = 'Salvar profiles';
        }, 1800);
      }
      return true;
    } catch (error) {
      alert(error instanceof Error ? error.message : String(error));
      return false;
    }
  }

  // Save agent policy when radio buttons change
  document.querySelectorAll('input[name="exec"], input[name="approval"]').forEach(r => {
    r.addEventListener('change', () => saveAgentPolicy());
  });

  document.getElementById('agent-provider-select')?.addEventListener('change', async (event) => {
    const nextValue = event.target?.value;
    if (!nextValue) return;
    const ok = await saveAgentProviderSelection(nextValue);
    if (!ok) {
      renderAgentProviderSelector();
    }
  });

  document.getElementById('agent-add-provider-profile')?.addEventListener('click', () => {
    const list = document.getElementById('agent-provider-profile-list');
    if (!list) return;
    const isEmpty = list.querySelector('.agent-empty-copy');
    if (isEmpty) list.innerHTML = '';

    const profiles = Array.isArray(state.agentConfig?.provider_profiles)
      ? state.agentConfig.provider_profiles
      : [];
    const draft = createProviderProfileDraft({
      id: `openai-${profiles.length + 1}`,
      provider: 'openai',
      model_id: defaultCloudModelForProvider('openai'),
      description: '',
      api_key_ref: 'OPENAI_API_KEY',
    });

    state.agentConfig = {
      ...(state.agentConfig || {}),
      provider_profiles: [...profiles, draft],
    };
    renderAgentProviderProfiles();
  });

  document.getElementById('agent-save-provider-profiles')?.addEventListener('click', () => {
    void saveAgentProviderProfiles();
  });

  // -- Models -------------------------------------------------
  async function loadModels({ force = false } = {}) {
    if (state.modelsLoading) return state.modelsPromise;
    if (!force && state.modelsLoaded && !state.modelsStale) return state.models;

    state.modelsLoading = true;
    renderInstalledModels();

    state.modelsPromise = (async () => {
      try {
        const models = await api('/models');
        state.models = Array.isArray(models) ? models : [];
        if (state.agentConfig?.model_id) ensureVisibleModel(state.agentConfig.model_id, state.agentConfig.provider);
        if (state.currentModel) state.currentModel = resolveModelId(state.currentModel, state.agentConfig?.provider);
        if (state.currentModel) ensureVisibleModel(state.currentModel, state.agentConfig?.provider);
        if (state.agentConfig && (!state.agentConfig.model_id || isAgentPanelActive())) {
          void ensureAgentCompatibleModel({ persist: isAgentPanelActive() });
        }
        state.modelsLoaded = true;
        state.modelsStale = false;
        saveModelCache();
        renderModelPicker();
        renderInstalledModels();
        return state.models;
      } catch (e) {
        console.error('Models load failed:', e);
        if (!state.modelsLoaded) {
          state.models = [];
          renderModelPicker();
        }
        renderInstalledModels();
        throw e;
      } finally {
        state.modelsLoading = false;
        state.modelsPromise = null;
        renderInstalledModels();
      }
    })();

    return state.modelsPromise;
  }

  function invalidateModels() {
    state.modelsStale = true;
  }

  function refreshModelsInBackground() {
    if (state.modelsLoading) return;
    void loadModels({ force: true }).catch(() => {});
  }

  function showInstalledModels() {
    renderInstalledModels();
    if (!state.modelsLoaded || state.modelsStale) refreshModelsInBackground();
  }

  function renderModelPicker() {
    const menu = document.getElementById('model-menu');
    if (!menu) return;
    menu.innerHTML = '';
    const visibleModels = visibleModelsForCurrentPanel();
    if (visibleModels.length === 0) {
      if (isAgentPanelActive()) {
        menu.innerHTML = '<div class="model-menu-item" style="pointer-events:none;color:var(--text-tertiary)">Nenhum modelo Tool-ready disponivel para o Agent</div>';
        return;
      }
      menu.innerHTML = '<div class="model-menu-item" style="pointer-events:none;color:var(--text-tertiary)">Nenhum modelo encontrado</div>';
      return;
    }
    visibleModels.forEach(m => {
      const badge = capabilityBadge(modelCapabilityMode(m));
      const item = document.createElement('div');
      item.className = 'model-menu-item' + (state.currentModel === m.id ? ' selected' : '');
      item.dataset.model = m.id;
      item.title = modelCapabilityReason(m);
      item.innerHTML = `
        <div class="model-menu-info">
          <span class="model-menu-name">${esc(m.name || m.id)}</span>
          <span class="model-menu-meta">${esc(m.provider || '')}</span>
        </div>
        <div class="model-menu-badges">
          <span class="model-capability-badge ${badge.tone}">${badge.label}</span>
        </div>`;
      item.addEventListener('click', (e) => {
        e.stopPropagation();
        selectModel(m.id, { persistAgentConfig: isAgentPanelActive() });
        menu.classList.add('hidden');
      });
      menu.appendChild(item);
    });
    if (!state.currentModel && visibleModels.length > 0) {
      selectModel(visibleModels[0].id, { persistAgentConfig: isAgentPanelActive() });
    }
  }

  function selectModel(id, { persistAgentConfig = false } = {}) {
    const resolvedId = resolveModelId(id, state.agentConfig?.provider);
    state.currentModel = resolvedId || id;
    try {
      localStorage.setItem(CURRENT_MODEL_KEY, state.currentModel);
    } catch {
      /* ignore storage errors */
    }
    const nameEl = document.getElementById('current-model');
    const model = state.models.find(m => m.id === state.currentModel);
    if (nameEl) nameEl.textContent = model ? (model.name || model.id) : humanizeModelLabel(state.currentModel);
    renderModelPicker();
    updateAgentWorkspaceSummary();
    if (persistAgentConfig) void persistAgentModelSelection(state.currentModel);
  }

  function renderInstalledModels() {
    const list = document.getElementById('installed-list');
    const count = document.getElementById('installed-count');
    if (!list) return;
    if (count) {
      if (!state.modelsLoaded && state.modelsLoading) {
        count.textContent = 'Carregando modelos...';
      } else {
        const suffix = state.modelsLoading ? ' • atualizando...' : '';
        const toolReadyCount = state.models.filter(isToolReadyModel).length;
        count.textContent = `${state.models.length} modelo${state.models.length !== 1 ? 's' : ''} instalado${state.models.length !== 1 ? 's' : ''} • ${toolReadyCount} Tool-ready${suffix}`;
      }
    }
    list.innerHTML = '';
    if (!state.modelsLoaded && state.modelsLoading) {
      list.innerHTML = '<div style="padding:24px;text-align:center;color:var(--text-tertiary)">Carregando modelos...</div>';
      return;
    }
    if (state.models.length === 0) {
      list.innerHTML = '<div style="padding:24px;text-align:center;color:var(--text-tertiary)">Nenhum modelo instalado</div>';
      return;
    }
    state.models.forEach(m => {
      const badge = capabilityBadge(modelCapabilityMode(m));
      const item = document.createElement('div');
      item.className = 'installed-item';
      const ic = modelIcon(m.id);
      item.innerHTML = `
        <span class="installed-icon ${ic}">${(m.name || m.id)[0].toUpperCase()}</span>
        <div class="installed-info">
          <span class="installed-name">${esc(m.name || m.id)}</span>
          <span class="installed-meta">${esc(m.provider || '')} &middot; ${m.is_available ? 'Disponível' : 'Indisponível'}</span>
          <span class="installed-capability"><span class="model-capability-badge ${badge.tone}" title="${esc(modelCapabilityReason(m))}">${badge.label}</span></span>
        </div>
        <div class="installed-actions">
          <button class="action-btn" data-act="chat" data-id="${esc(m.id)}">Chat</button>
          <button class="action-btn danger" data-act="del" data-id="${esc(m.id)}">Remover</button>
        </div>`;
      list.appendChild(item);
    });
    list.querySelectorAll('[data-act="chat"]').forEach(b => b.addEventListener('click', () => { selectModel(b.dataset.id); switchTab('chat'); }));
    list.querySelectorAll('[data-act="del"]').forEach(b => b.addEventListener('click', async () => {
      if (!confirm(`Remover modelo ${b.dataset.id}?`)) return;
      try {
        await api(`/models/${encodeURIComponent(b.dataset.id)}`, { method: 'DELETE' });
        invalidateModels();
        refreshModelsInBackground();
      } catch (e) { alert('Erro: ' + e.message); }
    }));
  }

  // -- Catalog ------------------------------------------------
  async function searchCatalog(query) {
    try {
      const models = await api(`/catalog/models?source=huggingface&query=${encodeURIComponent(query)}&limit=20`);
      state.catalogModels = Array.isArray(models) ? models : [];
      renderCatalog();
    } catch (e) {
      console.error('Catalog search failed:', e);
      const c = document.getElementById('catalog-results');
      if (c) c.innerHTML = `<div style="padding:24px;text-align:center;color:var(--rose)">Erro: ${esc(e.message)}</div>`;
    }
  }

  async function startDownload(source, modelId) {
    try {
      await api('/catalog/downloads', { method: 'POST', body: JSON.stringify({ source, model_id: modelId }) });
      invalidateModels();
      alert('Download iniciado: ' + modelId);
    } catch (e) { alert('Erro no download: ' + e.message); }
  }

  function renderCatalog() {
    const container = document.getElementById('catalog-results');
    if (!container) return;
    container.innerHTML = '';
    if (state.catalogModels.length === 0) {
      container.innerHTML = '<div style="padding:24px;text-align:center;color:var(--text-tertiary)">Nenhum modelo encontrado.</div>';
      return;
    }
    state.catalogModels.forEach(m => {
      const card = document.createElement('div');
      card.className = 'model-card';
      const ic = modelIcon(m.model_id || m.name);
      const size = m.size_bytes ? fmtBytes(m.size_bytes) : 'N/A';
      const dl = m.downloads ? fmtNum(m.downloads) : '0';
      const lk = m.likes ? fmtNum(m.likes) : '0';
      card.innerHTML = `
        <div class="model-card-header">
          <div class="model-card-icon ${ic}">${(m.name || m.model_id || 'M')[0].toUpperCase()}</div>
          <div class="model-card-info">
            <h3>${esc(m.name || m.model_id)}</h3>
            <span class="model-card-source">${esc(m.author || m.source || '')}</span>
          </div>
          <button class="download-btn" data-src="huggingface" data-mid="${esc(m.model_id)}">
            <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M8 2v9M4 8l4 4 4-4M2 14h12"/></svg>
            Baixar
          </button>
        </div>
        <div class="model-card-stats">
          <span class="stat"><span class="stat-val">${esc(size)}</span> tamanho</span>
          <span class="stat"><span class="stat-val">${esc(dl)}</span> downloads</span>
          <span class="stat"><span class="stat-val">${esc(lk)}</span> likes</span>
        </div>`;
      container.appendChild(card);
    });
    container.querySelectorAll('.download-btn').forEach(b => b.addEventListener('click', () => startDownload(b.dataset.src, b.dataset.mid)));
  }

  // -- Chat Streaming -----------------------------------------
  async function sendChatMessage(text) {
    if (!text.trim() || state.isStreaming) return;
    const modelId = activeModelId();
    if (!modelId) { addSystemMsg('Selecione um modelo primeiro.'); return; }

    addMessage('user', text);
    const input = document.getElementById('chat-input');
    if (input) { input.value = ''; input.style.height = 'auto'; }

    // Remove welcome message if present
    const welcome = document.querySelector('.welcome-message');
    if (welcome) welcome.remove();

    state.messages.push({ role: 'user', content: text });
    const assistantEl = addMessage('assistant', '');

    state.isStreaming = true;
    state.streamController = new AbortController();

    const payload = {
      model_id: modelId,
      messages: state.messages,
      options: { temperature: 0.2, airllm_enabled: state.airllmEnabled },
    };

    let streamedThinking = '', rawAnswer = '', metrics = {};

    try {
      const res = await fetch(state.daemonUrl + '/chat/stream', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        signal: state.streamController.signal,
      });

      if (!res.ok) {
        if (res.status === 404 || res.status === 405) return sendChatNonStreaming(payload, assistantEl);
        throw new Error(`HTTP ${res.status}`);
      }

      const reader = res.body.getReader();
      const decoder = createStreamDecoder();
      let buf = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        const lines = buf.split('\n');
        buf = lines.pop() || '';
        for (const line of lines) {
          if (!line.trim()) continue;
          let evt;
          try { evt = JSON.parse(line); } catch { continue; }
          if (evt.event === 'status') {
            updateStreamStatus(assistantEl, evt.status);
          } else if (evt.event === 'thinking_delta') {
            streamedThinking += evt.delta || '';
            renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
          } else if (evt.event === 'answer_delta') {
            rawAnswer += evt.delta || '';
            renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
          } else if (evt.event === 'metrics') {
            metrics = { ...metrics, ...evt };
          } else if (evt.event === 'done') {
            metrics = { ...metrics, ...evt };
            addMetrics(assistantEl, metrics);
            const rendered = renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking, finalize: true });
            if (rendered.answer) {
              state.messages.push({ role: 'assistant', content: rendered.answer });
            }
          } else if (evt.event === 'error') {
            throw new Error(evt.message || 'Erro desconhecido');
          }
        }
      }
    } catch (e) {
      if (e.name === 'AbortError') addSystemMsg('Geração interrompida.');
      else { addSystemMsg(`Erro: ${e.message}`); console.error('Chat:', e); }
    } finally {
      state.isStreaming = false;
      state.streamController = null;
    }
  }

  async function sendChatNonStreaming(payload, el = addMessage('assistant', '')) {
    updateStreamStatus(el, 'thinking');
    try {
      const res = await api('/chat', { method: 'POST', body: JSON.stringify(payload) });
      const content = res?.message?.content || res?.final_response || 'Sem resposta.';
      const rendered = renderAssistantOutput(el, { rawAnswer: content, finalize: true });
      if (rendered.answer) {
        state.messages.push({ role: 'assistant', content: rendered.answer });
      }
      if (res?.usage) addMetrics(el, { prompt_tokens: res.usage.prompt_tokens, completion_tokens: res.usage.completion_tokens, total_tokens: res.usage.total_tokens, latency_ms: res.latency_ms });
    } catch (e) {
      updateAnswer(el, `Erro: ${e.message}`);
    }
    state.isStreaming = false;
  }

  // -- Message DOM helpers ------------------------------------
  function addMessage(role, content) {
    const container = document.getElementById('chat-messages');
    if (!container) return null;
    const div = document.createElement('div');
    div.className = `message ${role}-message`;
    const letter = role === 'user' ? 'U' : 'AI';
    const cls = role === 'assistant' ? ' assistant' : '';
    const now = new Date();
    const time = `${String(now.getHours()).padStart(2,'0')}:${String(now.getMinutes()).padStart(2,'0')}`;
    div.innerHTML = `<div class="msg-avatar${cls}">${letter}</div><div class="msg-body"><div class="msg-content markdown-body">${esc(content)}</div><div class="msg-time">${time}</div></div>`;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
    return div;
  }

  function addSystemMsg(text) {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    const div = document.createElement('div');
    div.style.cssText = 'text-align:center;padding:8px;font-size:12px;color:var(--text-tertiary)';
    div.textContent = text;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
  }

  function updateStreamStatus(el, status) {
    const c = el?.querySelector('.msg-content');
    if (!c) return;
    if (status === 'thinking') c.innerHTML = '<div class="thinking-indicator"><span>Pensando</span><span class="dots"><span>.</span><span>.</span><span>.</span></span></div>';
    else if (status === 'answering') c.innerHTML = '';
  }

  function updateThinking(el, text) {
    if (!el) return;
    const normalized = String(text || '').trim();
    const body = el.querySelector('.msg-body');
    if (!body) return;
    let toggle = el.querySelector('.msg-thinking-toggle');
    let block = el.querySelector('.msg-thinking');
    if (!normalized) {
      toggle?.remove();
      block?.remove();
      return;
    }
    if (!block) {
      toggle = document.createElement('button');
      toggle.type = 'button';
      toggle.className = 'msg-thinking-toggle';
      toggle.innerHTML = '<span class="thinking-chevron">&#9662;</span><span class="thinking-label">Pensando</span>';
      block = document.createElement('div');
      block.className = 'msg-thinking';
      block.innerHTML = `<div class="thinking-content markdown-body"></div>`;
      toggle.addEventListener('click', () => {
        const collapsed = toggle.classList.toggle('collapsed');
        block.style.display = collapsed ? 'none' : 'block';
      });
      body.insertBefore(block, body.firstChild);
      body.insertBefore(toggle, block);
    }
    block.querySelector('.thinking-content').innerHTML = renderMarkdown(normalized);
  }

  function updateAnswer(el, text) {
    const c = el?.querySelector('.msg-content');
    if (c) c.innerHTML = renderMarkdown(text);
  }

  function joinThinkingSections(...sections) {
    return sections
      .map(section => String(section || '').trim())
      .filter(Boolean)
      .join('\n\n')
      .trim();
  }

  function splitThinkingBlocks(text) {
    const source = String(text || '').replace(/\r\n?/g, '\n');
    if (!source) return { thinking: '', answer: '' };

    const thinkingParts = [];
    const answerParts = [];
    const regex = /<think>([\s\S]*?)<\/think>/gi;
    let cursor = 0;
    let match;

    while ((match = regex.exec(source))) {
      answerParts.push(source.slice(cursor, match.index));
      thinkingParts.push(match[1]);
      cursor = regex.lastIndex;
    }

    const tail = source.slice(cursor);
    const lowerTail = tail.toLowerCase();
    const openIndex = lowerTail.indexOf('<think>');
    if (openIndex >= 0) {
      answerParts.push(tail.slice(0, openIndex));
      thinkingParts.push(tail.slice(openIndex + '<think>'.length));
    } else {
      answerParts.push(tail);
    }

    const answer = answerParts.join('').replace(/<\/?think>/gi, '').trim();
    const thinking = thinkingParts.join('\n\n').replace(/<\/?think>/gi, '').trim();
    return { thinking, answer };
  }

  function renderAssistantOutput(el, { rawAnswer = '', streamedThinking = '', finalize = false } = {}) {
    const parsed = splitThinkingBlocks(rawAnswer);
    const combinedThinking = joinThinkingSections(streamedThinking, parsed.thinking);
    if (combinedThinking) updateThinking(el, combinedThinking);

    const answerText = parsed.answer;
    const hasThinkMarkup = /<\/?think>/i.test(rawAnswer);
    if (answerText || (finalize && rawAnswer && !hasThinkMarkup)) {
      updateAnswer(el, answerText || rawAnswer);
    }

    return { thinking: combinedThinking, answer: answerText || (!hasThinkMarkup ? String(rawAnswer || '').trim() : '') };
  }

  function createStreamDecoder() {
    const Decoder = window.TextDecoder || globalThis.TextDecoder;
    if (!Decoder) throw new Error('Streaming indisponivel: TextDecoder nao encontrado');
    return new Decoder();
  }

  async function sendAgentMessageStreaming(payload, assistantEl) {
    let streamedThinking = '';
    let rawAnswer = '';
    let metrics = {};

    const res = await fetch(state.daemonUrl + '/agent/stream', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ...payload, streaming: true }),
    });

    if (!res.ok) {
      if (res.status === 404 || res.status === 405 || res.status === 501) return null;
      throw new Error(`HTTP ${res.status}`);
    }

    const reader = res.body?.getReader?.();
    if (!reader) return null;
    const decoder = createStreamDecoder();
    let buffer = '';

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let idx;
      while ((idx = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, idx).trim();
        buffer = buffer.slice(idx + 1);
        if (!line) continue;
        const evt = JSON.parse(line);
        if (evt.event === 'status') {
          updateStreamStatus(assistantEl, evt.status);
        } else if (evt.event === 'thinking_delta') {
          streamedThinking += evt.delta || '';
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'answer_delta') {
          rawAnswer += evt.delta || '';
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'tool_call_started') {
          streamedThinking = joinThinkingSections(streamedThinking, `Executando tool '${evt.tool || '?'}'...`);
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'tool_call_completed') {
          const preview = evt.message ? `: ${evt.message}` : '';
          streamedThinking = joinThinkingSections(streamedThinking, `Tool '${evt.tool || '?'}' concluida${preview}`);
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'tool_call_denied') {
          const preview = evt.message ? `: ${evt.message}` : '';
          streamedThinking = joinThinkingSections(streamedThinking, `Tool '${evt.tool || '?'}' negada${preview}`);
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'done') {
          metrics = { ...metrics, ...evt };
        } else if (evt.event === 'error') {
          throw new Error(evt.message || 'Falha no streaming do agent');
        }
      }
    }

    const rendered = renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking, finalize: true });
    if (rendered.answer) {
      state.messages.push({ role: 'assistant', content: rendered.answer });
    }
    if (metrics?.total_tokens) addMetrics(assistantEl, metrics);
    return { answer: rendered.answer, metrics, session_id: metrics?.session_id || payload.session_id || null };
  }

  function addMetrics(el, m) {
    const body = el?.querySelector('.msg-body');
    if (!body) return;
    const div = document.createElement('div');
    div.className = 'msg-metrics';
    let h = '';
    if (m.total_tokens != null) h += `<span class="metric"><span class="metric-label">Tokens</span> <span class="metric-value">${fmtNum(m.total_tokens)}</span></span>`;
    if (m.latency_ms != null) h += `<span class="metric"><span class="metric-label">Tempo</span> <span class="metric-value">${(m.latency_ms / 1000).toFixed(1)}s</span></span>`;
    if (m.generation_tps != null) h += `<span class="metric"><span class="metric-label">TPS</span> <span class="metric-value">${m.generation_tps.toFixed(1)}</span></span>`;
    if (m.airllm_used) h += `<span class="metric"><span class="metric-label">AIRLLM</span> <span class="metric-value">Ativo</span></span>`;
    if (m.iterations != null) h += `<span class="metric"><span class="metric-label">Iterações</span> <span class="metric-value">${m.iterations}</span></span>`;
    if (m.tool_calls_made != null) h += `<span class="metric"><span class="metric-label">Tools</span> <span class="metric-value">${m.tool_calls_made}</span></span>`;
    div.innerHTML = h;
    body.appendChild(div);
  }

  // -- Sessions (sidebar history) -----------------------------
  async function loadSessions() {
    try {
      const sessions = await api('/agent/sessions');
      state.agentSessions = Array.isArray(sessions) ? sessions : [];
      if (state.currentSessionId && !state.agentSessions.some(session => session.id === state.currentSessionId)) state.currentSessionId = null;
      if (!state.currentSessionId && state.agentSessions[0]?.id) state.currentSessionId = state.agentSessions[0].id;
      renderSidebarHistory();
    } catch {
      state.agentSessions = [];
      state.currentSessionId = null;
      renderSidebarHistory();
    }
  }

  function renderSidebarHistory() {
    renderSessionCollection(document.getElementById('chat-history'), 'sidebar');
    renderSessionCollection(document.getElementById('agent-session-list'), 'agent');
    updateAgentWorkspaceSummary();
  }

  function renderSessionCollection(container, variant) {
    if (!container) return;
    container.innerHTML = '';
    if (state.agentSessions.length === 0) {
      container.innerHTML = variant === 'agent'
        ? '<div class="agent-empty-copy">Nenhuma sessao ainda</div>'
        : '<div style="padding:8px 12px;font-size:12px;color:var(--text-tertiary)">Nenhuma sessao ainda</div>';
      return;
    }
    state.agentSessions.forEach(s => {
      const item = document.createElement('div');
      const name = s.name || `Sessao ${s.id?.substring(0, 6) || '?'}`;
      const count = s.message_count || 0;
      const active = s.id === state.currentSessionId;
      if (variant === 'agent') {
        item.className = 'agent-session-item' + (active ? ' active' : '');
        item.innerHTML = `
          <div class="agent-session-title">
            <span class="agent-session-name" title="${esc(name)}">${esc(name)}</span>
            <span class="agent-session-count">${fmtNum(count)}</span>
          </div>
          <div class="agent-session-meta">${count} msg${count === 1 ? '' : 's'}</div>`;
      } else {
        item.className = 'history-item' + (active ? ' active' : '');
        item.innerHTML = `<span class="history-icon">&#9679;</span><span class="history-label" title="${esc(name)}">${esc(name)} <span style="opacity:0.5;font-size:11px">(${count})</span></span>`;
      }
      item.addEventListener('click', () => {
        state.currentSessionId = s.id;
        renderAgentChatEmptyState();
        renderSidebarHistory();
      });
      container.appendChild(item);
    });
  }

  async function createNewSession() {
    try {
      const session = await api('/agent/sessions', { method: 'POST', body: JSON.stringify({}) });
      if (session?.id) {
        state.currentSessionId = session.id;
        state.messages = [];
        const msgs = document.getElementById('chat-messages');
        if (msgs) msgs.innerHTML = '';
        await loadSessions();
        renderAgentChatEmptyState();
      }
    } catch (e) { console.error('New session failed:', e); }
  }
  // -- Plugins ------------------------------------------------
  async function loadPlugins() {
    try {
      const plugins = await api('/agent/plugins');
      state.plugins = Array.isArray(plugins) ? plugins : [];
      renderPlugins();
    } catch { state.plugins = []; renderPlugins(); }
  }

  function renderPlugins() {
    const list = document.getElementById('plugin-list');
    if (!list) {
      updateAgentWorkspaceSummary();
      return;
    }
    list.innerHTML = '';
    if (state.plugins.length === 0) {
      list.innerHTML = '<div class="agent-empty-copy">Nenhum plugin</div>';
      updateAgentWorkspaceSummary();
      return;
    }
    state.plugins.forEach(p => {
      const id = p.id || p.plugin_id || p.name || '?';
      const item = document.createElement('div');
      item.className = 'plugin-item';
      item.innerHTML = `
        <div class="plugin-toggle ${p.enabled ? 'active' : ''}" data-pid="${esc(id)}"><div class="toggle-knob"></div></div>
        <div class="plugin-info"><span class="plugin-name">${esc(id)}</span><span class="plugin-desc">${esc(p.description || '')}</span></div>`;
      list.appendChild(item);
    });
    list.querySelectorAll('.plugin-toggle').forEach(t => {
      t.addEventListener('click', async () => {
        const id = t.dataset.pid;
        const enable = !t.classList.contains('active');
        try {
          await api(enable ? '/agent/plugins/enable' : '/agent/plugins/disable', { method: 'POST', body: JSON.stringify({ plugin_id: id }) });
          const plugin = state.plugins.find(entry => (entry.id || entry.plugin_id || entry.name || '?') === id);
          if (plugin) plugin.enabled = enable;
          t.classList.toggle('active', enable);
          updateAgentWorkspaceSummary();
        } catch (e) { alert('Erro: ' + e.message); }
      });
    });
    updateAgentWorkspaceSummary();
  }

  // -- Skills -------------------------------------------------
  async function loadSkills() {
    try {
      const data = await api('/agent/skills/check');
      state.skills = Array.isArray(data?.skills) ? data.skills : [];
      renderSkills();
    } catch { state.skills = []; renderSkills(); }
  }

  function renderSkills() {
    const list = document.getElementById('skills-list');
    if (!list) {
      updateAgentWorkspaceSummary();
      return;
    }
    list.innerHTML = '';
    if (state.skills.length === 0) {
      list.innerHTML = '<div class="agent-empty-copy">Nenhuma skill</div>';
      updateAgentWorkspaceSummary();
      return;
    }
    state.skills.forEach(s => {
      const chip = document.createElement('span');
      chip.className = `skill-chip ${s.active || s.enabled ? 'active' : ''}`;
      chip.textContent = s.name;
      chip.title = s.description || '';
      chip.addEventListener('click', async () => {
        try {
          await api(s.active || s.enabled ? '/agent/skills/disable' : '/agent/skills/enable', { method: 'POST', body: JSON.stringify({ skill: s.name }) });
          s.active = !(s.active || s.enabled);
          s.enabled = s.active;
          chip.classList.toggle('active', s.active);
          updateAgentWorkspaceSummary();
        } catch (e) { alert('Erro: ' + e.message); }
      });
      list.appendChild(chip);
    });
    updateAgentWorkspaceSummary();
  }

  // -- Tools --------------------------------------------------
  async function loadTools() {
    try {
      const tools = await api('/agent/tools');
      state.tools = Array.isArray(tools) ? tools : [];
      renderTools();
    } catch { state.tools = []; renderTools(); }
  }

  function renderTools() {
    const grid = document.getElementById('tools-grid');
    if (!grid) {
      updateAgentWorkspaceSummary();
      return;
    }
    grid.innerHTML = '';
    if (state.tools.length === 0) {
      grid.innerHTML = '<span style="color:var(--text-tertiary);font-size:12px">Nenhum tool</span>';
      updateAgentWorkspaceSummary();
      return;
    }
    state.tools.forEach(t => {
      const chip = document.createElement('div');
      chip.className = 'tool-chip';
      chip.textContent = t.name;
      chip.title = t.description || '';
      chip.style.opacity = t.enabled ? '1' : '0.4';
      grid.appendChild(chip);
    });
    updateAgentWorkspaceSummary();
  }

  // -- Channels -----------------------------------------------
  async function loadChannels() {
    try {
      const channels = await api('/agent/channels', { headers: { 'x-channel-protocol-version': 'v1' } });
      state.channels = Array.isArray(channels) ? channels : [];
      renderChannels();
    } catch { state.channels = []; renderChannels(); }
  }

  function renderChannels() {
    const list = document.getElementById('channel-list');
    if (!list) {
      updateAgentWorkspaceSummary();
      return;
    }
    list.innerHTML = '';
    if (state.channels.length === 0) {
      list.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhum channel configurado</div>';
      updateAgentWorkspaceSummary();
      return;
    }
    state.channels.forEach(ch => {
      const channelId = ch.channel_id || ch.id || ch.name || '?';
      const accounts = ch.accounts || [];
      if (accounts.length === 0) {
        list.appendChild(makeChannelCard(channelId, null, channelId));
      } else {
        accounts.forEach(acc => {
          list.appendChild(makeChannelCard(channelId, acc, `${channelId} — ${acc.account_id || acc.id || ''}`));
        });
      }
    });
    updateAgentWorkspaceSummary();
  }

  function makeChannelCard(channelId, account, displayName) {
    const card = document.createElement('div');
    card.className = 'channel-card';
    const connected = account?.status === 'connected' || account?.enabled;
    card.innerHTML = `
      <div class="channel-status"><span class="status-dot ${connected ? 'online' : 'offline'}"></span></div>
      <div class="channel-info">
        <span class="channel-name">${esc(displayName)}</span>
        <span class="channel-meta">${esc(channelId)} · ${connected ? 'Conectado' : 'Desconectado'}</span>
      </div>
      <div class="channel-actions">
        <button class="action-btn danger" data-ch="${esc(channelId)}" data-acc="${esc(account?.account_id || account?.id || '')}">Remover</button>
      </div>`;
    card.querySelectorAll('.action-btn.danger').forEach(btn => {
      btn.addEventListener('click', async () => {
        if (!confirm('Remover channel?')) return;
        try {
          const body = { channel: btn.dataset.ch };
          if (btn.dataset.acc) body.account_id = btn.dataset.acc;
          await api('/agent/channels/remove', { method: 'POST', headers: { 'x-channel-protocol-version': 'v1' }, body: JSON.stringify(body) });
          loadChannels();
        } catch (e) { alert('Erro: ' + e.message); }
      });
    });
    return card;
  }

  // -- Audit --------------------------------------------------
  async function loadAudit() {
    try {
      const data = await api('/agent/audit?limit=30');
      state.auditEntries = data?.entries || [];
      renderAuditFeed();
    } catch { state.auditEntries = []; renderAuditFeed(); }
  }

  function renderAuditFeed() {
    const feed = document.getElementById('audit-feed');
    if (!feed) {
      updateAgentWorkspaceSummary();
      return;
    }
    feed.innerHTML = '';
    if (state.auditEntries.length === 0) {
      feed.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhum evento</div>';
      updateAgentWorkspaceSummary();
      return;
    }
    state.auditEntries.forEach(entry => {
      const item = document.createElement('div');
      item.className = 'audit-item';
      const dot = entry.status === 'denied' ? 'error' : entry.tool_name ? 'tool' : 'success';
      let time = '';
      if (entry.timestamp) { try { time = new Date(entry.timestamp).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' }); } catch { /* ok */ } }
      item.innerHTML = `
        <span class="audit-dot ${dot}"></span>
        <div class="audit-body">
          <span class="audit-action">${esc(entry.event_type || 'event')}${entry.tool_name ? `: <code>${esc(entry.tool_name)}</code>` : ''}</span>
          <span class="audit-detail">${esc(entry.summary || entry.status || '')}</span>
          <span class="audit-time">${time}</span>
        </div>`;
      feed.appendChild(item);
    });
    updateAgentWorkspaceSummary();
  }

  // -- Environment --------------------------------------------
  async function loadEnvironment() {
    try {
      const data = await api('/environment?reveal=false');
      state.environmentVars = data?.variables || [];
      renderEnvironment();
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
    } catch (e) {
      console.error('Environment load failed:', e);
      state.environmentVars = [];
      renderEnvironment();
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
    }
  }

  function renderEnvironment() {
    const table = document.getElementById('env-table');
    if (!table) return;
    table.innerHTML = '';
    if (state.environmentVars.length === 0) {
      table.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhuma variável</div>';
      return;
    }
    state.environmentVars.forEach(v => {
      const row = document.createElement('div');
      row.className = 'env-row';
      const displayValue = v.is_secret
        ? (v.present ? (v.masked || '') : '')
        : (v.value || '');
      row.innerHTML = `
        <span class="env-key">${esc(v.key)}</span>
        <input
          type="${v.is_secret ? 'password' : 'text'}"
          class="input env-val"
          value="${esc(displayValue)}"
          data-key="${esc(v.key)}"
          data-secret="${v.is_secret ? 'true' : 'false'}"
          data-present="${v.present ? 'true' : 'false'}"
          data-initial-value="${esc(v.value || '')}"
          data-masked-value="${esc(v.masked || '')}"
          data-dirty="false"
          data-revealed="false"
        />
        ${v.is_secret ? `<button class="action-btn reveal-btn"${v.present ? '' : ' disabled'}>${v.present ? 'Revelar' : 'Sem secret'}</button>` : ''}`;
      table.appendChild(row);
    });
    const syncSecretButton = (input, btn) => {
      if (!input || !btn) return;
      const hasStoredSecret = input.dataset.present === 'true';
      const revealed = input.dataset.revealed === 'true';
      const hasDraft = String(input.value || '').trim() !== '' && input.dataset.dirty === 'true';
      if (revealed) {
        btn.disabled = false;
        btn.textContent = 'Ocultar';
        return;
      }
      if (hasStoredSecret) {
        btn.disabled = false;
        btn.textContent = 'Revelar';
        return;
      }
      btn.disabled = !hasDraft;
      btn.textContent = hasDraft ? 'Mostrar' : 'Sem secret';
    };

    table.querySelectorAll('.env-row').forEach((row) => {
      const input = row.querySelector('.env-val');
      const btn = row.querySelector('.reveal-btn');
      if (!input) return;

      const hiddenBaseline = () => (
        input.dataset.secret === 'true'
          ? (input.dataset.present === 'true' ? (input.dataset.maskedValue || '') : '')
          : (input.dataset.initialValue || '')
      );

      input.addEventListener('input', () => {
        const baseline = input.dataset.revealed === 'true'
          ? (input.dataset.initialValue || '')
          : hiddenBaseline();
        input.dataset.dirty = input.value !== baseline ? 'true' : 'false';
        syncSecretButton(input, btn);
      });

      syncSecretButton(input, btn);
    });

    table.querySelectorAll('.reveal-btn').forEach(btn => {
      btn.addEventListener('click', async () => {
        const input = btn.previousElementSibling;
        if (!input) return;
        if (input.dataset.revealed === 'true') {
          input.type = 'password';
          input.dataset.revealed = 'false';
          if (input.dataset.dirty !== 'true') {
            input.value = input.dataset.present === 'true' ? (input.dataset.maskedValue || '') : '';
          }
          syncSecretButton(input, btn);
          return;
        }

        if (input.dataset.dirty === 'true') {
          input.type = 'text';
          input.dataset.revealed = 'true';
          syncSecretButton(input, btn);
          return;
        }

        if (input.dataset.present !== 'true') {
          syncSecretButton(input, btn);
          return;
        }

        try {
          const data = await api('/environment?reveal=true');
          const found = (data?.variables || []).find(v => v.key === input.dataset.key);
          if (found?.present) {
            input.value = found.value || '';
            input.dataset.initialValue = found.value || '';
            input.type = 'text';
            input.dataset.revealed = 'true';
            input.dataset.dirty = 'false';
          }
        } catch {
          /* ok */
        }
        syncSecretButton(input, btn);
      });
    });
  }

  async function saveEnvironment() {
    const vals = {};
    document.querySelectorAll('#env-table .env-val').forEach(input => {
      if (!input.dataset.key) return;
      const isSecret = input.dataset.secret === 'true';
      const dirty = input.dataset.dirty === 'true';
      const revealed = input.dataset.revealed === 'true';
      const current = String(input.value || '');
      const initial = String(input.dataset.initialValue || '');

      if (isSecret) {
        if (dirty || (revealed && current !== initial)) {
          vals[input.dataset.key] = current;
        }
        return;
      }

      if (dirty || current !== initial) {
        vals[input.dataset.key] = current;
      }
    });
    if (Object.keys(vals).length === 0) { alert('Nenhuma variável foi revelada para edição.'); return; }
    try {
      await api('/environment', { method: 'POST', body: JSON.stringify({ values: vals }) });
      await loadEnvironment();
      const btn = document.getElementById('save-env-btn');
      if (btn) { btn.textContent = 'Salvo!'; setTimeout(() => { btn.textContent = 'Salvar Variáveis'; }, 2000); }
    } catch (e) { alert('Erro: ' + e.message); }
  }

  // -- Tab Navigation -----------------------------------------
  function switchTab(target) {

    state.activePanel = target;
    document.querySelectorAll('.tab').forEach(t => { t.classList.remove('active'); t.setAttribute('aria-selected', 'false'); });
    document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
    const tab = document.querySelector(`[data-panel="${target}"]`);
    const panel = document.getElementById(`panel-${target}`);
    if (tab) { tab.classList.add('active'); tab.setAttribute('aria-selected', 'true'); }
    if (panel) panel.classList.add('active');
    syncShellLayout(target);
    renderModelPicker();

    if (target === 'discover') {
      searchCatalog('llama');
      if (state.activeDiscoverTab === 'installed') showInstalledModels();
    }
    if (target === 'agent') {
      void ensureAgentCompatibleModel({ persist: true });
      updateAgentWorkspaceSummary();
    }
    if (target === 'ai-interaction') initAICanvas();
  }

  document.querySelectorAll('.tab').forEach(tab => tab.addEventListener('click', () => switchTab(tab.dataset.panel)));

  document.querySelectorAll('.agent-view-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.agent-view-tab').forEach(t => {
        t.classList.remove('active');
        t.setAttribute('aria-selected', 'false');
      });
      document.querySelectorAll('.agent-view').forEach(view => {
        view.classList.remove('active');
        view.style.display = 'none';
      });
      tab.classList.add('active');
      tab.setAttribute('aria-selected', 'true');
      const view = document.getElementById(`agent-view-${tab.dataset.agentView}`);
      if (view) {
        view.classList.add('active');
        view.style.display = 'block';
      }
      if (tab.dataset.agentView === 'config') loadAudit();
      updateAgentWorkspaceSummary();
    });
  });

  // -- Model Picker -------------------------------------------
  document.getElementById('model-trigger')?.addEventListener('click', (e) => {
    e.stopPropagation();
    document.getElementById('model-menu')?.classList.toggle('hidden');
  });
  document.addEventListener('click', () => document.getElementById('model-menu')?.classList.add('hidden'));

  // -- Discover Sub-tabs --------------------------------------
  document.querySelectorAll('.discover-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.discover-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      const d = tab.dataset.dtab;
      state.activeDiscoverTab = d;
      document.getElementById('dtab-catalog').style.display = d === 'catalog' ? 'block' : 'none';
      document.getElementById('dtab-installed').style.display = d === 'installed' ? 'block' : 'none';
      if (d === 'installed') showInstalledModels();
    });
  });

  // Refresh installed models
  document.getElementById('refresh-installed')?.addEventListener('click', () => {
    invalidateModels();
    refreshModelsInBackground();
  });

  // -- Catalog Search -----------------------------------------
  let searchTimeout;
  document.getElementById('catalog-search')?.addEventListener('input', (e) => {
    clearTimeout(searchTimeout);
    searchTimeout = setTimeout(() => { if (e.target.value.trim().length >= 2) searchCatalog(e.target.value.trim()); }, 500);
  });
  // -- Agent Chat ---------------------------------------------
  const agentInput = document.getElementById('agent-command-input');
  const agentSendBtn = document.getElementById('agent-send-btn');

  document.querySelectorAll('.agent-prompt-card').forEach(card => {
    card.addEventListener('click', () => {
      if (!agentInput) return;
      agentInput.value = card.dataset.agentPrompt || '';
      resizeTextArea(agentInput, 220);
      agentInput.focus();
    });
  });

  agentInput?.addEventListener('input', () => resizeTextArea(agentInput, 220));
  agentSendBtn?.addEventListener('click', async () => {
    if (!agentInput?.value.trim()) return;
    const msg = agentInput.value.trim();
    agentInput.value = '';
    resizeTextArea(agentInput, 220);

    const box = ensureAgentChatReady();
    if (!box) return;

    box.insertAdjacentHTML('beforeend', `<div class="message user-message"><div class="msg-avatar">U</div><div class="msg-body"><div class="msg-content">${esc(msg)}</div></div></div>`);
    const agDiv = document.createElement('div');
    agDiv.className = 'message assistant-message';
    agDiv.innerHTML = `<div class="msg-avatar assistant">AG</div><div class="msg-body"><div class="msg-content markdown-body"><div class="thinking-indicator"><span>Processando</span><span class="dots"><span>.</span><span>.</span><span>.</span></span></div></div></div>`;
    box.appendChild(agDiv);
    box.scrollTop = box.scrollHeight;

    try {
      const modelId = activeAgentModelId();
      if (!modelId) throw new Error('Selecione um modelo valido antes de executar o agent.');
      const payload = {
        session_id: state.currentSessionId,
        message: msg,
        provider: state.agentConfig?.provider || inferModelProvider(modelId, 'ollama') || 'ollama',
        model_id: modelId,
        execution_mode: state.agentConfig?.execution_mode || 'full',
        approval_mode: state.agentConfig?.approval_mode || 'ask',
        max_iterations: 25,
      };
      let res = null;
      const streamed = await sendAgentMessageStreaming(payload, agDiv);
      if (!streamed) {
        res = await api('/agent/run', { method: 'POST', body: JSON.stringify(payload) });
      } else if (streamed.session_id) {
        state.currentSessionId = streamed.session_id;
        await loadSessions();
      }
      if (res?.session_id) {
        state.currentSessionId = res.session_id;
        await loadSessions();
      } else {
        updateAgentWorkspaceSummary();
      }
      if (res) {
        const content = res?.final_response || 'Sem resposta.';
        renderAssistantOutput(agDiv, { rawAnswer: content, finalize: true });
        if (res?.total_tokens) addMetrics(agDiv, res);
      }
    } catch (e) {
      agDiv.querySelector('.msg-content').innerHTML = `<span style="color:var(--rose)">Erro: ${esc(e.message)}</span>`;
    }
    box.scrollTop = box.scrollHeight;
  });
  agentInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      agentSendBtn?.click();
    }
  });

  // -- Audit Refresh ------------------------------------------
  document.getElementById('refresh-audit')?.addEventListener('click', () => loadAudit());

  // -- Settings Save ------------------------------------------
  document.getElementById('save-settings-btn')?.addEventListener('click', async () => {
    const ok = await saveDaemonConfig();
    const btn = document.getElementById('save-settings-btn');
    btn.textContent = ok ? 'Configurações salvas!' : 'Erro ao salvar';
    setTimeout(() => { btn.textContent = 'Aplicar Todas as Configurações'; }, 2000);
  });

  document.getElementById('save-env-btn')?.addEventListener('click', saveEnvironment);

  // Range input live value
  document.getElementById('set-airllm-threshold')?.addEventListener('input', (e) => {
    const tv = document.getElementById('set-airllm-threshold-val');
    if (tv) tv.textContent = e.target.value + '%';
  });

  // -- Sidebar: New Chat --------------------------------------
  document.getElementById('btn-new-chat')?.addEventListener('click', () => {
    state.messages = [];
    state.currentSessionId = null;
    const msgs = document.getElementById('chat-messages');
    if (msgs) msgs.innerHTML = '<div class="welcome-message" style="text-align:center;padding:60px 20px;max-width:500px;margin:0 auto"><h3 style="font-family:var(--font-heading);font-size:20px;margin-bottom:8px">MLX Pilot Chat</h3><p style="font-size:14px;color:var(--text-tertiary)">Selecione um modelo e envie sua mensagem.</p></div>';
    createNewSession();
    switchTab('chat');
  });

  document.getElementById('topbar-brand')?.addEventListener('click', () => switchTab('chat'));

  // -- Daemon URL ---------------------------------------------
  document.getElementById('save-url')?.addEventListener('click', () => {
    const input = document.getElementById('daemon-url');
    if (input?.value.trim()) {
      state.daemonUrl = input.value.trim().replace(/\/+$/, '');
      localStorage.setItem('mlxPilotDaemonUrl', state.daemonUrl);
      const sidebarUrl = document.getElementById('sidebar-daemon-url');
      if (sidebarUrl) sidebarUrl.textContent = `Daemon ${state.daemonUrl.replace(/^https?:\/\//, '')}`;
      bootSequence();
    }
  });

  // -- Toggle Chips -------------------------------------------
  document.querySelectorAll('.toggle-chip').forEach(chip => {
    chip.addEventListener('click', () => {
      chip.classList.toggle('active');
      if (chip.id === 'web-search-toggle') state.webSearchEnabled = chip.classList.contains('active');
      if (chip.id === 'airllm-toggle') state.airllmEnabled = chip.classList.contains('active');
    });
  });

  // -- Chat Input ---------------------------------------------
  const chatInput = document.getElementById('chat-input');
  chatInput?.addEventListener('input', () => resizeTextArea(chatInput, 160));
  chatInput?.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChatMessage(chatInput.value); } });
  document.getElementById('send-btn')?.addEventListener('click', () => sendChatMessage(chatInput?.value || ''));

  // -- Radio Card generic -------------------------------------
  document.querySelectorAll('.radio-card input[type="radio"]').forEach(radio => {
    radio.addEventListener('change', () => {
      document.querySelectorAll(`input[name="${radio.name}"]`).forEach(r => r.closest('.radio-card')?.classList.remove('selected'));
      radio.closest('.radio-card')?.classList.add('selected');
    });
  });

  // -- Code Copy ----------------------------------------------
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('.code-copy');
    if (!btn) return;
    const code = btn.closest('.code-block')?.querySelector('code');
    if (code) { navigator.clipboard.writeText(code.textContent).then(() => { btn.textContent = 'Copiado!'; setTimeout(() => { btn.textContent = 'Copiar'; }, 2000); }); }
  });

  // -- AI Canvas Particles ------------------------------------
  let aiCanvas, aiCtx, aiAnimFrame, particles = [];
  function initAICanvas() {
    aiCanvas = document.getElementById('ai-canvas');
    if (!aiCanvas) return;
    aiCtx = aiCanvas.getContext('2d');
    const r = aiCanvas.parentElement.getBoundingClientRect();
    aiCanvas.width = r.width; aiCanvas.height = r.height;
    if (!particles.length) {
      const n = Math.min(80, Math.floor(window.innerWidth / 15));
      for (let i = 0; i < n; i++) particles.push({ x: Math.random() * aiCanvas.width, y: Math.random() * aiCanvas.height, vx: (Math.random() - 0.5) * 0.3, vy: (Math.random() - 0.5) * 0.3, size: Math.random() * 2 + 0.5, opacity: Math.random() * 0.4 + 0.1, hue: Math.random() > 0.5 ? 190 : 260 });
    }
    if (!aiAnimFrame) animParticles();
  }
  function animParticles() {
    if (!aiCtx || !aiCanvas) return;
    aiCtx.clearRect(0, 0, aiCanvas.width, aiCanvas.height);
    for (let i = 0; i < particles.length; i++) {
      for (let j = i + 1; j < particles.length; j++) {
        const dx = particles[i].x - particles[j].x, dy = particles[i].y - particles[j].y, d = Math.sqrt(dx * dx + dy * dy);
        if (d < 120) { aiCtx.beginPath(); aiCtx.moveTo(particles[i].x, particles[i].y); aiCtx.lineTo(particles[j].x, particles[j].y); aiCtx.strokeStyle = `rgba(0,212,255,${(1 - d / 120) * 0.08})`; aiCtx.lineWidth = 0.5; aiCtx.stroke(); }
      }
    }
    particles.forEach(p => {
      p.x += p.vx; p.y += p.vy;
      if (p.x < 0) p.x = aiCanvas.width; if (p.x > aiCanvas.width) p.x = 0;
      if (p.y < 0) p.y = aiCanvas.height; if (p.y > aiCanvas.height) p.y = 0;
      aiCtx.beginPath(); aiCtx.arc(p.x, p.y, p.size, 0, Math.PI * 2);
      aiCtx.fillStyle = `hsla(${p.hue},80%,60%,${p.opacity})`; aiCtx.fill();
      aiCtx.beginPath(); aiCtx.arc(p.x, p.y, p.size * 3, 0, Math.PI * 2);
      aiCtx.fillStyle = `hsla(${p.hue},80%,60%,${p.opacity * 0.15})`; aiCtx.fill();
    });
    aiAnimFrame = requestAnimationFrame(animParticles);
  }

  // -- Atmosphere ---------------------------------------------
  const atmCanvas = document.getElementById('atmosphere');
  if (atmCanvas) {
    const ctx = atmCanvas.getContext('2d');
    let ap = [];
    function resizeA() { atmCanvas.width = window.innerWidth; atmCanvas.height = window.innerHeight; }
    function mkA() { ap = []; for (let i = 0; i < Math.min(40, Math.floor(window.innerWidth / 30)); i++) ap.push({ x: Math.random() * atmCanvas.width, y: Math.random() * atmCanvas.height, vx: (Math.random() - 0.5) * 0.15, vy: (Math.random() - 0.5) * 0.1, size: Math.random() * 1.2 + 0.3, opacity: Math.random() * 0.15 + 0.03 }); }
    function loopA() { ctx.clearRect(0, 0, atmCanvas.width, atmCanvas.height); ap.forEach(p => { p.x += p.vx; p.y += p.vy; if (p.x < 0) p.x = atmCanvas.width; if (p.x > atmCanvas.width) p.x = 0; if (p.y < 0) p.y = atmCanvas.height; if (p.y > atmCanvas.height) p.y = 0; ctx.beginPath(); ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2); ctx.fillStyle = `rgba(0,212,255,${p.opacity})`; ctx.fill(); }); requestAnimationFrame(loopA); }
    resizeA(); mkA(); loopA();
    window.addEventListener('resize', () => { resizeA(); mkA(); });
  }
  // -- Agent: New Session / Export -----------------------------
  document.getElementById('btn-new-session')?.addEventListener('click', async () => {
    try {
      const session = await api('/agent/sessions', { method: 'POST', body: JSON.stringify({ name: '' }) });
      if (session?.id) {
        state.currentSessionId = session.id;
        await loadSessions();
        renderAgentChatEmptyState();
        updateAgentWorkspaceSummary();
        agentInput?.focus();
      }
    } catch (e) { alert('Erro: ' + e.message); }
  });

  document.getElementById('btn-export-session')?.addEventListener('click', () => {
    if (!state.currentSessionId) { alert('Nenhuma sessão selecionada'); return; }
    window.open(state.daemonUrl + '/agent/sessions/' + state.currentSessionId + '/export', '_blank');
  });

  // -- Agent: New Channel -------------------------------------
  document.getElementById('btn-new-channel')?.addEventListener('click', async () => {
    const channelId = prompt('Nome/ID do channel (ex: whatsapp, slack, http):');
    if (!channelId) return;
    try {
      await api('/agent/channels/upsert', {
        method: 'POST',
        headers: { 'x-channel-protocol-version': 'v1' },
        body: JSON.stringify({ channel: channelId, enabled: true, accounts: [] }),
      });
      loadChannels();
    } catch (e) { alert('Erro: ' + e.message); }
  });

  // -- AI Visual Panel ----------------------------------------
  const aiInput = document.getElementById('ai-input');
  const aiSendBtn = document.getElementById('ai-send-btn');

  async function renderAIVisual(prompt) {
    if (!prompt?.trim()) return;
    // Show loading state on the canvas overlay
    const overlay = document.querySelector('.ai-overlay');
    let resultEl = overlay?.querySelector('.ai-result');
    if (!resultEl) {
      resultEl = document.createElement('div');
      resultEl.className = 'ai-result';
      resultEl.style.cssText = 'margin-top:20px;padding:16px 20px;background:rgba(10,14,23,0.8);backdrop-filter:blur(16px);border:1px solid var(--border);border-radius:var(--r-lg);text-align:left;max-height:200px;overflow-y:auto;';
      overlay?.appendChild(resultEl);
    }
    resultEl.innerHTML = '<div class="thinking-indicator"><span>Renderizando</span><span class="dots"><span>.</span><span>.</span><span>.</span></span></div>';

    // Send to daemon chat for scene description
    const modelId = activeModelId();
    if (modelId) {
      try {
        const msgs = [{ role: 'user', content: prompt }];
        const res = await api('/chat', {
          method: 'POST',
          body: JSON.stringify({ model_id: modelId, messages: msgs, options: { temperature: 0.7 } }),
        });
        const content = res?.message?.content || 'Sem resposta.';
        resultEl.innerHTML = renderMarkdown(content);
      } catch (e) {
        // If no model, show a local visual response
        resultEl.innerHTML = renderMarkdown(`**Cena Visual:** ${prompt}\n\n*Conecte-se ao daemon para respostas reais do modelo.*`);
      }
    } else {
      resultEl.innerHTML = renderMarkdown(`**Cena Visual:** ${prompt}\n\n*Selecione um modelo para obter respostas do daemon.*`);
    }

    // Trigger particle burst effect
    triggerParticleBurst();
  }

  function triggerParticleBurst() {
    if (!particles.length || !aiCanvas) return;
    const cx = aiCanvas.width / 2, cy = aiCanvas.height / 2;
    particles.forEach(p => {
      const angle = Math.random() * Math.PI * 2;
      p.vx = Math.cos(angle) * (Math.random() * 1.5 + 0.5);
      p.vy = Math.sin(angle) * (Math.random() * 1.5 + 0.5);
      p.x = cx + (Math.random() - 0.5) * 50;
      p.y = cy + (Math.random() - 0.5) * 50;
      p.hue = Math.random() > 0.5 ? 190 : 260;
      p.opacity = Math.random() * 0.6 + 0.2;
    });
    // Slowly return to ambient speeds
    setTimeout(() => {
      particles.forEach(p => { p.vx *= 0.15; p.vy *= 0.15; p.opacity = Math.min(p.opacity, 0.4); });
    }, 2000);
  }

  aiSendBtn?.addEventListener('click', () => renderAIVisual(aiInput?.value));
  aiInput?.addEventListener('keydown', (e) => { if (e.key === 'Enter') renderAIVisual(aiInput.value); });

  // Example buttons -> fill input and render
  document.querySelectorAll('.example-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const prompt = btn.dataset.prompt || btn.textContent;
      if (aiInput) aiInput.value = prompt;
      renderAIVisual(prompt);
    });
  });

  // -- Keyboard -----------------------------------------------
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'k') { e.preventDefault(); document.getElementById('model-menu')?.classList.toggle('hidden'); }
    if (e.key === 'Escape') document.getElementById('model-menu')?.classList.add('hidden');
    if (!e.ctrlKey && !e.metaKey && !e.altKey && !['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName)) {
      const n = parseInt(e.key);
      if (n >= 1 && n <= 6) switchTab(['chat', 'discover', 'agent', 'ai-interaction', 'settings'][n - 1]);
    }
    if ((e.ctrlKey || e.metaKey) && e.key === '.') state.streamController?.abort();
  });

  // -- Utilities ----------------------------------------------
  function esc(s) { if (!s) return ''; const d = document.createElement('div'); d.textContent = String(s); return d.innerHTML; }
  function fmtBytes(b) { if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB'; if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB'; return (b / 1e3).toFixed(0) + ' KB'; }
  function fmtNum(n) { if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M'; if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K'; return String(n); }
  function fmtDuration(s) { if (s < 60) return s + 's'; if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`; return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`; }
  function modelIcon(id) { const l = (id || '').toLowerCase(); if (l.includes('llama')) return 'llama'; if (l.includes('mistral')) return 'mistral'; if (l.includes('qwen')) return 'qwen'; if (l.includes('deepseek')) return 'deepseek'; if (l.includes('phi')) return 'phi'; return 'llama'; }

  function stashHtmlToken(tokens, html) {
    const token = `\uE000${tokens.length}\uE001`;
    tokens.push(html);
    return token;
  }

  function restoreHtmlTokens(text, tokens) {
    return String(text || '').replace(/\uE000(\d+)\uE001/g, (_, index) => tokens[Number(index)] || '');
  }

  function sanitizeHref(href) {
    const value = String(href || '').trim();
    if (/^(https?:|mailto:)/i.test(value)) return esc(value);
    return '#';
  }

  function renderInlineMarkdown(text) {
    if (!text) return '';
    const tokens = [];
    let output = String(text);

    output = output.replace(/`([^`\n]+)`/g, (_, code) => stashHtmlToken(tokens, `<code>${esc(code)}</code>`));
    output = output.replace(/\[([^\]]+)\]\(([^)\s]+)(?:\s+"([^"]*)")?\)/g, (_, label, href, title) => {
      const titleAttr = title ? ` title="${esc(title)}"` : '';
      return stashHtmlToken(tokens, `<a href="${sanitizeHref(href)}" target="_blank" rel="noopener"${titleAttr}>${renderInlineMarkdown(label)}</a>`);
    });

    output = esc(output);
    output = output.replace(/(^|[\s(])\*\*([^*]+)\*\*(?=$|[\s).,!?:;])/g, '$1<strong>$2</strong>');
    output = output.replace(/(^|[\s(])__([^_]+)__(?=$|[\s).,!?:;])/g, '$1<strong>$2</strong>');
    output = output.replace(/(^|[\s(])\*([^*]+)\*(?=$|[\s).,!?:;])/g, '$1<em>$2</em>');
    output = output.replace(/(^|[\s(])_([^_]+)_(?=$|[\s).,!?:;])/g, '$1<em>$2</em>');
    output = output.replace(/~~([^~]+)~~/g, '<del>$1</del>');
    output = output.replace(/(https?:\/\/[^\s<]+)/g, '<a href="$1" target="_blank" rel="noopener">$1</a>');
    return restoreHtmlTokens(output, tokens);
  }

  function splitTableRow(line) {
    return String(line || '')
      .trim()
      .replace(/^\|/, '')
      .replace(/\|$/, '')
      .split('|')
      .map(cell => cell.trim());
  }

  function isTableSeparator(line) {
    return /^\s*\|?\s*:?-{3,}:?\s*(\|\s*:?-{3,}:?\s*)+\|?\s*$/.test(line || '');
  }

  function isMarkdownBlockBoundary(line, nextLine) {
    const trimmed = String(line || '').trim();
    if (!trimmed) return true;
    if (/^\uE000\d+\uE001$/.test(trimmed)) return true;
    if (/^#{1,6}\s+/.test(trimmed)) return true;
    if (/^>\s?/.test(trimmed)) return true;
    if (/^([-+*]|\d+\.)\s+/.test(trimmed)) return true;
    if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) return true;
    if (trimmed.includes('|') && isTableSeparator(nextLine || '')) return true;
    return false;
  }

  function renderMarkdown(text) {
    const source = String(text || '').replace(/\r\n?/g, '\n').trim();
    if (!source) return '';

    const blockTokens = [];
    const normalized = source.replace(/```([\w.+-]*)\n?([\s\S]*?)```/g, (_, lang, code) => {
      const language = (lang || 'code').trim();
      const body = esc((code || '').replace(/\n$/, ''));
      return stashHtmlToken(blockTokens, `<div class="code-block"><div class="code-header"><span class="code-lang">${esc(language)}</span><button class="code-copy">Copiar</button></div><pre><code>${body}</code></pre></div>`);
    });

    const lines = normalized.split('\n');
    const blocks = [];

    for (let index = 0; index < lines.length;) {
      const line = lines[index];
      const trimmed = line.trim();

      if (!trimmed) {
        index += 1;
        continue;
      }

      if (/^\uE000\d+\uE001$/.test(trimmed)) {
        blocks.push(trimmed);
        index += 1;
        continue;
      }

      const heading = trimmed.match(/^(#{1,6})\s+(.+)$/);
      if (heading) {
        const level = heading[1].length;
        blocks.push(`<h${level}>${renderInlineMarkdown(heading[2].trim())}</h${level}>`);
        index += 1;
        continue;
      }

      if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) {
        blocks.push('<hr>');
        index += 1;
        continue;
      }

      if (/^>\s?/.test(trimmed)) {
        const quoteLines = [];
        while (index < lines.length && /^>\s?/.test(lines[index].trim())) {
          quoteLines.push(lines[index].trim().replace(/^>\s?/, ''));
          index += 1;
        }
        blocks.push(`<blockquote>${renderMarkdown(quoteLines.join('\n'))}</blockquote>`);
        continue;
      }

      const listItem = trimmed.match(/^([-+*]|\d+\.)\s+(.+)$/);
      if (listItem) {
        const ordered = /\d+\./.test(listItem[1]);
        const tag = ordered ? 'ol' : 'ul';
        const items = [];
        while (index < lines.length) {
          const current = lines[index].trim().match(/^([-+*]|\d+\.)\s+(.+)$/);
          if (!current) break;
          items.push(`<li>${renderInlineMarkdown(current[2])}</li>`);
          index += 1;
        }
        blocks.push(`<${tag}>${items.join('')}</${tag}>`);
        continue;
      }

      if (trimmed.includes('|') && isTableSeparator(lines[index + 1] || '')) {
        const header = splitTableRow(lines[index]);
        index += 2;
        const rows = [];
        while (index < lines.length && lines[index].includes('|') && lines[index].trim()) {
          rows.push(splitTableRow(lines[index]));
          index += 1;
        }
        const headHtml = `<thead><tr>${header.map(cell => `<th>${renderInlineMarkdown(cell)}</th>`).join('')}</tr></thead>`;
        const bodyHtml = rows.length
          ? `<tbody>${rows.map(row => `<tr>${row.map(cell => `<td>${renderInlineMarkdown(cell)}</td>`).join('')}</tr>`).join('')}</tbody>`
          : '';
        blocks.push(`<div class="markdown-table-wrap"><table>${headHtml}${bodyHtml}</table></div>`);
        continue;
      }

      const paragraph = [];
      while (index < lines.length) {
        const current = lines[index];
        const next = lines[index + 1];
        if (!current.trim()) break;
        if (paragraph.length > 0 && isMarkdownBlockBoundary(current, next)) break;
        paragraph.push(current.trim());
        index += 1;
      }
      blocks.push(`<p>${renderInlineMarkdown(paragraph.join('\n')).replace(/\n/g, '<br>')}</p>`);
    }

    return restoreHtmlTokens(blocks.join(''), blockTokens);
  }

})();

