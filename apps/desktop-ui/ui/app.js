/* ============================================================
   MLX PILOT — Orbital Command
   Fully functional frontend with backend API integration
   ============================================================ */

(function () {
  'use strict';

  // ── State ──────────────────────────────────────────────────
  const state = {
    daemonUrl: localStorage.getItem('mlxPilotDaemonUrl') || 'http://127.0.0.1:11435',
    models: [],
    modelsLoaded: false,
    modelsLoading: false,
    modelsStale: true,
    modelsPromise: null,
    currentModel: null,
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
    openclawFramework: 'openclaw',
    agentConfig: null,
    agentSessions: [],
    currentSessionId: null,
    auditEntries: [],
    plugins: [],
    skills: [],
    tools: [],
    channels: [],
    environmentVars: [],
    activeDiscoverTab: 'catalog',
  };

  // ── API ────────────────────────────────────────────────────
  async function api(path, opts = {}) {
    const url = state.daemonUrl + path;
    const res = await fetch(url, {
      ...opts,
      headers: { 'Content-Type': 'application/json', ...opts.headers },
    });
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

  // ── Splash ─────────────────────────────────────────────────
  const splash = document.getElementById('splash');
  const appEl = document.getElementById('app');

  setTimeout(() => {
    splash.classList.add('fade-out');
    appEl.classList.remove('hidden');
    setTimeout(() => { splash.style.display = 'none'; }, 800);
    bootSequence();
  }, 2200);

  async function bootSequence() {
    // Update sidebar daemon URL display
    const sidebarUrl = document.getElementById('sidebar-daemon-url');
    if (sidebarUrl) sidebarUrl.textContent = `Daemon ${state.daemonUrl.replace(/^https?:\/\//, '')}`;

    try {
      const health = await api('/health');
      state.healthOk = health?.status === 'ok';
      state.provider = health?.provider || 'auto';
      updateStatusBadge(state.healthOk);
    } catch {
      updateStatusBadge(false);
    }

    // Parallel data loads
    await Promise.allSettled([
      loadDaemonConfig(),
      loadModels({ force: true }),
      loadAgentConfig(),
      loadSessions(),
      loadPlugins(),
      loadSkills(),
      loadTools(),
      loadChannels(),
      loadAudit(),
      loadEnvironment(),
    ]);
  }

  function updateStatusBadge(online) {
    const badge = document.getElementById('status-badge');
    if (!badge) return;
    badge.innerHTML = online
      ? '<span class="badge-dot online"></span><span>Online</span>'
      : '<span class="badge-dot offline"></span><span>Offline</span>';
    badge.style.background = online ? 'var(--green-soft)' : 'var(--rose-soft)';
    badge.style.color = online ? 'var(--green)' : 'var(--rose)';
  }

  // ── Daemon Config (/config) ────────────────────────────────
  async function loadDaemonConfig() {
    try {
      const config = await api('/config');
      state.daemonConfig = config;
      populateSettings(config);
      populateOpenClawConfig(config);
    } catch (e) {
      console.error('Config load failed:', e);
    }
  }

  function populateSettings(c) {
    if (!c) return;
    const set = (id, val) => { const el = document.getElementById(id); if (el && val != null) el.value = val; };
    const setCheck = (id, val) => { const el = document.getElementById(id); if (el) el.checked = !!val; };
    const fw = document.querySelector('input[name="settings-framework"][value="' + (c.active_agent_framework || 'openclaw') + '"]');
    if (fw) { fw.checked = true; fw.dispatchEvent(new Event('change')); }

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

  function populateOpenClawConfig(c) {
    if (!c) return;
    const set = (id, val) => { const el = document.getElementById(id); if (el && val != null) el.value = val; };
    set('oc-models-dir', c.models_dir);
    set('oc-cli-path', c.openclaw_cli_path);
    set('oc-state-dir', c.openclaw_state_dir);

    const fw = document.querySelector('#oc-framework-cards input[value="' + (c.active_agent_framework || 'openclaw') + '"]');
    if (fw) { fw.checked = true; fw.dispatchEvent(new Event('change')); }
  }

  async function saveDaemonConfig() {
    try {
      // Gather from settings inputs
      const c = state.daemonConfig || {};
      const get = (id) => { const el = document.getElementById(id); return el ? el.value : undefined; };
      const getNum = (id) => { const v = get(id); return v != null && v !== '' ? Number(v) : undefined; };
      const getCheck = (id) => { const el = document.getElementById(id); return el ? el.checked : undefined; };
      const fw = document.querySelector('input[name="settings-framework"]:checked');

      if (fw) c.active_agent_framework = fw.value;
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

  // ── Agent Config (/agent/config) ───────────────────────────
  async function loadAgentConfig() {
    try {
      const config = await api('/agent/config');
      state.agentConfig = config;
      populateAgentPolicy(config);
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
      return true;
    } catch (e) {
      console.error('Save agent policy failed:', e);
      return false;
    }
  }

  // Save agent policy when radio buttons change
  document.querySelectorAll('input[name="exec"], input[name="approval"]').forEach(r => {
    r.addEventListener('change', () => saveAgentPolicy());
  });

  // ── Models ─────────────────────────────────────────────────
  async function loadModels({ force = false } = {}) {
    if (state.modelsLoading) return state.modelsPromise;
    if (!force && state.modelsLoaded && !state.modelsStale) return state.models;

    state.modelsLoading = true;
    renderInstalledModels();

    state.modelsPromise = (async () => {
      try {
        const models = await api('/models');
        state.models = Array.isArray(models) ? models : [];
        state.modelsLoaded = true;
        state.modelsStale = false;
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
    if (state.models.length === 0) {
      menu.innerHTML = '<div class="model-menu-item" style="pointer-events:none;color:var(--text-tertiary)">Nenhum modelo encontrado</div>';
      return;
    }
    state.models.forEach(m => {
      const item = document.createElement('div');
      item.className = 'model-menu-item' + (state.currentModel === m.id ? ' selected' : '');
      item.dataset.model = m.id;
      item.innerHTML = `<span class="model-menu-name">${esc(m.name || m.id)}</span><span class="model-menu-meta">${esc(m.provider || '')}</span>`;
      item.addEventListener('click', (e) => {
        e.stopPropagation();
        selectModel(m.id);
        menu.classList.add('hidden');
      });
      menu.appendChild(item);
    });
    if (!state.currentModel && state.models.length > 0) selectModel(state.models[0].id);
  }

  function selectModel(id) {
    state.currentModel = id;
    const nameEl = document.getElementById('current-model');
    const model = state.models.find(m => m.id === id);
    if (nameEl) nameEl.textContent = model ? (model.name || model.id) : id;
    renderModelPicker();
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
        count.textContent = `${state.models.length} modelo${state.models.length !== 1 ? 's' : ''} instalado${state.models.length !== 1 ? 's' : ''}${suffix}`;
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
      const item = document.createElement('div');
      item.className = 'installed-item';
      const ic = modelIcon(m.id);
      item.innerHTML = `
        <span class="installed-icon ${ic}">${(m.name || m.id)[0].toUpperCase()}</span>
        <div class="installed-info">
          <span class="installed-name">${esc(m.name || m.id)}</span>
          <span class="installed-meta">${esc(m.provider || '')} &middot; ${m.is_available ? 'Disponível' : 'Indisponível'}</span>
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

  // ── Catalog ────────────────────────────────────────────────
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

  // ── Chat Streaming ─────────────────────────────────────────
  async function sendChatMessage(text) {
    if (!text.trim() || state.isStreaming) return;
    if (!state.currentModel) { addSystemMsg('Selecione um modelo primeiro.'); return; }

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
      model_id: state.currentModel,
      messages: state.messages,
      options: { temperature: 0.2, airllm_enabled: state.airllmEnabled },
    };

    let thinking = '', answer = '', metrics = {};

    try {
      const res = await fetch(state.daemonUrl + '/chat/stream', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        signal: state.streamController.signal,
      });

      if (!res.ok) {
        if (res.status === 404 || res.status === 405) return sendChatNonStreaming(payload);
        throw new Error(`HTTP ${res.status}`);
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
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
            thinking += evt.delta || '';
            updateThinking(assistantEl, thinking);
          } else if (evt.event === 'answer_delta') {
            answer += evt.delta || '';
            updateAnswer(assistantEl, answer);
          } else if (evt.event === 'metrics') {
            metrics = { ...metrics, ...evt };
          } else if (evt.event === 'done') {
            metrics = { ...metrics, ...evt };
            addMetrics(assistantEl, metrics);
            state.messages.push({ role: 'assistant', content: answer });
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

  async function sendChatNonStreaming(payload) {
    const el = addMessage('assistant', '');
    updateStreamStatus(el, 'thinking');
    try {
      const res = await api('/chat', { method: 'POST', body: JSON.stringify(payload) });
      const content = res?.message?.content || res?.final_response || 'Sem resposta.';
      updateAnswer(el, content);
      state.messages.push({ role: 'assistant', content });
      if (res?.usage) addMetrics(el, { prompt_tokens: res.usage.prompt_tokens, completion_tokens: res.usage.completion_tokens, total_tokens: res.usage.total_tokens, latency_ms: res.latency_ms });
    } catch (e) {
      updateAnswer(el, `Erro: ${e.message}`);
    }
    state.isStreaming = false;
  }

  // ── Message DOM helpers ────────────────────────────────────
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
    let block = el.querySelector('.msg-thinking');
    const body = el.querySelector('.msg-body');
    if (!block) {
      const toggle = document.createElement('div');
      toggle.className = 'msg-thinking-toggle';
      toggle.innerHTML = '<span class="thinking-chevron">&#9662;</span> Pensando...';
      toggle.addEventListener('click', () => { block.style.display = block.style.display === 'none' ? 'block' : 'none'; });
      block = document.createElement('div');
      block.className = 'msg-thinking';
      block.innerHTML = `<div class="thinking-content"></div>`;
      body.insertBefore(block, body.firstChild);
      body.insertBefore(toggle, block);
    }
    block.querySelector('.thinking-content').textContent = text;
  }

  function updateAnswer(el, text) {
    const c = el?.querySelector('.msg-content');
    if (c) c.innerHTML = renderMarkdown(text);
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

  // ── Sessions (sidebar history) ─────────────────────────────
  async function loadSessions() {
    try {
      const sessions = await api('/agent/sessions');
      state.agentSessions = Array.isArray(sessions) ? sessions : [];
      renderSidebarHistory();
    } catch {
      state.agentSessions = [];
      renderSidebarHistory();
    }
  }

  function renderSidebarHistory() {
    const container = document.getElementById('chat-history');
    if (!container) return;
    container.innerHTML = '';
    if (state.agentSessions.length === 0) {
      container.innerHTML = '<div style="padding:8px 12px;font-size:12px;color:var(--text-tertiary)">Nenhuma sessão ainda</div>';
      return;
    }
    state.agentSessions.forEach(s => {
      const item = document.createElement('div');
      item.className = 'history-item' + (s.id === state.currentSessionId ? ' active' : '');
      const name = s.name || `Sessão ${s.id?.substring(0, 6) || '?'}`;
      const count = s.message_count || 0;
      item.innerHTML = `<span class="history-icon">&#9679;</span><span class="history-label" title="${esc(name)}">${esc(name)} <span style="opacity:0.5;font-size:11px">(${count})</span></span>`;
      item.addEventListener('click', () => {
        state.currentSessionId = s.id;
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
      }
    } catch (e) { console.error('New session failed:', e); }
  }

  // ── OpenClaw Runtime ───────────────────────────────────────
  function agentEndpoint(path) {
    const fw = state.openclawFramework;
    return fw === 'nanobot' ? `/nanobot${path}` : `/openclaw${path}`;
  }

  async function loadRuntimeStatus() {
    try {
      const runtime = await api(agentEndpoint('/runtime'));
      const card = document.getElementById('runtime-status-card');
      if (!card || !runtime) return;
      const isRunning = runtime.service_state === 'running' || runtime.running === true;
      card.querySelector('.runtime-badge').className = `runtime-badge ${isRunning ? 'running' : ''}`;
      card.querySelector('.runtime-badge').innerHTML = `<span class="badge-dot"></span> ${isRunning ? 'Executando' : 'Parado'}`;
      const meta = card.querySelector('.runtime-meta');
      if (meta) {
        const parts = [];
        if (runtime.pid) parts.push(`PID: ${runtime.pid}`);
        if (runtime.uptime_seconds) parts.push(`Uptime: ${fmtDuration(runtime.uptime_seconds)}`);
        meta.innerHTML = parts.map(p => `<span>${p}</span>`).join('');
      }
    } catch (e) { console.error('Runtime load failed:', e); }
  }

  async function loadOpenClawObservability() {
    try {
      const data = await api(agentEndpoint('/observability'));
      if (!data) return;
      const mv = document.querySelector('.obs-model-value');
      if (mv) mv.textContent = data.model || '-';
      const uv = document.querySelector('.obs-usage-value');
      if (uv && data.usage) uv.textContent = fmtNum(data.usage.total || 0) + ' ';
      const sl = document.querySelector('.obs-skills-list');
      if (sl && data.skills?.length) sl.innerHTML = data.skills.map(s => `<span class="skill-chip active">${esc(s)}</span>`).join('');
      else if (sl) sl.innerHTML = '<span style="color:var(--text-tertiary);font-size:12px">Nenhuma skill ativa</span>';
      const tl = document.querySelector('.obs-tools-list');
      if (tl && data.tools?.length) tl.innerHTML = data.tools.map(t => `<span class="tool-chip-sm">${esc(t)}</span>`).join('');
      else if (tl) tl.innerHTML = '<span style="color:var(--text-tertiary);font-size:12px">Nenhum tool disponível</span>';
    } catch { /* ok */ }
  }

  async function loadOpenClawLogs(stream) {
    const body = document.getElementById('log-body');
    if (!body) return;
    body.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-tertiary)">Carregando...</div>';
    try {
      const data = await api(agentEndpoint(`/logs?stream=${stream || 'gateway'}&max_bytes=8000`));
      const content = data?.content || '';
      if (!content.trim()) {
        body.innerHTML = '<div style="padding:20px;text-align:center;color:var(--text-tertiary)">Nenhum log disponível</div>';
        return;
      }
      body.innerHTML = content.split('\n').filter(Boolean).map(line => {
        const lvl = line.includes('ERROR') ? 'error' : line.includes('WARN') ? 'warn' : line.includes('DEBUG') ? 'debug' : 'info';
        return `<div class="log-line"><span class="log-level ${lvl}">${lvl.toUpperCase().substring(0,4)}</span> ${esc(line)}</div>`;
      }).join('');
    } catch (e) {
      body.innerHTML = `<div style="padding:20px;text-align:center;color:var(--rose)">Erro: ${esc(e.message)}</div>`;
    }
  }

  async function openClawChat(message) {
    try {
      const res = await api(agentEndpoint('/chat'), { method: 'POST', body: JSON.stringify({ message }) });
      return res?.reply || 'Sem resposta.';
    } catch (e) { return `Erro: ${e.message}`; }
  }

  // ── Plugins ────────────────────────────────────────────────
  async function loadPlugins() {
    try {
      const plugins = await api('/agent/plugins');
      state.plugins = Array.isArray(plugins) ? plugins : [];
      renderPlugins();
    } catch { state.plugins = []; renderPlugins(); }
  }

  function renderPlugins() {
    const list = document.getElementById('plugin-list');
    if (!list) return;
    list.innerHTML = '';
    if (state.plugins.length === 0) {
      list.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhum plugin</div>';
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
          t.classList.toggle('active');
        } catch (e) { alert('Erro: ' + e.message); }
      });
    });
  }

  // ── Skills ─────────────────────────────────────────────────
  async function loadSkills() {
    try {
      const data = await api('/agent/skills/check');
      state.skills = Array.isArray(data?.skills) ? data.skills : [];
      renderSkills();
    } catch { state.skills = []; renderSkills(); }
  }

  function renderSkills() {
    const list = document.getElementById('skills-list');
    if (!list) return;
    list.innerHTML = '';
    if (state.skills.length === 0) {
      list.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhuma skill</div>';
      return;
    }
    state.skills.forEach(s => {
      const chip = document.createElement('span');
      chip.className = `skill-chip ${s.active ? 'active' : ''}`;
      chip.textContent = s.name;
      chip.title = s.description || '';
      chip.addEventListener('click', async () => {
        try {
          await api(s.active ? '/agent/skills/disable' : '/agent/skills/enable', { method: 'POST', body: JSON.stringify({ skill: s.name }) });
          s.active = !s.active;
          chip.classList.toggle('active');
        } catch (e) { alert('Erro: ' + e.message); }
      });
      list.appendChild(chip);
    });
  }

  // ── Tools ──────────────────────────────────────────────────
  async function loadTools() {
    try {
      const tools = await api('/agent/tools');
      state.tools = Array.isArray(tools) ? tools : [];
      renderTools();
    } catch { state.tools = []; renderTools(); }
  }

  function renderTools() {
    const grid = document.getElementById('tools-grid');
    if (!grid) return;
    grid.innerHTML = '';
    if (state.tools.length === 0) {
      grid.innerHTML = '<span style="color:var(--text-tertiary);font-size:12px">Nenhum tool</span>';
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
  }

  // ── Channels ───────────────────────────────────────────────
  async function loadChannels() {
    try {
      const channels = await api('/agent/channels', { headers: { 'x-channel-protocol-version': 'v1' } });
      state.channels = Array.isArray(channels) ? channels : [];
      renderChannels();
    } catch { state.channels = []; renderChannels(); }
  }

  function renderChannels() {
    const list = document.getElementById('channel-list');
    if (!list) return;
    list.innerHTML = '';
    if (state.channels.length === 0) {
      list.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhum channel configurado</div>';
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

  // ── Audit ──────────────────────────────────────────────────
  async function loadAudit() {
    try {
      const data = await api('/agent/audit?limit=30');
      state.auditEntries = data?.entries || [];
      renderAuditFeed();
    } catch { state.auditEntries = []; renderAuditFeed(); }
  }

  function renderAuditFeed() {
    const feed = document.getElementById('audit-feed');
    if (!feed) return;
    feed.innerHTML = '';
    if (state.auditEntries.length === 0) {
      feed.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhum evento</div>';
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
  }

  // ── Environment ────────────────────────────────────────────
  async function loadEnvironment() {
    try {
      const data = await api('/environment?reveal=false');
      state.environmentVars = data?.variables || [];
      renderEnvironment();
    } catch { state.environmentVars = []; renderEnvironment(); }
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
      row.innerHTML = `
        <span class="env-key">${esc(v.key)}</span>
        <input type="${v.is_secret ? 'password' : 'text'}" class="input env-val" value="${esc(v.masked || v.value || '')}" data-key="${esc(v.key)}" ${v.is_secret ? 'data-secret="true"' : ''} />
        ${v.is_secret ? '<button class="action-btn reveal-btn">Revelar</button>' : ''}`;
      table.appendChild(row);
    });
    table.querySelectorAll('.reveal-btn').forEach(btn => {
      btn.addEventListener('click', async () => {
        const input = btn.previousElementSibling;
        if (input.dataset.revealed === 'true') {
          input.type = 'password'; input.dataset.revealed = 'false'; btn.textContent = 'Revelar';
        } else {
          try {
            const data = await api('/environment?reveal=true');
            const found = (data?.variables || []).find(v => v.key === input.dataset.key);
            if (found) { input.value = found.value; input.type = 'text'; input.dataset.revealed = 'true'; btn.textContent = 'Ocultar'; }
          } catch { /* ok */ }
        }
      });
    });
  }

  async function saveEnvironment() {
    const vals = {};
    document.querySelectorAll('#env-table .env-val').forEach(input => {
      if (input.dataset.key && input.dataset.revealed === 'true') vals[input.dataset.key] = input.value;
    });
    if (Object.keys(vals).length === 0) { alert('Nenhuma variável foi revelada para edição.'); return; }
    try {
      await api('/environment', { method: 'POST', body: JSON.stringify({ values: vals }) });
      const btn = document.getElementById('save-env-btn');
      if (btn) { btn.textContent = 'Salvo!'; setTimeout(() => { btn.textContent = 'Salvar Variáveis'; }, 2000); }
    } catch (e) { alert('Erro: ' + e.message); }
  }

  // ── Tab Navigation ─────────────────────────────────────────
  function switchTab(target) {
    document.querySelectorAll('.tab').forEach(t => { t.classList.remove('active'); t.setAttribute('aria-selected', 'false'); });
    document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
    const tab = document.querySelector(`[data-panel="${target}"]`);
    const panel = document.getElementById(`panel-${target}`);
    if (tab) { tab.classList.add('active'); tab.setAttribute('aria-selected', 'true'); }
    if (panel) panel.classList.add('active');

    if (target === 'discover') {
      searchCatalog('llama');
      if (state.activeDiscoverTab === 'installed') showInstalledModels();
    }
    if (target === 'openclaw') { loadRuntimeStatus(); loadOpenClawObservability(); }
    if (target === 'ai-interaction') initAICanvas();
  }

  document.querySelectorAll('.tab').forEach(tab => tab.addEventListener('click', () => switchTab(tab.dataset.panel)));

  // ── Model Picker ───────────────────────────────────────────
  document.getElementById('model-trigger')?.addEventListener('click', (e) => {
    e.stopPropagation();
    document.getElementById('model-menu')?.classList.toggle('hidden');
  });
  document.addEventListener('click', () => document.getElementById('model-menu')?.classList.add('hidden'));

  // ── Discover Sub-tabs ──────────────────────────────────────
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

  // ── Catalog Search ─────────────────────────────────────────
  let searchTimeout;
  document.getElementById('catalog-search')?.addEventListener('input', (e) => {
    clearTimeout(searchTimeout);
    searchTimeout = setTimeout(() => { if (e.target.value.trim().length >= 2) searchCatalog(e.target.value.trim()); }, 500);
  });

  // ── OpenClaw Sub-tabs ──────────────────────────────────────
  document.querySelectorAll('.oc-tab').forEach(tab => {
    tab.addEventListener('click', async () => {
      document.querySelectorAll('.oc-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      document.querySelectorAll('.oc-content').forEach(c => c.style.display = 'none');
      document.getElementById(`oc-${tab.dataset.oc}`).style.display = 'block';
      if (tab.dataset.oc === 'skills-tools') loadOpenClawObservability();
      if (tab.dataset.oc === 'logs') loadOpenClawLogs('gateway');
    });
  });

  // Log refresh + stream selector
  document.getElementById('log-refresh-btn')?.addEventListener('click', () => {
    const sel = document.getElementById('log-stream-select');
    loadOpenClawLogs(sel?.value || 'gateway');
  });
  document.getElementById('log-stream-select')?.addEventListener('change', (e) => {
    loadOpenClawLogs(e.target.value);
  });

  // ── OpenClaw Config Save ───────────────────────────────────
  document.getElementById('oc-save-config')?.addEventListener('click', async () => {
    const c = state.daemonConfig || {};
    const get = (id) => document.getElementById(id)?.value;
    if (get('oc-models-dir')) c.models_dir = get('oc-models-dir');
    if (get('oc-cli-path')) c.openclaw_cli_path = get('oc-cli-path');
    if (get('oc-state-dir')) c.openclaw_state_dir = get('oc-state-dir');
    const fw = document.querySelector('#oc-framework-cards input:checked');
    if (fw) c.active_agent_framework = fw.value;
    try {
      await api('/config', { method: 'POST', body: JSON.stringify(c) });
      state.daemonConfig = c;
      const btn = document.getElementById('oc-save-config');
      btn.textContent = 'Salvo!'; setTimeout(() => { btn.textContent = 'Aplicar Configurações'; }, 2000);
    } catch (e) { alert('Erro: ' + e.message); }
  });

  // ── OpenClaw Chat ──────────────────────────────────────────
  const ocInput = document.querySelector('#oc-chat .oc-input input');
  const ocSendBtn = document.querySelector('#oc-chat .send-btn');
  ocSendBtn?.addEventListener('click', async () => {
    if (!ocInput?.value.trim()) return;
    const msg = ocInput.value.trim(); ocInput.value = '';
    const box = document.querySelector('#oc-chat .oc-messages');
    box.innerHTML += `<div class="message user-message"><div class="msg-avatar">U</div><div class="msg-body"><div class="msg-content">${esc(msg)}</div></div></div>`;
    const reply = await openClawChat(msg);
    box.innerHTML += `<div class="message assistant-message"><div class="msg-avatar assistant">OC</div><div class="msg-body"><div class="msg-content markdown-body">${renderMarkdown(reply)}</div></div></div>`;
    box.scrollTop = box.scrollHeight;
  });
  ocInput?.addEventListener('keydown', (e) => { if (e.key === 'Enter') ocSendBtn?.click(); });

  // ── Agent Chat ─────────────────────────────────────────────
  const agentInput = document.querySelector('#panel-agent .oc-input input');
  const agentSendBtn = document.querySelector('#panel-agent .send-btn');
  agentSendBtn?.addEventListener('click', async () => {
    if (!agentInput?.value.trim()) return;
    const msg = agentInput.value.trim(); agentInput.value = '';
    const box = document.querySelector('#panel-agent .agent-chat-messages');
    box.innerHTML += `<div class="message user-message"><div class="msg-avatar">U</div><div class="msg-body"><div class="msg-content">${esc(msg)}</div></div></div>`;
    const agDiv = document.createElement('div');
    agDiv.className = 'message assistant-message';
    agDiv.innerHTML = `<div class="msg-avatar assistant">AG</div><div class="msg-body"><div class="msg-content markdown-body"><div class="thinking-indicator"><span>Processando</span><span class="dots"><span>.</span><span>.</span><span>.</span></span></div></div></div>`;
    box.appendChild(agDiv);
    box.scrollTop = box.scrollHeight;

    try {
      const payload = {
        session_id: state.currentSessionId,
        message: msg,
        provider: state.agentConfig?.provider || 'ollama',
        model_id: state.currentModel || state.agentConfig?.model_id || '',
        execution_mode: state.agentConfig?.execution_mode || 'full',
        approval_mode: state.agentConfig?.approval_mode || 'ask',
        max_iterations: 25,
      };
      const res = await api('/agent/run', { method: 'POST', body: JSON.stringify(payload) });
      if (res?.session_id) { state.currentSessionId = res.session_id; loadSessions(); }
      const content = res?.final_response || 'Sem resposta.';
      agDiv.querySelector('.msg-content').innerHTML = renderMarkdown(content);
      if (res?.total_tokens) {
        addMetrics(agDiv, res);
      }
    } catch (e) {
      agDiv.querySelector('.msg-content').innerHTML = `<span style="color:var(--rose)">Erro: ${esc(e.message)}</span>`;
    }
    box.scrollTop = box.scrollHeight;
  });
  agentInput?.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); agentSendBtn?.click(); } });

  // ── Audit Refresh ──────────────────────────────────────────
  document.getElementById('refresh-audit')?.addEventListener('click', () => loadAudit());

  // ── Settings Save ──────────────────────────────────────────
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

  // ── Sidebar: New Chat ──────────────────────────────────────
  document.getElementById('btn-new-chat')?.addEventListener('click', () => {
    state.messages = [];
    state.currentSessionId = null;
    const msgs = document.getElementById('chat-messages');
    if (msgs) msgs.innerHTML = '<div class="welcome-message" style="text-align:center;padding:60px 20px;max-width:500px;margin:0 auto"><h3 style="font-family:var(--font-heading);font-size:20px;margin-bottom:8px">MLX Pilot Chat</h3><p style="font-size:14px;color:var(--text-tertiary)">Selecione um modelo e envie sua mensagem.</p></div>';
    createNewSession();
    switchTab('chat');
  });

  // ── Daemon URL ─────────────────────────────────────────────
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

  // ── Toggle Chips ───────────────────────────────────────────
  document.querySelectorAll('.toggle-chip').forEach(chip => {
    chip.addEventListener('click', () => {
      chip.classList.toggle('active');
      if (chip.id === 'web-search-toggle') state.webSearchEnabled = chip.classList.contains('active');
      if (chip.id === 'airllm-toggle') state.airllmEnabled = chip.classList.contains('active');
    });
  });

  // ── Chat Input ─────────────────────────────────────────────
  const chatInput = document.getElementById('chat-input');
  chatInput?.addEventListener('input', () => { chatInput.style.height = 'auto'; chatInput.style.height = Math.min(chatInput.scrollHeight, 160) + 'px'; });
  chatInput?.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChatMessage(chatInput.value); } });
  document.getElementById('send-btn')?.addEventListener('click', () => sendChatMessage(chatInput?.value || ''));

  // ── Radio Card generic ─────────────────────────────────────
  document.querySelectorAll('.radio-card input[type="radio"]').forEach(radio => {
    radio.addEventListener('change', () => {
      document.querySelectorAll(`input[name="${radio.name}"]`).forEach(r => r.closest('.radio-card')?.classList.remove('selected'));
      radio.closest('.radio-card')?.classList.add('selected');
    });
  });

  // ── Code Copy ──────────────────────────────────────────────
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('.code-copy');
    if (!btn) return;
    const code = btn.closest('.code-block')?.querySelector('code');
    if (code) { navigator.clipboard.writeText(code.textContent).then(() => { btn.textContent = 'Copiado!'; setTimeout(() => { btn.textContent = 'Copiar'; }, 2000); }); }
  });

  // ── AI Canvas Particles ────────────────────────────────────
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

  // ── Atmosphere ─────────────────────────────────────────────
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

  // ── OpenClaw Runtime Controls ──────────────────────────────
  async function runtimeAction(action) {
    try {
      const res = await api(agentEndpoint('/runtime'), { method: 'POST', body: JSON.stringify({ action }) });
      if (res?.runtime) loadRuntimeStatus();
    } catch (e) { alert('Erro: ' + e.message); }
  }

  document.getElementById('runtime-restart')?.addEventListener('click', () => runtimeAction('restart'));
  document.getElementById('runtime-stop')?.addEventListener('click', () => runtimeAction('stop'));
  document.getElementById('runtime-logs')?.addEventListener('click', () => {
    // Switch to logs tab
    document.querySelectorAll('.oc-tab').forEach(t => t.classList.remove('active'));
    document.querySelector('.oc-tab[data-oc="logs"]')?.classList.add('active');
    document.querySelectorAll('.oc-content').forEach(c => c.style.display = 'none');
    document.getElementById('oc-logs').style.display = 'block';
    loadOpenClawLogs('gateway');
  });

  // ── Agent: New Session / Export ─────────────────────────────
  document.getElementById('btn-new-session')?.addEventListener('click', async () => {
    try {
      const session = await api('/agent/sessions', { method: 'POST', body: JSON.stringify({ name: '' }) });
      if (session?.id) {
        state.currentSessionId = session.id;
        state.messages = [];
        const msgs = document.getElementById('chat-messages');
        if (msgs) msgs.innerHTML = '';
        await loadSessions();
      }
    } catch (e) { alert('Erro: ' + e.message); }
  });

  document.getElementById('btn-export-session')?.addEventListener('click', () => {
    if (!state.currentSessionId) { alert('Nenhuma sessão selecionada'); return; }
    window.open(state.daemonUrl + '/agent/sessions/' + state.currentSessionId + '/export', '_blank');
  });

  // ── Agent: New Channel ─────────────────────────────────────
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

  // ── AI Visual Panel ────────────────────────────────────────
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
    if (state.currentModel) {
      try {
        const msgs = [{ role: 'user', content: prompt }];
        const res = await api('/chat', {
          method: 'POST',
          body: JSON.stringify({ model_id: state.currentModel, messages: msgs, options: { temperature: 0.7 } }),
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

  // Example buttons → fill input and render
  document.querySelectorAll('.example-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const prompt = btn.dataset.prompt || btn.textContent;
      if (aiInput) aiInput.value = prompt;
      renderAIVisual(prompt);
    });
  });

  // ── Keyboard ───────────────────────────────────────────────
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'k') { e.preventDefault(); document.getElementById('model-menu')?.classList.toggle('hidden'); }
    if (e.key === 'Escape') document.getElementById('model-menu')?.classList.add('hidden');
    if (!e.ctrlKey && !e.metaKey && !e.altKey && !['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName)) {
      const n = parseInt(e.key);
      if (n >= 1 && n <= 6) switchTab(['chat', 'discover', 'openclaw', 'agent', 'ai-interaction', 'settings'][n - 1]);
    }
    if ((e.ctrlKey || e.metaKey) && e.key === '.') state.streamController?.abort();
  });

  // ── Utilities ──────────────────────────────────────────────
  function esc(s) { if (!s) return ''; const d = document.createElement('div'); d.textContent = String(s); return d.innerHTML; }
  function fmtBytes(b) { if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB'; if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB'; return (b / 1e3).toFixed(0) + ' KB'; }
  function fmtNum(n) { if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M'; if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K'; return String(n); }
  function fmtDuration(s) { if (s < 60) return s + 's'; if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`; return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`; }
  function modelIcon(id) { const l = (id || '').toLowerCase(); if (l.includes('llama')) return 'llama'; if (l.includes('mistral')) return 'mistral'; if (l.includes('qwen')) return 'qwen'; if (l.includes('deepseek')) return 'deepseek'; if (l.includes('phi')) return 'phi'; return 'llama'; }

  function renderMarkdown(text) {
    if (!text) return '';
    let h = esc(text);
    h = h.replace(/```(\w*)\n([\s\S]*?)```/g, (_, l, c) => `<div class="code-block"><div class="code-header"><span class="code-lang">${esc(l || 'code')}</span><button class="code-copy">Copiar</button></div><pre><code>${c.trim()}</code></pre></div>`);
    h = h.replace(/`([^`]+)`/g, '<code>$1</code>');
    h = h.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
    h = h.replace(/\n\n/g, '</p><p>');
    h = h.replace(/\n/g, '<br>');
    h = '<p>' + h + '</p>';
    h = h.replace(/(https?:\/\/[^\s<]+)/g, '<a href="$1" target="_blank" rel="noopener">$1</a>');
    return h;
  }

})();
