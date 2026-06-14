/* MLX Pilot — Conversation history (feature).
 *
 * The "Histórico" tab: list/search/sort sessions, pin/folder/fork/archive,
 * delete, browse a transcript and edit/truncate/delete individual messages,
 * plus multi-format export download. Reuses core api()/state and the shared
 * wave-common helpers. Migrated out of the former wave1.js monolith.
 */

import { api } from '../core/api.js';
import { state } from '../core/state.js';
import { esc, el, fmtDate, toast, injectWave1Styles } from './wave-common.js';

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
  const list = el('hist-list');
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
  const list = el('hist-list');
  if (list) list.innerHTML = '<div class="wave1-empty">Carregando…</div>';
  try { sessionsCache = await api('/agent/sessions'); renderSessionList(); }
  catch (err) { if (list) list.innerHTML = '<div class="wave1-empty">Erro: ' + esc(err.message) + '</div>'; }
}

async function openTranscript(id) {
  historyState.selectedId = id;
  const detail = el('hist-detail');
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
  const url = state.daemonUrl + '/agent/sessions/' + id + '/download?format=' + format;
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
  const root = el('historico-root');
  if (!root) return;
  root.innerHTML = historyRootHtml();
  let timer = null;
  el('hist-search').addEventListener('input', function (e) { historyState.query = e.target.value; clearTimeout(timer); timer = setTimeout(renderSessionList, 200); });
  el('hist-archived').addEventListener('click', function () { historyState.showArchived = !historyState.showArchived; initHistoryPanel(); });
  el('hist-refresh').addEventListener('click', refreshSessions);

  el('hist-list').addEventListener('click', async function (e) {
    const t = e.target;
    const attr = function (n) { return t.getAttribute && t.getAttribute(n); };
    const id = attr('data-pin') || attr('data-folder') || attr('data-fork') || attr('data-archive') || attr('data-export') || attr('data-del');
    try {
      if (attr('data-pin')) { const s = sessionsCache.find(function (x) { return x.id === id; }); await api('/agent/sessions/' + id + '/flags', { method: 'POST', body: JSON.stringify({ pinned: !s.pinned }) }); await refreshSessions(); }
      else if (attr('data-archive')) { const s = sessionsCache.find(function (x) { return x.id === id; }); await api('/agent/sessions/' + id + '/flags', { method: 'POST', body: JSON.stringify({ archived: !s.archived }) }); await refreshSessions(); }
      else if (attr('data-folder')) { const f = prompt('Nome da pasta (vazio = remover):', ''); if (f !== null) { await api('/agent/sessions/' + id + '/flags', { method: 'POST', body: JSON.stringify({ folder: f }) }); await refreshSessions(); } }
      else if (attr('data-fork')) { const out = await api('/agent/sessions/' + id + '/fork', { method: 'POST', body: JSON.stringify({}) }); toast('Fork criado'); await refreshSessions(); if (out.new_session_id) openTranscript(out.new_session_id); }
      else if (attr('data-export')) { const fmt = prompt('Formato: md, json, txt ou html', 'md'); if (fmt) downloadExport(id, fmt.trim()); }
      else if (attr('data-del')) { if (confirm('Excluir esta conversa?')) { await api('/agent/sessions/' + id, { method: 'DELETE' }); if (historyState.selectedId === id) el('hist-detail').innerHTML = '<div class="wave1-empty">Selecione uma conversa.</div>'; await refreshSessions(); } }
      else { const open = attr('data-open') || (t.closest('[data-open]') && t.closest('[data-open]').getAttribute('data-open')); if (open) openTranscript(open); }
    } catch (err) { toast(err.message, 'error'); }
  });

  el('hist-detail').addEventListener('click', async function (e) {
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

injectWave1Styles();
const historicoTab = document.querySelector('[data-panel="historico"]');
if (historicoTab) historicoTab.addEventListener('click', initHistoryPanel);
