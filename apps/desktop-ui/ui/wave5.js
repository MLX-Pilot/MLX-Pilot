/* MLX Pilot — Wave 5 features (Deep Research + Hardware Fit).
 *
 * Self-contained module loaded after app.js + wave1.js. Reuses the daemon
 * URL pattern, API helper, esc(), CSS theme variables, and the generic
 * tab switching in app.js (panels with class `.panel` + id `panel-<name>`
 * are shown/hidden automatically).
 */
(function () {
  'use strict';

  // ── daemon client (mirror wave1.js) ────────────────────────────────────
  const DEFAULT_DAEMON_URL = 'http://127.0.0.1:11435';
  function daemonUrl() {
    return (
      window.__MLX_PILOT_DAEMON_URL__ ||
      (function () {
        try { return localStorage.getItem('mlxPilotDaemonUrl'); } catch (_) { return null; }
      })() ||
      DEFAULT_DAEMON_URL
    );
  }

  async function api(path, opts) {
    opts = opts || {};
    const headers = Object.assign({ 'Content-Type': 'application/json' }, opts.headers || {});
    const res = await fetch(daemonUrl() + path, Object.assign({}, opts, { headers }));
    if (!res.ok) {
      let message = 'HTTP ' + res.status;
      try { const body = await res.json(); if (body && body.error) message = body.error; } catch (_) {}
      throw new Error(message);
    }
    const ct = res.headers.get('content-type') || '';
    return ct.indexOf('application/json') >= 0 ? res.json() : res.text();
  }

  function esc(value) {
    return String(value == null ? '' : value)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }
  function $(id) { return document.getElementById(id); }

  // ── styles ─────────────────────────────────────────────────────────────
  function injectStyles() {
    if ($('wave5-styles')) return;
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
  injectStyles();

  // ═════════════════════════════════════════════════════════════════════════
  // DEEP RESEARCH
  // ═════════════════════════════════════════════════════════════════════════

  const researchJobs = {}; // jobId -> { es, phase, pct }

  // ── Populate model selector from /models/all (unified local+cloud) ────
  async function populateResearchModels() {
    const sel = $('research-model');
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
    const query = $('research-query').value.trim();
    if (!query) return;

    const btn = $('btn-research-start');
    const msg = $('research-start-msg');
    btn.disabled = true;
    btn.textContent = 'Iniciando...';
    msg.textContent = '';

    const payload = {
      query: query,
      max_rounds: parseInt($('research-rounds').value) || 3,
      search_provider: $('research-provider').value || null,
      model_id: $('research-model').value || null,
      category: $('research-category').value || null
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
        $('research-jobs-section').style.display = 'block';
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
    const es = new EventSource(daemonUrl() + '/api/research/stream/' + jobId);
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
    const container = $('research-jobs-list');
    const ids = Object.keys(researchJobs);
    if (!ids.length) { $('research-jobs-section').style.display = 'none'; return; }
    $('research-jobs-section').style.display = 'block';

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
        (!isDone ? '<button class="wave1-btn danger sm" style="margin-top:6px" onclick="window._wave5Cancel(\'' + id + '\')">Cancelar</button>' : '') +
        '</div>';
    }).join('');
  }

  // ── Library ────────────────────────────────────────────────────────────
  async function loadResearchLibrary() {
    var container = $('research-library-list');
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
        return '<div class="wave5-lib-item" onclick="window._wave5ViewReport(\'' + esc(s.id) + '\')">' +
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
    $('research-form-card').style.display = 'none';
    $('research-jobs-section').style.display = 'none';
    $('research-library-list').parentElement.style.display = 'none'; // hide library header too
    var viewer = $('research-report-viewer');
    viewer.style.display = 'block';
    viewer.dataset.sessionId = sessionId;
    $('research-report-iframe').src = daemonUrl() + '/api/research/report/' + sessionId;
    // Scroll to viewer
    viewer.scrollIntoView({ behavior: 'smooth' });
  }

  function backToLibrary() {
    $('research-report-viewer').style.display = 'none';
    $('research-form-card').style.display = '';
    $('research-jobs-section').style.display = Object.keys(researchJobs).length ? 'block' : 'none';
    $('research-library-list').parentElement.style.display = '';
    loadResearchLibrary();
  }

  async function spinoffResearch() {
    var sessionId = $('research-report-viewer').dataset.sessionId;
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
    var sessionId = $('research-report-viewer').dataset.sessionId;
    if (!sessionId || !confirm('Excluir esta pesquisa?')) return;
    try {
      await api('/research/' + sessionId, { method: 'DELETE' });
      backToLibrary();
    } catch (e) { console.error(e); }
  }

  // ═════════════════════════════════════════════════════════════════════════
  // HARDWARE & MODEL FIT
  // ═════════════════════════════════════════════════════════════════════════

  const GPU_PRESETS = [
    { id: 'current', name: 'Hardware detectado / usar atual', vramGb: null, backend: null },
    { id: 'rtx-5090', name: 'NVIDIA GeForce RTX 5090', vramGb: 32, backend: 'cuda' },
    { id: 'rtx-5080', name: 'NVIDIA GeForce RTX 5080', vramGb: 16, backend: 'cuda' },
    { id: 'rtx-5070', name: 'NVIDIA GeForce RTX 5070', vramGb: 12, backend: 'cuda' },
    { id: 'rtx-4090', name: 'NVIDIA GeForce RTX 4090', vramGb: 24, backend: 'cuda' },
    { id: 'rtx-4080', name: 'NVIDIA GeForce RTX 4080', vramGb: 16, backend: 'cuda' },
    { id: 'rtx-4070', name: 'NVIDIA GeForce RTX 4070', vramGb: 12, backend: 'cuda' },
    { id: 'rtx-3090', name: 'NVIDIA GeForce RTX 3090', vramGb: 24, backend: 'cuda' },
    { id: 'rtx-3080', name: 'NVIDIA GeForce RTX 3080', vramGb: 10, backend: 'cuda' },
    { id: 'rtx-3060', name: 'NVIDIA GeForce RTX 3060', vramGb: 12, backend: 'cuda' },
    { id: 'rx-9070-xt', name: 'AMD Radeon RX 9070 XT', vramGb: 16, backend: 'rocm' },
    { id: 'rx-7900-xtx', name: 'AMD Radeon RX 7900 XTX', vramGb: 24, backend: 'rocm' },
    { id: 'rx-7900-xt', name: 'AMD Radeon RX 7900 XT', vramGb: 20, backend: 'rocm' },
    { id: 'rx-7800-xt', name: 'AMD Radeon RX 7800 XT', vramGb: 16, backend: 'rocm' },
    { id: 'rx-7700-xt', name: 'AMD Radeon RX 7700 XT', vramGb: 12, backend: 'rocm' },
    { id: 'custom', name: 'Personalizada', vramGb: null, backend: null }
  ];

  var hwProfile = null;
  var hwSimulation = null;

  function parseOptionalInt(raw) {
    if (raw === '' || raw == null) return null;
    var n = parseInt(raw, 10);
    return Number.isNaN(n) ? null : n;
  }

  function parseOptionalFloat(raw) {
    if (raw === '' || raw == null) return null;
    var n = parseFloat(raw);
    return Number.isNaN(n) ? null : n;
  }

  function updateModeBadge() {
    var badge = $('hw-mode-badge');
    if (!badge) return;
    if (hwSimulation) {
      badge.textContent = 'Modo: Simulado';
      badge.style.background = 'rgba(57, 208, 216, 0.15)';
      badge.style.borderColor = 'var(--cyan,#39d0d8)';
      badge.style.color = 'var(--cyan,#39d0d8)';
      badge.style.fontWeight = '700';
    } else {
      badge.textContent = 'Modo: Detectado';
      badge.style.background = 'var(--bg-deep,#0c0c18)';
      badge.style.borderColor = 'var(--border,#2a2a44)';
      badge.style.color = 'var(--text-tertiary,#8a8aa0)';
      badge.style.fontWeight = '600';
    }
  }

  function onGpuPresetChange() {
    var presetId = $('hw-sim-gpu-preset').value;
    var customBox = $('hw-sim-gpu-custom-box');
    var vramInput = $('hw-sim-vram');
    var backendSelect = $('hw-sim-backend');
    var countInput = $('hw-sim-gpu-count');

    if (presetId === 'custom') {
      customBox.style.display = 'block';
      if (!countInput.value || countInput.value === '0') countInput.value = '1';
    } else if (presetId === 'current') {
      customBox.style.display = 'none';
      $('hw-sim-gpu-custom-name').value = '';
      vramInput.value = '';
      backendSelect.value = '';
    } else {
      customBox.style.display = 'none';
      $('hw-sim-gpu-custom-name').value = '';
      var preset = GPU_PRESETS.find(function (p) { return p.id === presetId; });
      if (preset) {
        if (preset.vramGb != null) vramInput.value = preset.vramGb;
        if (preset.backend) backendSelect.value = preset.backend;
        if (!countInput.value || countInput.value === '0') countInput.value = '1';
      }
    }
  }

  function onRamPresetChange() {
    var ramPreset = $('hw-sim-ram-preset').value;
    var customBox = $('hw-sim-ram-custom-box');
    if (ramPreset === 'custom') {
      customBox.style.display = 'block';
    } else {
      customBox.style.display = 'none';
      $('hw-sim-ram-custom').value = '';
    }
  }

  function onBackendChange() {
    var backend = $('hw-sim-backend').value;
    var countInput = $('hw-sim-gpu-count');
    var vramInput = $('hw-sim-vram');
    if (backend === 'cpu') {
      countInput.value = '0';
      vramInput.value = '';
    } else if (backend && (countInput.value === '0' || !countInput.value)) {
      countInput.value = '1';
    }
  }

  function buildHardwareOverrideParams() {
    if (!hwSimulation) return '';
    var p = '&manual_mode=true';
    if (hwSimulation.gpuName) p += '&manual_gpu_name=' + encodeURIComponent(hwSimulation.gpuName);
    if (hwSimulation.gpuCount != null) p += '&manual_gpu_count=' + hwSimulation.gpuCount;
    if (hwSimulation.vramGb != null) p += '&manual_vram_gb=' + hwSimulation.vramGb;
    if (hwSimulation.ramGb != null) p += '&manual_ram_gb=' + hwSimulation.ramGb;
    if (hwSimulation.backend) p += '&manual_backend=' + encodeURIComponent(hwSimulation.backend);
    if (hwSimulation.ignoreDetectedGpu) p += '&ignore_detected_gpu=true';
    if (hwSimulation.ignoreDetectedRam) p += '&ignore_detected_ram=true';
    return p;
  }

  function readSimulationFromForm() {
    var presetId = $('hw-sim-gpu-preset').value;
    var customName = $('hw-sim-gpu-custom-name').value.trim();
    var gpuCount = parseOptionalInt($('hw-sim-gpu-count').value);
    var vramGb = parseOptionalFloat($('hw-sim-vram').value);
    var ramPreset = $('hw-sim-ram-preset').value;
    var ramGb = null;
    if (ramPreset === 'custom') {
      ramGb = parseOptionalFloat($('hw-sim-ram-custom').value);
    } else if (ramPreset !== 'current') {
      ramGb = parseOptionalFloat(ramPreset);
    }
    var backend = $('hw-sim-backend').value || null;

    var gpuName = null;
    var isPreset = presetId !== 'current' && presetId !== 'custom';
    if (isPreset) {
      var preset = GPU_PRESETS.find(function (p) { return p.id === presetId; });
      if (preset) {
        gpuName = preset.name;
        if (!backend && preset.backend) backend = preset.backend;
        if (vramGb == null && preset.vramGb != null) vramGb = preset.vramGb;
      }
    } else if (presetId === 'custom') {
      gpuName = customName || 'GPU Personalizada';
    }

    if (gpuCount == null) {
      if (backend === 'cpu') gpuCount = 0;
      else if (gpuName || vramGb != null) gpuCount = 1;
    }

    if (gpuCount != null && (gpuCount < 0 || gpuCount > 8)) {
      throw new Error('Quantidade de GPUs deve estar entre 0 e 8.');
    }
    if (gpuCount != null && gpuCount > 0 && (vramGb == null || vramGb <= 0)) {
      throw new Error('Informe VRAM por GPU maior que zero.');
    }
    if (ramPreset === 'custom' && (ramGb == null || ramGb < 4)) {
      throw new Error('RAM simulada personalizada deve ser de pelo menos 4 GB.');
    }
    if (ramGb != null && ramGb < 4) {
      throw new Error('RAM simulada deve ser de pelo menos 4 GB.');
    }
    if (backend === 'cpu' && gpuCount != null && gpuCount > 0) {
      throw new Error('Backend CPU-only exige 0 GPUs.');
    }
    if (gpuCount != null && gpuCount > 0 && !backend) {
      throw new Error('Selecione um backend para simular GPU.');
    }

    var isGpuSimulated = presetId !== 'current' || (gpuCount != null && gpuCount >= 0) || vramGb != null || backend != null;
    var isRamSimulated = ramGb != null;

    if (!isGpuSimulated && !isRamSimulated) {
      return null;
    }

    return {
      gpuName: gpuName,
      gpuCount: gpuCount,
      vramGb: vramGb,
      ramGb: ramGb,
      backend: backend,
      ignoreDetectedGpu: isGpuSimulated,
      ignoreDetectedRam: isRamSimulated
    };
  }

  // ── Scan hardware ──────────────────────────────────────────────────────
  async function scanHardware(fresh) {
    if (fresh) {
      hwSimulation = null;
      updateModeBadge();
    }
    $('hw-cards').innerHTML = '<div class="wave1-empty">Escaneando hardware...</div>';
    try {
      var r = await fetch(daemonUrl() + '/api/hwfit/system?fresh=' + (fresh ? 'true' : 'false'));
      hwProfile = await r.json();
      updateModeBadge();
      renderHardwareCards();
      loadModelRanking();
      $('hw-models-section').style.display = 'block';
    } catch (e) {
      $('hw-cards').innerHTML = '<div class="wave1-empty" style="color:#ff7a8a">Erro ao escanear: ' + esc(e.message) + '</div>';
    }
  }

  // ── Hardware cards ─────────────────────────────────────────────────────
  function renderHardwareCards() {
    if (!hwProfile) return;
    var gpuDetail = (hwProfile.gpus && hwProfile.gpus.length)
      ? hwProfile.gpus.map(function (g) { return esc(g.name) + ' (' + g.vram_gb.toFixed(1) + ' GB ' + esc(g.backend) + ')'; }).join('<br>')
      : 'Nenhuma GPU detectada (CPU-only)';

    $('hw-cards').innerHTML =
      '<div class="wave5-hw-card"><div class="eyebrow">CPU</div><div class="value">' + esc(hwProfile.cpu_name) + '</div><div class="detail">' + hwProfile.cpu_cores + ' núcleos</div></div>' +
      '<div class="wave5-hw-card"><div class="eyebrow">RAM</div><div class="value">' + hwProfile.ram_gb.toFixed(1) + ' GB</div><div class="detail">' + hwProfile.available_ram_gb.toFixed(1) + ' GB disponível</div></div>' +
      '<div class="wave5-hw-card"><div class="eyebrow">GPU</div><div class="value">' + hwProfile.gpu_count + ' GPU' + (hwProfile.gpu_count !== 1 ? 's' : '') + ' · ' + hwProfile.total_vram_gb.toFixed(1) + ' GB VRAM</div><div class="detail">' + gpuDetail + '</div></div>' +
      '<div class="wave5-hw-card"><div class="eyebrow">Backend</div><div class="value" style="text-transform:uppercase">' + esc(hwProfile.primary_backend) + '</div><div class="detail">' + (hwProfile.is_cpu_only ? 'Modo CPU-only' : 'Aceleração GPU disponível') + '</div></div>';
  }

  // ── Model ranking table ────────────────────────────────────────────────
  async function loadModelRanking() {
    var sort = $('hw-sort').value;
    var useCase = $('hw-use-case').value;
    var tbody = $('hw-model-table-body');
    tbody.innerHTML = '<tr><td colspan="8" class="wave1-empty">Ranqueando modelos...</td></tr>';

    try {
      var params = '?sort=' + encodeURIComponent(sort) + '&use_case=' + encodeURIComponent(useCase) + '&fit_only=false' + buildHardwareOverrideParams();
      var r = await fetch(daemonUrl() + '/api/hwfit/models' + params);
      var data = await r.json();
      if (!data || !data.models) return;

      if (data.hardware) {
        hwProfile = data.hardware;
        renderHardwareCards();
      }
      updateModeBadge();

      tbody.innerHTML = data.models.map(function (m) {
        var fitColor = m.fit_level === 'excellent' ? 'var(--green,#3fb950)' :
                       m.fit_level === 'good' ? 'var(--cyan,#39d0d8)' :
                       m.fit_level === 'tight' ? '#d2991d' : '#ff7a8a';
        var badges = (m.badges || []).map(function (b) { return '<span class="wave5-badge">' + esc(b) + '</span>'; }).join('');
        return '<tr style="border-bottom:1px solid var(--border,#2a2a44);cursor:pointer" onclick="window._wave5ShowProfiles(\'' + esc(m.model.id) + '\',' + m.model.params_b + ',\'' + esc(m.model.architecture) + '\',' + (m.model.is_moe || false) + ',' + (m.model.context_length || 4096) + ')">' +
          '<td style="padding:8px"><div style="font-weight:500">' + esc(m.model.name) + '</div><div style="font-size:10px;color:var(--text-tertiary)">' + esc((m.model.id || '').slice(0,40)) + badges + '</div></td>' +
          '<td style="padding:8px;font-size:12px">' + m.model.params_b.toFixed(1) + 'B' + (m.model.is_moe ? ' MoE' : '') + '</td>' +
          '<td style="padding:8px;font-size:11px;font-family:var(--font-mono,monospace)">' + esc(m.recommended_quant) + '</td>' +
          '<td style="padding:8px;font-size:12px">' + m.estimated_vram_gb.toFixed(1) + ' GB</td>' +
          '<td style="padding:8px;font-size:12px">' + m.estimated_tps.toFixed(0) + ' tok/s</td>' +
          '<td style="padding:8px"><span style="color:' + fitColor + ';font-weight:600;font-size:11px;text-transform:uppercase">' + esc(m.fit_level) + '</span></td>' +
          '<td style="padding:8px"><span style="font-weight:700;font-size:14px">' + m.composite_score.toFixed(0) + '</span></td>' +
          '<td style="padding:8px"><button class="wave1-btn ghost sm" style="font-size:10px" onclick="event.stopPropagation();window._wave5Download(\'' + esc(m.model.id) + '\')">Baixar</button></td>' +
          '</tr>';
      }).join('');
    } catch (e) {
      console.error('wave5: model ranking failed', e);
      tbody.innerHTML = '<tr><td colspan="8" class="wave1-empty" style="color:#ff7a8a">Erro ao carregar ranking</td></tr>';
    }
  }

  // ── Serve profiles ─────────────────────────────────────────────────────
  async function showServeProfiles(modelId, paramsB, arch, isMoe, ctxLen) {
    $('hw-profiles-section').style.display = 'block';
    var list = $('hw-profiles-list');
    list.innerHTML = '<div class="wave1-empty">Carregando perfis...</div>';

    try {
      var q = '?model_id=' + encodeURIComponent(modelId) + '&params_b=' + paramsB + '&architecture=' + encodeURIComponent(arch) + '&is_moe=' + isMoe + '&context_length=' + ctxLen + buildHardwareOverrideParams();
      var r = await fetch(daemonUrl() + '/api/hwfit/profiles' + q);
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
    $('hw-profiles-section').scrollIntoView({ behavior: 'smooth' });
  }

  // ── Simulate hardware ──────────────────────────────────────────────────
  async function applySimulatedHardware() {
    try {
      var sim = readSimulationFromForm();
      if (!sim) {
        await resetHardwareSimulation();
        return;
      }
      hwSimulation = sim;
      updateModeBadge();
      $('hw-models-section').style.display = 'block';
      await loadModelRanking();
    } catch (e) {
      console.error('wave5: simulation failed', e);
      alert(e.message || 'Erro ao aplicar simulação');
    }
  }

  async function resetHardwareSimulation() {
    hwSimulation = null;
    $('hw-sim-gpu-preset').value = 'current';
    $('hw-sim-gpu-custom-box').style.display = 'none';
    $('hw-sim-gpu-custom-name').value = '';
    $('hw-sim-gpu-count').value = '1';
    $('hw-sim-vram').value = '';
    $('hw-sim-ram-preset').value = 'current';
    $('hw-sim-ram-custom-box').style.display = 'none';
    $('hw-sim-ram-custom').value = '';
    $('hw-sim-backend').value = '';
    updateModeBadge();
    await scanHardware(false);
  }

  // ── Download model via catalog ─────────────────────────────────────────
  async function downloadModel(modelId) {
    if (!confirm('Baixar ' + modelId + '?')) return;
    try {
      var r = await fetch(daemonUrl() + '/catalog/downloads', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ model_id: modelId, source: 'huggingface' })
      });
      var res = await r.json();
      alert('Download iniciado: ' + (res.job_id || 'ok'));
    } catch (e) { console.error(e); }
  }

  // ═════════════════════════════════════════════════════════════════════════
  // EVENT BINDINGS + INIT
  // ═════════════════════════════════════════════════════════════════════════

  // Expose functions called from inline onclick handlers
  window._wave5Cancel = cancelResearch;
  window._wave5ViewReport = viewResearchReport;
  window._wave5ShowProfiles = showServeProfiles;
  window._wave5Download = downloadModel;

  // Button bindings
  $('btn-research-start').addEventListener('click', startResearch);
  $('btn-refresh-library').addEventListener('click', loadResearchLibrary);
  $('btn-report-back').addEventListener('click', backToLibrary);
  $('btn-report-pdf').addEventListener('click', function () { window.print(); });
  $('btn-report-html').addEventListener('click', function () {
    var sid = $('research-report-viewer').dataset.sessionId;
    if (sid) window.open(daemonUrl() + '/api/research/report/' + sid, '_blank');
  });
  $('btn-report-spinoff').addEventListener('click', spinoffResearch);
  $('btn-report-delete').addEventListener('click', deleteResearch);

  $('btn-hw-scan').addEventListener('click', function () { scanHardware(true); });
  $('btn-hw-sim-apply').addEventListener('click', applySimulatedHardware);
  $('btn-hw-sim-reset').addEventListener('click', resetHardwareSimulation);
  $('hw-sim-gpu-preset').addEventListener('change', onGpuPresetChange);
  $('hw-sim-ram-preset').addEventListener('change', onRamPresetChange);
  $('hw-sim-backend').addEventListener('change', onBackendChange);
  $('hw-sort').addEventListener('change', loadModelRanking);
  $('hw-use-case').addEventListener('change', loadModelRanking);

  // Tab-aware init: populate models + scan hardware when respective tab opens.
  // We hook into the existing switchTab by listening for clicks on tabs.
  document.querySelectorAll('.tab[data-panel]').forEach(function (tab) {
    tab.addEventListener('click', function () {
      var panel = tab.dataset.panel;
      if (panel === 'research') {
        populateResearchModels();
        loadResearchLibrary();
        // Also refresh the model selector since models may have changed
        setTimeout(populateResearchModels, 500);
      }
      if (panel === 'hardware') {
        if (!hwProfile) scanHardware(false);
        else { updateModeBadge(); renderHardwareCards(); loadModelRanking(); }
      }
    });
  });

  // Also handle keyboard shortcut tab switches (app.js update)
  // The keyboard listener is in app.js — we just need to add our new tabs.
  // app.js uses: switchTab(['chat','discover','agent','ai-interaction','console','historico','memoria','comparar','research','hardware','settings'][n-1])
  // This is handled by the app.js update below.

  console.log('Wave 5: Deep Research + Hardware Fit ready');
})();
