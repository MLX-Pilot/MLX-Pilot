/* MLX Pilot — Orchestration Monitor (feature).
 *
 * Observability tab for agent runs: a central reasoning console (streamed via
 * SSE with replay + polling fallback), a phases/agents sidebar, an active/
 * historical run list, and a persistent global status bar (time/tokens/active).
 *
 * All telemetry comes from the daemon's /agent/orchestration endpoints — no
 * invented metrics. Content is escaped via esc(); the feed appends incrementally
 * (no full re-render) so it can absorb thousands of events.
 */

// === imports ===
import { api } from '../core/api.js';
import { esc, fmtNum, fmtDuration } from '../core/dom.js';
import { state } from '../core/state.js';
// === end imports ===

const MAX_FEED_NODES = 600;
const POLL_INTERVAL_MS = 2500;
const TICK_INTERVAL_MS = 1000;

const mon = {
  runsTab: 'active',
  runs: [],
  selectedId: null,
  selectedRun: null,
  lastSeq: 0,
  es: null,
  polling: false,
  pollTimer: null,
  tickTimer: null,
  sseErrors: 0,
  active: false,
};

// ── styles ───────────────────────────────────────────────────────────────
function injectStyles() {
  if (document.getElementById('monitor-styles')) return;
  const css = document.createElement('style');
  css.id = 'monitor-styles';
  css.textContent = `
    #panel-monitor .mon-root{display:flex;flex-direction:column;height:100%;min-height:0;padding:18px 20px 0;gap:14px}
    .mon-head{display:flex;justify-content:space-between;align-items:flex-start;gap:12px}
    .mon-head-right{display:flex;align-items:center;gap:10px}
    .mon-conn{font-size:11px;font-weight:600;padding:3px 9px;border-radius:var(--r-full,999px);border:1px solid var(--border,#2a2a44);color:var(--text-tertiary,#8a8aa0);background:var(--bg-elevated,#16162a);white-space:nowrap}
    .mon-conn[data-state="live"]{color:var(--green,#3fb950);border-color:var(--green-soft,#2ea04326)}
    .mon-conn[data-state="reconnecting"]{color:var(--amber,#f0b429);border-color:var(--amber-soft,#f0b42926)}
    .mon-conn[data-state="polling"]{color:var(--cyan,#39d0d8);border-color:var(--cyan-soft,#39d0d826)}
    .mon-conn[data-state="error"]{color:var(--rose,#ff7a8a);border-color:var(--rose-soft,#ff7a8a26)}
    .mon-conn[data-state="done"]{color:var(--text-secondary,#b8b8d0)}

    .mon-grid{display:grid;grid-template-columns:230px 1fr 270px;gap:14px;flex:1;min-height:0}
    .mon-runs,.mon-phases{display:flex;flex-direction:column;min-height:0;background:var(--bg-surface,#12121f);border:1px solid var(--border,#2a2a44);border-radius:var(--r-lg,12px);overflow:hidden}
    .mon-runs-tabs{display:flex;border-bottom:1px solid var(--border,#2a2a44)}
    .mon-runs-tab{flex:1;background:none;border:none;padding:9px 0;font-size:12px;font-weight:600;color:var(--text-tertiary,#8a8aa0);cursor:pointer}
    .mon-runs-tab.active{color:var(--text-primary,#f0f0f8);box-shadow:inset 0 -2px 0 var(--cyan,#39d0d8)}
    .mon-runs-list,.mon-phases-list{overflow-y:auto;padding:8px;display:flex;flex-direction:column;gap:6px}
    .mon-phases-title{padding:11px 12px;font-size:12px;font-weight:600;color:var(--text-secondary,#b8b8d0);border-bottom:1px solid var(--border,#2a2a44)}

    .mon-run-card{text-align:left;background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:var(--r-md,8px);padding:9px 11px;cursor:pointer;transition:border-color .15s}
    .mon-run-card:hover{border-color:var(--border-hover,#3a3a55)}
    .mon-run-card.selected{border-color:var(--cyan,#39d0d8);background:var(--bg-active,#1c1c30)}
    .mon-run-card .rc-top{display:flex;justify-content:space-between;align-items:center;gap:8px;margin-bottom:3px}
    .mon-run-card .rc-label{font-size:12.5px;font-weight:600;color:var(--text-primary,#f0f0f8);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
    .mon-run-card .rc-meta{font-size:10.5px;color:var(--text-tertiary,#8a8aa0);font-family:var(--font-mono,monospace)}
    .mon-dot{width:8px;height:8px;border-radius:50%;flex:none}
    .mon-dot.running{background:var(--cyan,#39d0d8);box-shadow:0 0 6px var(--cyan,#39d0d8);animation:mon-pulse 1.4s infinite}
    .mon-dot.completed{background:var(--green,#3fb950)}
    .mon-dot.failed,.mon-dot.cancelled{background:var(--rose,#ff7a8a)}
    @keyframes mon-pulse{0%,100%{opacity:1}50%{opacity:.35}}

    .mon-console{display:flex;flex-direction:column;min-height:0;background:var(--bg-surface,#12121f);border:1px solid var(--border,#2a2a44);border-radius:var(--r-lg,12px);overflow:hidden}
    .mon-console-head{padding:11px 14px;border-bottom:1px solid var(--border,#2a2a44);display:flex;flex-direction:column;gap:6px}
    .mon-run-title{font-size:14px;font-weight:600;color:var(--text-primary,#f0f0f8)}
    .mon-run-badges{display:flex;flex-wrap:wrap;gap:6px}
    .mon-badge{font-size:10.5px;font-weight:600;padding:2px 8px;border-radius:var(--r-full,999px);background:var(--bg-deep,#0c0c18);color:var(--text-secondary,#b8b8d0);border:1px solid var(--border,#2a2a44)}
    .mon-badge.cancel{cursor:pointer;color:var(--rose,#ff7a8a);border-color:var(--rose-soft,#ff7a8a26)}
    .mon-badge.cancel[disabled]{opacity:.4;cursor:not-allowed}

    .mon-feed{flex:1;overflow-y:auto;padding:10px 14px;font-size:12.5px;line-height:1.5;font-family:var(--font-mono,monospace)}
    .mon-ev{padding:5px 0;border-bottom:1px solid var(--white-ghost,#ffffff08);display:flex;gap:9px;align-items:flex-start}
    .mon-ev .ts{color:var(--text-tertiary,#8a8aa0);flex:none;font-size:10.5px;padding-top:1px}
    .mon-ev .body{flex:1;min-width:0;word-break:break-word}
    .mon-ev .tag{font-weight:700;text-transform:uppercase;font-size:9.5px;letter-spacing:.04em;margin-right:6px}
    .mon-ev.thinking .body{color:var(--text-tertiary,#8a8aa0);font-style:italic}
    .mon-ev.answer .body{color:var(--text-primary,#f0f0f8)}
    .mon-ev.tool_call .tag{color:var(--cyan,#39d0d8)}
    .mon-ev.tool_call .body{color:var(--text-primary,#f0f0f8)}
    .mon-ev.tool_result .tag{color:var(--green,#3fb950)}
    .mon-ev.tool_result .body{color:var(--text-secondary,#b8b8d0)}
    .mon-ev.phase .tag{color:var(--violet,#a371f7)}
    .mon-ev.phase .body{color:var(--text-secondary,#b8b8d0)}
    .mon-ev.action .tag{color:var(--amber,#f0b429)}
    .mon-ev.error .tag,.mon-ev.error .body{color:var(--rose,#ff7a8a)}
    .mon-ev .preview{display:block;color:var(--text-tertiary,#8a8aa0);font-size:11px;margin-top:2px;white-space:pre-wrap}

    .mon-phase{margin-bottom:10px}
    .mon-phase-head{display:flex;justify-content:space-between;align-items:center;font-size:12px;font-weight:600;color:var(--text-secondary,#b8b8d0);margin-bottom:5px}
    .mon-phase-prog{height:4px;background:var(--bg-deep,#0c0c18);border-radius:2px;overflow:hidden;margin-bottom:7px}
    .mon-phase-prog span{display:block;height:100%;background:var(--violet,#a371f7);transition:width .4s}
    .mon-agent{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:var(--r-sm,6px);padding:7px 9px;margin-bottom:5px}
    .mon-agent .ag-top{display:flex;justify-content:space-between;align-items:center;gap:6px;margin-bottom:3px}
    .mon-agent .ag-name{font-size:12px;font-weight:600;color:var(--text-primary,#f0f0f8);overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
    .mon-agent .ag-stats{display:flex;gap:10px;font-size:10.5px;color:var(--text-tertiary,#8a8aa0);font-family:var(--font-mono,monospace)}

    .mon-statusbar{display:flex;gap:26px;align-items:center;padding:11px 18px;margin:0 -20px;border-top:1px solid var(--border,#2a2a44);background:var(--bg-surface,#12121f)}
    .mon-stat{display:flex;flex-direction:column;gap:1px}
    .mon-stat-grow{margin-left:auto}
    .mon-stat-label{font-size:10px;text-transform:uppercase;letter-spacing:.05em;color:var(--text-tertiary,#8a8aa0)}
    .mon-stat-value{font-size:15px;font-weight:700;color:var(--text-primary,#f0f0f8);font-variant-numeric:tabular-nums}

    .mon-empty{padding:30px 16px;text-align:center;color:var(--text-tertiary,#8a8aa0);font-size:12.5px}

    @media (max-width:1024px){
      #panel-monitor .mon-grid{grid-template-columns:1fr}
      .mon-runs,.mon-phases{max-height:220px}
    }
  `;
  document.head.appendChild(css);
}

// ── helpers ────────────────────────────────────────────────────────────────
function daemonBase() {
  return state.daemonUrl || 'http://127.0.0.1:11435';
}

function fmtTs(iso) {
  if (!iso) return '--:--:--';
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '--:--:--';
  return d.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

function setConn(stateName, label) {
  const el = document.getElementById('mon-conn');
  if (!el) return;
  el.dataset.state = stateName;
  el.textContent = label;
}

const KIND_TAGS = {
  thinking: 'pensando',
  answer: 'resposta',
  tool_call: 'ferramenta',
  tool_result: 'resultado',
  phase: 'fase',
  action: 'ação',
  error: 'erro',
};

const STATUS_LABELS = {
  running: 'Executando',
  completed: 'Concluído',
  failed: 'Erro',
  cancelled: 'Cancelado',
};

// ── run list ────────────────────────────────────────────────────────────────
function renderRuns() {
  const list = document.getElementById('mon-runs-list');
  if (!list) return;
  const filtered = mon.runs.filter((r) =>
    mon.runsTab === 'active' ? r.status === 'running' : r.status !== 'running');

  if (!filtered.length) {
    list.innerHTML = `<div class="mon-empty">${
      mon.runsTab === 'active' ? 'Nenhum run ativo. Inicie um run do agente.' : 'Sem runs no histórico.'
    }</div>`;
    return;
  }

  list.innerHTML = '';
  filtered.forEach((r) => {
    const card = document.createElement('button');
    card.type = 'button';
    card.className = 'mon-run-card' + (r.run_id === mon.selectedId ? ' selected' : '');
    card.dataset.runId = r.run_id;
    const tokens = fmtNum(r.metrics?.tokens_total || 0);
    card.innerHTML =
      `<div class="rc-top">` +
      `<span class="rc-label">${esc(r.label || r.run_id)}</span>` +
      `<span class="mon-dot ${esc(r.status)}"></span>` +
      `</div>` +
      `<div class="rc-meta">${esc(STATUS_LABELS[r.status] || r.status)} · ${esc(tokens)} tok · ${r.metrics?.agents || 0} ag</div>`;
    card.addEventListener('click', () => selectRun(r.run_id));
    list.appendChild(card);
  });
}

// ── central console ───────────────────────────────────────────────────────
function renderRunHeader(run) {
  const title = document.getElementById('mon-run-title');
  const badges = document.getElementById('mon-run-badges');
  if (!title || !badges) return;
  if (!run) {
    title.textContent = 'Nenhum run selecionado';
    badges.innerHTML = '';
    return;
  }
  title.textContent = run.label || run.run_id;
  const m = run.metrics || {};
  const items = [
    `${m.agents || 0} Agentes`,
    fmtDuration(Math.floor((m.elapsed_ms || 0) / 1000)),
    `${fmtNum(m.tokens_total || 0)} tokens`,
    `${m.tool_calls || 0} ferramentas`,
    STATUS_LABELS[run.status] || run.status,
  ];
  let html = items.map((t) => `<span class="mon-badge">${esc(t)}</span>`).join('');
  if (run.status === 'running') {
    const dis = run.job_id ? '' : ' disabled title="Run sem job associado"';
    html += `<span class="mon-badge cancel" id="mon-cancel"${dis}>Cancelar</span>`;
  }
  badges.innerHTML = html;
  const cancelBtn = document.getElementById('mon-cancel');
  if (cancelBtn && !cancelBtn.hasAttribute('disabled')) {
    cancelBtn.addEventListener('click', () => cancelRun(run.run_id));
  }
}

function eventNode(ev) {
  const node = document.createElement('div');
  node.className = 'mon-ev ' + esc(ev.kind || 'phase');
  node.dataset.seq = ev.seq;
  const tag = KIND_TAGS[ev.kind] || ev.kind || '';
  const preview = ev.meta && ev.meta.preview ? `<span class="preview">${esc(ev.meta.preview)}</span>` : '';
  node.innerHTML =
    `<span class="ts">${esc(fmtTs(ev.ts))}</span>` +
    `<span class="body"><span class="tag">${esc(tag)}</span>${esc(ev.text || '')}${preview}</span>`;
  return node;
}

function feedAtBottom(feed) {
  return feed.scrollHeight - feed.scrollTop - feed.clientHeight < 40;
}

function appendEvent(ev) {
  if (ev.seq <= mon.lastSeq) return; // dedupe (defensive against replay overlap)
  mon.lastSeq = ev.seq;
  const feed = document.getElementById('mon-feed');
  if (!feed) return;
  const stick = feedAtBottom(feed);
  feed.appendChild(eventNode(ev));
  while (feed.childElementCount > MAX_FEED_NODES) feed.removeChild(feed.firstChild);
  if (stick) feed.scrollTop = feed.scrollHeight;
}

function renderFeed(events) {
  const feed = document.getElementById('mon-feed');
  if (!feed) return;
  feed.innerHTML = '';
  if (!events || !events.length) {
    feed.innerHTML = '<div class="mon-empty">Sem eventos de raciocínio ainda.</div>';
    return;
  }
  const frag = document.createDocumentFragment();
  events.forEach((ev) => frag.appendChild(eventNode(ev)));
  feed.appendChild(frag);
  feed.scrollTop = feed.scrollHeight;
}

// ── phases / agents sidebar ──────────────────────────────────────────────
function renderPhases(run) {
  const list = document.getElementById('mon-phases-list');
  if (!list) return;
  if (!run || !run.phases || !run.phases.length) {
    list.innerHTML = '<div class="mon-empty">Sem fases ainda.</div>';
    return;
  }
  list.innerHTML = run.phases.map((phase) => {
    const pct = Math.round((phase.progress || 0) * 100);
    const agents = (phase.agents || []).map((a) => {
      const dur = fmtDuration(Math.floor((a.elapsed_ms || 0) / 1000));
      return (
        `<div class="mon-agent">` +
        `<div class="ag-top"><span class="ag-name">${esc(a.label || a.id)}</span>` +
        `<span class="mon-dot ${esc(a.status)}"></span></div>` +
        `<div class="ag-stats"><span>${esc(fmtNum(a.tokens_total || 0))} tok</span>` +
        `<span>${a.tool_calls || 0} ferr</span><span>${esc(dur)}</span></div>` +
        `</div>`
      );
    }).join('');
    return (
      `<div class="mon-phase">` +
      `<div class="mon-phase-head"><span>${esc(phase.name)}</span><span>${pct}%</span></div>` +
      `<div class="mon-phase-prog"><span style="width:${pct}%"></span></div>` +
      agents +
      `</div>`
    );
  }).join('');
}

// ── global status bar ───────────────────────────────────────────────────
function renderGlobal(metrics) {
  if (!metrics) return;
  const time = document.getElementById('mon-stat-time');
  const tokens = document.getElementById('mon-stat-tokens');
  const activeEl = document.getElementById('mon-stat-active');
  const tools = document.getElementById('mon-stat-tools');
  if (time) time.textContent = fmtDuration(Math.floor((metrics.active_elapsed_ms || 0) / 1000));
  if (tokens) tokens.textContent = `${fmtNum(metrics.total_tokens || 0)} tokens`;
  if (activeEl) {
    const n = metrics.active_runs || 0;
    activeEl.textContent = `${n} ${n === 1 ? 'tarefa' : 'tarefas'}`;
  }
  if (tools) tools.textContent = fmtNum(metrics.total_tool_calls || 0);
  mon.globalMetrics = metrics;
}

// Smoothly tick the global time + selected run duration between polls.
function tick() {
  if (mon.globalMetrics && mon.globalMetrics.active_runs > 0) {
    mon.globalMetrics.active_elapsed_ms = (mon.globalMetrics.active_elapsed_ms || 0) + TICK_INTERVAL_MS;
    const time = document.getElementById('mon-stat-time');
    if (time) time.textContent = fmtDuration(Math.floor(mon.globalMetrics.active_elapsed_ms / 1000));
  }
}

// ── data loading ───────────────────────────────────────────────────────────
async function loadList() {
  try {
    const data = await api('/agent/orchestration?limit=50');
    mon.runs = Array.isArray(data?.runs) ? data.runs : [];
    renderGlobal(data?.metrics);
    renderRuns();
    if (!mon.selectedId) {
      const firstActive = mon.runs.find((r) => r.status === 'running') || mon.runs[0];
      if (firstActive) selectRun(firstActive.run_id);
    } else {
      // refresh selected run sidebar/header without disturbing the feed
      await refreshSelected();
    }
  } catch (e) {
    setConn('error', 'Daemon offline');
  }
}

async function refreshSelected() {
  if (!mon.selectedId) return;
  try {
    const run = await api(`/agent/orchestration/${encodeURIComponent(mon.selectedId)}`);
    mon.selectedRun = run;
    renderRunHeader(run);
    renderPhases(run);
    // If we are in polling mode, also fold in any new feed events.
    if (mon.polling && Array.isArray(run.events)) {
      run.events.forEach(appendEvent);
    }
  } catch (e) { /* run may have been pruned */ }
}

async function selectRun(runId) {
  if (mon.es) { mon.es.close(); mon.es = null; }
  mon.selectedId = runId;
  mon.lastSeq = 0;
  mon.sseErrors = 0;
  renderRuns();
  let run = null;
  try {
    run = await api(`/agent/orchestration/${encodeURIComponent(runId)}`);
  } catch (e) {
    setConn('error', 'Run indisponível');
    return;
  }
  mon.selectedRun = run;
  renderRunHeader(run);
  renderPhases(run);
  renderFeed(run.events);
  mon.lastSeq = run.last_seq || (run.events?.length ? run.events[run.events.length - 1].seq : 0);

  if (run.status === 'running') {
    openStream(runId);
  } else {
    setConn('done', STATUS_LABELS[run.status] || 'Concluído');
  }
}

// ── SSE streaming with replay + polling fallback ────────────────────────────
function openStream(runId) {
  if (!window.EventSource) { startPolling(); return; }
  setConn('reconnecting', 'Conectando…');
  const url = `${daemonBase()}/agent/orchestration/${encodeURIComponent(runId)}/stream?after=${mon.lastSeq}`;
  let es;
  try {
    es = new EventSource(url);
  } catch (e) {
    startPolling();
    return;
  }
  mon.es = es;

  es.addEventListener('reasoning', (e) => {
    mon.sseErrors = 0;
    setConn('live', 'Ao vivo');
    try {
      const ev = JSON.parse(e.data);
      appendEvent(ev);
      // Lifecycle events change the sidebar/header — refresh lazily.
      if (ev.kind === 'phase' || ev.kind === 'error' || ev.kind === 'tool_result') {
        scheduleSelectedRefresh();
      }
    } catch (_) { /* ignore malformed frame */ }
  });

  // Server signals a buffer gap on reconnect — reload the snapshot to resync.
  es.addEventListener('resync', () => { void resync(runId); });

  es.onerror = () => {
    mon.sseErrors += 1;
    if (es.readyState === EventSource.CLOSED || mon.sseErrors >= 4) {
      es.close();
      if (mon.es === es) mon.es = null;
      // The run may simply have finished; verify before falling back.
      void verifyOrPoll(runId);
    } else {
      setConn('reconnecting', 'Reconectando…');
    }
  };
}

let refreshScheduled = false;
function scheduleSelectedRefresh() {
  if (refreshScheduled) return;
  refreshScheduled = true;
  setTimeout(() => { refreshScheduled = false; void refreshSelected(); }, 400);
}

async function resync(runId) {
  try {
    const run = await api(`/agent/orchestration/${encodeURIComponent(runId)}`);
    mon.selectedRun = run;
    renderFeed(run.events);
    mon.lastSeq = run.last_seq || mon.lastSeq;
    renderRunHeader(run);
    renderPhases(run);
  } catch (_) { /* ignore */ }
}

async function verifyOrPoll(runId) {
  try {
    const run = await api(`/agent/orchestration/${encodeURIComponent(runId)}`);
    if (run.status !== 'running') {
      mon.selectedRun = run;
      renderRunHeader(run);
      renderPhases(run);
      setConn('done', STATUS_LABELS[run.status] || 'Concluído');
      return;
    }
  } catch (_) { /* fall through to polling */ }
  startPolling();
}

function startPolling() {
  if (mon.polling) return;
  mon.polling = true;
  setConn('polling', 'Polling');
}

// ── cancel ───────────────────────────────────────────────────────────────
async function cancelRun(runId) {
  try {
    const res = await api(`/agent/orchestration/${encodeURIComponent(runId)}/cancel`, { method: 'POST' });
    if (res && res.message) setConn(res.cancelled ? 'done' : 'error', res.message);
    await refreshSelected();
  } catch (e) {
    setConn('error', 'Falha ao cancelar');
  }
}

// ── lifecycle ───────────────────────────────────────────────────────────────
export function startMonitor() {
  injectStyles();
  if (mon.active) { void loadList(); return; }
  mon.active = true;

  document.querySelectorAll('.mon-runs-tab').forEach((tab) => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.mon-runs-tab').forEach((t) => t.classList.remove('active'));
      tab.classList.add('active');
      mon.runsTab = tab.dataset.runs;
      renderRuns();
    });
  });
  document.getElementById('mon-refresh')?.addEventListener('click', () => void loadList());

  void loadList();
  mon.pollTimer = setInterval(() => { if (mon.active) void loadList(); }, POLL_INTERVAL_MS);
  mon.tickTimer = setInterval(tick, TICK_INTERVAL_MS);
}

export function stopMonitor() {
  mon.active = false;
  mon.polling = false;
  if (mon.es) { mon.es.close(); mon.es = null; }
  if (mon.pollTimer) { clearInterval(mon.pollTimer); mon.pollTimer = null; }
  if (mon.tickTimer) { clearInterval(mon.tickTimer); mon.tickTimer = null; }
}
