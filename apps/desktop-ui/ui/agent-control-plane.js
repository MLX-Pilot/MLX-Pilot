function text(value, fallback = "-") {
  const normalized = String(value ?? "").trim();
  return normalized || fallback;
}

function formatJson(value) {
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return "{}";
  }
}

function parseJsonInput(rawValue, fallback = null) {
  const textValue = String(rawValue || "").trim();
  if (!textValue) {
    return fallback;
  }
  return JSON.parse(textValue);
}

function splitPatterns(rawValue) {
  return String(rawValue || "")
    .split(/[,\n]/)
    .map((entry) => entry.trim())
    .filter(Boolean);
}

function setNodeText(node, message) {
  if (node) {
    node.textContent = message;
  }
}

function formatRelativeDate(value) {
  if (!value) {
    return "-";
  }
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return text(value);
  }
  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function formatEpoch(value) {
  if (!value) {
    return "-";
  }
  const date = new Date(Number(value));
  if (Number.isNaN(date.getTime())) {
    return "-";
  }
  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function normalizeMemoryConfig(plugin) {
  const current = plugin?.config && typeof plugin.config === "object" ? plugin.config : {};
  return {
    backend: text(current.backend, "local"),
    compression_strategy: text(current.compression_strategy, "adaptive"),
    local_enabled: current.local_enabled !== false,
  };
}

function schemaProperties(schema) {
  if (!schema || typeof schema !== "object") {
    return {};
  }
  if (!schema.properties || typeof schema.properties !== "object") {
    return {};
  }
  return schema.properties;
}

function coerceFormValue(rawValue, schema = {}) {
  const type = Array.isArray(schema.type) ? schema.type[0] : schema.type;
  if (type === "boolean") {
    return Boolean(rawValue);
  }
  if (type === "integer") {
    return Number.parseInt(rawValue, 10);
  }
  if (type === "number") {
    return Number(rawValue);
  }
  if (type === "object" || type === "array") {
    return parseJsonInput(rawValue, type === "array" ? [] : {});
  }
  return String(rawValue ?? "");
}

function renderSchemaField(document, key, schema, value) {
  const wrapper = document.createElement("label");
  wrapper.className = "field-group";
  wrapper.dataset.schemaKey = key;

  const label = document.createElement("span");
  label.textContent = schema.title || key;
  wrapper.appendChild(label);

  const type = Array.isArray(schema.type) ? schema.type[0] : schema.type;
  const description = schema.description ? String(schema.description).trim() : "";

  let input;
  if (Array.isArray(schema.enum) && schema.enum.length) {
    input = document.createElement("select");
    input.className = "input";
    schema.enum.forEach((optionValue) => {
      const option = document.createElement("option");
      option.value = String(optionValue);
      option.textContent = String(optionValue);
      input.appendChild(option);
    });
    input.value = value != null ? String(value) : String(schema.enum[0]);
  } else if (type === "boolean") {
    const row = document.createElement("label");
    row.className = "agent-toggle-row";
    input = document.createElement("input");
    input.type = "checkbox";
    input.checked = Boolean(value);
    const textNode = document.createElement("span");
    textNode.textContent = description || `Habilitar ${key}`;
    row.appendChild(input);
    row.appendChild(textNode);
    wrapper.appendChild(row);
    input.dataset.schemaType = type || "boolean";
    input.dataset.schemaKey = key;
    return wrapper;
  } else if (type === "object" || type === "array") {
    input = document.createElement("textarea");
    input.className = "input";
    input.rows = 4;
    input.value = formatJson(value ?? (type === "array" ? [] : {}));
  } else {
    input = document.createElement("input");
    input.className = "input";
    input.type = type === "integer" || type === "number" ? "number" : "text";
    input.value = value == null ? "" : String(value);
    if (type === "integer") {
      input.step = "1";
    }
  }

  input.dataset.schemaKey = key;
  input.dataset.schemaType = type || "string";
  wrapper.appendChild(input);

  if (description) {
    const meta = document.createElement("p");
    meta.className = "meta-note";
    meta.textContent = description;
    wrapper.appendChild(meta);
  }

  return wrapper;
}

export function createAgentControlPlaneController({
  elements,
  fetchJson,
  onStatus,
  onToolPolicyChanged,
  loadAgentConfig,
}) {
  let pluginsCache = [];
  let selectedPluginId = null;
  let toolCatalogCache = null;
  let effectivePolicyCache = null;
  let runtimeChannelsCache = [];

  function status(message) {
    if (typeof onStatus === "function") {
      onStatus(message);
    }
  }

  function findPlugin(pluginId) {
    return pluginsCache.find((plugin) => plugin.id === pluginId) || null;
  }

  function selectedPlugin() {
    return findPlugin(selectedPluginId) || pluginsCache[0] || null;
  }

  function setSelectedPlugin(pluginId) {
    selectedPluginId = pluginId;
    renderPlugins();
    renderSelectedPluginDetail();
  }

  function syncMemorySection() {
    const plugin = findPlugin("memory");
    const config = normalizeMemoryConfig(plugin);
    if (elements.agentMemoryLocalToggle) {
      elements.agentMemoryLocalToggle.checked = Boolean(plugin?.enabled && config.local_enabled);
    }
    if (elements.agentMemoryBackendSelect) {
      elements.agentMemoryBackendSelect.value = config.backend;
    }
    if (elements.agentMemoryCompressionSelect) {
      elements.agentMemoryCompressionSelect.value = config.compression_strategy;
    }
  }

  function renderPlugins() {
    if (!elements.agentPluginsList) {
      return;
    }
    elements.agentPluginsList.innerHTML = "";

    if (!pluginsCache.length) {
      const empty = document.createElement("div");
      empty.className = "meta-note";
      empty.textContent = "Nenhum plugin detectado.";
      elements.agentPluginsList.appendChild(empty);
      return;
    }

    pluginsCache.forEach((plugin) => {
      const card = document.createElement("article");
      card.className = "agent-plugin-card";
      if (plugin.id === selectedPluginId) {
        card.classList.add("active");
      }

      const titleRow = document.createElement("div");
      titleRow.className = "agent-plugin-row";

      const head = document.createElement("div");
      const title = document.createElement("h4");
      title.textContent = plugin.name;
      title.style.margin = "0";
      const meta = document.createElement("p");
      meta.className = "meta-note";
      meta.textContent = `${text(plugin.class)} • health ${text(plugin.health)}`;
      head.appendChild(title);
      head.appendChild(meta);

      const toggle = document.createElement("input");
      toggle.type = "checkbox";
      toggle.checked = Boolean(plugin.enabled);
      toggle.dataset.pluginToggle = plugin.id;
      titleRow.appendChild(head);
      titleRow.appendChild(toggle);
      card.appendChild(titleRow);

      const desc = document.createElement("p");
      desc.className = "agent-toggle-desc";
      desc.textContent = plugin.docs?.summary || plugin.docs?.help || "Sem resumo.";
      card.appendChild(desc);

      const badges = document.createElement("div");
      badges.className = "agent-skill-badges";
      [
        plugin.configured ? "configurado" : "sem config",
        plugin.loaded ? "carregado" : "lazy",
        ...(Array.isArray(plugin.capabilities) ? plugin.capabilities.slice(0, 4) : []),
      ].forEach((label) => {
        const badge = document.createElement("span");
        badge.className = "agent-skill-tag";
        badge.textContent = label;
        badges.appendChild(badge);
      });
      card.appendChild(badges);

      if (Array.isArray(plugin.errors) && plugin.errors.length) {
        const errors = document.createElement("p");
        errors.className = "meta-note";
        errors.style.color = "var(--danger)";
        errors.textContent = plugin.errors.join(" | ");
        card.appendChild(errors);
      }

      const actions = document.createElement("div");
      actions.className = "agent-plugin-actions";
      const button = document.createElement("button");
      button.type = "button";
      button.className = "ghost-btn";
      button.dataset.pluginSelect = plugin.id;
      button.textContent = "Configurar";
      actions.appendChild(button);
      card.appendChild(actions);

      elements.agentPluginsList.appendChild(card);
    });
  }

  function renderSelectedPluginDetail() {
    const plugin = selectedPlugin();
    setNodeText(elements.agentPluginDetailTitle, plugin ? plugin.name : "Selecione um plugin");

    if (!plugin) {
      setNodeText(elements.agentPluginDetailMeta, "Nenhum plugin selecionado.");
      if (elements.agentPluginConfigEnabled) {
        elements.agentPluginConfigEnabled.checked = false;
      }
      if (elements.agentPluginConfigForm) {
        elements.agentPluginConfigForm.innerHTML = "";
      }
      return;
    }

    setNodeText(
      elements.agentPluginDetailMeta,
      `${text(plugin.class)} • aliases: ${(plugin.aliases || []).join(", ") || "-"} • ${plugin.docs?.help || plugin.docs?.summary || "Sem ajuda."}`,
    );

    if (elements.agentPluginConfigEnabled) {
      elements.agentPluginConfigEnabled.checked = Boolean(plugin.enabled);
    }

    if (!elements.agentPluginConfigForm) {
      return;
    }

    elements.agentPluginConfigForm.innerHTML = "";
    const props = schemaProperties(plugin.config_schema);
    const propKeys = Object.keys(props);
    const usedKeys = new Set(propKeys);

    if (!propKeys.length) {
      const rawWrapper = document.createElement("label");
      rawWrapper.className = "field-group";
      const label = document.createElement("span");
      label.textContent = "Config JSON";
      const textarea = document.createElement("textarea");
      textarea.className = "input";
      textarea.rows = 8;
      textarea.dataset.pluginRawConfig = "true";
      textarea.value = formatJson(plugin.config || {});
      rawWrapper.appendChild(label);
      rawWrapper.appendChild(textarea);
      elements.agentPluginConfigForm.appendChild(rawWrapper);
      return;
    }

    propKeys.forEach((key) => {
      const field = renderSchemaField(document, key, props[key] || {}, plugin.config?.[key]);
      elements.agentPluginConfigForm.appendChild(field);
    });

    const extraConfig = Object.fromEntries(
      Object.entries(plugin.config || {}).filter(([key]) => !usedKeys.has(key)),
    );
    if (Object.keys(extraConfig).length) {
      const extraWrapper = document.createElement("label");
      extraWrapper.className = "field-group";
      const label = document.createElement("span");
      label.textContent = "Extra config (JSON)";
      const textarea = document.createElement("textarea");
      textarea.className = "input";
      textarea.rows = 4;
      textarea.dataset.pluginExtraConfig = "true";
      textarea.value = formatJson(extraConfig);
      extraWrapper.appendChild(label);
      extraWrapper.appendChild(textarea);
      elements.agentPluginConfigForm.appendChild(extraWrapper);
    }
  }

  function collectSelectedPluginConfig() {
    const plugin = selectedPlugin();
    if (!plugin) {
      throw new Error("Nenhum plugin selecionado.");
    }

    if (!elements.agentPluginConfigForm) {
      return plugin.config || {};
    }

    const raw = elements.agentPluginConfigForm.querySelector("textarea[data-plugin-raw-config='true']");
    if (raw) {
      return parseJsonInput(raw.value, {});
    }

    const output = {};
    const props = schemaProperties(plugin.config_schema);

    elements.agentPluginConfigForm.querySelectorAll("[data-schema-key]").forEach((input) => {
      const key = input.dataset.schemaKey || "";
      if (!key || !props[key]) {
        return;
      }
      if (input instanceof HTMLInputElement && input.type === "checkbox") {
        output[key] = coerceFormValue(input.checked, props[key]);
        return;
      }
      output[key] = coerceFormValue(input.value, props[key]);
    });

    const extra = elements.agentPluginConfigForm.querySelector("textarea[data-plugin-extra-config='true']");
    if (extra) {
      Object.assign(output, parseJsonInput(extra.value, {}));
    }

    return output;
  }

  async function saveSelectedPlugin() {
    const plugin = selectedPlugin();
    if (!plugin) {
      throw new Error("Nenhum plugin selecionado.");
    }

    const response = await fetchJson("/agent/plugins/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        plugin_id: plugin.id,
        enabled: Boolean(elements.agentPluginConfigEnabled?.checked),
        config: collectSelectedPluginConfig(),
      }),
    });

    setNodeText(elements.agentPluginFeedback, `Plugin ${response.name} salvo.`);
    await loadPlugins();
    setSelectedPlugin(plugin.id);
    status(`Plugin ${response.id} atualizado.`);
    return response;
  }

  async function togglePlugin(pluginId, enabled) {
    const endpoint = enabled ? "/agent/plugins/enable" : "/agent/plugins/disable";
    const response = await fetchJson(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ plugin_id: pluginId }),
    });
    await loadPlugins();
    setSelectedPlugin(pluginId);
    status(`Plugin ${response.id} ${enabled ? "habilitado" : "desabilitado"}.`);
    return response;
  }

  async function loadPlugins() {
    const plugins = await fetchJson("/agent/plugins", { method: "GET" });
    pluginsCache = Array.isArray(plugins) ? plugins : [];
    if (!selectedPluginId || !findPlugin(selectedPluginId)) {
      selectedPluginId = pluginsCache[0]?.id || null;
    }
    renderPlugins();
    renderSelectedPluginDetail();
    syncMemorySection();
    syncRuntimeSources();
    return pluginsCache;
  }

  function syncPolicyEditors(config) {
    const agentRules = config?.tool_policy?.agent_overrides?.default || { allow: [], deny: [] };
    if (elements.agentToolProfileSelect && config?.tool_policy?.profile) {
      elements.agentToolProfileSelect.value = config.tool_policy.profile;
    }
    if (elements.agentPolicyAllowInput) {
      elements.agentPolicyAllowInput.value = Array.isArray(agentRules.allow) ? agentRules.allow.join("\n") : "";
    }
    if (elements.agentPolicyDenyInput) {
      elements.agentPolicyDenyInput.value = Array.isArray(agentRules.deny) ? agentRules.deny.join("\n") : "";
    }
  }

  function renderEffectivePolicy() {
    if (!elements.agentEffectivePolicyList) {
      return;
    }
    elements.agentEffectivePolicyList.innerHTML = "";
    const entries = Array.isArray(effectivePolicyCache?.entries) ? effectivePolicyCache.entries : [];
    if (!entries.length) {
      const empty = document.createElement("li");
      empty.className = "meta-note";
      empty.textContent = "Politica efetiva indisponivel.";
      elements.agentEffectivePolicyList.appendChild(empty);
      return;
    }

    entries.forEach((entry) => {
      const item = document.createElement("li");
      item.className = "agent-policy-item";
      item.innerHTML = `
        <div>
          <strong>${entry.name}</strong>
          <p class="meta-note">${entry.description}</p>
        </div>
        <div class="agent-policy-badges">
          <span class="agent-skill-tag">${entry.allowed ? "allow" : "deny"}</span>
          <span class="agent-skill-tag">${text(entry.section)}</span>
          <span class="agent-skill-tag">${text(entry.risk)}</span>
          <span class="agent-skill-tag">${text(entry.final_rule)}</span>
        </div>
      `;
      elements.agentEffectivePolicyList.appendChild(item);
    });
  }

  async function loadToolPolicies(config = null) {
    const [catalog, effective] = await Promise.all([
      fetchJson("/agent/tools/catalog", { method: "GET" }),
      fetchJson("/agent/tools/effective-policy", { method: "GET" }),
    ]);
    toolCatalogCache = catalog;
    effectivePolicyCache = effective;

    if (elements.agentToolProfileSelect) {
      const profiles = Array.isArray(toolCatalogCache?.profiles) ? toolCatalogCache.profiles : [];
      const current = elements.agentToolProfileSelect.value;
      elements.agentToolProfileSelect.innerHTML = "";
      profiles.forEach((profile) => {
        const option = document.createElement("option");
        option.value = profile.id;
        option.textContent = `${profile.id} (${Array.isArray(profile.tools) ? profile.tools.length : 0} tools)`;
        elements.agentToolProfileSelect.appendChild(option);
      });
      elements.agentToolProfileSelect.value = effective.profile || config?.tool_policy?.profile || current || "coding";
    }

    if (elements.agentToolCatalogSummary) {
      const profiles = Array.isArray(toolCatalogCache?.profiles) ? toolCatalogCache.profiles.length : 0;
      const entries = Array.isArray(toolCatalogCache?.entries) ? toolCatalogCache.entries.length : 0;
      elements.agentToolCatalogSummary.textContent = `${profiles} perfis, ${entries} tools catalogados.`;
    }

    if (config) {
      syncPolicyEditors(config);
    }

    renderEffectivePolicy();
    return effectivePolicyCache;
  }

  async function applyToolProfile() {
    const profile = text(elements.agentToolProfileSelect?.value, "coding");
    const response = await fetchJson("/agent/tools/profile", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ profile }),
    });
    effectivePolicyCache = response;
    setNodeText(elements.agentPolicyFeedback, `Profile ${profile} aplicado.`);
    renderEffectivePolicy();
    await onToolPolicyChanged?.();
    status(`Profile ${profile} aplicado.`);
    return response;
  }

  async function savePolicyOverrides() {
    const scope = text(elements.agentPolicyScopeSelect?.value, "agent");
    const response = await fetchJson("/agent/tools/allow-deny", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        scope,
        agent_id: scope === "agent" ? "default" : null,
        allow: splitPatterns(elements.agentPolicyAllowInput?.value),
        deny: splitPatterns(elements.agentPolicyDenyInput?.value),
        replace: true,
      }),
    });
    effectivePolicyCache = response;
    setNodeText(elements.agentPolicyFeedback, `Overrides ${scope} salvos.`);
    renderEffectivePolicy();
    await onToolPolicyChanged?.();
    status(`Politica ${scope} atualizada.`);
    return response;
  }

  async function saveMemorySettings() {
    const memoryPlugin = findPlugin("memory");
    const nextConfig = {
      ...(memoryPlugin?.config && typeof memoryPlugin.config === "object" ? memoryPlugin.config : {}),
      backend: text(elements.agentMemoryBackendSelect?.value, "local"),
      compression_strategy: text(elements.agentMemoryCompressionSelect?.value, "adaptive"),
      local_enabled: Boolean(elements.agentMemoryLocalToggle?.checked),
    };

    const response = await fetchJson("/agent/plugins/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        plugin_id: "memory",
        enabled: Boolean(elements.agentMemoryLocalToggle?.checked),
        config: nextConfig,
      }),
    });
    setNodeText(elements.agentMemoryFeedback, `Memoria ${response.enabled ? "habilitada" : "desabilitada"} com backend ${text(nextConfig.backend)}.`);
    await loadPlugins();
    status("Config de memoria atualizada.");
    return response;
  }

  function renderBudgetTelemetry(payload) {
    if (!elements.agentBudgetTelemetry) {
      return;
    }
    if (!payload || typeof payload !== "object") {
      elements.agentBudgetTelemetry.textContent = "Nenhuma telemetria de budget disponivel.";
      return;
    }

    const lines = [
      `Sessao: ${text(payload.session_id)}`,
      `Provider/modelo: ${text(payload.provider_id)}/${text(payload.model_id)}`,
      `Perfil: ${text(payload.model_profile)} • tools ${text(payload.tool_profile)}`,
      `Prompt: ${payload.prompt_tokens_estimate || 0}/${payload.max_prompt_tokens || 0} tokens`,
      `Antes da compressao: ${payload.prompt_tokens_before_compression || 0}`,
      `Historico usado: ${payload.history_messages_used || 0}/${payload.history_messages_total || 0}`,
      `Resumos: ${payload.summary_entries || 0} • mensagens resumidas ${payload.summarized_messages || 0}`,
      `Tools no prompt: ${payload.tools_in_prompt || 0}/${payload.tools_considered || 0}`,
      `Critico: ${payload.critical ? "sim" : "nao"}`,
      `Atualizado: ${formatRelativeDate(payload.last_updated)}`,
    ];
    elements.agentBudgetTelemetry.textContent = lines.join("\n");
  }

  async function loadBudgetTelemetry() {
    try {
      const payload = await fetchJson("/agent/context/budget", { method: "GET" });
      renderBudgetTelemetry(payload);
      return payload;
    } catch (error) {
      renderBudgetTelemetry(null);
      return null;
    }
  }

  function buildRuntimeDiagnostics({ health, framework, runtime, plugins, channels, budget }) {
    const issues = [];
    if (health?.status !== "ok") {
      issues.push(`Daemon reportou ${text(health?.status)}`);
    }

    if (framework === "openclaw") {
      if (runtime && (runtime.rpc_ok === false || runtime.service_status === "stopped")) {
        issues.push(`OpenClaw com runtime ${text(runtime.service_status)}/${text(runtime.service_state)}`);
      }
    } else if (framework === "nanobot") {
      if (runtime && runtime.running === false) {
        issues.push(`NanoBot gateway parado (${text(runtime.status)})`);
      }
    }

    (plugins || [])
      .filter((plugin) => plugin.enabled && ((plugin.errors || []).length || String(plugin.health).includes("error")))
      .forEach((plugin) => {
        issues.push(`Plugin ${plugin.id} com erro: ${(plugin.errors || [plugin.health]).join(" | ")}`);
      });

    (channels || []).forEach((channel) => {
      (channel.accounts || []).forEach((account) => {
        const state = account.health_state?.status || "unknown";
        if (!["healthy", "idle", "not_logged_in"].includes(state)) {
          issues.push(`Canal ${channel.id}:${account.account_id} em estado ${state}`);
        }
      });
    });

    if (budget?.critical) {
      issues.push(`Budget critico na sessao ${text(budget.session_id)}`);
    }

    return issues.length ? issues : ["Nenhuma falha critica detectada no snapshot atual."];
  }

  function renderRuntimeDiagnostics(issues) {
    if (!elements.agentRuntimeDiagnosticsList) {
      return;
    }
    elements.agentRuntimeDiagnosticsList.innerHTML = "";
    issues.forEach((issue) => {
      const item = document.createElement("li");
      item.className = "agent-audit-item";
      item.textContent = issue;
      elements.agentRuntimeDiagnosticsList.appendChild(item);
    });
  }

  function renderRuntimeSummary({ health, framework, runtime, plugins, channels, budget }) {
    if (elements.agentRuntimeSummary) {
      const enabledPlugins = (plugins || []).filter((plugin) => plugin.enabled).length;
      const unhealthyChannels = (channels || []).reduce((count, channel) => count + (channel.accounts || []).filter((account) => {
        const state = account.health_state?.status || "unknown";
        return !["healthy", "idle", "not_logged_in"].includes(state);
      }).length, 0);
      elements.agentRuntimeSummary.textContent = `daemon ${text(health?.status)} • framework ${framework} • plugins ativos ${enabledPlugins} • canais degradados ${unhealthyChannels}`;
    }

    if (elements.agentRuntimeFrameworkMeta) {
      if (framework === "openclaw") {
        elements.agentRuntimeFrameworkMeta.textContent = `OpenClaw • service ${text(runtime?.service_status)} • state ${text(runtime?.service_state)} • pid ${text(runtime?.pid)}`;
      } else {
        elements.agentRuntimeFrameworkMeta.textContent = `NanoBot • status ${text(runtime?.status)} • running ${runtime?.running ? "sim" : "nao"} • pid ${text(runtime?.pid)}`;
      }
    }

    renderRuntimeDiagnostics(buildRuntimeDiagnostics({ health, framework, runtime, plugins, channels, budget }));
  }

  function syncRuntimeSources() {
    if (!elements.agentRuntimeLogSource) {
      return;
    }

    const mode = elements.agentRuntimeLogMode?.value || "channel";
    const previous = elements.agentRuntimeLogSource.value;
    elements.agentRuntimeLogSource.innerHTML = "";

    if (mode === "plugin") {
      pluginsCache.forEach((plugin) => {
        const option = document.createElement("option");
        option.value = plugin.id;
        option.textContent = plugin.name;
        elements.agentRuntimeLogSource.appendChild(option);
      });
      if (elements.agentRuntimeLogAccount) {
        elements.agentRuntimeLogAccount.disabled = true;
        elements.agentRuntimeLogAccount.innerHTML = '<option value="">n/a</option>';
      }
    } else {
      runtimeChannelsCache.forEach((channel) => {
        const option = document.createElement("option");
        option.value = channel.id;
        option.textContent = channel.name;
        elements.agentRuntimeLogSource.appendChild(option);
      });
      syncRuntimeAccounts();
    }

    if (previous) {
      elements.agentRuntimeLogSource.value = previous;
    }
  }

  function syncRuntimeAccounts() {
    if (!elements.agentRuntimeLogAccount) {
      return;
    }
    const mode = elements.agentRuntimeLogMode?.value || "channel";
    if (mode === "plugin") {
      elements.agentRuntimeLogAccount.disabled = true;
      elements.agentRuntimeLogAccount.innerHTML = '<option value="">n/a</option>';
      return;
    }

    const channelId = elements.agentRuntimeLogSource?.value;
    const channel = runtimeChannelsCache.find((entry) => entry.id === channelId);
    elements.agentRuntimeLogAccount.disabled = false;
    elements.agentRuntimeLogAccount.innerHTML = '<option value="">Todas contas</option>';
    (channel?.accounts || []).forEach((account) => {
      const option = document.createElement("option");
      option.value = account.account_id;
      option.textContent = account.account_id;
      elements.agentRuntimeLogAccount.appendChild(option);
    });
  }

  function renderRuntimeLogEntries(entries) {
    if (!elements.agentRuntimeLogList) {
      return;
    }
    elements.agentRuntimeLogList.innerHTML = "";
    if (!entries.length) {
      const empty = document.createElement("li");
      empty.className = "meta-note";
      empty.textContent = "Nenhum evento para o filtro atual.";
      elements.agentRuntimeLogList.appendChild(empty);
      return;
    }

    entries.forEach((entry) => {
      const item = document.createElement("li");
      item.className = "agent-audit-item";
      item.textContent = entry;
      elements.agentRuntimeLogList.appendChild(item);
    });
  }

  async function loadRuntimeLogs() {
    const mode = elements.agentRuntimeLogMode?.value || "channel";
    if (mode === "plugin") {
      const plugin = findPlugin(elements.agentRuntimeLogSource?.value);
      const entries = (plugin?.errors || []).map((error) => `${plugin.id} • ${error}`);
      renderRuntimeLogEntries(entries);
      return entries;
    }

    const params = new URLSearchParams();
    if (elements.agentRuntimeLogSource?.value) {
      params.set("channel", elements.agentRuntimeLogSource.value);
    }
    if (elements.agentRuntimeLogAccount?.value) {
      params.set("account_id", elements.agentRuntimeLogAccount.value);
    }
    params.set("limit", "20");
    const logs = await fetchJson(`/agent/channels/logs?${params.toString()}`, { method: "GET", headers: { "x-channel-protocol-version": "v1" } });
    const entries = (Array.isArray(logs) ? logs : []).map((entry) => {
      const when = formatRelativeDate(entry.timestamp);
      return `${when} • ${entry.channel}:${entry.account_id} • ${text(entry.action || entry.operation)} • ${text(entry.result || entry.status)}${entry.error ? ` • ${entry.error}` : ""}`;
    });
    renderRuntimeLogEntries(entries);
    return entries;
  }

  async function loadRuntimeHealth() {
    const config = await fetchJson("/config", { method: "GET" });
    const framework = config?.active_agent_framework === "nanobot" ? "nanobot" : "openclaw";
    const requests = [
      fetchJson("/health", { method: "GET" }),
      fetchJson("/agent/plugins", { method: "GET" }),
      fetchJson("/agent/channels/status", { method: "GET", headers: { "x-channel-protocol-version": "v1" } }),
      loadBudgetTelemetry(),
      fetchJson(framework === "nanobot" ? "/nanobot/runtime" : "/openclaw/runtime", { method: "GET" }),
    ];

    const [health, plugins, channels, budget, runtime] = await Promise.all(requests);
    runtimeChannelsCache = Array.isArray(channels) ? channels : [];
    pluginsCache = Array.isArray(plugins) ? plugins : pluginsCache;
    renderPlugins();
    renderSelectedPluginDetail();
    syncMemorySection();
    syncRuntimeSources();
    renderRuntimeSummary({
      health,
      framework,
      runtime,
      plugins: pluginsCache,
      channels: runtimeChannelsCache,
      budget,
    });
    await loadRuntimeLogs();
    return { health, framework, runtime, plugins: pluginsCache, channels: runtimeChannelsCache, budget };
  }

  function updateRangeLabel(input, output, suffix = "") {
    if (!input || !output) {
      return;
    }
    output.textContent = `${input.value}${suffix}`;
  }

  function bindEvents() {
    elements.agentPluginsList?.addEventListener("click", async (event) => {
      const target = event.target instanceof HTMLElement ? event.target : null;
      const selectButton = target?.closest("[data-plugin-select]");
      if (selectButton) {
        setSelectedPlugin(selectButton.dataset.pluginSelect || "");
      }
    });

    elements.agentPluginsList?.addEventListener("change", async (event) => {
      const target = event.target instanceof HTMLInputElement ? event.target : null;
      if (!target?.dataset.pluginToggle) {
        return;
      }
      try {
        await togglePlugin(target.dataset.pluginToggle, target.checked);
      } catch (error) {
        target.checked = !target.checked;
        setNodeText(elements.agentPluginFeedback, `Falha ao alterar plugin: ${error.message}`);
      }
    });

    elements.agentPluginsRefreshBtn?.addEventListener("click", () => {
      void loadPlugins();
    });

    elements.agentPluginSaveBtn?.addEventListener("click", async () => {
      try {
        await saveSelectedPlugin();
      } catch (error) {
        setNodeText(elements.agentPluginFeedback, `Falha ao salvar plugin: ${error.message}`);
      }
    });

    elements.agentPluginResetBtn?.addEventListener("click", () => {
      renderSelectedPluginDetail();
      setNodeText(elements.agentPluginFeedback, "Formulario de plugin restaurado.");
    });

    elements.agentToolProfileApplyBtn?.addEventListener("click", async () => {
      try {
        await applyToolProfile();
      } catch (error) {
        setNodeText(elements.agentPolicyFeedback, `Falha ao trocar profile: ${error.message}`);
      }
    });

    elements.agentPolicySaveBtn?.addEventListener("click", async () => {
      try {
        await savePolicyOverrides();
      } catch (error) {
        setNodeText(elements.agentPolicyFeedback, `Falha ao salvar politica: ${error.message}`);
      }
    });

    elements.agentPolicyResetBtn?.addEventListener("click", async () => {
      try {
        const config = await loadAgentConfig?.();
        syncPolicyEditors(config);
        setNodeText(elements.agentPolicyFeedback, "Overrides restaurados do backend.");
      } catch (error) {
        setNodeText(elements.agentPolicyFeedback, `Falha ao restaurar politica: ${error.message}`);
      }
    });

    elements.agentMemorySaveBtn?.addEventListener("click", async () => {
      try {
        await saveMemorySettings();
      } catch (error) {
        setNodeText(elements.agentMemoryFeedback, `Falha ao salvar memoria: ${error.message}`);
      }
    });

    elements.agentBudgetRefreshBtn?.addEventListener("click", () => {
      void loadBudgetTelemetry();
    });

    elements.agentRuntimeRefreshBtn?.addEventListener("click", () => {
      void loadRuntimeHealth();
    });

    elements.agentRuntimeLogMode?.addEventListener("change", () => {
      syncRuntimeSources();
      void loadRuntimeLogs();
    });

    elements.agentRuntimeLogSource?.addEventListener("change", () => {
      syncRuntimeAccounts();
      void loadRuntimeLogs();
    });

    elements.agentRuntimeLogAccount?.addEventListener("change", () => {
      void loadRuntimeLogs();
    });

    updateRangeLabel(elements.agentMaxPromptInput, elements.agentMaxPromptValue, " tokens");
    updateRangeLabel(elements.agentMaxHistoryInput, elements.agentMaxHistoryValue, " msgs");
    updateRangeLabel(elements.agentMaxToolsInput, elements.agentMaxToolsValue, " tools");

    elements.agentMaxPromptInput?.addEventListener("input", () => {
      updateRangeLabel(elements.agentMaxPromptInput, elements.agentMaxPromptValue, " tokens");
    });
    elements.agentMaxHistoryInput?.addEventListener("input", () => {
      updateRangeLabel(elements.agentMaxHistoryInput, elements.agentMaxHistoryValue, " msgs");
    });
    elements.agentMaxToolsInput?.addEventListener("input", () => {
      updateRangeLabel(elements.agentMaxToolsInput, elements.agentMaxToolsValue, " tools");
    });
  }

  bindEvents();

  return {
    loadPlugins,
    loadToolPolicies,
    loadRuntimeHealth,
    loadBudgetTelemetry,
    syncPolicyEditors,
    syncMemorySection,
  };
}
