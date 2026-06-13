/* MLX Pilot — DOM & formatting utilities (core).
 *
 * Pure, dependency-free helpers shared across modules: HTML escaping and
 * human-readable formatting of bytes / counts / durations, plus the model
 * icon lookup. No app state, no API calls.
 */

export function esc(s) { if (!s) return ''; const d = document.createElement('div'); d.textContent = String(s); return d.innerHTML; }
export function fmtBytes(b) { if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB'; if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB'; return (b / 1e3).toFixed(0) + ' KB'; }
export function fmtNum(n) { if (n >= 1e6) return (n / 1e6).toFixed(1) + 'M'; if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K'; return String(n); }
export function fmtDuration(s) { if (s < 60) return s + 's'; if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`; return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`; }
export function modelIcon(id) { const l = (id || '').toLowerCase(); if (l.includes('llama')) return 'llama'; if (l.includes('mistral')) return 'mistral'; if (l.includes('qwen')) return 'qwen'; if (l.includes('deepseek')) return 'deepseek'; if (l.includes('phi')) return 'phi'; return 'llama'; }

let toastTimer = null;

export function showToast(message, tone = 'success') {
  document.querySelector('.app-toast')?.remove();
  if (toastTimer) clearTimeout(toastTimer);

  const toast = document.createElement('div');
  toast.className = `app-toast ${tone}`;
  toast.setAttribute('role', tone === 'error' ? 'alert' : 'status');
  toast.textContent = String(message || '');
  document.body.appendChild(toast);
  requestAnimationFrame(() => toast.classList.add('visible'));

  toastTimer = setTimeout(() => {
    toast.classList.remove('visible');
    setTimeout(() => toast.remove(), 180);
  }, 3200);
}

export function runConfirmation({
  title,
  message,
  detail = '',
  confirmLabel = 'Confirmar',
  cancelLabel = 'Cancelar',
  pendingLabel = 'Processando...',
  action,
}) {
  document.querySelector('.app-dialog-backdrop')?.remove();

  return new Promise((resolve) => {
    const backdrop = document.createElement('div');
    backdrop.className = 'app-dialog-backdrop';
    backdrop.innerHTML = `
      <section class="app-dialog" role="dialog" aria-modal="true" aria-labelledby="app-dialog-title">
        <div class="app-dialog-icon" aria-hidden="true">!</div>
        <div class="app-dialog-content">
          <h2 id="app-dialog-title"></h2>
          <p class="app-dialog-message"></p>
          <code class="app-dialog-detail"></code>
          <div class="app-dialog-error hidden" role="alert"></div>
        </div>
        <div class="app-dialog-actions">
          <button class="action-btn app-dialog-cancel" type="button"></button>
          <button class="action-btn danger app-dialog-confirm" type="button"></button>
        </div>
      </section>`;

    const titleElement = backdrop.querySelector('#app-dialog-title');
    const messageElement = backdrop.querySelector('.app-dialog-message');
    const detailElement = backdrop.querySelector('.app-dialog-detail');
    const errorElement = backdrop.querySelector('.app-dialog-error');
    const cancelButton = backdrop.querySelector('.app-dialog-cancel');
    const confirmButton = backdrop.querySelector('.app-dialog-confirm');
    titleElement.textContent = title;
    messageElement.textContent = message;
    detailElement.textContent = detail;
    detailElement.classList.toggle('hidden', !detail);
    cancelButton.textContent = cancelLabel;
    confirmButton.textContent = confirmLabel;

    let busy = false;
    const close = (result) => {
      document.removeEventListener('keydown', handleKeydown);
      backdrop.remove();
      resolve(result);
    };
    const handleKeydown = (event) => {
      if (event.key === 'Escape' && !busy) close(false);
    };

    cancelButton.addEventListener('click', () => {
      if (!busy) close(false);
    });
    backdrop.addEventListener('click', (event) => {
      if (event.target === backdrop && !busy) close(false);
    });
    confirmButton.addEventListener('click', async () => {
      if (busy) return;
      busy = true;
      errorElement.classList.add('hidden');
      cancelButton.disabled = true;
      confirmButton.disabled = true;
      confirmButton.textContent = pendingLabel;
      try {
        await action();
        close(true);
      } catch (error) {
        busy = false;
        errorElement.textContent = error instanceof Error ? error.message : String(error);
        errorElement.classList.remove('hidden');
        cancelButton.disabled = false;
        confirmButton.disabled = false;
        confirmButton.textContent = confirmLabel;
        confirmButton.focus();
      }
    });

    document.addEventListener('keydown', handleKeydown);
    document.body.appendChild(backdrop);
    cancelButton.focus();
  });
}
