/* MLX Pilot — UI event bindings & AI-visual panel (feature/bootstrap).
 *
 * Composition-root wiring for the agent-chat, audit, settings, sidebar, daemon
 * URL, toggle chips and chat inputs, plus the AI-visual canvas/particles and
 * the global keyboard shortcuts. Listeners run at module load, after the
 * feature functions they reference are defined.
 */

// === auto-imports (generated — do not edit) ===
import { activeAgentModelId, activeModelId, inferModelProvider, pushConsoleEntry } from '../../app.js';
import { api } from '../core/api.js';
import { esc } from '../core/dom.js';
import { renderMarkdown } from '../core/markdown.js';
import { switchTab } from '../core/router.js';
import { state } from '../core/state.js';
import { runAgentShortcut } from './agent-shortcuts.js';
import { loadAudit, loadChannels, renderAuditFeed } from './agent.js';
import { addMetrics, createNewSession, loadSessions, renderAssistantOutput, sendAgentMessageStreaming, sendChatMessage } from './chat.js';
import { clearConsole, consoleText, loadConsoleSnapshot, saveEnvironment } from './console.js';
import { bootSequence, ensureAgentChatReady, ensureRuntimeReadyForModel, renderAgentChatEmptyState, resizeTextArea, updateAgentWorkspaceSummary } from './runtime.js';
import { saveDaemonConfig } from './settings.js';
// === end auto-imports ===

  // -- Agent Chat ---------------------------------------------
  const agentInput = document.getElementById('agent-command-input');
  const agentSendBtn = document.getElementById('agent-send-btn');

  document.querySelectorAll('.agent-prompt-card').forEach(card => {
    card.addEventListener('click', () => {
      if (!agentInput) return;
      state.pendingAgentShortcut = card.dataset.agentShortcut || null;
      agentInput.value = card.dataset.agentPrompt || '';
      resizeTextArea(agentInput, 220);
      agentInput.focus();
    });
  });

  agentInput?.addEventListener('input', () => resizeTextArea(agentInput, 220));
  agentSendBtn?.addEventListener('click', async () => {
    if (!agentInput?.value.trim()) return;
    const msg = agentInput.value.trim();
    const shortcut = state.pendingAgentShortcut;
    state.pendingAgentShortcut = null;
    agentInput.value = '';
    resizeTextArea(agentInput, 220);

    const box = ensureAgentChatReady();
    if (!box) return;

    box.insertAdjacentHTML('beforeend', `<div class="message user-message"><div class="msg-avatar">U</div><div class="msg-body"><div class="msg-content">${esc(msg)}</div></div></div>`);
    const agDiv = document.createElement('div');
    agDiv.className = 'message assistant-message';
    agDiv.innerHTML = `<div class="msg-avatar assistant">AG</div><div class="msg-body"><div class="msg-content markdown-body"><div class="thinking-indicator"><span>Processando</span><span class="dots"><span>.</span><span>.</span><span>.</span></span></div></div></div>`;
    box.appendChild(agDiv);
    box.scrollTop = box.scrollHeight;

    try {
      if (shortcut) {
        await runAgentShortcut(shortcut, agDiv);
        return;
      }
      const modelId = activeAgentModelId();
      if (!modelId) throw new Error('Selecione um modelo valido antes de executar o agent.');
      ensureRuntimeReadyForModel(modelId, state.agentConfig?.provider || 'ollama');
      const payload = {
        session_id: state.currentSessionId,
        message: msg,
        provider: state.agentConfig?.provider || inferModelProvider(modelId, 'ollama') || 'ollama',
        model_id: modelId,
        execution_mode: state.agentConfig?.execution_mode || 'full',
        approval_mode: state.agentConfig?.approval_mode || 'ask',
        max_iterations: 25,
      };
      pushConsoleEntry('info', 'agent', `Enviando mensagem: provider=${payload.provider} model=${payload.model_id} session=${payload.session_id || 'nova'}`);
      let res = null;
      const streamed = await sendAgentMessageStreaming(payload, agDiv);
      if (!streamed) {
        pushConsoleEntry('info', 'agent', 'Stream indisponivel; usando /agent/run');
        res = await api('/agent/run', { method: 'POST', body: JSON.stringify(payload) });
      } else if (streamed.session_id) {
        state.currentSessionId = streamed.session_id;
        await loadSessions();
      }
      if (res?.session_id) {
        state.currentSessionId = res.session_id;
        await loadSessions();
      } else {
        updateAgentWorkspaceSummary();
      }
      if (res) {
        const content = res?.final_response || 'Sem resposta.';
        renderAssistantOutput(agDiv, { rawAnswer: content, finalize: true });
        if (res?.total_tokens) addMetrics(agDiv, res);
        pushConsoleEntry('info', 'agent', `Run concluido: tokens=${res.total_tokens ?? '-'} tempo=${res.latency_ms ?? '-'}ms session=${res.session_id || '-'}`);
      }
    } catch (e) {
      pushConsoleEntry('error', 'agent', e.message);
      agDiv.querySelector('.msg-content').innerHTML = `<span style="color:var(--rose)">Erro: ${esc(e.message)}</span>`;
    }
    box.scrollTop = box.scrollHeight;
  });
  agentInput?.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      agentSendBtn?.click();
    }
  });

  // -- Audit Refresh ------------------------------------------
  document.getElementById('refresh-audit')?.addEventListener('click', async () => {
    const btn = document.getElementById('refresh-audit');
    btn.textContent = 'Carregando...';
    btn.disabled = true;
    await loadAudit();
    btn.textContent = 'Atualizar';
    btn.disabled = false;
  });

  document.querySelectorAll('.audit-filter-pill').forEach(pill => {
    pill.addEventListener('click', () => {
      document.querySelectorAll('.audit-filter-pill').forEach(p => p.classList.remove('active'));
      pill.classList.add('active');
      state.auditFilter = pill.dataset.auditFilter || 'all';
      renderAuditFeed();
    });
  });

  document.getElementById('refresh-console')?.addEventListener('click', () => loadConsoleSnapshot());
  document.getElementById('clear-console')?.addEventListener('click', () => clearConsole());
  document.getElementById('copy-console')?.addEventListener('click', async () => {
    try {
      await navigator.clipboard.writeText(consoleText());
      pushConsoleEntry('info', 'console', 'Conteudo copiado para a area de transferencia');
    } catch (error) {
      pushConsoleEntry('error', 'console', `Falha ao copiar console: ${error.message}`);
    }
  });

  // -- Settings Save ------------------------------------------
  document.getElementById('save-settings-btn')?.addEventListener('click', async () => {
    const ok = await saveDaemonConfig();
    const btn = document.getElementById('save-settings-btn');
    btn.textContent = ok ? 'Configurações salvas!' : 'Erro ao salvar';
    setTimeout(() => { btn.textContent = 'Aplicar Todas as Configurações'; }, 2000);
  });

  document.getElementById('save-env-btn')?.addEventListener('click', saveEnvironment);

  // Range input live value
  document.getElementById('set-airllm-threshold')?.addEventListener('input', (e) => {
    const tv = document.getElementById('set-airllm-threshold-val');
    if (tv) tv.textContent = e.target.value + '%';
  });

  // -- Sidebar: New Chat --------------------------------------
  document.getElementById('btn-new-chat')?.addEventListener('click', () => {
    state.messages = [];
    state.currentSessionId = null;
    const msgs = document.getElementById('chat-messages');
    if (msgs) msgs.innerHTML = '<div class="welcome-message" style="text-align:center;padding:60px 20px;max-width:500px;margin:0 auto"><h3 style="font-family:var(--font-heading);font-size:20px;margin-bottom:8px">MLX Pilot Chat</h3><p style="font-size:14px;color:var(--text-tertiary)">Selecione um modelo e envie sua mensagem.</p></div>';
    createNewSession();
    switchTab('chat');
  });

  document.getElementById('topbar-brand')?.addEventListener('click', () => switchTab('chat'));

  // -- Daemon URL ---------------------------------------------
  document.getElementById('save-url')?.addEventListener('click', () => {
    const input = document.getElementById('daemon-url');
    if (input?.value.trim()) {
      state.daemonUrl = input.value.trim().replace(/\/+$/, '');
      localStorage.setItem('mlxPilotDaemonUrl', state.daemonUrl);
      const sidebarUrl = document.getElementById('sidebar-daemon-url');
      if (sidebarUrl) sidebarUrl.textContent = `Daemon ${state.daemonUrl.replace(/^https?:\/\//, '')}`;
      bootSequence();
    }
  });

  // -- Toggle Chips -------------------------------------------
  document.querySelectorAll('.toggle-chip').forEach(chip => {
    if (chip.id === 'web-search-toggle') state.webSearchEnabled = chip.classList.contains('active');
    if (chip.id === 'airllm-toggle') state.airllmEnabled = chip.classList.contains('active');
    chip.addEventListener('click', () => {
      chip.classList.toggle('active');
      if (chip.id === 'web-search-toggle') state.webSearchEnabled = chip.classList.contains('active');
      if (chip.id === 'airllm-toggle') state.airllmEnabled = chip.classList.contains('active');
    });
  });

  // -- Chat Input ---------------------------------------------
  const chatInput = document.getElementById('chat-input');
  chatInput?.addEventListener('input', () => resizeTextArea(chatInput, 160));
  chatInput?.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendChatMessage(chatInput.value); } });
  document.getElementById('send-btn')?.addEventListener('click', () => sendChatMessage(chatInput?.value || ''));

  // -- Radio Card generic -------------------------------------
  document.querySelectorAll('.radio-card input[type="radio"]').forEach(radio => {
    radio.addEventListener('change', () => {
      document.querySelectorAll(`input[name="${radio.name}"]`).forEach(r => r.closest('.radio-card')?.classList.remove('selected'));
      radio.closest('.radio-card')?.classList.add('selected');
    });
  });

  // -- Code Copy ----------------------------------------------
  document.addEventListener('click', (e) => {
    const btn = e.target.closest('.code-copy');
    if (!btn) return;
    const code = btn.closest('.code-block')?.querySelector('code');
    if (code) { navigator.clipboard.writeText(code.textContent).then(() => { btn.textContent = 'Copiado!'; setTimeout(() => { btn.textContent = 'Copiar'; }, 2000); }); }
  });

  // -- AI Canvas Particles ------------------------------------
  let aiCanvas, aiCtx, aiAnimFrame, particles = [];
  export function initAICanvas() {
    aiCanvas = document.getElementById('ai-canvas');
    if (!aiCanvas) return;
    aiCtx = aiCanvas.getContext('2d');
    const r = aiCanvas.parentElement.getBoundingClientRect();
    aiCanvas.width = r.width; aiCanvas.height = r.height;
    if (!particles.length) {
      const n = Math.min(80, Math.floor(window.innerWidth / 15));
      for (let i = 0; i < n; i++) particles.push({ x: Math.random() * aiCanvas.width, y: Math.random() * aiCanvas.height, vx: (Math.random() - 0.5) * 0.3, vy: (Math.random() - 0.5) * 0.3, size: Math.random() * 2 + 0.5, opacity: Math.random() * 0.4 + 0.1, hue: Math.random() > 0.5 ? 190 : 260 });
    }
    if (!aiAnimFrame) animParticles();
  }
  function animParticles() {
    if (!aiCtx || !aiCanvas) return;
    aiCtx.clearRect(0, 0, aiCanvas.width, aiCanvas.height);
    for (let i = 0; i < particles.length; i++) {
      for (let j = i + 1; j < particles.length; j++) {
        const dx = particles[i].x - particles[j].x, dy = particles[i].y - particles[j].y, d = Math.sqrt(dx * dx + dy * dy);
        if (d < 120) { aiCtx.beginPath(); aiCtx.moveTo(particles[i].x, particles[i].y); aiCtx.lineTo(particles[j].x, particles[j].y); aiCtx.strokeStyle = `rgba(0,212,255,${(1 - d / 120) * 0.08})`; aiCtx.lineWidth = 0.5; aiCtx.stroke(); }
      }
    }
    particles.forEach(p => {
      p.x += p.vx; p.y += p.vy;
      if (p.x < 0) p.x = aiCanvas.width; if (p.x > aiCanvas.width) p.x = 0;
      if (p.y < 0) p.y = aiCanvas.height; if (p.y > aiCanvas.height) p.y = 0;
      aiCtx.beginPath(); aiCtx.arc(p.x, p.y, p.size, 0, Math.PI * 2);
      aiCtx.fillStyle = `hsla(${p.hue},80%,60%,${p.opacity})`; aiCtx.fill();
      aiCtx.beginPath(); aiCtx.arc(p.x, p.y, p.size * 3, 0, Math.PI * 2);
      aiCtx.fillStyle = `hsla(${p.hue},80%,60%,${p.opacity * 0.15})`; aiCtx.fill();
    });
    aiAnimFrame = requestAnimationFrame(animParticles);
  }

  // -- Atmosphere ---------------------------------------------
  const atmCanvas = document.getElementById('atmosphere');
  if (atmCanvas) {
    const ctx = atmCanvas.getContext('2d');
    let ap = [];
    function resizeA() { atmCanvas.width = window.innerWidth; atmCanvas.height = window.innerHeight; }
    function mkA() { ap = []; for (let i = 0; i < Math.min(40, Math.floor(window.innerWidth / 30)); i++) ap.push({ x: Math.random() * atmCanvas.width, y: Math.random() * atmCanvas.height, vx: (Math.random() - 0.5) * 0.15, vy: (Math.random() - 0.5) * 0.1, size: Math.random() * 1.2 + 0.3, opacity: Math.random() * 0.15 + 0.03 }); }
    function loopA() { ctx.clearRect(0, 0, atmCanvas.width, atmCanvas.height); ap.forEach(p => { p.x += p.vx; p.y += p.vy; if (p.x < 0) p.x = atmCanvas.width; if (p.x > atmCanvas.width) p.x = 0; if (p.y < 0) p.y = atmCanvas.height; if (p.y > atmCanvas.height) p.y = 0; ctx.beginPath(); ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2); ctx.fillStyle = `rgba(0,212,255,${p.opacity})`; ctx.fill(); }); requestAnimationFrame(loopA); }
    resizeA(); mkA(); loopA();
    window.addEventListener('resize', () => { resizeA(); mkA(); });
  }
  // -- Agent: New Session / Export -----------------------------
  document.getElementById('btn-new-session')?.addEventListener('click', async () => {
    try {
      const session = await api('/agent/sessions', { method: 'POST', body: JSON.stringify({ name: '' }) });
      if (session?.id) {
        state.currentSessionId = session.id;
        await loadSessions();
        renderAgentChatEmptyState();
        updateAgentWorkspaceSummary();
        agentInput?.focus();
      }
    } catch (e) { alert('Erro: ' + e.message); }
  });

  document.getElementById('btn-export-session')?.addEventListener('click', () => {
    if (!state.currentSessionId) { alert('Nenhuma sessão selecionada'); return; }
    window.open(state.daemonUrl + '/agent/sessions/' + state.currentSessionId + '/export', '_blank');
  });

  // -- Agent: New Channel -------------------------------------
  document.getElementById('btn-new-channel')?.addEventListener('click', async () => {
    const channelId = prompt('Nome/ID do channel (ex: whatsapp, slack, http):');
    if (!channelId) return;
    try {
      await api('/agent/channels/upsert', {
        method: 'POST',
        headers: { 'x-channel-protocol-version': 'v1' },
        body: JSON.stringify({ channel: channelId, enabled: true, accounts: [] }),
      });
      loadChannels();
    } catch (e) { alert('Erro: ' + e.message); }
  });

  // -- AI Visual Panel ----------------------------------------
  const aiInput = document.getElementById('ai-input');
  const aiSendBtn = document.getElementById('ai-send-btn');

  async function renderAIVisual(prompt) {
    if (!prompt?.trim()) return;
    // Show loading state on the canvas overlay
    const overlay = document.querySelector('.ai-overlay');
    let resultEl = overlay?.querySelector('.ai-result');
    if (!resultEl) {
      resultEl = document.createElement('div');
      resultEl.className = 'ai-result';
      resultEl.style.cssText = 'margin-top:20px;padding:16px 20px;background:rgba(10,14,23,0.8);backdrop-filter:blur(16px);border:1px solid var(--border);border-radius:var(--r-lg);text-align:left;max-height:200px;overflow-y:auto;';
      overlay?.appendChild(resultEl);
    }
    resultEl.innerHTML = '<div class="thinking-indicator"><span>Renderizando</span><span class="dots"><span>.</span><span>.</span><span>.</span></span></div>';

    // Send to daemon chat for scene description
    const modelId = activeModelId();
    if (modelId) {
      try {
        const msgs = [{ role: 'user', content: prompt }];
        const res = await api('/chat', {
          method: 'POST',
          body: JSON.stringify({ model_id: modelId, messages: msgs, options: { temperature: 0.7 } }),
        });
        const content = res?.message?.content || 'Sem resposta.';
        resultEl.innerHTML = renderMarkdown(content);
      } catch (e) {
        // If no model, show a local visual response
        resultEl.innerHTML = renderMarkdown(`**Cena Visual:** ${prompt}\n\n*Conecte-se ao daemon para respostas reais do modelo.*`);
      }
    } else {
      resultEl.innerHTML = renderMarkdown(`**Cena Visual:** ${prompt}\n\n*Selecione um modelo para obter respostas do daemon.*`);
    }

    // Trigger particle burst effect
    triggerParticleBurst();
  }

  function triggerParticleBurst() {
    if (!particles.length || !aiCanvas) return;
    const cx = aiCanvas.width / 2, cy = aiCanvas.height / 2;
    particles.forEach(p => {
      const angle = Math.random() * Math.PI * 2;
      p.vx = Math.cos(angle) * (Math.random() * 1.5 + 0.5);
      p.vy = Math.sin(angle) * (Math.random() * 1.5 + 0.5);
      p.x = cx + (Math.random() - 0.5) * 50;
      p.y = cy + (Math.random() - 0.5) * 50;
      p.hue = Math.random() > 0.5 ? 190 : 260;
      p.opacity = Math.random() * 0.6 + 0.2;
    });
    // Slowly return to ambient speeds
    setTimeout(() => {
      particles.forEach(p => { p.vx *= 0.15; p.vy *= 0.15; p.opacity = Math.min(p.opacity, 0.4); });
    }, 2000);
  }

  aiSendBtn?.addEventListener('click', () => renderAIVisual(aiInput?.value));
  aiInput?.addEventListener('keydown', (e) => { if (e.key === 'Enter') renderAIVisual(aiInput.value); });

  // Example buttons -> fill input and render
  document.querySelectorAll('.example-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      const prompt = btn.dataset.prompt || btn.textContent;
      if (aiInput) aiInput.value = prompt;
      renderAIVisual(prompt);
    });
  });

  // -- Keyboard -----------------------------------------------
  document.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'k') { e.preventDefault(); document.getElementById('model-menu')?.classList.toggle('hidden'); }
    if (e.key === 'Escape') document.getElementById('model-menu')?.classList.add('hidden');
    if (!e.ctrlKey && !e.metaKey && !e.altKey && !['INPUT', 'TEXTAREA'].includes(document.activeElement?.tagName)) {
      const n = parseInt(e.key);
      var tabs = ['chat', 'discover', 'agent', 'ai-interaction', 'console', 'historico', 'memoria', 'comparar', 'research', 'hardware', 'settings'];
      if (n >= 1 && n <= 9) switchTab(tabs[n - 1]);
      else if (n === 0) switchTab('settings');
    }
    if ((e.ctrlKey || e.metaKey) && e.key === '.') state.streamController?.abort();
  });
