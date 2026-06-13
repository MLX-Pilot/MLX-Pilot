/* MLX Pilot — Console & environment panel (feature).
 *
 * Renders the in-app console feed (UI + native desktop logs) and the daemon
 * environment-variables editor, with their load/clear/save actions.
 */

// === auto-imports (generated — do not edit) ===
import { pushConsoleEntry } from '../../app.js';
import { api, nativeInvoke } from '../core/api.js';
import { esc } from '../core/dom.js';
import { state } from '../core/state.js';
import { renderAgentProviderProfiles, renderAgentProviderSelector } from './providers.js';
import { probeDaemon } from './runtime.js';
// === end auto-imports ===

  // -- Console ------------------------------------------------
  function formatConsoleEntry(entry) {
    const time = entry?.time
      ? new Date(entry.time).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' })
      : '--:--:--';
    return `${time} [${String(entry?.level || 'info').toUpperCase()}] ${entry?.source || 'ui'} - ${entry?.message || ''}`;
  }

  function normalizeNativeLogLine(line) {
    const raw = String(line || '').trim();
    const match = raw.match(/^(\d{10,})\s+(\[[^\]]+\])\s+(.*)$/);
    if (!match) return raw;
    const date = new Date(Number(match[1]));
    const time = Number.isNaN(date.getTime())
      ? match[1]
      : date.toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    return `${time} ${match[2]} native - ${match[3]}`;
  }

  export function consoleText() {
    const uiLines = state.consoleEntries.map(formatConsoleEntry);
    const nativeLines = state.desktopLogEntries.map(normalizeNativeLogLine);
    return [
      '== UI ==',
      ...(uiLines.length ? uiLines : ['Sem eventos de UI nesta sessao.']),
      '',
      '== Native ==',
      ...(nativeLines.length ? nativeLines : ['Log nativo indisponivel ou vazio.']),
    ].join('\n');
  }

  export function renderConsole() {
    const feed = document.getElementById('console-feed');
    if (!feed) return;
    const textValue = consoleText();
    feed.textContent = textValue;
    feed.scrollTop = feed.scrollHeight;

    const count = document.getElementById('console-entry-count');
    if (count) {
      count.textContent = `${state.consoleEntries.length + state.desktopLogEntries.length} linhas`;
    }
  }

  function renderConsoleStatus(health) {
    const healthEl = document.getElementById('console-health');
    const daemonUrlEl = document.getElementById('console-daemon-url');
    const processEl = document.getElementById('console-process');
    const runtimeEl = document.getElementById('console-runtime');
    const logPathEl = document.getElementById('console-log-path');
    const refreshedEl = document.getElementById('console-last-refresh');
    const runtime = state.desktopRuntimeInfo || {};

    if (healthEl) healthEl.textContent = health?.status === 'ok' ? 'Online' : 'Offline';
    if (daemonUrlEl) daemonUrlEl.textContent = state.daemonUrl || runtime.daemon_url || '-';
    if (processEl) processEl.textContent = runtime.pid ? `PID ${runtime.pid}` : 'Navegador';
    if (runtimeEl) runtimeEl.textContent = runtime.embedded_daemon_enabled === false ? 'Daemon externo' : 'Daemon embutido';
    if (logPathEl) logPathEl.textContent = runtime.log_path || 'Indisponivel no navegador';
    if (refreshedEl) refreshedEl.textContent = `Atualizado ${new Date().toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' })}`;
  }

  export async function loadConsoleSnapshot() {
    const health = await probeDaemon(state.daemonUrl, 900);
    try {
      state.desktopRuntimeInfo = await nativeInvoke('desktop_runtime_info');
    } catch {
      state.desktopRuntimeInfo = null;
    }

    try {
      const snapshot = await nativeInvoke('desktop_log_snapshot', { limit: 220 });
      state.desktopLogEntries = Array.isArray(snapshot?.entries) ? snapshot.entries : [];
      if (snapshot?.path && !state.desktopRuntimeInfo) {
        state.desktopRuntimeInfo = { log_path: snapshot.path };
      }
    } catch {
      state.desktopLogEntries = [];
    }

    renderConsoleStatus(health);
    renderConsole();
  }

  export async function clearConsole() {
    state.consoleEntries = [];
    state.desktopLogEntries = [];
    try {
      await nativeInvoke('desktop_log_clear');
    } catch {
      /* Browser preview has no native log to clear. */
    }
    pushConsoleEntry('info', 'console', 'Console limpo');
    renderConsole();
  }

  // -- Environment --------------------------------------------
  export async function loadEnvironment() {
    try {
      const data = await api('/environment?reveal=false');
      state.environmentVars = data?.variables || [];
      renderEnvironment();
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
    } catch (e) {
      console.error('Environment load failed:', e);
      state.environmentVars = [];
      renderEnvironment();
      renderAgentProviderSelector();
      renderAgentProviderProfiles();
    }
  }

  function renderEnvironment() {
    const table = document.getElementById('env-table');
    if (!table) return;
    table.innerHTML = '';
    if (state.environmentVars.length === 0) {
      table.innerHTML = '<div style="padding:16px;text-align:center;color:var(--text-tertiary)">Nenhuma variável</div>';
      return;
    }
    state.environmentVars.forEach(v => {
      const row = document.createElement('div');
      row.className = 'env-row';
      const displayValue = v.is_secret
        ? (v.present ? (v.masked || '') : '')
        : (v.value || '');
      row.innerHTML = `
        <span class="env-key">${esc(v.key)}</span>
        <input
          type="${v.is_secret ? 'password' : 'text'}"
          class="input env-val"
          value="${esc(displayValue)}"
          data-key="${esc(v.key)}"
          data-secret="${v.is_secret ? 'true' : 'false'}"
          data-present="${v.present ? 'true' : 'false'}"
          data-initial-value="${esc(v.value || '')}"
          data-masked-value="${esc(v.masked || '')}"
          data-dirty="false"
          data-revealed="false"
        />
        ${v.is_secret ? `<button class="action-btn reveal-btn"${v.present ? '' : ' disabled'}>${v.present ? 'Revelar' : 'Sem secret'}</button>` : ''}`;
      table.appendChild(row);
    });
    const syncSecretButton = (input, btn) => {
      if (!input || !btn) return;
      const hasStoredSecret = input.dataset.present === 'true';
      const revealed = input.dataset.revealed === 'true';
      const hasDraft = String(input.value || '').trim() !== '' && input.dataset.dirty === 'true';
      if (revealed) {
        btn.disabled = false;
        btn.textContent = 'Ocultar';
        return;
      }
      if (hasStoredSecret) {
        btn.disabled = false;
        btn.textContent = 'Revelar';
        return;
      }
      btn.disabled = !hasDraft;
      btn.textContent = hasDraft ? 'Mostrar' : 'Sem secret';
    };

    table.querySelectorAll('.env-row').forEach((row) => {
      const input = row.querySelector('.env-val');
      const btn = row.querySelector('.reveal-btn');
      if (!input) return;

      const hiddenBaseline = () => (
        input.dataset.secret === 'true'
          ? (input.dataset.present === 'true' ? (input.dataset.maskedValue || '') : '')
          : (input.dataset.initialValue || '')
      );

      input.addEventListener('input', () => {
        const baseline = input.dataset.revealed === 'true'
          ? (input.dataset.initialValue || '')
          : hiddenBaseline();
        input.dataset.dirty = input.value !== baseline ? 'true' : 'false';
        syncSecretButton(input, btn);
      });

      syncSecretButton(input, btn);
    });

    table.querySelectorAll('.reveal-btn').forEach(btn => {
      btn.addEventListener('click', async () => {
        const input = btn.previousElementSibling;
        if (!input) return;
        if (input.dataset.revealed === 'true') {
          input.type = 'password';
          input.dataset.revealed = 'false';
          if (input.dataset.dirty !== 'true') {
            input.value = input.dataset.present === 'true' ? (input.dataset.maskedValue || '') : '';
          }
          syncSecretButton(input, btn);
          return;
        }

        if (input.dataset.dirty === 'true') {
          input.type = 'text';
          input.dataset.revealed = 'true';
          syncSecretButton(input, btn);
          return;
        }

        if (input.dataset.present !== 'true') {
          syncSecretButton(input, btn);
          return;
        }

        try {
          const data = await api('/environment?reveal=true');
          const found = (data?.variables || []).find(v => v.key === input.dataset.key);
          if (found?.present) {
            input.value = found.value || '';
            input.dataset.initialValue = found.value || '';
            input.type = 'text';
            input.dataset.revealed = 'true';
            input.dataset.dirty = 'false';
          }
        } catch {
          /* ok */
        }
        syncSecretButton(input, btn);
      });
    });
  }

  export async function saveEnvironment() {
    const vals = {};
    document.querySelectorAll('#env-table .env-val').forEach(input => {
      if (!input.dataset.key) return;
      const isSecret = input.dataset.secret === 'true';
      const dirty = input.dataset.dirty === 'true';
      const revealed = input.dataset.revealed === 'true';
      const current = String(input.value || '');
      const initial = String(input.dataset.initialValue || '');

      if (isSecret) {
        if (dirty || (revealed && current !== initial)) {
          vals[input.dataset.key] = current;
        }
        return;
      }

      if (dirty || current !== initial) {
        vals[input.dataset.key] = current;
      }
    });
    if (Object.keys(vals).length === 0) { alert('Nenhuma variável foi revelada para edição.'); return; }
    try {
      await api('/environment', { method: 'POST', body: JSON.stringify({ values: vals }) });
      await loadEnvironment();
      const btn = document.getElementById('save-env-btn');
      if (btn) { btn.textContent = 'Salvo!'; setTimeout(() => { btn.textContent = 'Salvar Variáveis'; }, 2000); }
    } catch (e) { alert('Erro: ' + e.message); }
  }
