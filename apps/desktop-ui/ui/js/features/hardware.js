/* MLX Pilot — Hardware & Model Fit (feature).
 *
 * The "Hardware" tab: scan/simulate the local machine, rank models by fit, show
 * serve profiles and trigger a catalog download. The /api/hwfit/* and
 * /catalog/downloads endpoints are hit with raw fetch + r.json() (preserving
 * the original parse-error surfacing), using the daemon URL from core state.
 * Migrated out of the former wave5.js IIFE: the window._wave5* globals and
 * inline onclick handlers are replaced by delegated listeners.
 */

import { esc, el, injectWave5Styles } from './wave-common.js';
import { state } from '../core/state.js';

injectWave5Styles();

var hwProfile = null;

// ── Scan hardware ──────────────────────────────────────────────────────
async function scanHardware(fresh) {
  el('hw-cards').innerHTML = '<div class="wave1-empty">Escaneando hardware...</div>';
  try {
    var r = await fetch(state.daemonUrl + '/api/hwfit/system?fresh=' + (fresh ? 'true' : 'false'));
    hwProfile = await r.json();
    renderHardwareCards();
    loadModelRanking();
    el('hw-models-section').style.display = 'block';
  } catch (e) {
    el('hw-cards').innerHTML = '<div class="wave1-empty" style="color:#ff7a8a">Erro ao escanear: ' + esc(e.message) + '</div>';
  }
}

// ── Hardware cards ─────────────────────────────────────────────────────
function renderHardwareCards() {
  if (!hwProfile) return;
  var gpuDetail = (hwProfile.gpus && hwProfile.gpus.length)
    ? hwProfile.gpus.map(function (g) { return g.name + ' (' + g.vram_gb.toFixed(1) + ' GB ' + esc(g.backend) + ')'; }).join('<br>')
    : 'Nenhuma GPU detectada (CPU-only)';

  el('hw-cards').innerHTML =
    '<div class="wave5-hw-card"><div class="eyebrow">CPU</div><div class="value">' + esc(hwProfile.cpu_name) + '</div><div class="detail">' + hwProfile.cpu_cores + ' núcleos</div></div>' +
    '<div class="wave5-hw-card"><div class="eyebrow">RAM</div><div class="value">' + hwProfile.ram_gb.toFixed(1) + ' GB</div><div class="detail">' + hwProfile.available_ram_gb.toFixed(1) + ' GB disponível</div></div>' +
    '<div class="wave5-hw-card"><div class="eyebrow">GPU</div><div class="value">' + hwProfile.gpu_count + ' GPU' + (hwProfile.gpu_count !== 1 ? 's' : '') + ' · ' + hwProfile.total_vram_gb.toFixed(1) + ' GB VRAM</div><div class="detail">' + gpuDetail + '</div></div>' +
    '<div class="wave5-hw-card"><div class="eyebrow">Backend</div><div class="value" style="text-transform:uppercase">' + esc(hwProfile.primary_backend) + '</div><div class="detail">' + (hwProfile.is_cpu_only ? 'Modo CPU-only' : 'Aceleração GPU disponível') + '</div></div>';
}

// ── Model ranking table ────────────────────────────────────────────────
async function loadModelRanking() {
  var sort = el('hw-sort').value;
  var useCase = el('hw-use-case').value;
  var tbody = el('hw-model-table-body');
  tbody.innerHTML = '<tr><td colspan="8" class="wave1-empty">Ranqueando modelos...</td></tr>';

  try {
    var params = '?sort=' + sort + '&use_case=' + useCase + '&fit_only=false';
    var r = await fetch(state.daemonUrl + '/api/hwfit/models' + params);
    var data = await r.json();
    if (!data || !data.models) return;

    tbody.innerHTML = data.models.map(function (m) {
      var fitColor = m.fit_level === 'excellent' ? 'var(--green,#3fb950)' :
                     m.fit_level === 'good' ? 'var(--cyan,#39d0d8)' :
                     m.fit_level === 'tight' ? '#d2991d' : '#ff7a8a';
      var badges = (m.badges || []).map(function (b) { return '<span class="wave5-badge">' + esc(b) + '</span>'; }).join('');
      return '<tr style="border-bottom:1px solid var(--border,#2a2a44);cursor:pointer" data-show-profiles data-model-id="' + esc(m.model.id) + '" data-params-b="' + m.model.params_b + '" data-arch="' + esc(m.model.architecture) + '" data-is-moe="' + (m.model.is_moe || false) + '" data-ctx-len="' + (m.model.context_length || 4096) + '">' +
        '<td style="padding:8px"><div style="font-weight:500">' + esc(m.model.name) + '</div><div style="font-size:10px;color:var(--text-tertiary)">' + esc((m.model.id || '').slice(0,40)) + badges + '</div></td>' +
        '<td style="padding:8px;font-size:12px">' + m.model.params_b.toFixed(1) + 'B' + (m.model.is_moe ? ' MoE' : '') + '</td>' +
        '<td style="padding:8px;font-size:11px;font-family:var(--font-mono,monospace)">' + esc(m.recommended_quant) + '</td>' +
        '<td style="padding:8px;font-size:12px">' + m.estimated_vram_gb.toFixed(1) + ' GB</td>' +
        '<td style="padding:8px;font-size:12px">' + m.estimated_tps.toFixed(0) + ' tok/s</td>' +
        '<td style="padding:8px"><span style="color:' + fitColor + ';font-weight:600;font-size:11px;text-transform:uppercase">' + esc(m.fit_level) + '</span></td>' +
        '<td style="padding:8px"><span style="font-weight:700;font-size:14px">' + m.composite_score.toFixed(0) + '</span></td>' +
        '<td style="padding:8px"><button class="wave1-btn ghost sm" style="font-size:10px" data-download="' + esc(m.model.id) + '">Baixar</button></td>' +
        '</tr>';
    }).join('');
  } catch (e) {
    console.error('wave5: model ranking failed', e);
    tbody.innerHTML = '<tr><td colspan="8" class="wave1-empty" style="color:#ff7a8a">Erro ao carregar ranking</td></tr>';
  }
}

// ── Serve profiles ─────────────────────────────────────────────────────
async function showServeProfiles(modelId, paramsB, arch, isMoe, ctxLen) {
  el('hw-profiles-section').style.display = 'block';
  var list = el('hw-profiles-list');
  list.innerHTML = '<div class="wave1-empty">Carregando perfis...</div>';

  try {
    var q = '?model_id=' + encodeURIComponent(modelId) + '&params_b=' + paramsB + '&architecture=' + encodeURIComponent(arch) + '&is_moe=' + isMoe + '&context_length=' + ctxLen;
    var r = await fetch(state.daemonUrl + '/api/hwfit/profiles' + q);
    var profiles = await r.json();

    list.innerHTML = (profiles || []).map(function (p) {
      var bg = p.fits ? 'var(--bg-elevated,#16162a)' : 'rgba(255,122,138,0.08)';
      return '<div class="wave5-profile-card" style="background:' + bg + '">' +
        '<div style="font-weight:700;font-size:13px;margin-bottom:8px">' + esc(p.name) + '</div>' +
        '<div style="font-size:12px;color:var(--text-secondary);line-height:1.7">' +
        '<div>Quant: <strong>' + esc(p.quant) + '</strong></div>' +
        '<div>GPU Layers: <strong>' + (p.n_gpu_layers === -1 ? 'Todas' : p.n_gpu_layers) + '</strong></div>' +
        '<div>Cache: <strong>' + esc(p.cache_type) + '</strong></div>' +
        '<div>Contexto: <strong>' + p.context_size.toLocaleString() + '</strong></div>' +
        '<div>VRAM Est.: <strong>' + p.estimated_vram_gb.toFixed(1) + ' GB</strong></div>' +
        (p.note ? '<div style="margin-top:6px;font-style:italic;color:var(--text-tertiary);font-size:11px">' + esc(p.note) + '</div>' : '') +
        '</div></div>';
    }).join('');
  } catch (e) {
    list.innerHTML = '<div class="wave1-empty" style="color:#ff7a8a">Erro ao carregar perfis</div>';
  }
  el('hw-profiles-section').scrollIntoView({ behavior: 'smooth' });
}

// ── Simulate hardware ──────────────────────────────────────────────────
async function applySimulatedHardware() {
  var body = {
    manual_gpu_count: parseInt(el('hw-sim-gpu-count').value) || null,
    manual_vram_gb: parseFloat(el('hw-sim-vram').value) || null,
    manual_ram_gb: parseFloat(el('hw-sim-ram').value) || null,
    manual_backend: el('hw-sim-backend').value || null,
    ignore_detected_gpu: !!el('hw-sim-gpu-count').value,
    ignore_detected_ram: !!el('hw-sim-ram').value
  };
  try {
    var r = await fetch(state.daemonUrl + '/api/hwfit/simulate', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    hwProfile = await r.json();
    renderHardwareCards();
    loadModelRanking();
    el('hw-models-section').style.display = 'block';
  } catch (e) { console.error(e); }
}

// ── Download model via catalog ─────────────────────────────────────────
async function downloadModel(modelId) {
  if (!confirm('Baixar ' + modelId + '?')) return;
  try {
    var r = await fetch(state.daemonUrl + '/catalog/downloads', {
      method: 'POST', headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ model_id: modelId, source: 'huggingface' })
    });
    var res = await r.json();
    alert('Download iniciado: ' + (res.job_id || 'ok'));
  } catch (e) { console.error(e); }
}

// ── Event bindings + init ──────────────────────────────────────────────
el('btn-hw-scan').addEventListener('click', function () { scanHardware(true); });
el('btn-hw-sim-apply').addEventListener('click', applySimulatedHardware);
el('hw-sort').addEventListener('change', loadModelRanking);
el('hw-use-case').addEventListener('change', loadModelRanking);

// Delegated handlers replace the former inline onclick + window globals.
// The download button is checked first so its row's profile view is suppressed
// (matching the original event.stopPropagation()).
el('hw-model-table-body').addEventListener('click', function (e) {
  const dl = e.target.closest('[data-download]');
  if (dl) { downloadModel(dl.getAttribute('data-download')); return; }
  const row = e.target.closest('[data-show-profiles]');
  if (row) {
    showServeProfiles(
      row.getAttribute('data-model-id'),
      Number(row.getAttribute('data-params-b')),
      row.getAttribute('data-arch'),
      row.getAttribute('data-is-moe') === 'true',
      Number(row.getAttribute('data-ctx-len'))
    );
  }
});

// Tab-aware init: scan hardware (or re-render) when the hardware tab opens.
const hardwareTab = document.querySelector('.tab[data-panel="hardware"]');
if (hardwareTab) hardwareTab.addEventListener('click', function () {
  if (!hwProfile) scanHardware(false);
  else { renderHardwareCards(); loadModelRanking(); }
});

console.log('Wave 5: Deep Research + Hardware Fit ready');
