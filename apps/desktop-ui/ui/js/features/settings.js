/* MLX Pilot — Settings & agent config (feature).
 *
 * The daemon /config settings form and the agent /agent/config panel: policy,
 * provider selection, and provider-profile activation/save. Uses core api/state.
 */

// === auto-imports (generated — do not edit) ===
import { agentConfigModelId, createProviderProfileDraft, defaultCloudModelForProvider, ensureAgentCompatibleModel, inferModelProvider, isLocalProvider, isToolReadyModel, profileHasConfiguredSecret, readAgentProviderProfilesFromDom, recommendedAgentModelId, renderAgentProviderProfiles, renderAgentProviderSelector, resolveModelId } from '../../app.js';
import { api } from '../core/api.js';
import { state } from '../core/state.js';
import { ensureVisibleModel, hydrateModelShell, updateAgentWorkspaceSummary } from './runtime.js';
// === end auto-imports ===

  // -- Daemon Config (/config) --------------------------------
  export async function loadDaemonConfig() {
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

  export async function saveDaemonConfig() {
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
  export async function loadAgentConfig() {
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
          localModel?.provider || payload.provider || state.agentConfig?.provider || 'llamacpp',
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

  export async function activateAgentProviderProfile(profileId) {
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

  // Provider profiles collapse toggle
  document.getElementById('agent-toggle-provider-profiles')?.addEventListener('click', () => {
    const container = document.getElementById('agent-provider-profile-container');
    const btn = document.getElementById('agent-toggle-provider-profiles');
    if (!container || !btn) return;
    const hidden = container.style.display === 'none';
    container.style.display = hidden ? '' : 'none';
    btn.innerHTML = hidden ? '&#9660; Colapsar' : '&#9654; Expandir';
  });
