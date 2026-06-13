/* MLX Pilot — Agent panels: plugins, skills, tools, channels, audit (feature).
 *
 * Loads and renders the agent capability panels (plugins, skills, tools,
 * messaging channels) and the audit/observability feed. Talks to the daemon
 * agent endpoints via the core api helper.
 */

// === auto-imports (generated — do not edit) ===
import { updateAgentWorkspaceSummary } from '../../app.js';
import { api } from '../core/api.js';
import { esc } from '../core/dom.js';
import { state } from '../core/state.js';
// === end auto-imports ===

  // -- Plugins ------------------------------------------------
  export async function loadPlugins() {
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
  export async function loadSkills() {
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
  export async function loadTools() {
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
      chip.className = t.enabled ? 'tool-chip' : 'tool-chip disabled';
      chip.textContent = t.name;
      chip.title = t.description || '';
      grid.appendChild(chip);
    });
    updateAgentWorkspaceSummary();
  }

  // -- Channels -----------------------------------------------
  export async function loadChannels() {
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
      list.innerHTML = '<div class="agent-empty-copy" style="display:flex;flex-direction:column;align-items:center;gap:8px"><svg viewBox="0 0 20 20" width="24" height="24" fill="none" stroke="var(--text-tertiary)" stroke-width="1.5" opacity="0.5"><path d="M3 5h14v10H3z"/><path d="M7 5V3h6v2"/></svg><span>Nenhum channel conectado. Clique em + Novo Channel para começar.</span></div>';
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
    card.classList.add(connected ? 'connected' : 'disconnected');
    card.innerHTML = `
      <div class="channel-status">
        <span class="channel-status-badge ${connected ? 'connected' : 'disconnected'}">${connected ? 'Conectado' : 'Desconectado'}</span>
      </div>
      <div class="channel-info">
        <span class="channel-name">${esc(displayName)}</span>
        <span class="channel-meta">${esc(channelId)}</span>
      </div>
      <div class="channel-actions">
        ${connected ? '' : `<button class="action-btn" data-reconnect data-ch="${esc(channelId)}" data-acc="${esc(account?.account_id || account?.id || '')}">Reconectar</button>`}
        <button class="action-btn danger" data-ch="${esc(channelId)}" data-acc="${esc(account?.account_id || account?.id || '')}">Remover</button>
      </div>`;
    card.querySelectorAll('.action-btn[data-reconnect]').forEach(btn => {
      btn.addEventListener('click', async () => {
        try {
          const body = { channel: btn.dataset.ch };
          if (btn.dataset.acc) body.account_id = btn.dataset.acc;
          await api('/agent/channels/connect', { method: 'POST', headers: { 'x-channel-protocol-version': 'v1' }, body: JSON.stringify(body) });
          loadChannels();
        } catch (e) { alert('Erro: ' + e.message); }
      });
    });
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
  export async function loadAudit() {
    try {
      const data = await api('/agent/audit?limit=30');
      state.auditEntries = data?.entries || [];
      renderAuditFeed();
    } catch { state.auditEntries = []; renderAuditFeed(); }
  }

  export function renderAuditFeed() {
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
    const entriesWithType = state.auditEntries.map(entry => {
      let dot = 'success';
      let typeClass = 'type-success';
      if (entry.status === 'denied' || entry.status === 'error') { dot = 'error'; typeClass = 'type-error'; }
      else if (entry.event_type === 'approval' || entry.status === 'ask') { dot = 'approval'; typeClass = 'type-approval'; }
      else if (entry.tool_name) {
        const tn = String(entry.tool_name).toLowerCase();
        if (tn.includes('bash') || tn.includes('exec') || tn.includes('shell')) { dot = 'tool'; typeClass = 'type-bash'; }
        else { dot = 'tool'; typeClass = 'type-tool'; }
      }
      return { entry, dot, typeClass };
    });

    const filter = state.auditFilter || 'all';
    const filtered = filter === 'all'
      ? entriesWithType
      : entriesWithType.filter(e => {
          if (filter === 'tool') return e.typeClass === 'type-tool';
          if (filter === 'bash') return e.typeClass === 'type-bash';
          if (filter === 'approval') return e.typeClass === 'type-approval';
          if (filter === 'error') return e.typeClass === 'type-error';
          return true;
        });

    if (filtered.length === 0) {
      feed.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhum evento para este filtro</div>';
      updateAgentWorkspaceSummary();
      return;
    }

    filtered.forEach(({ entry, dot, typeClass }) => {
      const item = document.createElement('div');
      item.className = `audit-item ${typeClass}`;
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
