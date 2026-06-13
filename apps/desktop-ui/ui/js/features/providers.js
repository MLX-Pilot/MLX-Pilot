/* MLX Pilot — Model & provider helpers (feature).
 *
 * Model-id normalization/labeling, provider inference/display, model
 * capability checks, and the agent provider-profile editor (draft/sync/render/
 * read-from-DOM + provider option & selector building). Helpers shared by the
 * models, settings and agent features.
 */

// === auto-imports (generated — do not edit) ===
import { api } from '../core/api.js';
import { esc } from '../core/dom.js';
import { AGENT_LOCAL_PROVIDER_CHOICE, AGENT_PROVIDER_PROFILE_TYPES, CLOUD_PROVIDER_DEFAULTS, state } from '../core/state.js';
import { selectModel } from './models.js';
import { hydrateModelShell, updateAgentWorkspaceSummary } from './runtime.js';
import { activateAgentProviderProfile } from './settings.js';
// === end auto-imports ===

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

  export function providerDisplayName(provider) {
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
    const cloudPrefix = raw.match(/^([a-z0-9_-]+):/i)?.[1];
    if (cloudPrefix && CLOUD_PROVIDER_DEFAULTS[cloudPrefix]) return cloudPrefix;
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
      model.is_available !== false
      && isToolReadyModel(model)
      && isLocal(model)
      && (model.agent_recommended || /qwen3\.5:9b/i.test(`${model.id || ''} ${model.name || ''}`))
    );
    if (preferred) return preferred.id;
    const toolReady = state.models.find(model =>
      model.is_available !== false && isToolReadyModel(model) && isLocal(model)
    );
    if (toolReady) return toolReady.id;
    // No tool-ready model (e.g. local llama.cpp): fall back to the first available local model.
    return state.models.find(model => model.is_available !== false && isLocal(model))?.id || '';
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
    const catalogModel = state.models.find(model =>
      normalizeProviderId(model.provider) === normalized && model.is_available !== false
    );
    if (
      state.agentConfig
      && normalizeProviderId(state.agentConfig.provider) === normalized
      && String(state.agentConfig.model_id || '').trim()
    ) {
      const configured = resolveModelId(state.agentConfig.model_id, normalized);
      if (state.models.some(model => model.id === configured)) return configured;
    }
    return catalogModel?.id || CLOUD_PROVIDER_DEFAULTS[normalized]?.modelId || '';
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

    state.modelGroups
      .filter(group => group.kind === 'cloud' && group.configured !== false)
      .forEach(group => {
        const providerId = normalizeProviderId(group.provider);
        if (!providerId || options.some(option => option.provider === providerId)) return;
        options.push({
          value: `cloud:${providerId}`,
          label: group.label || providerDisplayName(providerId),
          provider: providerId,
          kind: 'cloud',
          profileId: null,
          modelId: defaultCloudModelForProvider(providerId),
          description: 'Provider cloud configurado no cofre.',
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

  export function selectedAgentProviderOption() {
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
    return state.models.filter(model => model.is_available !== false);
  }

  export function capabilityBadge(mode) {
    if (mode === 'unavailable') return { label: 'Indisponivel', tone: 'unknown' };
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
    if (!isLocalProvider(inferredProvider)) {
      return raw.replace(new RegExp(`^${inferredProvider}:`, 'i'), '');
    }
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
      const requestedId = resolveModelId(
        providerOption.modelId || state.agentConfig?.model_id || '',
        providerOption.provider,
      );
      const selected = state.models.find(model =>
        model.id === requestedId
        && normalizeProviderId(model.provider) === normalizeProviderId(providerOption.provider)
      ) || state.models.find(model =>
        normalizeProviderId(model.provider) === normalizeProviderId(providerOption.provider)
        && model.is_available !== false
      );
      if (!selected) return false;
      if (activeAgentModelId() !== selected.id) {
        selectModel(selected.id);
        if (persist) await persistAgentModelSelection(selected.id);
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
