/* MLX Pilot — Agent shortcut commands (feature).
 *
 * Builds the natural-language agent shortcut responses (status, channels,
 * audit summaries) shown in the agent chat, plus the catalog-search debounce
 * input binding it sits next to. Imports daemon/state/dom helpers from core.
 */

// === auto-imports (generated — do not edit) ===
import { activeAgentModelId, pushConsoleEntry, updateAgentWorkspaceSummary } from '../../app.js';
import { api } from '../core/api.js';
import { state } from '../core/state.js';
import { renderAssistantOutput, updateThinking } from './chat.js';
import { searchCatalog } from './models.js';
// === end auto-imports ===

  // -- Catalog Search -----------------------------------------
  let searchTimeout;
  document.getElementById('catalog-search')?.addEventListener('input', (e) => {
    clearTimeout(searchTimeout);
    searchTimeout = setTimeout(() => { if (e.target.value.trim().length >= 2) searchCatalog(e.target.value.trim()); }, 500);
  });

  function enabledNames(items, key = 'name') {
    return (Array.isArray(items) ? items : [])
      .filter(item => item?.enabled !== false && item?.active !== false)
      .map(item => item?.[key] || item?.id || item?.plugin_id || item?.name)
      .filter(Boolean);
  }

  function disabledNames(items, key = 'name') {
    return (Array.isArray(items) ? items : [])
      .filter(item => item?.enabled === false || item?.active === false)
      .map(item => item?.[key] || item?.id || item?.plugin_id || item?.name)
      .filter(Boolean);
  }

  async function loadAgentShortcutSnapshot() {
    const read = async (path, fallback) => {
      try { return await api(path); } catch { return fallback; }
    };
    const [config, tools, channels, audit, plugins, skills] = await Promise.all([
      read('/agent/config', state.agentConfig || {}),
      read('/agent/tools', state.tools || []),
      read('/agent/channels', state.channels || []),
      read('/agent/audit?limit=30', { entries: state.auditEntries || [] }),
      read('/agent/plugins', state.plugins || []),
      read('/agent/skills/check', { skills: state.skills || [] }),
    ]);
    return {
      config: config || {},
      tools: Array.isArray(tools) ? tools : [],
      channels: Array.isArray(channels) ? channels : [],
      auditEntries: Array.isArray(audit?.entries) ? audit.entries : [],
      plugins: Array.isArray(plugins) ? plugins : [],
      skills: Array.isArray(skills?.skills) ? skills.skills : [],
    };
  }

  function channelActionItems(channels) {
    const actions = [];
    (Array.isArray(channels) ? channels : []).forEach(channel => {
      const id = channel.channel_id || channel.id || channel.name || 'channel';
      const accounts = Array.isArray(channel.accounts) ? channel.accounts : [];
      if (!accounts.length) {
        actions.push(`${id}: sem conta configurada`);
        return;
      }
      accounts.forEach(account => {
        const accountId = account.account_id || account.id || 'default';
        const connected = account.status === 'connected' || account.enabled === true;
        if (!connected) actions.push(`${id}/${accountId}: desconectado`);
      });
    });
    return actions;
  }

  function riskyAuditItems(entries) {
    return (Array.isArray(entries) ? entries : []).filter(entry => {
      const text = `${entry.status || ''} ${entry.event_type || ''} ${entry.summary || ''}`.toLowerCase();
      return /denied|failed|error|panic|block|risco|falha|negad/.test(text);
    });
  }

  function formatBulletList(items, emptyText) {
    return items.length ? items.map(item => `- ${item}`).join('\n') : `- ${emptyText}`;
  }

  function buildAgentShortcutResponse(kind, snapshot) {
    const cfg = snapshot.config || {};
    const agentCfg = cfg.agent || cfg;
    const provider = agentCfg.provider || state.agentConfig?.provider || 'ollama';
    const model = activeAgentModelId() || agentCfg.model_id || agentCfg.model || '-';
    const execution = agentCfg.execution_mode || 'full';
    const approval = agentCfg.approval_mode || 'ask';
    const enabledTools = enabledNames(snapshot.tools);
    const disabledTools = disabledNames(snapshot.tools);
    const activePlugins = enabledNames(snapshot.plugins, 'id');
    const inactivePlugins = disabledNames(snapshot.plugins, 'id');
    const activeSkills = enabledNames(snapshot.skills);
    const inactiveSkills = disabledNames(snapshot.skills);

    if (kind === 'runtime') {
      const suggestions = [];
      if (!enabledTools.includes('exec')) suggestions.push('`exec` esta desativada; mantenha assim para uso seguro ou habilite apenas quando precisar executar comandos.');
      if (!enabledTools.includes('grep')) suggestions.push('Habilite `grep` para auditorias de codigo mais precisas.');
      if (approval === 'deny') suggestions.push('Approval em `deny` bloqueia acoes operacionais; use `ask` para fluxo assistido.');
      if (!suggestions.length) suggestions.push('Runtime coerente para operacao local: ferramentas principais ativas e aprovacao assistida.');
      return [
        '## Runtime e politicas',
        `- Provider: ${provider}`,
        `- Modelo: ${model}`,
        `- Execucao: ${execution}`,
        `- Approval: ${approval}`,
        `- Tools ativas: ${enabledTools.join(', ') || 'nenhuma'}`,
        `- Tools desativadas: ${disabledTools.join(', ') || 'nenhuma'}`,
        '',
        '## Ajustes sugeridos',
        formatBulletList(suggestions, 'Nenhum ajuste imediato.'),
      ].join('\n');
    }

    if (kind === 'integrations') {
      const channelActions = channelActionItems(snapshot.channels);
      const pluginActions = inactivePlugins.map(name => `${name}: plugin desativado`);
      const skillActions = inactiveSkills.map(name => `${name}: skill inativa ou inelegivel`);
      return [
        '## Integracoes ativas',
        `- Channels encontrados: ${snapshot.channels.length}`,
        `- Plugins ativos: ${activePlugins.join(', ') || 'nenhum'}`,
        `- Skills ativas: ${activeSkills.join(', ') || 'nenhuma'}`,
        '',
        '## Acao imediata',
        formatBulletList([...channelActions, ...pluginActions, ...skillActions], 'Nenhuma acao imediata detectada.'),
      ].join('\n');
    }

    const risky = riskyAuditItems(snapshot.auditEntries);
    const recent = snapshot.auditEntries.slice(0, 5).map(entry => {
      const tool = entry.tool_name ? ` tool=${entry.tool_name}` : '';
      return `${entry.event_type || 'event'}${tool}: ${entry.summary || entry.status || 'sem resumo'}`;
    });
    return [
      '## Riscos recentes',
      `- Eventos analisados: ${snapshot.auditEntries.length}`,
      `- Eventos com risco: ${risky.length}`,
      '',
      '## Itens de atencao',
      formatBulletList(risky.slice(0, 6).map(entry => `${entry.event_type || 'event'}: ${entry.summary || entry.status || 'sem resumo'}`), 'Nenhum evento recente com risco operacional evidente.'),
      '',
      '## Ultimos eventos',
      formatBulletList(recent, 'Sem eventos de auditoria recentes.'),
    ].join('\n');
  }

  export async function runAgentShortcut(kind, assistantEl) {
    pushConsoleEntry('info', 'agent-shortcut', `Executando atalho: ${kind}`);
    updateThinking(assistantEl, 'Coletando estado real do daemon...\nGerando diagnostico local deterministico...');
    const snapshot = await loadAgentShortcutSnapshot();
    const answer = buildAgentShortcutResponse(kind, snapshot);
    renderAssistantOutput(assistantEl, { rawAnswer: answer, finalize: true });
    pushConsoleEntry('info', 'agent-shortcut', `Atalho concluido: ${kind}`);
    state.agentConfig = snapshot.config || state.agentConfig;
    state.tools = snapshot.tools;
    state.channels = snapshot.channels;
    state.auditEntries = snapshot.auditEntries;
    state.plugins = snapshot.plugins;
    state.skills = snapshot.skills;
    updateAgentWorkspaceSummary();
  }
