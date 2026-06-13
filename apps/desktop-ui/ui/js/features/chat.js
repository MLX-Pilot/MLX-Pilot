/* MLX Pilot — Chat & sessions (feature).
 *
 * The chat tab: streaming + non-streaming send, thinking/answer rendering,
 * message DOM helpers, agent message streaming, and the sidebar session
 * history (load/create/switch). Uses core api, markdown and dom helpers.
 */

// === auto-imports (generated — do not edit) ===
import { activeModelId, pushConsoleEntry } from '../../app.js';
import { api, createStreamDecoder } from '../core/api.js';
import { esc, fmtNum } from '../core/dom.js';
import { renderMarkdown } from '../core/markdown.js';
import { state } from '../core/state.js';
import { buildWebAugmentedMessages } from './models.js';
import { ensureRuntimeReadyForModel, renderAgentChatEmptyState, updateAgentWorkspaceSummary } from './runtime.js';
// === end auto-imports ===

  // -- Chat Streaming -----------------------------------------
  export async function sendChatMessage(text) {
    if (!text.trim() || state.isStreaming) return;
    const modelId = activeModelId();
    if (!modelId) { addSystemMsg('Selecione um modelo primeiro.'); return; }
    try {
      ensureRuntimeReadyForModel(modelId);
    } catch (error) {
      addSystemMsg(error.message);
      return;
    }

    addMessage('user', text);
    const input = document.getElementById('chat-input');
    if (input) { input.value = ''; input.style.height = 'auto'; }

    // Remove welcome message if present
    const welcome = document.querySelector('.welcome-message');
    if (welcome) welcome.remove();

    state.messages.push({ role: 'user', content: text });
    const assistantEl = addMessage('assistant', '');
    state.isStreaming = true;
    const outboundMessages = await buildWebAugmentedMessages(text);

    state.streamController = new AbortController();

    const payload = {
      model_id: modelId,
      messages: outboundMessages,
      options: { temperature: 0.2, airllm_enabled: state.airllmEnabled },
    };

    let streamedThinking = '', rawAnswer = '', metrics = {};

    try {
      const res = await fetch(state.daemonUrl + '/chat/stream', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(payload),
        signal: state.streamController.signal,
      });

      if (!res.ok) {
        if (res.status === 404 || res.status === 405) return sendChatNonStreaming(payload, assistantEl);
        throw new Error(`HTTP ${res.status}`);
      }

      const reader = res.body.getReader();
      const decoder = createStreamDecoder();
      let buf = '';

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buf += decoder.decode(value, { stream: true });
        const lines = buf.split('\n');
        buf = lines.pop() || '';
        for (const line of lines) {
          if (!line.trim()) continue;
          let evt;
          try { evt = JSON.parse(line); } catch { continue; }
          if (evt.event === 'status') {
            updateStreamStatus(assistantEl, evt.status);
          } else if (evt.event === 'thinking_delta') {
            streamedThinking += evt.delta || '';
            renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
          } else if (evt.event === 'answer_delta') {
            rawAnswer += evt.delta || '';
            renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
          } else if (evt.event === 'metrics') {
            metrics = { ...metrics, ...evt };
          } else if (evt.event === 'done') {
            metrics = { ...metrics, ...evt };
            addMetrics(assistantEl, metrics);
            const rendered = renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking, finalize: true });
            if (rendered.answer) {
              state.messages.push({ role: 'assistant', content: rendered.answer });
            }
          } else if (evt.event === 'error') {
            throw new Error(evt.message || 'Erro desconhecido');
          }
        }
      }
    } catch (e) {
      if (e.name === 'AbortError') addSystemMsg('Geração interrompida.');
      else { addSystemMsg(`Erro: ${e.message}`); console.error('Chat:', e); }
    } finally {
      state.isStreaming = false;
      state.streamController = null;
    }
  }

  async function sendChatNonStreaming(payload, el = addMessage('assistant', '')) {
    updateStreamStatus(el, 'thinking');
    try {
      const res = await api('/chat', { method: 'POST', body: JSON.stringify(payload) });
      const content = res?.message?.content || res?.final_response || 'Sem resposta.';
      const rendered = renderAssistantOutput(el, { rawAnswer: content, finalize: true });
      if (rendered.answer) {
        state.messages.push({ role: 'assistant', content: rendered.answer });
      }
      if (res?.usage) addMetrics(el, { prompt_tokens: res.usage.prompt_tokens, completion_tokens: res.usage.completion_tokens, total_tokens: res.usage.total_tokens, latency_ms: res.latency_ms });
    } catch (e) {
      updateAnswer(el, `Erro: ${e.message}`);
    }
    state.isStreaming = false;
  }

  // -- Message DOM helpers ------------------------------------
  function addMessage(role, content) {
    const container = document.getElementById('chat-messages');
    if (!container) return null;
    const div = document.createElement('div');
    div.className = `message ${role}-message`;
    const letter = role === 'user' ? 'U' : 'AI';
    const cls = role === 'assistant' ? ' assistant' : '';
    const now = new Date();
    const time = `${String(now.getHours()).padStart(2,'0')}:${String(now.getMinutes()).padStart(2,'0')}`;
    div.innerHTML = `<div class="msg-avatar${cls}">${letter}</div><div class="msg-body"><div class="msg-content markdown-body">${esc(content)}</div><div class="msg-time">${time}</div></div>`;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
    return div;
  }

  export function addSystemMsg(text) {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    const div = document.createElement('div');
    div.style.cssText = 'text-align:center;padding:8px;font-size:12px;color:var(--text-tertiary)';
    div.textContent = text;
    container.appendChild(div);
    container.scrollTop = container.scrollHeight;
  }

  // Load a previous conversation into the Chat view and render its transcript.
  async function loadChatSessionIntoView(sessionId) {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    container.innerHTML = '<div style="text-align:center;padding:24px;font-size:12px;color:var(--text-tertiary)">Carregando conversa...</div>';
    try {
      const messages = await api(`/agent/sessions/${encodeURIComponent(sessionId)}`);
      state.messages = Array.isArray(messages) ? messages : [];
      container.innerHTML = '';
      const visible = state.messages.filter(m => {
        const role = m && m.role;
        return (role === 'user' || role === 'assistant') && String(m.content || '').trim();
      });
      if (!visible.length) {
        addSystemMsg('Esta conversa nao possui mensagens visiveis.');
        return;
      }
      visible.forEach(m => {
        const role = m.role === 'assistant' ? 'assistant' : 'user';
        const el = addMessage(role, role === 'assistant' ? '' : m.content);
        if (role === 'assistant' && el) updateAnswer(el, m.content);
      });
      container.scrollTop = container.scrollHeight;
    } catch (e) {
      container.innerHTML = '';
      addSystemMsg('Erro ao carregar conversa: ' + e.message);
    }
  }

  function updateStreamStatus(el, status) {
    const c = el?.querySelector('.msg-content');
    if (!c) return;
    if (status === 'thinking') c.innerHTML = '<div class="thinking-indicator"><span>Pensando</span><span class="dots"><span>.</span><span>.</span><span>.</span></span></div>';
    else if (status === 'answering') c.innerHTML = '';
  }

  export function updateThinking(el, text) {
    if (!el) return;
    const normalized = String(text || '').trim();
    const body = el.querySelector('.msg-body');
    if (!body) return;
    let toggle = el.querySelector('.msg-thinking-toggle');
    let block = el.querySelector('.msg-thinking');
    if (!normalized) {
      toggle?.remove();
      block?.remove();
      return;
    }
    if (!block) {
      toggle = document.createElement('button');
      toggle.type = 'button';
      toggle.className = 'msg-thinking-toggle';
      toggle.innerHTML = '<span class="thinking-chevron">&#9662;</span><span class="thinking-label">Pensando</span>';
      block = document.createElement('div');
      block.className = 'msg-thinking';
      block.innerHTML = `<div class="thinking-content markdown-body"></div>`;
      toggle.addEventListener('click', () => {
        const collapsed = toggle.classList.toggle('collapsed');
        block.style.display = collapsed ? 'none' : 'block';
      });
      body.insertBefore(block, body.firstChild);
      body.insertBefore(toggle, block);
    }
    block.querySelector('.thinking-content').innerHTML = renderMarkdown(normalized);
  }

  function updateAnswer(el, text) {
    const c = el?.querySelector('.msg-content');
    if (c) c.innerHTML = renderMarkdown(text);
  }

  function joinThinkingSections(...sections) {
    return sections
      .map(section => String(section || '').trim())
      .filter(Boolean)
      .join('\n\n')
      .trim();
  }

  function splitThinkingBlocks(text) {
    const source = String(text || '').replace(/\r\n?/g, '\n');
    if (!source) return { thinking: '', answer: '' };

    const thinkingParts = [];
    const answerParts = [];
    const regex = /<think>([\s\S]*?)<\/think>/gi;
    let cursor = 0;
    let match;

    while ((match = regex.exec(source))) {
      answerParts.push(source.slice(cursor, match.index));
      thinkingParts.push(match[1]);
      cursor = regex.lastIndex;
    }

    const tail = source.slice(cursor);
    const lowerTail = tail.toLowerCase();
    const openIndex = lowerTail.indexOf('<think>');
    if (openIndex >= 0) {
      answerParts.push(tail.slice(0, openIndex));
      thinkingParts.push(tail.slice(openIndex + '<think>'.length));
    } else {
      answerParts.push(tail);
    }

    const answer = answerParts.join('').replace(/<\/?think>/gi, '').trim();
    const thinking = thinkingParts.join('\n\n').replace(/<\/?think>/gi, '').trim();
    return { thinking, answer };
  }

  export function renderAssistantOutput(el, { rawAnswer = '', streamedThinking = '', finalize = false } = {}) {
    const parsed = splitThinkingBlocks(rawAnswer);
    const combinedThinking = joinThinkingSections(streamedThinking, parsed.thinking);
    if (combinedThinking) updateThinking(el, combinedThinking);

    const answerText = parsed.answer;
    const hasThinkMarkup = /<\/?think>/i.test(rawAnswer);
    if (answerText || (finalize && rawAnswer && !hasThinkMarkup)) {
      updateAnswer(el, answerText || rawAnswer);
    }

    return { thinking: combinedThinking, answer: answerText || (!hasThinkMarkup ? String(rawAnswer || '').trim() : '') };
  }

  export async function sendAgentMessageStreaming(payload, assistantEl) {
    let streamedThinking = '';
    let rawAnswer = '';
    let metrics = {};
    pushConsoleEntry('info', 'agent', `Iniciando stream: provider=${payload.provider || '-'} model=${payload.model_id || '-'} session=${payload.session_id || 'nova'}`);

    const res = await fetch(state.daemonUrl + '/agent/stream', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ ...payload, streaming: true }),
    });

    if (!res.ok) {
      if (res.status === 404 || res.status === 405 || res.status === 501) return null;
      throw new Error(`HTTP ${res.status}`);
    }

    const reader = res.body?.getReader?.();
    if (!reader) return null;
    const decoder = createStreamDecoder();
    let buffer = '';

    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      let idx;
      while ((idx = buffer.indexOf('\n')) >= 0) {
        const line = buffer.slice(0, idx).trim();
        buffer = buffer.slice(idx + 1);
        if (!line) continue;
        const evt = JSON.parse(line);
        if (evt.event === 'status') {
          pushConsoleEntry('info', 'agent', `Status: ${evt.status || 'desconhecido'} session=${evt.session_id || payload.session_id || '-'}`);
          updateStreamStatus(assistantEl, evt.status);
        } else if (evt.event === 'thinking_delta') {
          streamedThinking += evt.delta || '';
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'answer_delta') {
          rawAnswer += evt.delta || '';
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'tool_call_started') {
          pushConsoleEntry('info', 'agent-tool', `Iniciada: ${evt.tool || '?'} session=${evt.session_id || payload.session_id || '-'}`);
          streamedThinking = joinThinkingSections(streamedThinking, `Executando tool '${evt.tool || '?'}'...`);
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'tool_call_completed') {
          const preview = evt.message ? `: ${evt.message}` : '';
          pushConsoleEntry('info', 'agent-tool', `Concluida: ${evt.tool || '?'}${preview}`);
          streamedThinking = joinThinkingSections(streamedThinking, `Tool '${evt.tool || '?'}' concluida${preview}`);
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'tool_call_denied') {
          const preview = evt.message ? `: ${evt.message}` : '';
          pushConsoleEntry('warn', 'agent-tool', `Negada: ${evt.tool || '?'}${preview}`);
          streamedThinking = joinThinkingSections(streamedThinking, `Tool '${evt.tool || '?'}' negada${preview}`);
          renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking });
        } else if (evt.event === 'done') {
          metrics = { ...metrics, ...evt };
          pushConsoleEntry('info', 'agent', `Concluido: tokens=${evt.total_tokens ?? '-'} tempo=${evt.latency_ms ?? '-'}ms session=${evt.session_id || payload.session_id || '-'}`);
        } else if (evt.event === 'error') {
          pushConsoleEntry('error', 'agent', evt.message || 'Falha no streaming do agent');
          throw new Error(evt.message || 'Falha no streaming do agent');
        }
      }
    }

    const rendered = renderAssistantOutput(assistantEl, { rawAnswer, streamedThinking, finalize: true });
    if (rendered.answer) {
      state.messages.push({ role: 'assistant', content: rendered.answer });
    }
    if (metrics?.total_tokens) addMetrics(assistantEl, metrics);
    return { answer: rendered.answer, metrics, session_id: metrics?.session_id || payload.session_id || null };
  }

  export function addMetrics(el, m) {
    const body = el?.querySelector('.msg-body');
    if (!body) return;
    const div = document.createElement('div');
    div.className = 'msg-metrics';
    let h = '';
    if (m.total_tokens != null) h += `<span class="metric"><span class="metric-label">Tokens</span> <span class="metric-value">${fmtNum(m.total_tokens)}</span></span>`;
    if (m.latency_ms != null) h += `<span class="metric"><span class="metric-label">Tempo</span> <span class="metric-value">${(m.latency_ms / 1000).toFixed(1)}s</span></span>`;
    if (m.generation_tps != null) h += `<span class="metric"><span class="metric-label">TPS</span> <span class="metric-value">${m.generation_tps.toFixed(1)}</span></span>`;
    if (m.airllm_used) h += `<span class="metric"><span class="metric-label">AIRLLM</span> <span class="metric-value">Ativo</span></span>`;
    if (m.iterations != null) h += `<span class="metric"><span class="metric-label">Iterações</span> <span class="metric-value">${m.iterations}</span></span>`;
    if (m.tool_calls_made != null) h += `<span class="metric"><span class="metric-label">Tools</span> <span class="metric-value">${m.tool_calls_made}</span></span>`;
    div.innerHTML = h;
    body.appendChild(div);
  }

  // -- Sessions (sidebar history) -----------------------------
  export async function loadSessions() {
    try {
      const sessions = await api('/agent/sessions');
      state.agentSessions = Array.isArray(sessions) ? sessions : [];
      if (state.currentSessionId && !state.agentSessions.some(session => session.id === state.currentSessionId)) state.currentSessionId = null;
      if (!state.currentSessionId && state.agentSessions[0]?.id) state.currentSessionId = state.agentSessions[0].id;
      renderSidebarHistory();
    } catch {
      state.agentSessions = [];
      state.currentSessionId = null;
      renderSidebarHistory();
    }
  }

  function renderSidebarHistory() {
    renderSessionCollection(document.getElementById('chat-history'), 'sidebar');
    renderSessionCollection(document.getElementById('agent-session-list'), 'agent');
    updateAgentWorkspaceSummary();
  }

  function renderSessionCollection(container, variant) {
    if (!container) return;
    container.innerHTML = '';
    if (state.agentSessions.length === 0) {
      container.innerHTML = variant === 'agent'
        ? '<div class="agent-empty-copy">Nenhuma sessao ainda</div>'
        : '<div style="padding:8px 12px;font-size:12px;color:var(--text-tertiary)">Nenhuma sessao ainda</div>';
      return;
    }
    state.agentSessions.forEach(s => {
      const item = document.createElement('div');
      const name = s.name || `Sessao ${s.id?.substring(0, 6) || '?'}`;
      const count = s.message_count || 0;
      const active = s.id === state.currentSessionId;
      if (variant === 'agent') {
        item.className = 'agent-session-item' + (active ? ' active' : '');
        item.innerHTML = `
          <div class="agent-session-title">
            <span class="agent-session-name" title="${esc(name)}">${esc(name)}</span>
            <span class="agent-session-count">${fmtNum(count)}</span>
          </div>
          <div class="agent-session-meta">${count} msg${count === 1 ? '' : 's'}</div>`;
      } else {
        item.className = 'history-item' + (active ? ' active' : '');
        item.innerHTML = `<span class="history-icon">&#9679;</span><span class="history-label" title="${esc(name)}">${esc(name)} <span style="opacity:0.5;font-size:11px">(${count})</span></span>`;
      }
      item.addEventListener('click', () => {
        state.currentSessionId = s.id;
        if (variant === 'agent') {
          renderAgentChatEmptyState();
        } else {
          void loadChatSessionIntoView(s.id);
        }
        renderSidebarHistory();
      });
      container.appendChild(item);
    });
  }

  export async function createNewSession() {
    try {
      const session = await api('/agent/sessions', { method: 'POST', body: JSON.stringify({}) });
      if (session?.id) {
        state.currentSessionId = session.id;
        state.messages = [];
        const msgs = document.getElementById('chat-messages');
        if (msgs) msgs.innerHTML = '';
        await loadSessions();
        renderAgentChatEmptyState();
      }
    } catch (e) { console.error('New session failed:', e); }
  }
