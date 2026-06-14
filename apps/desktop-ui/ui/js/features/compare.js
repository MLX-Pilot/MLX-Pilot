/* MLX Pilot — Compare models (feature).
 *
 * The "Comparar" tab: run the same prompt across several models, view them
 * side-by-side (optionally blind), vote a winner, synthesize a judge verdict
 * and browse history. Reuses core api() and the shared wave-common helpers.
 * The /compare/run and /synthesize calls are long-running (multi-model
 * generation / LLM judge), so they pass a generous timeoutMs to match the
 * original client's unbounded fetch. Migrated out of the wave1.js monolith.
 */

import { api } from '../core/api.js';
import { esc, el, fmtDate, toast, injectWave1Styles } from './wave-common.js';

// Long enough that realistic multi-model runs never hit it (original client
// had no client-side timeout).
const LONG_RUN_MS = 600000;

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
  const box = el('cmp-models');
  if (!box) return;
  if (!availableModels.length) { box.innerHTML = '<span class="wave1-label">Nenhum modelo disponível. Baixe modelos na aba Modelos.</span>'; return; }
  box.innerHTML = availableModels.map(function (m) {
    const id = m.id || m.path || m.name;
    return `<label class="wave1-chk"><input type="checkbox" value="${esc(id)}"> ${esc(m.name || id)}</label>`;
  }).join('');
}

function renderComparison(cmp) {
  currentComparison = cmp;
  const result = el('cmp-result');
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
  const box = el('cmp-history');
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
  const root = el('comparar-root');
  if (!root) return;
  root.innerHTML = compareRootHtml();
  renderModelCheckboxes();
  if (!availableModels.length) loadModels().then(renderModelCheckboxes);

  el('cmp-refresh-models').addEventListener('click', function () { loadModels().then(renderModelCheckboxes); });
  el('cmp-run').addEventListener('click', async function () {
    const prompt = el('cmp-prompt').value.trim();
    const selected = Array.prototype.slice.call(el('cmp-models').querySelectorAll('input:checked')).map(function (i) { return i.value; });
    if (!prompt) { toast('Escreva um prompt', 'error'); return; }
    if (selected.length < 2) { toast('Escolha pelo menos 2 modelos', 'error'); return; }
    const btn = el('cmp-run'); btn.disabled = true; btn.textContent = 'Comparando…';
    el('cmp-result').innerHTML = '<div class="wave1-empty">Gerando respostas…</div>';
    revealModels = false;
    try {
      const cmp = await api('/compare/run', { method: 'POST', timeoutMs: LONG_RUN_MS, body: JSON.stringify({
        prompt: prompt,
        system_prompt: el('cmp-system').value,
        models: selected,
        blind: el('cmp-blind').checked,
        temperature: Number(el('cmp-temp').value) || null,
        max_tokens: Number(el('cmp-max').value) || null,
      }) });
      renderComparison(cmp);
      refreshCompareHistory();
    } catch (err) { el('cmp-result').innerHTML = '<div class="wave1-empty">Erro: ' + esc(err.message) + '</div>'; }
    finally { btn.disabled = false; btn.textContent = 'Comparar'; }
  });

  el('cmp-result').addEventListener('click', async function (e) {
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
      try { const updated = await api('/compare/' + currentComparison.id + '/synthesize', { method: 'POST', timeoutMs: LONG_RUN_MS, body: JSON.stringify({ model_id: judge }) });
        renderComparison(updated); }
      catch (err) { toast(err.message, 'error'); e.target.disabled = false; e.target.textContent = 'Sintetizar (juiz IA)'; }
    }
  });

  el('cmp-history').addEventListener('click', async function (e) {
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

injectWave1Styles();
const compararTab = document.querySelector('[data-panel="comparar"]');
if (compararTab) compararTab.addEventListener('click', initComparePanel);
