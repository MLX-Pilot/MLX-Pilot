/* MLX Pilot — Wave 1 features (Presets, Memory, History, Compare).
 *
 * Self-contained module loaded after app.js. It reuses the same daemon URL and
 * the generic tab switching in app.js (panels with class `.panel` + id
 * `panel-<name>` are shown/hidden automatically), and talks to the daemon's
 * /agent/presets, /agent/memory, /agent/sessions and /compare endpoints.
 */
(function () {
  'use strict';

  // ── daemon client ──────────────────────────────────────────────────────
  const DEFAULT_DAEMON_URL = 'http://127.0.0.1:11435';
  function daemonUrl() {
    return (
      window.__MLX_PILOT_DAEMON_URL__ ||
      (function () {
        try { return localStorage.getItem('mlxPilotDaemonUrl'); } catch (_) { return null; }
      })() ||
      DEFAULT_DAEMON_URL
    );
  }

  async function api(path, opts) {
    opts = opts || {};
    const headers = Object.assign({ 'Content-Type': 'application/json' }, opts.headers || {});
    const res = await fetch(daemonUrl() + path, Object.assign({}, opts, { headers }));
    if (!res.ok) {
      let message = 'HTTP ' + res.status;
      try { const body = await res.json(); if (body && body.error) message = body.error; } catch (_) {}
      throw new Error(message);
    }
    const ct = res.headers.get('content-type') || '';
    return ct.indexOf('application/json') >= 0 ? res.json() : res.text();
  }

  // ── tiny DOM helpers ────────────────────────────────────────────────────
  function esc(value) {
    return String(value == null ? '' : value)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }
  function $(id) { return document.getElementById(id); }
  function fmtDate(iso) {
    if (!iso) return '';
    const d = new Date(iso);
    if (isNaN(d.getTime())) return '';
    return d.toLocaleString();
  }
  let toastTimer = null;
  function toast(message, kind) {
    let box = $('wave1-toast');
    if (!box) {
      box = document.createElement('div');
      box.id = 'wave1-toast';
      document.body.appendChild(box);
    }
    box.textContent = message;
    box.className = 'wave1-toast show' + (kind === 'error' ? ' error' : '');
    clearTimeout(toastTimer);
    toastTimer = setTimeout(function () { box.className = 'wave1-toast'; }, 2800);
  }

  // ── styles (reuse app theme variables, with safe fallbacks) ─────────────
  function injectStyles() {
    if ($('wave1-styles')) return;
    const css = `
    .wave1-root{height:100%;overflow:auto;padding:20px 24px;color:var(--text-primary,#e9e9f2);}
    .wave1-head{display:flex;align-items:center;justify-content:space-between;gap:12px;margin-bottom:16px;flex-wrap:wrap;}
    .wave1-head h2{font-family:var(--font-heading,inherit);font-size:18px;font-weight:600;margin:0;}
    .wave1-head .sub{font-size:12px;color:var(--text-tertiary,#8a8aa0);margin-top:2px;}
    .wave1-toolbar{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-bottom:14px;}
    .wave1-input,.wave1-select,.wave1-textarea{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);
      color:var(--text-primary,#e9e9f2);border-radius:8px;padding:8px 10px;font-size:13px;font-family:inherit;outline:none;}
    .wave1-input:focus,.wave1-textarea:focus,.wave1-select:focus{border-color:var(--cyan,#39d0d8);}
    .wave1-textarea{width:100%;resize:vertical;min-height:64px;line-height:1.5;}
    .wave1-input.grow{flex:1;min-width:160px;}
    .wave1-btn{background:var(--cyan,#39d0d8);color:#04121a;border:none;border-radius:8px;padding:8px 14px;font-size:13px;
      font-weight:600;cursor:pointer;display:inline-flex;align-items:center;gap:6px;}
    .wave1-btn:hover{filter:brightness(1.08);}
    .wave1-btn.ghost{background:transparent;color:var(--text-secondary,#b9b9cc);border:1px solid var(--border,#2a2a44);}
    .wave1-btn.ghost:hover{border-color:var(--cyan,#39d0d8);color:var(--text-primary,#e9e9f2);}
    .wave1-btn.danger{background:transparent;color:#ff7a8a;border:1px solid #5a2a35;}
    .wave1-btn.sm{padding:4px 9px;font-size:12px;border-radius:6px;}
    .wave1-btn:disabled{opacity:.5;cursor:not-allowed;}
    .wave1-card{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:10px;padding:14px 16px;margin-bottom:10px;}
    .wave1-card .row{display:flex;justify-content:space-between;gap:10px;align-items:flex-start;}
    .wave1-card .title{font-weight:600;font-size:14px;}
    .wave1-card .preview{font-size:13px;color:var(--text-secondary,#b9b9cc);margin-top:6px;white-space:pre-wrap;word-break:break-word;line-height:1.5;}
    .wave1-badges{display:flex;gap:6px;flex-wrap:wrap;margin-top:8px;}
    .wave1-badge{font-size:11px;padding:2px 8px;border-radius:999px;background:var(--bg-deep,#0c0c18);border:1px solid var(--border,#2a2a44);color:var(--text-tertiary,#8a8aa0);}
    .wave1-badge.accent{color:var(--cyan,#39d0d8);border-color:var(--cyan,#39d0d8);}
    .wave1-actions{display:flex;gap:6px;flex-shrink:0;flex-wrap:wrap;}
    .wave1-empty{color:var(--text-tertiary,#8a8aa0);font-size:13px;text-align:center;padding:36px 12px;}
    .wave1-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:12px;margin-top:14px;}
    .wave1-pane{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:10px;display:flex;flex-direction:column;min-height:160px;}
    .wave1-pane.winner{border-color:var(--cyan,#39d0d8);box-shadow:0 0 0 1px var(--cyan,#39d0d8);}
    .wave1-pane .phead{display:flex;align-items:center;justify-content:space-between;padding:10px 12px;border-bottom:1px solid var(--border,#2a2a44);}
    .wave1-pane .label{font-weight:700;color:var(--cyan,#39d0d8);font-size:14px;}
    .wave1-pane .model{font-size:11px;color:var(--text-tertiary,#8a8aa0);}
    .wave1-pane .pbody{padding:12px;font-size:13px;white-space:pre-wrap;word-break:break-word;line-height:1.55;flex:1;overflow:auto;max-height:340px;}
    .wave1-pane .pfoot{padding:8px 12px;border-top:1px solid var(--border,#2a2a44);display:flex;gap:6px;align-items:center;}
    .wave1-pane .err{color:#ff7a8a;}
    .wave1-row-controls{display:flex;gap:8px;align-items:center;flex-wrap:wrap;margin-top:10px;}
    .wave1-checkboxes{display:flex;flex-wrap:wrap;gap:6px;max-height:140px;overflow:auto;padding:8px;border:1px solid var(--border,#2a2a44);border-radius:8px;background:var(--bg-deep,#0c0c18);}
    .wave1-chk{display:inline-flex;align-items:center;gap:6px;font-size:12px;background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:6px;padding:4px 8px;cursor:pointer;}
    .wave1-chk input{accent-color:var(--cyan,#39d0d8);}
    .wave1-split{display:grid;grid-template-columns:minmax(260px,360px) 1fr;gap:14px;height:calc(100% - 60px);}
    .wave1-list-pane{overflow:auto;padding-right:4px;}
    .wave1-detail-pane{overflow:auto;border-left:1px solid var(--border,#2a2a44);padding-left:14px;}
    .wave1-msg{border:1px solid var(--border,#2a2a44);border-radius:8px;padding:10px 12px;margin-bottom:8px;background:var(--bg-elevated,#16162a);}
    .wave1-msg .mrole{font-size:11px;text-transform:uppercase;letter-spacing:.04em;color:var(--text-tertiary,#8a8aa0);margin-bottom:4px;display:flex;justify-content:space-between;}
    .wave1-msg .mcontent{white-space:pre-wrap;word-break:break-word;font-size:13px;line-height:1.55;}
    .wave1-label{font-size:12px;color:var(--text-tertiary,#8a8aa0);display:flex;flex-direction:column;gap:4px;}
    .wave1-field{margin-bottom:12px;}
    .wave1-field label{font-size:12px;color:var(--text-tertiary,#8a8aa0);display:block;margin-bottom:4px;}
    .wave1-field input,.wave1-field textarea,.wave1-field select{width:100%;box-sizing:border-box;}
    .wave1-overlay{position:fixed;inset:0;background:rgba(4,6,14,.62);display:flex;align-items:center;justify-content:center;z-index:5000;}
    .wave1-modal{background:var(--bg-base,#101022);border:1px solid var(--border,#2a2a44);border-radius:14px;width:min(560px,92vw);max-height:88vh;overflow:auto;padding:20px 22px;box-shadow:0 20px 60px rgba(0,0,0,.5);}
    .wave1-modal h3{margin:0 0 14px;font-size:16px;}
    .wave1-modal .mfoot{display:flex;justify-content:flex-end;gap:8px;margin-top:8px;}
    .wave1-modal .two{display:grid;grid-template-columns:1fr 1fr;gap:10px;}
    .preset-picker{position:relative;}
    .preset-menu{position:absolute;bottom:calc(100% + 8px);left:0;min-width:240px;max-height:320px;overflow:auto;background:var(--bg-base,#101022);
      border:1px solid var(--border,#2a2a44);border-radius:10px;padding:6px;z-index:1200;box-shadow:0 12px 36px rgba(0,0,0,.5);}
    .preset-menu.hidden{display:none;}
    .preset-item{display:flex;justify-content:space-between;align-items:center;gap:8px;padding:8px 10px;border-radius:7px;cursor:pointer;font-size:13px;}
    .preset-item:hover{background:var(--bg-elevated,#16162a);}
    .preset-item .pname{font-weight:600;}
    .preset-item .pdesc{font-size:11px;color:var(--text-tertiary,#8a8aa0);}
    .preset-menu .divider{height:1px;background:var(--border,#2a2a44);margin:6px 4px;}
    .toggle-chip.active #preset-active-label,.toggle-chip.preset-on{color:var(--cyan,#39d0d8);}
    .wave1-toast{position:fixed;bottom:24px;left:50%;transform:translateX(-50%) translateY(20px);background:var(--bg-base,#101022);
      border:1px solid var(--cyan,#39d0d8);color:var(--text-primary,#e9e9f2);padding:10px 18px;border-radius:10px;font-size:13px;
      opacity:0;pointer-events:none;transition:all .25s;z-index:6000;}
    .wave1-toast.show{opacity:1;transform:translateX(-50%) translateY(0);}
    .wave1-toast.error{border-color:#ff7a8a;}
    `;
    const style = document.createElement('style');
    style.id = 'wave1-styles';
    style.textContent = css;
    document.head.appendChild(style);
  }

  // ── generic modal ────────────────────────────────────────────────────────
  function openModal(title, bodyHtml, onMount) {
    const root = $('wave1-modal-root') || document.body;
    const overlay = document.createElement('div');
    overlay.className = 'wave1-overlay';
    overlay.innerHTML = `<div class="wave1-modal"><h3>${esc(title)}</h3><div class="mbody">${bodyHtml}</div></div>`;
    overlay.addEventListener('mousedown', function (e) { if (e.target === overlay) close(); });
    function close() { overlay.remove(); document.removeEventListener('keydown', onKey); }
    function onKey(e) { if (e.key === 'Escape') close(); }
    document.addEventListener('keydown', onKey);
    root.appendChild(overlay);
    if (onMount) onMount(overlay.querySelector('.mbody'), close);
    return close;
  }

  // ════════════════════════════════════════════════════════════════════════
  // PRESETS
  // ════════════════════════════════════════════════════════════════════════
  let presetCache = [];

  async function loadPresets() {
    try { presetCache = await api('/agent/presets'); } catch (_) { presetCache = []; }
    return presetCache;
  }

  function renderPresetMenu() {
    const menu = $('preset-menu');
    if (!menu) return;
    let html = '';
    if (!presetCache.length) {
      html += '<div class="preset-item" style="cursor:default;color:var(--text-tertiary,#8a8aa0)">Nenhum preset ainda</div>';
    } else {
      presetCache.forEach(function (p) {
        html += `<div class="preset-item" data-apply="${esc(p.id)}">
          <div><div class="pname">${esc(p.name)}${p.favorite ? ' ★' : ''}</div>
          ${p.description ? `<div class="pdesc">${esc(p.description)}</div>` : ''}</div></div>`;
      });
    }
    html += '<div class="divider"></div>';
    html += '<div class="preset-item" data-manage="1"><div class="pname">⚙ Gerenciar presets…</div></div>';
    menu.innerHTML = html;
  }

  function applyPresetToInput(preset) {
    const input = $('chat-input');
    if (!input) { toast('Abra o Chat para usar um preset', 'error'); return; }
    const current = input.value || '';
    let composed = '';
    if (preset.system_prompt && preset.system_prompt.trim()) composed += preset.system_prompt.trim() + '\n\n';
    composed += (preset.prefix || '') + current + (preset.suffix || '');
    input.value = composed;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.focus();
    const chip = $('preset-toggle');
    const label = $('preset-active-label');
    if (label) label.textContent = preset.name.length > 14 ? preset.name.slice(0, 13) + '…' : preset.name;
    if (chip) chip.classList.add('preset-on');
    toast('Preset "' + preset.name + '" aplicado à mensagem');
  }

  function presetFormHtml(p) {
    p = p || {};
    return `
      <div class="wave1-field"><label>Nome *</label><input class="wave1-input" data-f="name" value="${esc(p.name)}" placeholder="Ex.: Revisor de código"></div>
      <div class="wave1-field"><label>Descrição</label><input class="wave1-input" data-f="description" value="${esc(p.description)}"></div>
      <div class="wave1-field"><label>System prompt</label><textarea class="wave1-textarea" data-f="system_prompt" rows="3">${esc(p.system_prompt)}</textarea></div>
      <div class="two">
        <div class="wave1-field"><label>Prefixo (antes da msg)</label><input class="wave1-input" data-f="prefix" value="${esc(p.prefix)}"></div>
        <div class="wave1-field"><label>Sufixo (após a msg)</label><input class="wave1-input" data-f="suffix" value="${esc(p.suffix)}"></div>
      </div>
      <div class="two">
        <div class="wave1-field"><label>Modelo preferido (opcional)</label><input class="wave1-input" data-f="model_id" value="${esc(p.model_id)}"></div>
        <div class="wave1-field"><label>Tags (vírgula)</label><input class="wave1-input" data-f="tags" value="${esc((p.tags||[]).join(', '))}"></div>
      </div>
      <div class="two">
        <div class="wave1-field"><label>Temperature</label><input class="wave1-input" type="number" step="0.05" min="0" max="2" data-f="temperature" value="${p.temperature != null ? p.temperature : ''}"></div>
        <div class="wave1-field"><label>Max tokens</label><input class="wave1-input" type="number" min="1" data-f="max_tokens" value="${p.max_tokens != null ? p.max_tokens : ''}"></div>
      </div>
      <div class="wave1-field"><label><input type="checkbox" data-f="favorite" ${p.favorite ? 'checked' : ''}> Favorito</label></div>
    `;
  }

  function collectPreset(scope, base) {
    const get = function (f) { const e = scope.querySelector(`[data-f="${f}"]`); return e ? e.value : ''; };
    const num = function (f) { const v = get(f).trim(); return v === '' ? null : Number(v); };
    const out = Object.assign({}, base || {});
    out.name = get('name').trim();
    out.description = get('description').trim();
    out.system_prompt = get('system_prompt');
    out.prefix = get('prefix');
    out.suffix = get('suffix');
    out.model_id = get('model_id').trim();
    out.tags = get('tags').split(',').map(function (t) { return t.trim(); }).filter(Boolean);
    out.temperature = num('temperature');
    out.max_tokens = num('max_tokens');
    const fav = scope.querySelector('[data-f="favorite"]');
    out.favorite = !!(fav && fav.checked);
    return out;
  }

  function openPresetManager() {
    openModal('Gerenciar presets', '<div class="manager-list"></div><div class="mfoot"><button class="wave1-btn" data-new>+ Novo preset</button></div>', function (body) {
      function refresh() {
        const list = body.querySelector('.manager-list');
        if (!presetCache.length) { list.innerHTML = '<div class="wave1-empty">Nenhum preset. Crie o primeiro!</div>'; return; }
        list.innerHTML = presetCache.map(function (p) {
          return `<div class="wave1-card"><div class="row"><div>
            <div class="title">${esc(p.name)}${p.favorite ? ' ★' : ''}</div>
            <div class="preview">${esc((p.description || p.system_prompt || '').slice(0, 120))}</div></div>
            <div class="wave1-actions">
              <button class="wave1-btn ghost sm" data-edit="${esc(p.id)}">Editar</button>
              <button class="wave1-btn danger sm" data-del="${esc(p.id)}">Excluir</button>
            </div></div></div>`;
        }).join('');
      }
      refresh();
      body.addEventListener('click', async function (e) {
        const editId = e.target.getAttribute && e.target.getAttribute('data-edit');
        const delId = e.target.getAttribute && e.target.getAttribute('data-del');
        if (e.target.hasAttribute && e.target.hasAttribute('data-new')) { openPresetEditor(null, refresh); }
        else if (editId) { openPresetEditor(presetCache.find(function (p) { return p.id === editId; }), refresh); }
        else if (delId) {
          if (!confirm('Excluir este preset?')) return;
          try { await api('/agent/presets/' + delId, { method: 'DELETE' }); await loadPresets(); refresh(); renderPresetMenu(); toast('Preset excluído'); }
          catch (err) { toast(err.message, 'error'); }
        }
      });
    });
  }

  function openPresetEditor(preset, after) {
    openModal(preset ? 'Editar preset' : 'Novo preset',
      presetFormHtml(preset) + '<div class="mfoot"><button class="wave1-btn ghost" data-cancel>Cancelar</button><button class="wave1-btn" data-save>Salvar</button></div>',
      function (body, close) {
        body.querySelector('[data-cancel]').addEventListener('click', close);
        body.querySelector('[data-save]').addEventListener('click', async function () {
          const payload = collectPreset(body, preset || {});
          if (!payload.name) { toast('Nome é obrigatório', 'error'); return; }
          try {
            await api('/agent/presets', { method: 'POST', body: JSON.stringify(payload) });
            await loadPresets(); renderPresetMenu(); close();
            if (after) after();
            toast('Preset salvo');
          } catch (err) { toast(err.message, 'error'); }
        });
      });
  }

  function initPresets() {
    const toggle = $('preset-toggle');
    const menu = $('preset-menu');
    if (!toggle || !menu) return;
    toggle.addEventListener('click', function (e) {
      e.stopPropagation();
      const hidden = menu.classList.contains('hidden');
      if (hidden) { renderPresetMenu(); menu.classList.remove('hidden'); }
      else menu.classList.add('hidden');
    });
    menu.addEventListener('click', function (e) {
      const applyId = e.target.closest && e.target.closest('[data-apply]');
      const manage = e.target.closest && e.target.closest('[data-manage]');
      if (manage) { menu.classList.add('hidden'); openPresetManager(); return; }
      if (applyId) {
        const p = presetCache.find(function (x) { return x.id === applyId.getAttribute('data-apply'); });
        if (p) applyPresetToInput(p);
        menu.classList.add('hidden');
      }
    });
    document.addEventListener('click', function (e) {
      if (!menu.classList.contains('hidden') && !e.target.closest('#preset-picker')) menu.classList.add('hidden');
    });
    loadPresets();
  }

  // ════════════════════════════════════════════════════════════════════════
  // MEMORY
  // ════════════════════════════════════════════════════════════════════════
  let memoryState = { query: '', pinnedOnly: false };

  function memoryRootHtml() {
    return `
      <div class="wave1-head">
        <div><h2>Memória</h2><div class="sub">Conhecimento persistente que o agente recupera ao longo do tempo (busca híbrida por palavra-chave).</div></div>
        <button class="wave1-btn" id="mem-add">+ Adicionar memória</button>
      </div>
      <div class="wave1-toolbar">
        <input class="wave1-input grow" id="mem-search" placeholder="Buscar na memória…" value="${esc(memoryState.query)}">
        <button class="wave1-btn ghost ${memoryState.pinnedOnly ? 'active' : ''}" id="mem-pinned">${memoryState.pinnedOnly ? '★ Fixadas' : '☆ Só fixadas'}</button>
        <button class="wave1-btn ghost" id="mem-refresh">Atualizar</button>
      </div>
      <div id="mem-list"></div>`;
  }

  function renderMemoryList(records) {
    const list = $('mem-list');
    if (!list) return;
    if (!records.length) { list.innerHTML = '<div class="wave1-empty">Nenhuma memória encontrada.</div>'; return; }
    list.innerHTML = records.map(function (r) {
      const pinned = (r.pin_state === 'pinned');
      const preview = (r.preview != null ? r.preview : r.content) || '';
      return `<div class="wave1-card"><div class="row"><div style="flex:1">
        <div class="title">${esc(r.title || '(sem título)')}</div>
        <div class="preview">${esc(preview.slice(0, 400))}</div>
        <div class="wave1-badges">
          ${r.kind ? `<span class="wave1-badge">${esc(r.kind)}</span>` : ''}
          ${r.scope ? `<span class="wave1-badge">${esc(r.scope)}</span>` : ''}
          ${(r.tags || []).map(function (t) { return `<span class="wave1-badge accent">${esc(t)}</span>`; }).join('')}
          ${r.created_at ? `<span class="wave1-badge">${esc(fmtDate(r.created_at))}</span>` : ''}
        </div></div>
        <div class="wave1-actions">
          <button class="wave1-btn ghost sm" data-pin="${esc(r.id)}">${pinned ? '★' : '☆'}</button>
          <button class="wave1-btn danger sm" data-del="${esc(r.id)}">Excluir</button>
        </div></div></div>`;
    }).join('');
  }

  async function refreshMemory() {
    const list = $('mem-list');
    if (list) list.innerHTML = '<div class="wave1-empty">Carregando…</div>';
    try {
      let records;
      if (memoryState.query.trim()) {
        records = await api('/agent/memory/search?q=' + encodeURIComponent(memoryState.query) + '&limit=50');
        if (memoryState.pinnedOnly) records = records.filter(function (r) { return r.pin_state === 'pinned'; });
      } else {
        records = await api('/agent/memory?limit=200' + (memoryState.pinnedOnly ? '&pinned=true' : ''));
      }
      renderMemoryList(records);
    } catch (err) {
      if (list) list.innerHTML = '<div class="wave1-empty">Erro: ' + esc(err.message) + '</div>';
    }
  }

  function openMemoryEditor() {
    openModal('Adicionar memória', `
      <div class="wave1-field"><label>Título</label><input class="wave1-input" data-f="title" placeholder="Ex.: Preferência de stack"></div>
      <div class="wave1-field"><label>Conteúdo *</label><textarea class="wave1-textarea" data-f="content" rows="4"></textarea></div>
      <div class="two">
        <div class="wave1-field"><label>Tipo</label><input class="wave1-input" data-f="kind" value="note"></div>
        <div class="wave1-field"><label>Tags (vírgula)</label><input class="wave1-input" data-f="tags"></div>
      </div>
      <div class="mfoot"><button class="wave1-btn ghost" data-cancel>Cancelar</button><button class="wave1-btn" data-save>Salvar</button></div>
    `, function (body, close) {
      body.querySelector('[data-cancel]').addEventListener('click', close);
      body.querySelector('[data-save]').addEventListener('click', async function () {
        const get = function (f) { const e = body.querySelector(`[data-f="${f}"]`); return e ? e.value : ''; };
        const payload = {
          title: get('title').trim(),
          content: get('content').trim(),
          kind: get('kind').trim() || 'note',
          tags: get('tags').split(',').map(function (t) { return t.trim(); }).filter(Boolean),
        };
        if (!payload.content) { toast('Conteúdo é obrigatório', 'error'); return; }
        try { await api('/agent/memory', { method: 'POST', body: JSON.stringify(payload) }); close(); refreshMemory(); toast('Memória adicionada'); }
        catch (err) { toast(err.message, 'error'); }
      });
    });
  }

  function initMemoryPanel() {
    const root = $('memoria-root');
    if (!root) return;
    root.innerHTML = memoryRootHtml();
    let searchTimer = null;
    $('mem-search').addEventListener('input', function (e) {
      memoryState.query = e.target.value;
      clearTimeout(searchTimer);
      searchTimer = setTimeout(refreshMemory, 250);
    });
    $('mem-pinned').addEventListener('click', function () { memoryState.pinnedOnly = !memoryState.pinnedOnly; initMemoryPanel(); });
    $('mem-refresh').addEventListener('click', refreshMemory);
    $('mem-add').addEventListener('click', openMemoryEditor);
    $('mem-list').addEventListener('click', async function (e) {
      const pin = e.target.getAttribute && e.target.getAttribute('data-pin');
      const del = e.target.getAttribute && e.target.getAttribute('data-del');
      if (pin) {
        const card = e.target.closest('.wave1-card');
        const isPinned = e.target.textContent.trim() === '★';
        try { await api('/agent/memory/' + pin + '/pin', { method: 'POST', body: JSON.stringify({ pinned: !isPinned }) }); refreshMemory(); }
        catch (err) { toast(err.message, 'error'); }
        void card;
      } else if (del) {
        if (!confirm('Excluir esta memória?')) return;
        try { await api('/agent/memory/' + del, { method: 'DELETE' }); refreshMemory(); toast('Memória excluída'); }
        catch (err) { toast(err.message, 'error'); }
      }
    });
    refreshMemory();
  }

  // ════════════════════════════════════════════════════════════════════════
  // COMPARE
  // ════════════════════════════════════════════════════════════════════════
  let availableModels = [];
  let currentComparison = null;
  let revealModels = false;

  async function loadModels() {
    try {
      const models = await api('/models');
      availableModels = Array.isArray(models) ? models : [];
    } catch (_) { availableModels = []; }
    return availableModels;
  }

  function compareRootHtml() {
    return `
      <div class="wave1-head">
        <div><h2>Comparar modelos</h2><div class="sub">Envie o mesmo prompt para vários modelos, compare lado a lado (cego) e vote no melhor.</div></div>
      </div>
      <textarea class="wave1-textarea" id="cmp-prompt" rows="3" placeholder="Prompt para comparar…"></textarea>
      <div class="wave1-field" style="margin-top:10px"><label>System prompt (opcional)</label><textarea class="wave1-textarea" id="cmp-system" rows="2"></textarea></div>
      <div class="wave1-label" style="margin-top:6px">Modelos (escolha ao menos 2)</div>
      <div class="wave1-checkboxes" id="cmp-models"></div>
      <div class="wave1-row-controls">
        <label class="wave1-chk"><input type="checkbox" id="cmp-blind" checked> Teste cego</label>
        <label class="wave1-label">Temp <input class="wave1-input" id="cmp-temp" type="number" step="0.05" min="0" max="2" value="0.7" style="width:70px"></label>
        <label class="wave1-label">Max tokens <input class="wave1-input" id="cmp-max" type="number" min="1" value="512" style="width:90px"></label>
        <button class="wave1-btn" id="cmp-run">Comparar</button>
        <button class="wave1-btn ghost" id="cmp-refresh-models">↻ Modelos</button>
      </div>
      <div id="cmp-result"></div>
      <div class="wave1-head" style="margin-top:24px;margin-bottom:8px"><div><h2 style="font-size:15px">Histórico</h2></div></div>
      <div id="cmp-history"></div>`;
  }

  function renderModelCheckboxes() {
    const box = $('cmp-models');
    if (!box) return;
    if (!availableModels.length) { box.innerHTML = '<span class="wave1-label">Nenhum modelo disponível. Baixe modelos na aba Modelos.</span>'; return; }
    box.innerHTML = availableModels.map(function (m) {
      const id = m.id || m.path || m.name;
      return `<label class="wave1-chk"><input type="checkbox" value="${esc(id)}"> ${esc(m.name || id)}</label>`;
    }).join('');
  }

  function renderComparison(cmp) {
    currentComparison = cmp;
    const result = $('cmp-result');
    if (!result) return;
    const blind = cmp.blind && !revealModels;
    const panes = cmp.entries.map(function (e) {
      const isWinner = cmp.winner_label && cmp.winner_label === e.label;
      const modelLine = blind ? '<span class="model">modelo oculto</span>' : `<span class="model">${esc(e.model_id)}${e.provider_id ? ' · ' + esc(e.provider_id) : ''}</span>`;
      const body = e.error
        ? `<span class="err">Erro: ${esc(e.error)}</span>`
        : esc(e.content || '(vazio)');
      return `<div class="wave1-pane ${isWinner ? 'winner' : ''}">
        <div class="phead"><span class="label">${esc(e.label)}</span>${modelLine}</div>
        <div class="pbody">${body}</div>
        <div class="pfoot">
          <button class="wave1-btn ghost sm" data-vote="${esc(e.label)}">${isWinner ? '✓ Melhor' : 'Votar melhor'}</button>
          <span class="model" style="margin-left:auto">${e.latency_ms ? (e.latency_ms + ' ms') : ''}</span>
        </div></div>`;
    }).join('');
    result.innerHTML = `<div class="wave1-grid">${panes}</div>
      <div class="wave1-row-controls">
        <button class="wave1-btn ghost sm" id="cmp-reveal">${cmp.blind ? (revealModels ? 'Ocultar modelos' : 'Revelar modelos') : 'Modelos visíveis'}</button>
        <button class="wave1-btn ghost sm" id="cmp-synth">Sintetizar (juiz IA)</button>
        <span class="wave1-label" style="margin-left:auto">${cmp.winner_label ? 'Voto: ' + esc(cmp.winner_label) : ''}</span>
      </div>
      ${cmp.synthesis ? `<div class="wave1-card" style="margin-top:12px"><div class="title">Síntese do juiz</div><div class="preview">${esc(cmp.synthesis)}</div></div>` : '<div id="cmp-synth-slot"></div>'}`;
  }

  async function refreshCompareHistory() {
    const box = $('cmp-history');
    if (!box) return;
    try {
      const items = await api('/compare/history?limit=50');
      if (!items.length) { box.innerHTML = '<div class="wave1-empty">Nenhuma comparação ainda.</div>'; return; }
      box.innerHTML = items.map(function (c) {
        return `<div class="wave1-card"><div class="row"><div style="flex:1">
          <div class="title">${esc((c.prompt || '').slice(0, 90))}</div>
          <div class="wave1-badges">
            <span class="wave1-badge">${c.entries.length} modelos</span>
            ${c.blind ? '<span class="wave1-badge">cego</span>' : ''}
            ${c.winner_label ? `<span class="wave1-badge accent">voto: ${esc(c.winner_label)}</span>` : ''}
            <span class="wave1-badge">${esc(fmtDate(c.created_at))}</span>
          </div></div>
          <div class="wave1-actions">
            <button class="wave1-btn ghost sm" data-open="${esc(c.id)}">Abrir</button>
            <button class="wave1-btn danger sm" data-del="${esc(c.id)}">Excluir</button>
          </div></div></div>`;
      }).join('');
    } catch (err) { box.innerHTML = '<div class="wave1-empty">Erro: ' + esc(err.message) + '</div>'; }
  }

  function initComparePanel() {
    const root = $('comparar-root');
    if (!root) return;
    root.innerHTML = compareRootHtml();
    renderModelCheckboxes();
    if (!availableModels.length) loadModels().then(renderModelCheckboxes);

    $('cmp-refresh-models').addEventListener('click', function () { loadModels().then(renderModelCheckboxes); });
    $('cmp-run').addEventListener('click', async function () {
      const prompt = $('cmp-prompt').value.trim();
      const selected = Array.prototype.slice.call($('cmp-models').querySelectorAll('input:checked')).map(function (i) { return i.value; });
      if (!prompt) { toast('Escreva um prompt', 'error'); return; }
      if (selected.length < 2) { toast('Escolha pelo menos 2 modelos', 'error'); return; }
      const btn = $('cmp-run'); btn.disabled = true; btn.textContent = 'Comparando…';
      $('cmp-result').innerHTML = '<div class="wave1-empty">Gerando respostas…</div>';
      revealModels = false;
      try {
        const cmp = await api('/compare/run', { method: 'POST', body: JSON.stringify({
          prompt: prompt,
          system_prompt: $('cmp-system').value,
          models: selected,
          blind: $('cmp-blind').checked,
          temperature: Number($('cmp-temp').value) || null,
          max_tokens: Number($('cmp-max').value) || null,
        }) });
        renderComparison(cmp);
        refreshCompareHistory();
      } catch (err) { $('cmp-result').innerHTML = '<div class="wave1-empty">Erro: ' + esc(err.message) + '</div>'; }
      finally { btn.disabled = false; btn.textContent = 'Comparar'; }
    });

    $('cmp-result').addEventListener('click', async function (e) {
      const vote = e.target.getAttribute && e.target.getAttribute('data-vote');
      if (vote && currentComparison) {
        try { await api('/compare/' + currentComparison.id + '/vote', { method: 'POST', body: JSON.stringify({ winner_label: vote }) });
          currentComparison.winner_label = vote; renderComparison(currentComparison); refreshCompareHistory(); }
        catch (err) { toast(err.message, 'error'); }
      } else if (e.target.id === 'cmp-reveal' && currentComparison) {
        revealModels = !revealModels; renderComparison(currentComparison);
      } else if (e.target.id === 'cmp-synth' && currentComparison) {
        const judge = availableModels[0] && (availableModels[0].id || availableModels[0].path);
        if (!judge) { toast('Nenhum modelo para sintetizar', 'error'); return; }
        e.target.disabled = true; e.target.textContent = 'Sintetizando…';
        try { const updated = await api('/compare/' + currentComparison.id + '/synthesize', { method: 'POST', body: JSON.stringify({ model_id: judge }) });
          renderComparison(updated); }
        catch (err) { toast(err.message, 'error'); e.target.disabled = false; e.target.textContent = 'Sintetizar (juiz IA)'; }
      }
    });

    $('cmp-history').addEventListener('click', async function (e) {
      const open = e.target.getAttribute && e.target.getAttribute('data-open');
      const del = e.target.getAttribute && e.target.getAttribute('data-del');
      if (open) {
        try { const cmp = await api('/compare/' + open); revealModels = false; renderComparison(cmp); window.scrollTo && root.scrollTo(0, 0); }
        catch (err) { toast(err.message, 'error'); }
      } else if (del) {
        if (!confirm('Excluir esta comparação?')) return;
        try { await api('/compare/' + del, { method: 'DELETE' }); refreshCompareHistory(); } catch (err) { toast(err.message, 'error'); }
      }
    });

    refreshCompareHistory();
  }

  // ════════════════════════════════════════════════════════════════════════
  // HISTORY
  // ════════════════════════════════════════════════════════════════════════
  let sessionsCache = [];
  let historyState = { query: '', showArchived: false, selectedId: null };

  function historyRootHtml() {
    return `
      <div class="wave1-head">
        <div><h2>Histórico de conversas</h2><div class="sub">Organize, busque, ramifique (fork), edite e exporte suas sessões.</div></div>
      </div>
      <div class="wave1-toolbar">
        <input class="wave1-input grow" id="hist-search" placeholder="Buscar por nome…" value="${esc(historyState.query)}">
        <button class="wave1-btn ghost ${historyState.showArchived ? 'active' : ''}" id="hist-archived">${historyState.showArchived ? 'Ocultar arquivadas' : 'Mostrar arquivadas'}</button>
        <button class="wave1-btn ghost" id="hist-refresh">Atualizar</button>
      </div>
      <div class="wave1-split">
        <div class="wave1-list-pane" id="hist-list"></div>
        <div class="wave1-detail-pane" id="hist-detail"><div class="wave1-empty">Selecione uma conversa para ver o transcript.</div></div>
      </div>`;
  }

  function renderSessionList() {
    const list = $('hist-list');
    if (!list) return;
    let items = sessionsCache.slice();
    if (!historyState.showArchived) items = items.filter(function (s) { return !s.archived; });
    const q = historyState.query.trim().toLowerCase();
    if (q) items = items.filter(function (s) { return (s.name || '').toLowerCase().indexOf(q) >= 0 || (s.summary || '').toLowerCase().indexOf(q) >= 0; });
    items.sort(function (a, b) {
      if (!!b.pinned !== !!a.pinned) return b.pinned ? 1 : -1;
      return new Date(b.updated_at) - new Date(a.updated_at);
    });
    if (!items.length) { list.innerHTML = '<div class="wave1-empty">Nenhuma conversa.</div>'; return; }
    list.innerHTML = items.map(function (s) {
      return `<div class="wave1-card" style="cursor:pointer" data-open="${esc(s.id)}"><div class="row"><div style="flex:1">
        <div class="title">${s.pinned ? '📌 ' : ''}${esc(s.name || 'Sem título')}</div>
        <div class="wave1-badges">
          ${s.model_id ? `<span class="wave1-badge">${esc(s.model_id)}</span>` : ''}
          <span class="wave1-badge">${s.message_count} msgs</span>
          ${s.folder ? `<span class="wave1-badge accent">${esc(s.folder)}</span>` : ''}
          ${s.archived ? '<span class="wave1-badge">arquivada</span>' : ''}
          <span class="wave1-badge">${esc(fmtDate(s.updated_at))}</span>
        </div></div></div>
        <div class="wave1-actions" style="margin-top:8px">
          <button class="wave1-btn ghost sm" data-pin="${esc(s.id)}">${s.pinned ? 'Desafixar' : 'Fixar'}</button>
          <button class="wave1-btn ghost sm" data-folder="${esc(s.id)}">Pasta</button>
          <button class="wave1-btn ghost sm" data-fork="${esc(s.id)}">Fork</button>
          <button class="wave1-btn ghost sm" data-archive="${esc(s.id)}">${s.archived ? 'Desarquivar' : 'Arquivar'}</button>
          <button class="wave1-btn ghost sm" data-export="${esc(s.id)}">Exportar ▾</button>
          <button class="wave1-btn danger sm" data-del="${esc(s.id)}">Excluir</button>
        </div></div>`;
    }).join('');
  }

  async function refreshSessions() {
    const list = $('hist-list');
    if (list) list.innerHTML = '<div class="wave1-empty">Carregando…</div>';
    try { sessionsCache = await api('/agent/sessions'); renderSessionList(); }
    catch (err) { if (list) list.innerHTML = '<div class="wave1-empty">Erro: ' + esc(err.message) + '</div>'; }
  }

  async function openTranscript(id) {
    historyState.selectedId = id;
    const detail = $('hist-detail');
    detail.innerHTML = '<div class="wave1-empty">Carregando transcript…</div>';
    try {
      const msgs = await api('/agent/sessions/' + id + '/messages');
      if (!msgs.length) { detail.innerHTML = '<div class="wave1-empty">Conversa vazia.</div>'; return; }
      detail.innerHTML = msgs.filter(function (m) { return (m.content || '').trim() || m.kind === 'tool_call'; }).map(function (m) {
        return `<div class="wave1-msg" data-eid="${m.event_id}">
          <div class="mrole"><span>${esc(m.role)}${m.kind && m.kind !== m.role ? ' · ' + esc(m.kind) : ''}</span>
          <span class="wave1-actions">
            <button class="wave1-btn ghost sm" data-edit="${m.event_id}">Editar</button>
            <button class="wave1-btn ghost sm" data-trunc="${m.event_id}">Truncar daqui</button>
            <button class="wave1-btn danger sm" data-delmsg="${m.event_id}">×</button>
          </span></div>
          <div class="mcontent">${esc(m.content)}</div></div>`;
      }).join('');
    } catch (err) { detail.innerHTML = '<div class="wave1-empty">Erro: ' + esc(err.message) + '</div>'; }
  }

  function downloadExport(id, format) {
    const url = daemonUrl() + '/agent/sessions/' + id + '/download?format=' + format;
    fetch(url).then(function (r) { return r.blob().then(function (b) { return { b: b, r: r }; }); }).then(function (o) {
      const cd = o.r.headers.get('content-disposition') || '';
      const m = cd.match(/filename="?([^"]+)"?/);
      const a = document.createElement('a');
      a.href = URL.createObjectURL(o.b);
      a.download = m ? m[1] : ('session.' + format);
      document.body.appendChild(a); a.click(); a.remove();
      setTimeout(function () { URL.revokeObjectURL(a.href); }, 1000);
    }).catch(function (err) { toast(err.message, 'error'); });
  }

  function initHistoryPanel() {
    const root = $('historico-root');
    if (!root) return;
    root.innerHTML = historyRootHtml();
    let timer = null;
    $('hist-search').addEventListener('input', function (e) { historyState.query = e.target.value; clearTimeout(timer); timer = setTimeout(renderSessionList, 200); });
    $('hist-archived').addEventListener('click', function () { historyState.showArchived = !historyState.showArchived; initHistoryPanel(); });
    $('hist-refresh').addEventListener('click', refreshSessions);

    $('hist-list').addEventListener('click', async function (e) {
      const t = e.target;
      const attr = function (n) { return t.getAttribute && t.getAttribute(n); };
      const id = attr('data-pin') || attr('data-folder') || attr('data-fork') || attr('data-archive') || attr('data-export') || attr('data-del');
      try {
        if (attr('data-pin')) { const s = sessionsCache.find(function (x) { return x.id === id; }); await api('/agent/sessions/' + id + '/flags', { method: 'POST', body: JSON.stringify({ pinned: !s.pinned }) }); await refreshSessions(); }
        else if (attr('data-archive')) { const s = sessionsCache.find(function (x) { return x.id === id; }); await api('/agent/sessions/' + id + '/flags', { method: 'POST', body: JSON.stringify({ archived: !s.archived }) }); await refreshSessions(); }
        else if (attr('data-folder')) { const f = prompt('Nome da pasta (vazio = remover):', ''); if (f !== null) { await api('/agent/sessions/' + id + '/flags', { method: 'POST', body: JSON.stringify({ folder: f }) }); await refreshSessions(); } }
        else if (attr('data-fork')) { const out = await api('/agent/sessions/' + id + '/fork', { method: 'POST', body: JSON.stringify({}) }); toast('Fork criado'); await refreshSessions(); if (out.new_session_id) openTranscript(out.new_session_id); }
        else if (attr('data-export')) { const fmt = prompt('Formato: md, json, txt ou html', 'md'); if (fmt) downloadExport(id, fmt.trim()); }
        else if (attr('data-del')) { if (confirm('Excluir esta conversa?')) { await api('/agent/sessions/' + id, { method: 'DELETE' }); if (historyState.selectedId === id) $('hist-detail').innerHTML = '<div class="wave1-empty">Selecione uma conversa.</div>'; await refreshSessions(); } }
        else { const open = attr('data-open') || (t.closest('[data-open]') && t.closest('[data-open]').getAttribute('data-open')); if (open) openTranscript(open); }
      } catch (err) { toast(err.message, 'error'); }
    });

    $('hist-detail').addEventListener('click', async function (e) {
      const t = e.target;
      const eid = (t.getAttribute && (t.getAttribute('data-edit') || t.getAttribute('data-trunc') || t.getAttribute('data-delmsg')));
      const sid = historyState.selectedId;
      if (!eid || !sid) return;
      try {
        if (t.getAttribute('data-edit')) {
          const msgEl = t.closest('.wave1-msg').querySelector('.mcontent');
          const current = msgEl.textContent;
          const next = prompt('Editar mensagem:', current);
          if (next !== null) { await api('/agent/sessions/' + sid + '/messages/' + eid, { method: 'PATCH', body: JSON.stringify({ content: next }) }); openTranscript(sid); }
        } else if (t.getAttribute('data-trunc')) {
          if (confirm('Remover todas as mensagens APÓS esta?')) { await api('/agent/sessions/' + sid + '/truncate', { method: 'POST', body: JSON.stringify({ event_id: Number(eid) }) }); openTranscript(sid); refreshSessions(); }
        } else if (t.getAttribute('data-delmsg')) {
          if (confirm('Excluir esta mensagem?')) { await api('/agent/sessions/' + sid + '/messages/' + eid, { method: 'DELETE' }); openTranscript(sid); refreshSessions(); }
        }
      } catch (err) { toast(err.message, 'error'); }
    });

    refreshSessions();
  }

  // ── wiring ───────────────────────────────────────────────────────────────
  function bindTab(panel, init) {
    const btn = document.querySelector('[data-panel="' + panel + '"]');
    if (btn) btn.addEventListener('click', init);
  }

  function boot() {
    injectStyles();
    // Presets migrated to js/features/presets.js (wired in main.js).
    bindTab('memoria', initMemoryPanel);
    bindTab('comparar', initComparePanel);
    bindTab('historico', initHistoryPanel);
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', boot);
  } else {
    boot();
  }
})();
