/* MLX Pilot — Wave feature shared helpers (feature).
 *
 * Presentation utilities shared by the presets, memory, history, compare,
 * research and hardware modules: attribute-safe HTML escaping, an element
 * lookup, date formatting, the `#wave1-toast` notice, the `.wave1-overlay`
 * modal, and the wave1/wave5 theme-CSS injectors. Network access goes through
 * core api()/state — this module only holds the wave-specific markup helpers
 * whose exact output must be preserved for visual parity. `esc` intentionally
 * differs from core dom.esc: it escapes quotes (for attribute contexts) and
 * stringifies falsy values, matching the original wave templates.
 */

export function esc(value) {
  return String(value == null ? '' : value)
    .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

export function el(id) { return document.getElementById(id); }

export function fmtDate(iso) {
  if (!iso) return '';
  const d = new Date(iso);
  if (isNaN(d.getTime())) return '';
  return d.toLocaleString();
}

let toastTimer = null;
export function toast(message, kind) {
  let box = el('wave1-toast');
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

// ── generic modal ────────────────────────────────────────────────────────
export function openModal(title, bodyHtml, onMount) {
  const root = el('wave1-modal-root') || document.body;
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

// ── styles: wave1 (presets/memory/history/compare) ──────────────────────
export function injectWave1Styles() {
  if (el('wave1-styles')) return;
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

// ── styles: wave5 (research/hardware) ───────────────────────────────────
export function injectWave5Styles() {
  if (el('wave5-styles')) return;
  const css = document.createElement('style');
  css.id = 'wave5-styles';
  css.textContent = `
      .wave5-label{font-size:11px;color:var(--text-tertiary,#8a8aa0);text-transform:uppercase;letter-spacing:.04em;margin-bottom:2px;display:block}
      .wave5-progress{height:4px;background:var(--bg-deep,#0c0c18);border-radius:2px;overflow:hidden;margin:4px 0}
      .wave5-progress-fill{height:100%;background:var(--cyan,#39d0d8);transition:width .3s}
      .wave5-progress-fill.error{background:#ff7a8a}
      .wave5-progress-fill.done{background:var(--green,#3fb950)}
      .wave5-job-card{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:8px;padding:12px 14px;margin-bottom:8px}
      .wave5-job-card .head{display:flex;justify-content:space-between;align-items:center;margin-bottom:4px}
      .wave5-job-card .id{font-family:var(--font-mono,monospace);font-size:11px;color:var(--text-tertiary,#8a8aa0)}
      .wave5-job-card .phase{font-size:11px;font-weight:600}
      .wave5-lib-item{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:8px;padding:12px 14px;margin-bottom:6px;cursor:pointer}
      .wave5-lib-item:hover{border-color:var(--cyan,#39d0d8)}
      .wave5-lib-item .meta{font-size:11px;color:var(--text-tertiary,#8a8aa0);margin-top:2px}
      .wave5-hw-card{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:8px;padding:14px}
      .wave5-hw-card .eyebrow{font-size:10px;color:var(--text-tertiary,#8a8aa0);text-transform:uppercase;letter-spacing:.05em;margin-bottom:4px}
      .wave5-hw-card .value{font-weight:600;font-size:14px}
      .wave5-hw-card .detail{font-size:11px;color:var(--text-secondary,#b9b9cc);margin-top:2px}
      .wave5-badge{display:inline-block;font-size:10px;padding:1px 6px;border-radius:4px;margin-left:3px;background:var(--bg-deep,#0c0c18);border:1px solid var(--border,#2a2a44);color:var(--text-tertiary,#8a8aa0)}
      .wave5-profile-card{background:var(--bg-elevated,#16162a);border:1px solid var(--border,#2a2a44);border-radius:8px;padding:14px;flex:1;min-width:180px}
    `;
  document.head.appendChild(css);
}
