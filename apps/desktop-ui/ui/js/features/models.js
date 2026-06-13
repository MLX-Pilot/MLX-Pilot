/* MLX Pilot — Models, downloads & catalog (feature).
 *
 * Model listing/refresh, the model picker + installed-models views, the
 * download queue (start/cancel/progress) and the discover catalog search.
 * Uses the core api/state/dom helpers.
 */

// === auto-imports (generated — do not edit) ===
import { pushConsoleEntry } from '../../app.js';
import { api } from '../core/api.js';
import { esc, fmtBytes, fmtNum, modelIcon, runConfirmation, showToast } from '../core/dom.js';
import { switchTab } from '../core/router.js';
import { API_SLOW_TIMEOUT_MS, CURRENT_MODEL_KEY, state } from '../core/state.js';
import { addSystemMsg } from './chat.js';
import { capabilityBadge, ensureAgentCompatibleModel, humanizeModelLabel, isAgentPanelActive, isLocalProvider, isToolReadyModel, modelCapabilityMode, modelCapabilityReason, persistAgentModelSelection, resolveModelId, visibleModelsForCurrentPanel } from './providers.js';
import { ensureVisibleModel, saveModelCache, updateAgentWorkspaceSummary } from './runtime.js';
// === end auto-imports ===

  // -- Models -------------------------------------------------
  function descriptorFromGroup(group, model) {
    const flags = Array.isArray(model.flags) ? model.flags : [];
    return {
      id: model.id,
      name: model.label || model.id,
      provider: model.provider || group.provider,
      path: model.id,
      is_available: true,
      agent_tool_mode: flags.includes('tool_use') ? 'tool_ready' : 'chat_only',
      agent_tool_reason: flags.includes('tool_use')
        ? 'O provider declara suporte a tool use.'
        : 'Modelo disponivel para chat; tool use nao foi declarado.',
      agent_recommended: flags.includes('recommended'),
      model_kind: group.kind,
      model_badge: model.badge || group.kind,
      model_group: group.provider,
      model_group_label: group.label,
      model_group_status: group.status,
      context: model.context || 0,
      flags,
    };
  }

  function fallbackLocalGroup(installedModels) {
    return {
      provider: 'local',
      kind: 'local',
      label: 'Local',
      status: 'active',
      models: installedModels
        .filter(model => model.is_available !== false)
        .map(model => ({
          id: model.id,
          label: model.name || model.id,
          provider: model.provider,
          badge: 'local',
          context: 0,
          flags: model.agent_recommended ? ['recommended'] : [],
        })),
    };
  }

  export async function loadModels({ force = false } = {}) {
    if (state.modelsLoading) return state.modelsPromise;
    if (!force && state.modelsLoaded && !state.modelsStale) return state.models;

    state.modelsLoading = true;
    renderInstalledModels();

    state.modelsPromise = (async () => {
      try {
        const [installedResult, groupedResult] = await Promise.allSettled([
          api('/models'),
          api('/models/all', { timeoutMs: API_SLOW_TIMEOUT_MS }),
        ]);
        if (installedResult.status === 'rejected') throw installedResult.reason;

        state.installedModels = Array.isArray(installedResult.value) ? installedResult.value : [];
        if (groupedResult.status === 'rejected') {
          console.warn('Unified model catalog load failed:', groupedResult.reason);
        }
        const returnedGroups = groupedResult.status === 'fulfilled' && Array.isArray(groupedResult.value)
          ? groupedResult.value
          : [];
        state.modelGroups = returnedGroups.length
          ? returnedGroups
          : [fallbackLocalGroup(state.installedModels)];
        state.models = state.modelGroups.flatMap(group =>
          (Array.isArray(group.models) ? group.models : []).map(model => descriptorFromGroup(group, model))
        );
        if (state.agentConfig?.model_id && isLocalProvider(state.agentConfig.provider)) {
          ensureVisibleModel(state.agentConfig.model_id, state.agentConfig.provider);
        }
        if (state.currentModel) {
          state.currentModel = resolveModelId(state.currentModel, state.agentConfig?.provider);
          const currentAvailable = state.models.some(model =>
            model.id === state.currentModel && model.is_available !== false
          );
          if (!currentAvailable && !isLocalProvider(state.agentConfig?.provider)) {
            state.currentModel = null;
          } else if (state.currentModel) {
            ensureVisibleModel(state.currentModel, state.agentConfig?.provider);
          }
        }
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
          state.installedModels = [];
          state.modelGroups = [];
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

  export function invalidateModels() {
    state.modelsStale = true;
  }

  export function refreshModelsInBackground() {
    if (state.modelsLoading) return;
    void loadModels({ force: true }).catch(() => {});
  }

  export function showInstalledModels() {
    renderInstalledModels();
    if (!state.modelsLoaded || state.modelsStale) refreshModelsInBackground();
  }

  export function renderModelPicker() {
    const menu = document.getElementById('model-menu');
    if (!menu) return;
    menu.innerHTML = '';
    const visibleModels = visibleModelsForCurrentPanel();
    const localModels = visibleModels.filter(model => model.model_kind !== 'cloud');
    const cloudModels = visibleModels.filter(model => model.model_kind === 'cloud');
    const scope = state.modelPickerScope === 'cloud' ? 'cloud' : 'local';
    const scopedModels = scope === 'cloud' ? cloudModels : localModels;

    const switcher = document.createElement('div');
    switcher.className = 'model-menu-switch';
    switcher.setAttribute('role', 'tablist');
    switcher.innerHTML = `
      <button type="button" role="tab" data-model-scope="local" class="${scope === 'local' ? 'active' : ''}" aria-selected="${scope === 'local'}">
        Local <span>${localModels.length}</span>
      </button>
      <button type="button" role="tab" data-model-scope="cloud" class="${scope === 'cloud' ? 'active' : ''}" aria-selected="${scope === 'cloud'}">
        Cloud <span>${cloudModels.length}</span>
      </button>`;
    switcher.querySelectorAll('[data-model-scope]').forEach(button => {
      button.addEventListener('click', (event) => {
        event.stopPropagation();
        state.modelPickerScope = button.dataset.modelScope;
        renderModelPicker();
      });
    });
    menu.appendChild(switcher);

    const scroll = document.createElement('div');
    scroll.className = 'model-menu-scroll';
    menu.appendChild(scroll);

    if (scopedModels.length === 0) {
      const empty = document.createElement('div');
      empty.className = 'model-menu-empty';
      empty.textContent = scope === 'cloud'
        ? 'Nenhum provider cloud configurado.'
        : 'Nenhum modelo local disponível.';
      scroll.appendChild(empty);
      return;
    }

    const grouped = new Map();
    scopedModels.forEach(model => {
      const key = model.model_group || (model.model_kind === 'cloud' ? model.provider : 'local');
      if (!grouped.has(key)) {
        grouped.set(key, {
          label: model.model_group_label || (model.model_kind === 'cloud' ? model.provider : 'Local'),
          kind: model.model_kind || 'local',
          status: model.model_group_status || 'active',
          models: [],
        });
      }
      grouped.get(key).models.push(model);
    });

    grouped.forEach(group => {
      if (scope === 'cloud') {
        const header = document.createElement('div');
        header.className = 'model-menu-group';
        header.innerHTML = `
          <span>${esc(group.label)}</span>
          ${group.status === 'degraded' ? '<span class="model-group-status">Catalogo fallback</span>' : ''}`;
        scroll.appendChild(header);
      }

      group.models.forEach(m => {
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
            <span class="model-origin-badge ${esc(m.model_badge || group.kind)}">${group.kind === 'cloud' ? 'Cloud' : 'Local'}</span>
            <span class="model-capability-badge ${badge.tone}">${badge.label}</span>
          </div>`;
        item.addEventListener('click', (e) => {
          e.stopPropagation();
          selectModel(m.id, { persistAgentConfig: isAgentPanelActive() });
          menu.classList.add('hidden');
        });
        scroll.appendChild(item);
      });
    });
    if (scope === 'cloud') {
      const notice = document.createElement('div');
      notice.className = 'model-menu-cloud-notice';
      notice.textContent = 'Modelos cloud enviam a conversa ao provider e podem gerar custos.';
      scroll.appendChild(notice);
    }
    if (!state.currentModel && localModels.length > 0) {
      selectModel(localModels[0].id, { persistAgentConfig: isAgentPanelActive() });
    } else if (!state.currentModel && scopedModels.length > 0) {
      selectModel(scopedModels[0].id, { persistAgentConfig: isAgentPanelActive() });
    }
  }

  export function selectModel(id, { persistAgentConfig = false } = {}) {
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

  export function renderInstalledModels() {
    const list = document.getElementById('installed-list');
    const count = document.getElementById('installed-count');
    if (!list) return;
    if (count) {
      if (!state.modelsLoaded && state.modelsLoading) {
        count.textContent = 'Carregando modelos...';
      } else {
        const suffix = state.modelsLoading ? ' • atualizando...' : '';
        const toolReadyCount = state.installedModels.filter(isToolReadyModel).length;
        count.textContent = `${state.installedModels.length} modelo${state.installedModels.length !== 1 ? 's' : ''} instalado${state.installedModels.length !== 1 ? 's' : ''} • ${toolReadyCount} Tool-ready${suffix}`;
      }
    }
    list.innerHTML = '';
    if (!state.modelsLoaded && state.modelsLoading) {
      list.innerHTML = '<div style="padding:24px;text-align:center;color:var(--text-tertiary)">Carregando modelos...</div>';
      return;
    }
    if (state.installedModels.length === 0) {
      list.innerHTML = '<div style="padding:24px;text-align:center;color:var(--text-tertiary)">Nenhum modelo instalado</div>';
      return;
    }
    state.installedModels.forEach(m => {
      const badge = capabilityBadge(modelCapabilityMode(m));
      const available = m.is_available !== false;
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
          <button class="action-btn" data-act="chat" data-id="${esc(m.id)}" ${available ? '' : 'disabled title="Provider indisponivel neste sistema"'}>Chat</button>
          <button class="action-btn danger" data-act="del" data-id="${esc(m.id)}" data-provider="${esc(m.provider || '')}">Remover</button>
        </div>`;
      list.appendChild(item);
    });
    list.querySelectorAll('[data-act="chat"]').forEach(b => b.addEventListener('click', () => { selectModel(b.dataset.id); switchTab('chat'); }));
    list.querySelectorAll('[data-act="del"]').forEach(b => b.addEventListener('click', async () => {
      const model = state.installedModels.find(entry => entry.id === b.dataset.id);
      const provider = b.dataset.provider || model?.provider || '';
      const query = provider ? `?provider=${encodeURIComponent(provider)}` : '';
      const removed = await runConfirmation({
        title: 'Remover modelo',
        message: 'Esta ação apaga os arquivos locais do modelo e não pode ser desfeita.',
        detail: model?.name || b.dataset.id,
        confirmLabel: 'Remover',
        pendingLabel: 'Removendo...',
        action: () => api(`/models/${encodeURIComponent(b.dataset.id)}${query}`, { method: 'DELETE' }),
      });
      if (!removed) return;

      state.installedModels = state.installedModels.filter(entry => entry.id !== b.dataset.id);
      state.models = state.models.filter(entry => entry.id !== b.dataset.id);
      invalidateModels();
      renderInstalledModels();
      renderModelPicker();
      showToast('Modelo removido com sucesso.');
      refreshModelsInBackground();
    }));
  }

  function isWebSearchEnabled() {
    const toggle = document.getElementById('web-search-toggle');
    if (toggle) return toggle.classList.contains('active');
    return Boolean(state.webSearchEnabled);
  }

  function renderWebSearchContext(searchResponse) {
    const results = Array.isArray(searchResponse?.results) ? searchResponse.results : [];
    if (!results.length) return '';

    const lines = results.slice(0, 5).map((result, index) => {
      const title = String(result.title || '').trim();
      const url = String(result.url || '').trim();
      const description = String(result.description || '').trim();
      return [
        `${index + 1}. ${title || url || 'Resultado sem titulo'}`,
        url ? `URL: ${url}` : '',
        description ? `Resumo: ${description}` : '',
      ].filter(Boolean).join('\n');
    });

    return [
      'Contexto de busca web recente. Use estes resultados quando responder e deixe claro quando a informacao veio da web.',
      `Consulta: ${searchResponse.query || ''}`,
      '',
      lines.join('\n\n'),
    ].join('\n');
  }

  export async function buildWebAugmentedMessages(userText) {
    if (!isWebSearchEnabled()) return state.messages;

    try {
      const searchResponse = await api('/web/brave/search', {
        method: 'POST',
        timeoutMs: API_SLOW_TIMEOUT_MS,
        body: JSON.stringify({
          query: userText,
          max_results: 5,
        }),
      });
      const webContext = renderWebSearchContext(searchResponse);
      const resultCount = Array.isArray(searchResponse?.results) ? searchResponse.results.length : 0;
      if (!webContext) {
        addSystemMsg('Busca web executada, mas nenhum resultado relevante foi retornado.');
        return state.messages;
      }
      addSystemMsg(`Busca web anexada ao contexto (${resultCount} resultado${resultCount === 1 ? '' : 's'}).`);
      return [
        { role: 'system', content: webContext },
        ...state.messages,
      ];
    } catch (error) {
      addSystemMsg(`Busca web nao executada: ${error.message}`);
      pushConsoleEntry('warn', 'web', `Busca web falhou: ${error.message}`);
      return state.messages;
    }
  }

  // -- Catalog ------------------------------------------------
  const ACTIVE_DOWNLOAD_STATUSES = new Set(['queued', 'running', 'cancelling']);

  function downloadPercent(job) {
    const percent = Number(job?.progress_percent || 0);
    return Math.round(Math.max(0, Math.min(100, Number.isFinite(percent) ? percent : 0)));
  }

  function downloadStatusLabel(status) {
    return {
      queued: 'Na fila',
      running: 'Baixando',
      cancelling: 'Cancelando',
      completed: 'Concluido',
      failed: 'Falhou',
      cancelled: 'Cancelado',
    }[status] || String(status || 'Aguardando');
  }

  function activeDownloadForModel(source, modelId) {
    return state.downloads.find(job =>
      job.source === source
      && job.model_id === modelId
      && ACTIVE_DOWNLOAD_STATUSES.has(job.status)
    );
  }

  function upsertDownload(job) {
    state.downloads = [
      job,
      ...state.downloads.filter(entry => entry.id !== job.id),
    ];
  }

  function scheduleDownloadRefresh() {
    if (state.downloadRefreshTimer) {
      clearTimeout(state.downloadRefreshTimer);
      state.downloadRefreshTimer = null;
    }
    if (!state.downloads.some(job => ACTIVE_DOWNLOAD_STATUSES.has(job.status))) return;
    state.downloadRefreshTimer = setTimeout(() => {
      state.downloadRefreshTimer = null;
      void loadDownloads();
    }, 800);
  }

  export async function loadDownloads() {
    if (state.downloadsLoading) return;
    state.downloadsLoading = true;
    const previous = new Map(state.downloads.map(job => [job.id, job.status]));
    try {
      const jobs = await api('/catalog/downloads');
      state.downloads = Array.isArray(jobs) ? jobs : [];
      const completedNow = state.downloads.some(job =>
        job.status === 'completed' && ACTIVE_DOWNLOAD_STATUSES.has(previous.get(job.id))
      );
      renderDownloads();
      renderCatalog();
      if (completedNow) {
        invalidateModels();
        refreshModelsInBackground();
      }
    } catch (error) {
      console.error('Download refresh failed:', error);
    } finally {
      state.downloadsLoading = false;
      scheduleDownloadRefresh();
    }
  }

  async function cancelDownload(jobId) {
    const current = state.downloads.find(job => job.id === jobId);
    if (current) {
      current.status = 'cancelling';
      current.can_cancel = false;
      renderDownloads();
      renderCatalog();
    }
    try {
      const job = await api(`/catalog/downloads/${encodeURIComponent(jobId)}/cancel`, {
        method: 'POST',
      });
      upsertDownload(job);
      renderDownloads();
      renderCatalog();
      scheduleDownloadRefresh();
    } catch (error) {
      await loadDownloads();
      alert('Erro ao cancelar download: ' + error.message);
    }
  }

  function renderDownloads() {
    const panel = document.getElementById('catalog-download-panel');
    const list = document.getElementById('catalog-download-list');
    const summary = document.getElementById('catalog-download-summary');
    if (!panel || !list) return;

    panel.hidden = state.downloads.length === 0;
    if (state.downloads.length === 0) {
      list.innerHTML = '';
      if (summary) summary.textContent = '';
      return;
    }

    const activeCount = state.downloads.filter(job => ACTIVE_DOWNLOAD_STATUSES.has(job.status)).length;
    if (summary) {
      summary.textContent = activeCount > 0
        ? `${activeCount} em andamento`
        : `${state.downloads.length} recente${state.downloads.length === 1 ? '' : 's'}`;
    }

    list.innerHTML = '';
    state.downloads.slice(0, 6).forEach(job => {
      const percent = downloadPercent(job);
      const row = document.createElement('div');
      row.className = `download-row ${esc(job.status)}`;
      const byteProgress = job.bytes_total > 0
        ? `${fmtBytes(job.bytes_downloaded || 0)} de ${fmtBytes(job.bytes_total)}`
        : `${job.completed_files || 0} de ${job.total_files || 0} arquivos`;
      const detail = job.error || job.current_file || byteProgress;
      const canCancel = Boolean(job.can_cancel) && ACTIVE_DOWNLOAD_STATUSES.has(job.status);
      row.innerHTML = `
        <div class="download-row-header">
          <span class="download-row-name" title="${esc(job.model_id)}">${esc(job.model_id)}</span>
          <span class="download-status ${esc(job.status)}">${downloadStatusLabel(job.status)} ${percent}%</span>
        </div>
        <div class="download-progress-track" role="progressbar" aria-label="Download de ${esc(job.model_id)}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent}">
          <div class="download-progress-fill" style="width:${percent}%"></div>
        </div>
        <div class="download-row-meta">
          <span class="download-current-file" title="${esc(detail)}">${esc(detail)}</span>
          ${canCancel ? `<button class="download-cancel-btn" type="button" data-download-cancel="${esc(job.id)}">Cancelar</button>` : ''}
        </div>`;
      list.appendChild(row);
    });

    list.querySelectorAll('[data-download-cancel]').forEach(button => {
      button.addEventListener('click', () => void cancelDownload(button.dataset.downloadCancel));
    });
  }

  export async function searchCatalog(query) {
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
    if (activeDownloadForModel(source, modelId)) return;
    try {
      const job = await api('/catalog/downloads', { method: 'POST', body: JSON.stringify({ source, model_id: modelId }) });
      upsertDownload(job);
      invalidateModels();
      renderDownloads();
      renderCatalog();
      scheduleDownloadRefresh();
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
      const activeDownload = activeDownloadForModel('huggingface', m.model_id);
      const downloadLabel = activeDownload
        ? `${downloadStatusLabel(activeDownload.status)} ${downloadPercent(activeDownload)}%`
        : 'Baixar';
      card.innerHTML = `
        <div class="model-card-header">
          <div class="model-card-icon ${ic}">${(m.name || m.model_id || 'M')[0].toUpperCase()}</div>
          <div class="model-card-info">
            <h3>${esc(m.name || m.model_id)}</h3>
            <span class="model-card-source">${esc(m.author || m.source || '')}</span>
          </div>
          <button class="download-btn" data-src="huggingface" data-mid="${esc(m.model_id)}" ${activeDownload ? 'disabled' : ''}>
            <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="2"><path d="M8 2v9M4 8l4 4 4-4M2 14h12"/></svg>
            ${downloadLabel}
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
