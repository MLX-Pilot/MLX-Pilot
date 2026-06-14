/* MLX Pilot — Memory (feature).
 *
 * The "Memória" tab: keyword/pinned search over the daemon's /agent/memory
 * store, plus add/pin/delete. Reuses core api() and the shared wave-common
 * helpers (esc, el, fmtDate, toast, modal, injectWave1Styles). Migrated out of
 * the former wave1.js monolith.
 */

import { api } from '../core/api.js';
import { esc, el, fmtDate, toast, openModal, injectWave1Styles } from './wave-common.js';

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
  const list = el('mem-list');
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
  const list = el('mem-list');
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
  const root = el('memoria-root');
  if (!root) return;
  root.innerHTML = memoryRootHtml();
  let searchTimer = null;
  el('mem-search').addEventListener('input', function (e) {
    memoryState.query = e.target.value;
    clearTimeout(searchTimer);
    searchTimer = setTimeout(refreshMemory, 250);
  });
  el('mem-pinned').addEventListener('click', function () { memoryState.pinnedOnly = !memoryState.pinnedOnly; initMemoryPanel(); });
  el('mem-refresh').addEventListener('click', refreshMemory);
  el('mem-add').addEventListener('click', openMemoryEditor);
  el('mem-list').addEventListener('click', async function (e) {
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

injectWave1Styles();
const memoriaTab = document.querySelector('[data-panel="memoria"]');
if (memoriaTab) memoriaTab.addEventListener('click', initMemoryPanel);
