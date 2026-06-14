/* MLX Pilot — Presets (feature).
 *
 * The chat composer's preset picker (`#preset-toggle`/`#preset-menu`): list,
 * apply-to-input, and the create/edit/delete manager modal. Talks to the
 * daemon's /agent/presets endpoints via core api(); shares the wave1 toast,
 * modal and theme CSS. Migrated out of the former wave1.js monolith.
 */

import { api } from '../core/api.js';
import { esc, el, toast, openModal, injectWave1Styles } from './wave-common.js';

let presetCache = [];

async function loadPresets() {
  try { presetCache = await api('/agent/presets'); } catch (_) { presetCache = []; }
  return presetCache;
}

function renderPresetMenu() {
  const menu = el('preset-menu');
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
  const input = el('chat-input');
  if (!input) { toast('Abra o Chat para usar um preset', 'error'); return; }
  const current = input.value || '';
  let composed = '';
  if (preset.system_prompt && preset.system_prompt.trim()) composed += preset.system_prompt.trim() + '\n\n';
  composed += (preset.prefix || '') + current + (preset.suffix || '');
  input.value = composed;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  input.focus();
  const chip = el('preset-toggle');
  const label = el('preset-active-label');
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
  const toggle = el('preset-toggle');
  const menu = el('preset-menu');
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

injectWave1Styles();
initPresets();
