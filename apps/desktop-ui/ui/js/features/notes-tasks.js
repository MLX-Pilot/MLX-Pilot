/* MLX Pilot — Notes & Tasks feature module.
 *
 * Tab "Notas & Tarefas": sticky notes (colour, pin, checklist, due date)
 * and scheduled tasks (once / interval / cron) with run-now, pause/resume,
 * history, and toast-SSE listener. No native dialogs — everything uses
 * custom overlays.
 *
 * Reuses daemon endpoints:
 *   GET/POST        /api/notes
 *   PUT/DELETE       /api/notes/:id
 *   POST             /api/tasks/:id/pause
 *   POST             /api/tasks/:id/resume
 *   POST             /api/tasks/:id/run-now
 *   GET              /api/tasks/:id/history
 *   GET              /api/toast/stream         (SSE)
 *   POST             /api/webhook/send
 *   GET/POST         /scheduler/tasks
 *   DELETE           /scheduler/tasks/:id
 *   GET              /scheduler/tasks/:id/runs
 */

import { api } from '../core/api.js';
import { state } from '../core/state.js';
import {
  esc, el, fmtDate, toast, openModal, injectWave1Styles, injectWave5Styles
} from './wave-common.js';

// ── State ──────────────────────────────────────────────────────────
let notes = [];
let tasks = [];
let activeTab = 'notes'; // 'notes' | 'tasks'
let toastEventSource = null;

// ── Colour palette for notes ───────────────────────────────────────
const NOTE_COLORS = [
  { label: 'Default', value: null },
  { label: 'Red', value: '#ff6b6b' },
  { label: 'Orange', value: '#ffa94d' },
  { label: 'Yellow', value: '#ffd43b' },
  { label: 'Green', value: '#69db7c' },
  { label: 'Cyan', value: '#39d0d8' },
  { label: 'Blue', value: '#74c0fc' },
  { label: 'Purple', value: '#b197fc' },
  { label: 'Pink', value: '#f783ac' },
];

// ── Schedule kind labels ───────────────────────────────────────────
const SCHEDULE_LABELS = {
  once: 'Uma vez',
  interval: 'Intervalo',
  cron: 'Cron',
};

// ── CSS injection ──────────────────────────────────────────────────
function injectStyles() {
  if (el('notes-tasks-styles')) return;
  const css = document.createElement('style');
  css.id = 'notes-tasks-styles';
  css.textContent = `
    .nt-root { height:100%; overflow:auto; padding:20px 24px; color:var(--text-primary,#e9e9f2); }
    .nt-head { display:flex; align-items:center; justify-content:space-between; gap:12px; margin-bottom:16px; flex-wrap:wrap; }
    .nt-head h2 { font-family:var(--font-heading,inherit); font-size:18px; font-weight:600; margin:0; }

    /* tabs */
    .nt-tabs { display:flex; gap:4px; margin-bottom:16px; border-bottom:1px solid var(--border,#2a2a44); }
    .nt-tab { background:none; border:none; color:var(--text-tertiary,#8a8aa0); padding:8px 16px; font-size:13px; cursor:pointer; border-bottom:2px solid transparent; font-family:inherit; }
    .nt-tab.active { color:var(--cyan,#39d0d8); border-bottom-color:var(--cyan,#39d0d8); }
    .nt-tab:hover { color:var(--text-primary,#e9e9f2); }

    /* notes grid */
    .nt-notes-grid { display:grid; grid-template-columns:repeat(auto-fill,minmax(260px,1fr)); gap:12px; }
    .nt-note-card { background:var(--bg-elevated,#16162a); border:1px solid var(--border,#2a2a44); border-radius:10px; padding:14px; position:relative; cursor:pointer; transition:border-color .2s; }
    .nt-note-card:hover { border-color:var(--cyan,#39d0d8); }
    .nt-note-card.pinned { border-color:var(--amber,#f0a040); }
    .nt-note-card .pin-icon { position:absolute; top:8px; right:8px; font-size:14px; opacity:.5; }
    .nt-note-card.pinned .pin-icon { opacity:1; color:var(--amber,#f0a040); }
    .nt-note-card .nt-title { font-weight:600; font-size:14px; margin-bottom:4px; padding-right:20px; }
    .nt-note-card .nt-content { font-size:12px; color:var(--text-secondary,#b9b9cc); white-space:pre-wrap; word-break:break-word; line-height:1.45; max-height:80px; overflow:hidden; }
    .nt-note-card .nt-meta { display:flex; gap:8px; margin-top:8px; font-size:11px; color:var(--text-tertiary,#8a8aa0); align-items:center; flex-wrap:wrap; }
    .nt-note-card .nt-due { color:var(--amber,#f0a040); }
    .nt-note-card .nt-due.overdue { color:#ff7a8a; }
    .nt-note-card .nt-checklist-preview { font-size:11px; color:var(--text-tertiary,#8a8aa0); margin-top:6px; }

    /* task list */
    .nt-task-card { background:var(--bg-elevated,#16162a); border:1px solid var(--border,#2a2a44); border-radius:10px; padding:14px; margin-bottom:10px; }
    .nt-task-card .ttop { display:flex; justify-content:space-between; align-items:flex-start; gap:8px; }
    .nt-task-card .tname { font-weight:600; font-size:14px; }
    .nt-task-card .tkind { font-size:11px; padding:1px 6px; border-radius:4px; background:var(--bg-deep,#0c0c18); border:1px solid var(--border,#2a2a44); }
    .nt-task-card .tinfo { font-size:12px; color:var(--text-secondary,#b9b9cc); margin-top:4px; }
    .nt-task-card .tactions { display:flex; gap:6px; margin-top:10px; flex-wrap:wrap; }
    .nt-task-card.paused { opacity:.6; }
    .nt-runs { margin-top:10px; }
    .nt-run-row { display:flex; gap:10px; align-items:center; padding:4px 0; font-size:12px; border-bottom:1px solid var(--border,#2a2a44); }
    .nt-run-row .status { font-size:10px; padding:1px 5px; border-radius:3px; }
    .nt-run-row .status.success { background:#1a3a1a; color:#69db7c; }
    .nt-run-row .status.error { background:#3a1a1a; color:#ff7a8a; }
    .nt-run-row .status.running { background:#1a2a3a; color:#39d0d8; }

    /* toolbar */
    .nt-toolbar { display:flex; gap:8px; align-items:center; flex-wrap:wrap; margin-bottom:14px; }
    .nt-empty { color:var(--text-tertiary,#8a8aa0); font-size:13px; text-align:center; padding:36px 12px; }

    /* form fields */
    .nt-field { margin-bottom:10px; }
    .nt-field label { font-size:12px; color:var(--text-tertiary,#8a8aa0); display:block; margin-bottom:4px; }
    .nt-field input, .nt-field select, .nt-field textarea { width:100%; box-sizing:border-box; background:var(--bg-elevated,#16162a); border:1px solid var(--border,#2a2a44); color:var(--text-primary,#e9e9f2); border-radius:8px; padding:8px 10px; font-size:13px; font-family:inherit; outline:none; }
    .nt-field input:focus, .nt-field textarea:focus, .nt-field select:focus { border-color:var(--cyan,#39d0d8); }
    .nt-field textarea { resize:vertical; min-height:64px; line-height:1.5; }
    .nt-row { display:flex; gap:10px; }
    .nt-row .nt-field { flex:1; }
    .nt-color-row { display:flex; gap:6px; flex-wrap:wrap; }
    .nt-color-chip { width:28px; height:28px; border-radius:6px; border:2px solid transparent; cursor:pointer; }
    .nt-color-chip:hover { border-color:var(--text-secondary,#b9b9cc); }
    .nt-color-chip.selected { border-color:var(--cyan,#39d0d8); box-shadow:0 0 0 1px var(--cyan,#39d0d8); }
    .nt-checklist-item { display:flex; align-items:center; gap:8px; margin-bottom:4px; }
    .nt-checklist-item input[type=checkbox] { accent-color:var(--cyan,#39d0d8); }
    .nt-checklist-item input[type=text] { flex:1; background:var(--bg-deep,#0c0c18); border:1px solid var(--border,#2a2a44); color:var(--text-primary,#e9e9f2); border-radius:6px; padding:4px 8px; font-size:12px; font-family:inherit; outline:none; }
    .nt-checklist-item button { background:none; border:none; color:var(--text-tertiary,#8a8aa0); cursor:pointer; font-size:14px; padding:2px 4px; }
    .nt-checklist-item button:hover { color:#ff7a8a; }

    /* toast area */
    .nt-toast-area { position:fixed; bottom:24px; right:24px; z-index:7000; display:flex; flex-direction:column-reverse; gap:8px; max-width:360px; }
    .nt-toast-item { background:var(--bg-elevated,#16162a); border:1px solid var(--border,#2a2a44); border-radius:10px; padding:10px 14px; font-size:12px; animation:ntSlideUp .3s ease; }
    .nt-toast-item.info { border-left:3px solid var(--cyan,#39d0d8); }
    .nt-toast-item.success { border-left:3px solid #69db7c; }
    .nt-toast-item.error { border-left:3px solid #ff7a8a; }
    .nt-toast-item.warning { border-left:3px solid var(--amber,#f0a040); }
    .nt-toast-item .ttl { font-weight:600; margin-bottom:2px; }
    .nt-toast-item .msg { color:var(--text-secondary,#b9b9cc); }
    @keyframes ntSlideUp { from { opacity:0; transform:translateY(10px); } to { opacity:1; transform:translateY(0); } }
  `;
  document.head.appendChild(css);
}

// ── Render ─────────────────────────────────────────────────────────
export function renderNotesTasks(root) {
  injectStyles();
  injectWave1Styles();
  root.innerHTML = `
    <div class="nt-root">
      <div class="nt-head">
        <h2>Notas & Tarefas</h2>
        <div style="display:flex;gap:8px;">
          <button class="wave1-btn sm" id="nt-btn-new-note">+ Nota</button>
          <button class="wave1-btn sm" id="nt-btn-new-task">+ Tarefa</button>
        </div>
      </div>
      <div class="nt-tabs">
        <button class="nt-tab active" data-tab="notes">Notas</button>
        <button class="nt-tab" data-tab="tasks">Tarefas Agendadas</button>
      </div>
      <div id="nt-content"></div>
      <div class="nt-toast-area" id="nt-toast-area"></div>
    </div>
  `;

  // Tab switching
  root.querySelectorAll('.nt-tab').forEach(function (btn) {
    btn.addEventListener('click', function () {
      activeTab = btn.dataset.tab;
      root.querySelectorAll('.nt-tab').forEach(function (b) { b.classList.remove('active'); });
      btn.classList.add('active');
      refresh();
    });
  });

  root.querySelector('#nt-btn-new-note').onclick = function () { showNoteEditor(null); };
  root.querySelector('#nt-btn-new-task').onclick = function () { showTaskEditor(null); };

  refresh();
  connectToastSSE();
}

// ── Refresh ────────────────────────────────────────────────────────
async function refresh() {
  if (activeTab === 'notes') {
    await loadNotes();
    renderNotes();
  } else {
    await loadTasks();
    renderTasks();
  }
}

// ═══════════════════════════════════════════════════════════════════
// Notes
// ═══════════════════════════════════════════════════════════════════

async function loadNotes() {
  try {
    notes = await api('/api/notes');
  } catch (e) { notes = []; }
}

function renderNotes() {
  var box = el('nt-content');
  if (!box) return;
  if (!notes.length) {
    box.innerHTML = '<div class="nt-empty">Nenhuma nota ainda. Clique em "+ Nota".</div>';
    return;
  }
  var html = '<div class="nt-notes-grid">';
  notes.forEach(function (n) {
    var colorStyle = n.color ? 'border-left:3px solid ' + esc(n.color) : '';
    var pinnedClass = n.pinned ? ' pinned' : '';
    var isOverdue = n.due_date && new Date(n.due_date) < new Date() && !n.checklist.every(function (i) { return i.done; });
    var doneCount = n.checklist.filter(function (i) { return i.done; }).length;
    var totalCount = n.checklist.length;
    html += '<div class="nt-note-card' + pinnedClass + '" style="' + colorStyle + '" data-id="' + esc(n.id) + '">';
    html += '<span class="pin-icon">' + (n.pinned ? '📌' : '📌') + '</span>';
    html += '<div class="nt-title">' + esc(n.title || 'Sem título') + '</div>';
    if (n.content) html += '<div class="nt-content">' + esc(n.content) + '</div>';
    html += '<div class="nt-meta">';
    if (n.due_date) html += '<span class="nt-due' + (isOverdue ? ' overdue' : '') + '">📅 ' + esc(n.due_date) + '</span>';
    if (totalCount) html += '<span>☑ ' + doneCount + '/' + totalCount + '</span>';
    html += '</div>';
    html += '</div>';
  });
  html += '</div>';
  box.innerHTML = html;

  // Click to edit
  box.querySelectorAll('.nt-note-card').forEach(function (card) {
    card.addEventListener('click', function () {
      var id = card.dataset.id;
      var note = notes.find(function (n) { return n.id === id; });
      if (note) showNoteEditor(note);
    });
  });
}

function showNoteEditor(note) {
  var isNew = !note;
  var title = note ? note.title : '';
  var content = note ? note.content : '';
  var color = note ? note.color : null;
  var pinned = note ? note.pinned : false;
  var dueDate = note ? note.due_date || '' : '';
  var checklist = note ? (note.checklist || []) : [];

  var colorChips = NOTE_COLORS.map(function (c) {
    var sel = c.value === color ? ' selected' : '';
    var bg = c.value || '#3a3a55';
    return '<div class="nt-color-chip' + sel + '" data-color="' + (c.value || '') + '" style="background:' + bg + '" title="' + esc(c.label) + '"></div>';
  }).join('');

  var checklistHtml = checklist.map(function (item, i) {
    return '<div class="nt-checklist-item">' +
      '<input type="checkbox" ' + (item.done ? 'checked' : '') + ' data-idx="' + i + '">' +
      '<input type="text" value="' + esc(item.text) + '" data-idx="' + i + '" placeholder="Item...">' +
      '<button data-del="' + i + '">✕</button>' +
      '</div>';
  }).join('');

  var body = `
    <div class="nt-field"><label>Título</label><input id="nte-title" value="${esc(title)}"></div>
    <div class="nt-field"><label>Conteúdo</label><textarea id="nte-content" rows="3">${esc(content)}</textarea></div>
    <div class="nt-field"><label>Cor</label><div class="nt-color-row" id="nte-colors">${colorChips}</div></div>
    <div class="nt-row">
      <div class="nt-field"><label>Vencimento</label><input type="date" id="nte-due" value="${esc(dueDate)}"></div>
      <div class="nt-field"><label style="visibility:hidden">.</label>
        <label class="wave1-chk"><input type="checkbox" id="nte-pin" ${pinned ? 'checked' : ''}> Fixar</label>
      </div>
    </div>
    <div class="nt-field">
      <label>Checklist <button class="wave1-btn sm ghost" id="nte-add-item">+ item</button></label>
      <div id="nte-checklist">${checklistHtml}</div>
    </div>
  `;

  openModal(isNew ? 'Nova Nota' : 'Editar Nota', body, function (mbody, close) {
    var selColor = color;
    mbody.querySelector('#nte-colors').addEventListener('click', function (e) {
      var chip = e.target.closest('.nt-color-chip');
      if (!chip) return;
      selColor = chip.dataset.color || null;
      mbody.querySelectorAll('.nt-color-chip').forEach(function (c) { c.classList.remove('selected'); });
      chip.classList.add('selected');
    });

    mbody.querySelector('#nte-add-item').onclick = function () {
      var div = document.createElement('div');
      div.className = 'nt-checklist-item';
      var idx = mbody.querySelectorAll('.nt-checklist-item').length;
      div.innerHTML = '<input type="checkbox" data-idx="' + idx + '"><input type="text" data-idx="' + idx + '" placeholder="Item..."><button data-del="' + idx + '">✕</button>';
      mbody.querySelector('#nte-checklist').appendChild(div);
    };

    mbody.querySelector('#nte-checklist').addEventListener('click', function (e) {
      if (e.target.dataset.del !== undefined) {
        e.target.closest('.nt-checklist-item').remove();
      }
    });

    // Save button
    var foot = document.createElement('div');
    foot.className = 'mfoot';
    foot.innerHTML = '<button class="wave1-btn ghost" id="nte-cancel">Cancelar</button><button class="wave1-btn" id="nte-save">Salvar</button>';
    mbody.appendChild(foot);

    mbody.querySelector('#nte-cancel').onclick = close;
    mbody.querySelector('#nte-save').onclick = async function () {
      var items = [];
      mbody.querySelectorAll('#nte-checklist .nt-checklist-item').forEach(function (row) {
        var cb = row.querySelector('input[type=checkbox]');
        var txt = row.querySelector('input[type=text]');
        if (txt && txt.value.trim()) {
          items.push({ text: txt.value.trim(), done: cb ? cb.checked : false });
        }
      });

      var payload = {
        title: mbody.querySelector('#nte-title').value.trim(),
        content: mbody.querySelector('#nte-content').value.trim(),
        color: selColor,
        pinned: mbody.querySelector('#nte-pin').checked,
        due_date: mbody.querySelector('#nte-due').value || null,
        checklist: items,
      };

      try {
        if (isNew) {
          await api('/api/notes', { method: 'POST', body: JSON.stringify(payload) });
        } else {
          await api('/api/notes/' + note.id, { method: 'PUT', body: JSON.stringify(payload) });
        }
        close();
        await loadNotes();
        renderNotes();
        toast('Nota salva!', 'success');
      } catch (e) {
        toast('Erro ao salvar nota: ' + e.message, 'error');
      }
    };
  });
}

// ═══════════════════════════════════════════════════════════════════
// Tasks
// ═══════════════════════════════════════════════════════════════════

async function loadTasks() {
  try {
    tasks = await api('/scheduler/tasks');
  } catch (e) { tasks = []; }
}

function renderTasks() {
  var box = el('nt-content');
  if (!box) return;
  if (!tasks.length) {
    box.innerHTML = '<div class="nt-empty">Nenhuma tarefa agendada. Clique em "+ Tarefa".</div>';
    return;
  }
  var html = '';
  tasks.forEach(function (t) {
    var pausedClass = t.paused ? ' paused' : '';
    var schedInfo = formatScheduleInfo(t);
    html += '<div class="nt-task-card' + pausedClass + '" id="nt-task-' + esc(t.id) + '">';
    html += '<div class="ttop"><span class="tname">' + esc(t.name) + '</span><span class="tkind">' + (SCHEDULE_LABELS[t.schedule_kind] || t.schedule_kind) + '</span></div>';
    html += '<div class="tinfo">' + esc(schedInfo) + ' · ' + esc(t.job_kind) + (t.action_type !== 'builtin' ? ' · ' + esc(t.action_type) : '') + (t.paused ? ' · ⏸ Pausada' : '') + '</div>';
    html += '<div class="tactions">';
    html += '<button class="wave1-btn sm" data-action="run-now" data-id="' + esc(t.id) + '">▶ Run Now</button>';
    if (t.paused) {
      html += '<button class="wave1-btn sm" data-action="resume" data-id="' + esc(t.id) + '">▶ Retomar</button>';
    } else {
      html += '<button class="wave1-btn sm ghost" data-action="pause" data-id="' + esc(t.id) + '">⏸ Pausar</button>';
    }
    html += '<button class="wave1-btn sm ghost" data-action="history" data-id="' + esc(t.id) + '">📋 Histórico</button>';
    html += '<button class="wave1-btn sm danger" data-action="delete" data-id="' + esc(t.id) + '">✕</button>';
    html += '</div>';
    html += '<div class="nt-runs" id="nt-runs-' + esc(t.id) + '" style="display:none"></div>';
    html += '</div>';
  });
  box.innerHTML = html;

  // Wire actions
  box.querySelectorAll('[data-action]').forEach(function (btn) {
    btn.addEventListener('click', async function (e) {
      e.stopPropagation();
      var action = btn.dataset.action;
      var id = btn.dataset.id;
      if (action === 'run-now') {
        await runTaskNow(id);
      } else if (action === 'pause') {
        await togglePause(id, true);
      } else if (action === 'resume') {
        await togglePause(id, false);
      } else if (action === 'history') {
        await showTaskHistory(id);
      } else if (action === 'delete') {
        if (confirm('Excluir esta tarefa?')) {
          await api('/scheduler/tasks/' + id, { method: 'DELETE' });
          await loadTasks();
          renderTasks();
          toast('Tarefa excluída', 'info');
        }
      }
    });
  });
}

function formatScheduleInfo(t) {
  if (t.schedule_kind === 'once') {
    return t.run_at ? 'Em ' + fmtDate(t.run_at) : 'Uma vez';
  }
  if (t.schedule_kind === 'interval') {
    var secs = t.interval_secs || 0;
    if (secs < 60) return 'A cada ' + secs + 's';
    if (secs < 3600) return 'A cada ' + Math.round(secs / 60) + 'min';
    return 'A cada ' + Math.round(secs / 3600) + 'h';
  }
  if (t.schedule_kind === 'cron') {
    return 'Cron: ' + (t.cron_expr || '?');
  }
  return t.schedule_kind;
}

async function runTaskNow(id) {
  try {
    var result = await api('/api/tasks/' + id + '/run-now', { method: 'POST', body: '{}' });
    toast('Task disparada · Job ' + result.job_id, 'success');
    await loadTasks();
    renderTasks();
  } catch (e) {
    toast('Erro ao disparar task: ' + e.message, 'error');
  }
}

async function togglePause(id, pause) {
  try {
    var path = '/api/tasks/' + id + '/' + (pause ? 'pause' : 'resume');
    await api(path, { method: 'POST' });
    await loadTasks();
    renderTasks();
    toast(pause ? 'Tarefa pausada' : 'Tarefa retomada', 'info');
  } catch (e) {
    toast('Erro: ' + e.message, 'error');
  }
}

async function showTaskHistory(id) {
  var runsBox = el('nt-runs-' + id);
  if (!runsBox) return;
  if (runsBox.style.display === 'block') { runsBox.style.display = 'none'; return; }
  try {
    var data = await api('/api/tasks/' + id + '/history');
    var runs = data.runs || [];
    if (!runs.length) {
      runsBox.innerHTML = '<div style="font-size:11px;color:var(--text-tertiary);padding:6px 0">Nenhuma execução.</div>';
    } else {
      runsBox.innerHTML = runs.map(function (r) {
        var statusClass = r.status === 'success' ? 'success' : (r.status === 'error' ? 'error' : 'running');
        var start = fmtDate(r.started_at);
        var output = r.output ? '<div style="font-size:10px;color:var(--text-tertiary);margin-top:2px;max-height:60px;overflow:auto">' + esc(r.output) + '</div>' : '';
        var err = r.error ? '<div style="font-size:10px;color:#ff7a8a">Erro: ' + esc(r.error) + '</div>' : '';
        return '<div class="nt-run-row"><span class="status ' + statusClass + '">' + r.status + '</span><span>' + start + '</span>' + err + output + '</div>';
      }).join('');
    }
    runsBox.style.display = 'block';
  } catch (e) {
    toast('Erro ao carregar histórico: ' + e.message, 'error');
  }
}

// ── Task editor (new / edit) ───────────────────────────────────────
function showTaskEditor(_existing) {
  var body = `
    <div class="nt-field"><label>Nome</label><input id="tke-name" placeholder="Minha Tarefa"></div>
    <div class="nt-row">
      <div class="nt-field">
        <label>Tipo de Agendamento</label>
        <select id="tke-kind">
          <option value="once">Uma vez</option>
          <option value="interval">Intervalo</option>
          <option value="cron">Cron</option>
        </select>
      </div>
      <div class="nt-field" id="tke-kind-config"></div>
    </div>
    <div class="nt-row">
      <div class="nt-field">
        <label>Ação</label>
        <select id="tke-action-type">
          <option value="builtin">Built-in (genérico)</option>
          <option value="llm_prompt">LLM Prompt</option>
          <option value="agent_run">Agent Run</option>
        </select>
      </div>
      <div class="nt-field">
        <label>Job Kind</label>
        <input id="tke-job-kind" value="generic" placeholder="generic">
      </div>
    </div>
    <div class="nt-field" id="tke-action-config-field" style="display:none">
      <label>Configuração da Ação (JSON)</label>
      <textarea id="tke-action-config" rows="3" placeholder='{"prompt": "...", "model_id": "..."}'></textarea>
    </div>
    <div class="nt-field">
      <label>Payload (JSON)</label>
      <textarea id="tke-payload" rows="2" placeholder='{"key":"value"}'></textarea>
    </div>
  `;

  openModal('Nova Tarefa', body, function (mbody, close) {
    var kindSelect = mbody.querySelector('#tke-kind');
    var kindConfig = mbody.querySelector('#tke-kind-config');
    var actionTypeSelect = mbody.querySelector('#tke-action-type');
    var actionConfigField = mbody.querySelector('#tke-action-config-field');

    function updateKindConfig() {
      var k = kindSelect.value;
      if (k === 'once') {
        kindConfig.innerHTML = '<label>Data/Hora (ISO)</label><input type="datetime-local" id="tke-run-at">';
      } else if (k === 'interval') {
        kindConfig.innerHTML = '<label>Intervalo (segundos)</label><input type="number" id="tke-interval" value="3600" min="10">';
      } else if (k === 'cron') {
        kindConfig.innerHTML = '<label>Expressão Cron</label><input id="tke-cron" placeholder="0 0 * * * *" value="0 * * * * *">';
      }
    }

    kindSelect.addEventListener('change', updateKindConfig);
    actionTypeSelect.addEventListener('change', function () {
      actionConfigField.style.display = actionTypeSelect.value === 'builtin' ? 'none' : 'block';
    });
    updateKindConfig();

    // Save button
    var foot = document.createElement('div');
    foot.className = 'mfoot';
    foot.innerHTML = '<button class="wave1-btn ghost" id="tke-cancel">Cancelar</button><button class="wave1-btn" id="tke-save">Criar</button>';
    mbody.appendChild(foot);

    mbody.querySelector('#tke-cancel').onclick = close;
    mbody.querySelector('#tke-save').onclick = async function () {
      var kind = kindSelect.value;
      var req = {
        name: mbody.querySelector('#tke-name').value.trim() || 'Nova Tarefa',
        schedule_kind: kind,
        job_kind: mbody.querySelector('#tke-job-kind').value.trim() || 'generic',
        payload_json: mbody.querySelector('#tke-payload').value.trim() || null,
        action_type: actionTypeSelect.value,
        action_config: mbody.querySelector('#tke-action-config').value.trim() || null,
        enabled: true,
      };

      if (kind === 'once') {
        var runAtEl = mbody.querySelector('#tke-run-at');
        req.run_at = runAtEl ? runAtEl.value : null;
      } else if (kind === 'interval') {
        var intEl = mbody.querySelector('#tke-interval');
        req.interval_secs = intEl ? parseInt(intEl.value, 10) : 3600;
      } else if (kind === 'cron') {
        var cronEl = mbody.querySelector('#tke-cron');
        req.cron_expr = cronEl ? cronEl.value.trim() : null;
      }

      try {
        await api('/scheduler/tasks', { method: 'POST', body: JSON.stringify(req) });
        close();
        await loadTasks();
        renderTasks();
        toast('Tarefa criada!', 'success');
      } catch (e) {
        toast('Erro: ' + e.message, 'error');
      }
    };
  });
}

// ═══════════════════════════════════════════════════════════════════
// Toast SSE
// ═══════════════════════════════════════════════════════════════════

function connectToastSSE() {
  if (toastEventSource) {
    try { toastEventSource.close(); } catch (_) {}
  }
  var baseUrl = (state && state.daemonUrl) || 'http://127.0.0.1:11435';
  var url = baseUrl.replace(/\/+$/, '') + '/api/toast/stream';
  toastEventSource = new EventSource(url);
  toastEventSource.onmessage = function (event) {
    try {
      var data = JSON.parse(event.data);
      showNativeToast(data);
    } catch (_) {}
  };
  toastEventSource.onerror = function () {
    // Reconnect after 5s.
    setTimeout(connectToastSSE, 5000);
  };
}

function showNativeToast(data) {
  var area = el('nt-toast-area');
  if (!area) return;
  var div = document.createElement('div');
  div.className = 'nt-toast-item ' + (data.kind || 'info');
  div.innerHTML = '<div class="ttl">' + esc(data.title || '') + '</div><div class="msg">' + esc(data.message || '') + '</div>';
  area.appendChild(div);
  setTimeout(function () { div.remove(); }, 6000);
}

// ── Cleanup on tab switch ─────────────────────────────────────────
export function destroyNotesTasks() {
  if (toastEventSource) {
    toastEventSource.close();
    toastEventSource = null;
  }
}

// ── Tab-click init (re-renders every time for freshness) ───────────
var ntTab = document.querySelector('.tab[data-panel="notes-tasks"]');
if (ntTab) {
  ntTab.addEventListener('click', function () {
    var root = el('notes-tasks-root');
    if (root) {
      if (!root.querySelector('.nt-root')) {
        renderNotesTasks(root);
      } else {
        refresh();
      }
    }
  });
}

