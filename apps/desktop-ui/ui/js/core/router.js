/* MLX Pilot — Tab router (core).
 *
 * switchTab() activates a panel + its tab and triggers the per-tab loaders,
 * plus the tab / agent-view / config-subtab / model-picker / discover-subtab
 * click wiring. Imported by every feature that navigates between panels.
 */

// === auto-imports (generated — do not edit) ===
import { ensureAgentCompatibleModel, syncShellLayout, updateAgentWorkspaceSummary } from '../../app.js';
import { state } from './state.js';
import { loadAudit } from '../features/agent.js';
import { loadConsoleSnapshot } from '../features/console.js';
import { invalidateModels, loadDownloads, refreshModelsInBackground, renderModelPicker, searchCatalog, showInstalledModels } from '../features/models.js';
import { initAICanvas } from '../features/ui-bindings.js';
// === end auto-imports ===

  // -- Tab Navigation -----------------------------------------
  export function switchTab(target) {

    state.activePanel = target;
    document.querySelectorAll('.tab').forEach(t => { t.classList.remove('active'); t.setAttribute('aria-selected', 'false'); });
    document.querySelectorAll('.panel').forEach(p => p.classList.remove('active'));
    const tab = document.querySelector(`[data-panel="${target}"]`);
    const panel = document.getElementById(`panel-${target}`);
    if (tab) { tab.classList.add('active'); tab.setAttribute('aria-selected', 'true'); }
    if (panel) panel.classList.add('active');
    syncShellLayout(target);
    renderModelPicker();

    if (target === 'discover') {
      searchCatalog('llama');
      void loadDownloads();
      if (state.activeDiscoverTab === 'installed') showInstalledModels();
    }
    if (target === 'agent') {
      void ensureAgentCompatibleModel({ persist: true });
      updateAgentWorkspaceSummary();
    }
    if (target === 'ai-interaction') initAICanvas();
    if (target === 'console') void loadConsoleSnapshot();
  }

  document.querySelectorAll('.tab').forEach(tab => tab.addEventListener('click', () => switchTab(tab.dataset.panel)));

  document.querySelectorAll('.agent-view-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.agent-view-tab').forEach(t => {
        t.classList.remove('active');
        t.setAttribute('aria-selected', 'false');
      });
      document.querySelectorAll('.agent-view').forEach(view => {
        view.classList.remove('active');
        view.style.display = 'none';
      });
      tab.classList.add('active');
      tab.setAttribute('aria-selected', 'true');
      const view = document.getElementById(`agent-view-${tab.dataset.agentView}`);
      if (view) {
        view.classList.add('active');
        view.style.display = 'block';
      }
      if (tab.dataset.agentView === 'config') loadAudit();
      updateAgentWorkspaceSummary();
    });
  });

  // -- Config Sub-tabs ----------------------------------------
  document.querySelectorAll('.agent-config-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.agent-config-tab').forEach(t => {
        t.classList.remove('active');
        t.setAttribute('aria-selected', 'false');
      });
      document.querySelectorAll('.agent-config-pane').forEach(p => p.classList.remove('active'));
      tab.classList.add('active');
      tab.setAttribute('aria-selected', 'true');
      const pane = document.querySelector(`[data-config-pane="${tab.dataset.configSection}"]`);
      if (pane) pane.classList.add('active');
      if (tab.dataset.configSection === 'observability') loadAudit();
    });
  });

  // -- Model Picker -------------------------------------------
  document.getElementById('model-trigger')?.addEventListener('click', (e) => {
    e.stopPropagation();
    document.getElementById('model-menu')?.classList.toggle('hidden');
  });
  document.addEventListener('click', () => document.getElementById('model-menu')?.classList.add('hidden'));

  // -- Discover Sub-tabs --------------------------------------
  document.querySelectorAll('.discover-tab').forEach(tab => {
    tab.addEventListener('click', () => {
      document.querySelectorAll('.discover-tab').forEach(t => t.classList.remove('active'));
      tab.classList.add('active');
      const d = tab.dataset.dtab;
      state.activeDiscoverTab = d;
      document.getElementById('dtab-catalog').style.display = d === 'catalog' ? 'block' : 'none';
      document.getElementById('dtab-installed').style.display = d === 'installed' ? 'block' : 'none';
      if (d === 'installed') showInstalledModels();
      if (d === 'catalog') void loadDownloads();
    });
  });

  // Refresh installed models
  document.getElementById('refresh-installed')?.addEventListener('click', () => {
    invalidateModels();
    refreshModelsInBackground();
  });
