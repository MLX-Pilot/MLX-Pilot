/* MLX Pilot — Deep Research (feature).
 *
 * The "Pesquisa" tab: start an iterative research job, stream its progress over
 * SSE, browse the library and view/spin-off/delete a report. Reuses core
 * api()/state and the shared wave-common helpers (esc, el, injectWave5Styles).
 * Migrated out of the former wave5.js IIFE: the window._wave5* globals and
 * inline onclick handlers are replaced by delegated listeners.
 */

import { api } from '../core/api.js';
import { state } from '../core/state.js';
import { esc, el, injectWave5Styles } from './wave-common.js';

injectWave5Styles();

const researchJobs = {}; // jobId -> { es, phase, pct }

// ── Populate model selector from /models/all (unified local+cloud) ────
async function populateResearchModels() {
  const sel = el('research-model');
  if (!sel) return;
  sel.innerHTML = '<option value="auto">Auto (detectar)</option>';
  try {
    const groups = await api('/models/all');
    if (!Array.isArray(groups)) return;
    for (const g of groups) {
      if (!g.models || !g.models.length) continue;
      const optgroup = document.createElement('optgroup');
      optgroup.label = g.label + (g.configured ? '' : ' (não configurado)');
      for (const m of g.models) {
        const opt = document.createElement('option');
        opt.value = m.id;
        opt.textContent = m.label + (m.badge === 'cloud' ? ' ☁' : '');
        if (g.kind === 'cloud' && !g.configured) opt.disabled = true;
        optgroup.appendChild(opt);
      }
      sel.appendChild(optgroup);
    }
  } catch (e) {
    console.warn('wave5: failed to load models for research selector', e);
  }
}

// ── Start research ─────────────────────────────────────────────────────
async function startResearch() {
  const query = el('research-query').value.trim();
  if (!query) return;

  const btn = el('btn-research-start');
  const msg = el('research-start-msg');
  btn.disabled = true;
  btn.textContent = 'Iniciando...';
  msg.textContent = '';

  const payload = {
    query: query,
    max_rounds: parseInt(el('research-rounds').value) || 3,
    search_provider: el('research-provider').value || null,
    model_id: el('research-model').value || null,
    category: el('research-category').value || null
  };

  try {
    const res = await api('/research/start', { method: 'POST', body: JSON.stringify(payload) });

    // Check for fail-fast error response
    if (res.error) {
      msg.textContent = res.message || 'Erro ao iniciar pesquisa';
      msg.style.color = '#ff7a8a';
      btn.disabled = false;
      btn.innerHTML = '<svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><circle cx="10" cy="10" r="7"/><path d="M10 6v8M6 10h8"/></svg> Iniciar Pesquisa';
      return;
    }

    if (res && res.job_id) {
      attachResearchJob(res.job_id);
      el('research-jobs-section').style.display = 'block';
    }
  } catch (e) {
    msg.textContent = e.message;
    msg.style.color = '#ff7a8a';
  }
  btn.disabled = false;
  btn.innerHTML = '<svg viewBox="0 0 20 20" width="16" height="16" fill="none" stroke="currentColor" stroke-width="2"><circle cx="10" cy="10" r="7"/><path d="M10 6v8M6 10h8"/></svg> Iniciar Pesquisa';
}

// ── SSE streaming ──────────────────────────────────────────────────────
function attachResearchJob(jobId) {
  const es = new EventSource(state.daemonUrl + '/api/research/stream/' + jobId);
  researchJobs[jobId] = { es: es, phase: 'queued', pct: 0 };

  es.onmessage = function (e) {
    try {
      const d = JSON.parse(e.data);
      researchJobs[jobId].phase = d.phase || 'running';
      researchJobs[jobId].pct = d.percent || 0;
      researchJobs[jobId].msg = d.message || '';
      renderResearchJobs();
    } catch (_) {}
  };
  es.onerror = function () {
    es.close();
    setTimeout(function () {
      delete researchJobs[jobId];
      renderResearchJobs();
      loadResearchLibrary();
    }, 2000);
  };
  renderResearchJobs();
}

// ── Cancel ─────────────────────────────────────────────────────────────
async function cancelResearch(jobId) {
  try {
    await api('/research/cancel/' + jobId, { method: 'POST' });
    if (researchJobs[jobId]) {
      researchJobs[jobId].es.close();
      delete researchJobs[jobId];
      renderResearchJobs();
      loadResearchLibrary();
    }
  } catch (e) { console.error(e); }
}

// ── Render job cards ───────────────────────────────────────────────────
function renderResearchJobs() {
  const container = el('research-jobs-list');
  const ids = Object.keys(researchJobs);
  if (!ids.length) { el('research-jobs-section').style.display = 'none'; return; }
  el('research-jobs-section').style.display = 'block';

  container.innerHTML = ids.map(function (id) {
    var j = researchJobs[id] || {};
    var pct = j.pct || 0;
    var phase = j.phase || 'queued';
    var msg = esc(j.msg || 'Aguardando...');
    var labels = {planning:'Planejando',searching:'Buscando',extracting:'Extraindo',synthesizing:'Sintetizando',deciding:'Decidindo',finalizing:'Finalizando',done:'Concluída',cancelled:'Cancelada',error:'Erro'};
    var label = labels[phase] || phase;
    var isDone = phase === 'done' || phase === 'error' || phase === 'cancelled';
    var cls = phase === 'error' ? 'error' : (isDone ? 'done' : '');
    return '<div class="wave5-job-card">' +
      '<div class="head"><span class="id">#' + esc(id.slice(0,8)) + '</span><span class="phase" style="color:' + (phase==='error'?'#ff7a8a':phase==='done'?'var(--green,#3fb950)':'var(--cyan,#39d0d8)') + '">' + esc(label) + '</span></div>' +
      '<div class="wave5-progress"><div class="wave5-progress-fill ' + cls + '" style="width:' + pct + '%"></div></div>' +
      '<div style="font-size:11px;color:var(--text-tertiary);margin-top:4px">' + msg + '</div>' +
      (!isDone ? '<button class="wave1-btn danger sm" style="margin-top:6px" data-cancel-job="' + esc(id) + '">Cancelar</button>' : '') +
      '</div>';
  }).join('');
}

// ── Library ────────────────────────────────────────────────────────────
async function loadResearchLibrary() {
  var container = el('research-library-list');
  if (!container) return;
  try {
    var sessions = await api('/research/library');
    if (!Array.isArray(sessions) || !sessions.length) {
      container.innerHTML = '<div class="wave1-empty">Nenhuma pesquisa concluída</div>';
      return;
    }
    container.innerHTML = sessions.map(function (s) {
      var date = s.created_at ? new Date(s.created_at).toLocaleDateString('pt-BR') : '';
      var badge = s.status === 'done' ? '<span style="color:var(--green,#3fb950);font-size:11px">Concluída</span>' :
                  s.status === 'error' ? '<span style="color:#ff7a8a;font-size:11px">Erro</span>' :
                  '<span style="color:var(--text-tertiary);font-size:11px">' + esc(s.status) + '</span>';
      return '<div class="wave5-lib-item" data-view-report="' + esc(s.id) + '">' +
        '<div style="display:flex;justify-content:space-between;align-items:flex-start">' +
        '<div><div style="font-weight:500;font-size:13px">' + esc((s.query || '').slice(0,90)) + (s.query && s.query.length > 90 ? '...' : '') + '</div>' +
        '<div class="meta">' + date + ' · ' + (s.rounds||0) + ' rounds · ' + (s.sources||0) + ' fontes' + (s.category ? ' · ' + esc(s.category) : '') + '</div></div>' +
        badge + '</div></div>';
    }).join('');
  } catch (e) {
    console.error('wave5: library load failed', e);
    container.innerHTML = '<div class="wave1-empty" style="color:#ff7a8a">Erro ao carregar biblioteca</div>';
  }
}

// ── Report viewer ──────────────────────────────────────────────────────
function viewResearchReport(sessionId) {
  el('research-form-card').style.display = 'none';
  el('research-jobs-section').style.display = 'none';
  el('research-library-list').parentElement.style.display = 'none'; // hide library header too
  var viewer = el('research-report-viewer');
  viewer.style.display = 'block';
  viewer.dataset.sessionId = sessionId;
  el('research-report-iframe').src = state.daemonUrl + '/api/research/report/' + sessionId;
  // Scroll to viewer
  viewer.scrollIntoView({ behavior: 'smooth' });
}

function backToLibrary() {
  el('research-report-viewer').style.display = 'none';
  el('research-form-card').style.display = '';
  el('research-jobs-section').style.display = Object.keys(researchJobs).length ? 'block' : 'none';
  el('research-library-list').parentElement.style.display = '';
  loadResearchLibrary();
}

async function spinoffResearch() {
  var sessionId = el('research-report-viewer').dataset.sessionId;
  if (!sessionId) return;
  try {
    var res = await api('/research/spinoff/' + sessionId, { method: 'POST' });
    if (res && res.session_id) {
      // Try switching to chat tab
      var chatTab = document.querySelector('.tab[data-panel="chat"]');
      if (chatTab) chatTab.click();
    }
  } catch (e) { console.error(e); }
}

async function deleteResearch() {
  var sessionId = el('research-report-viewer').dataset.sessionId;
  if (!sessionId || !confirm('Excluir esta pesquisa?')) return;
  try {
    await api('/research/' + sessionId, { method: 'DELETE' });
    backToLibrary();
  } catch (e) { console.error(e); }
}

// ── Event bindings + init ──────────────────────────────────────────────
el('btn-research-start').addEventListener('click', startResearch);
el('btn-refresh-library').addEventListener('click', loadResearchLibrary);
el('btn-report-back').addEventListener('click', backToLibrary);
el('btn-report-pdf').addEventListener('click', function () { window.print(); });
el('btn-report-html').addEventListener('click', function () {
  var sid = el('research-report-viewer').dataset.sessionId;
  if (sid) window.open(state.daemonUrl + '/api/research/report/' + sid, '_blank');
});
el('btn-report-spinoff').addEventListener('click', spinoffResearch);
el('btn-report-delete').addEventListener('click', deleteResearch);

// Delegated handlers replace the former inline onclick + window globals.
el('research-jobs-list').addEventListener('click', function (e) {
  const cancelBtn = e.target.closest('[data-cancel-job]');
  if (cancelBtn) cancelResearch(cancelBtn.getAttribute('data-cancel-job'));
});
el('research-library-list').addEventListener('click', function (e) {
  const item = e.target.closest('[data-view-report]');
  if (item) viewResearchReport(item.getAttribute('data-view-report'));
});

// Tab-aware init: populate models + library when the research tab opens.
const researchTab = document.querySelector('.tab[data-panel="research"]');
if (researchTab) researchTab.addEventListener('click', function () {
  populateResearchModels();
  loadResearchLibrary();
  // Also refresh the model selector since models may have changed
  setTimeout(populateResearchModels, 500);
});
