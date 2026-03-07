const CHANNEL_PROTOCOL_VERSION = "v1";
const CHANNEL_PROTOCOL_HEADER = "x-channel-protocol-version";

function parseOptionalJsonInput(rawValue, fallback = {}) {
  const text = String(rawValue || "").trim();
  if (!text) {
    return fallback;
  }
  return JSON.parse(text);
}

function parseCredentialsInput(rawValue) {
  const text = String(rawValue || "").trim();
  if (!text) {
    return null;
  }
  if (text.startsWith("{")) {
    return JSON.parse(text);
  }
  return { token: text };
}

export function createAgentChannelsController({
  elements,
  fetchJson,
  promptText,
  confirmAction,
}) {
  let channelsCache = [];

  function protocolHeaders(extra = {}) {
    return {
      ...extra,
      [CHANNEL_PROTOCOL_HEADER]: CHANNEL_PROTOCOL_VERSION,
    };
  }

  function findChannelView(channelId) {
    return channelsCache.find((channel) => channel.id === channelId) || null;
  }

  function syncAgentAccountOptions(channelId) {
    const channel = findChannelView(channelId);
    const accountOptions = (channel?.accounts || []).map((account) => {
      const suffix = account.is_default ? " (default)" : "";
      return `<option value="${account.account_id}">${account.account_id}${suffix}</option>`;
    }).join("");

    if (elements.agentSendAccountSelect) {
      const selected = elements.agentSendAccountSelect.value;
      elements.agentSendAccountSelect.innerHTML = `<option value="">Resolver automaticamente</option>${accountOptions}`;
      if (selected) {
        elements.agentSendAccountSelect.value = selected;
      }
    }
    if (elements.agentChannelLogsAccountSelect) {
      const selected = elements.agentChannelLogsAccountSelect.value;
      elements.agentChannelLogsAccountSelect.innerHTML = `<option value="">Todas contas</option>${accountOptions}`;
      if (selected) {
        elements.agentChannelLogsAccountSelect.value = selected;
      }
    }
  }

  function syncAgentLogsAccountOptions(channelId) {
    const channel = findChannelView(channelId);
    const accountOptions = (channel?.accounts || []).map((account) => {
      const suffix = account.is_default ? " (default)" : "";
      return `<option value="${account.account_id}">${account.account_id}${suffix}</option>`;
    }).join("");

    if (elements.agentChannelLogsAccountSelect) {
      const selected = elements.agentChannelLogsAccountSelect.value;
      elements.agentChannelLogsAccountSelect.innerHTML = `<option value="">Todas contas</option>${accountOptions}`;
      if (selected) {
        elements.agentChannelLogsAccountSelect.value = selected;
      }
    }
  }

  function syncAgentChannelSelectors() {
    const currentChannel = elements.agentChannelSelect?.value || elements.agentSendChannelSelect?.value || "";
    const options = channelsCache.map(
      (channel) => `<option value="${channel.id}">${channel.name}</option>`
    ).join("");

    if (elements.agentChannelSelect) {
      elements.agentChannelSelect.innerHTML = options;
      if (currentChannel) {
        elements.agentChannelSelect.value = currentChannel;
      }
    }
    if (elements.agentSendChannelSelect) {
      elements.agentSendChannelSelect.innerHTML = options;
      if (currentChannel) {
        elements.agentSendChannelSelect.value = currentChannel;
      }
    }
    if (elements.agentChannelLogsChannelSelect) {
      const selected = elements.agentChannelLogsChannelSelect.value;
      elements.agentChannelLogsChannelSelect.innerHTML = `<option value="">Todos canais</option>${options}`;
      if (selected) {
        elements.agentChannelLogsChannelSelect.value = selected;
      }
    }

    syncAgentAccountOptions(elements.agentSendChannelSelect?.value || elements.agentChannelSelect?.value || "");
    syncAgentLogsAccountOptions(elements.agentChannelLogsChannelSelect?.value || "");
  }

  function populateChannelForm(channelId, accountId) {
    const channel = findChannelView(channelId);
    const account = channel?.accounts?.find((entry) => entry.account_id === accountId);
    if (!channel || !account) {
      return;
    }
    if (elements.agentChannelSelect) elements.agentChannelSelect.value = channel.id;
    if (elements.agentChannelAccountIdInput) elements.agentChannelAccountIdInput.value = account.account_id;
    if (elements.agentChannelMetadataInput) {
      elements.agentChannelMetadataInput.value = JSON.stringify(account.metadata || {}, null, 2);
    }
    if (elements.agentChannelRoutingDefaultsInput) {
      elements.agentChannelRoutingDefaultsInput.value = JSON.stringify(account.routing_defaults || {}, null, 2);
    }
    if (elements.agentChannelEnabledToggle) {
      elements.agentChannelEnabledToggle.checked = Boolean(account.enabled);
    }
    if (elements.agentChannelSetDefaultToggle) {
      elements.agentChannelSetDefaultToggle.checked = Boolean(account.is_default);
    }
    if (elements.agentChannelCredentialsInput) {
      elements.agentChannelCredentialsInput.value = "";
      elements.agentChannelCredentialsInput.placeholder = account.credentials_ref
        ? `Credencial mantida em ${account.credentials_ref}`
        : '{"token":"..."}';
    }
  }

  function clearChannelForm() {
    if (elements.agentChannelAccountIdInput) elements.agentChannelAccountIdInput.value = "";
    if (elements.agentChannelCredentialsInput) {
      elements.agentChannelCredentialsInput.value = "";
      elements.agentChannelCredentialsInput.placeholder = '{"token":"..."} ou xoxb-...';
    }
    if (elements.agentChannelMetadataInput) elements.agentChannelMetadataInput.value = "";
    if (elements.agentChannelRoutingDefaultsInput) elements.agentChannelRoutingDefaultsInput.value = "";
    if (elements.agentChannelEnabledToggle) elements.agentChannelEnabledToggle.checked = true;
    if (elements.agentChannelSetDefaultToggle) elements.agentChannelSetDefaultToggle.checked = false;
  }

  function renderAgentChannels() {
    if (!elements.agentChannelList) {
      return;
    }

    if (!channelsCache.length) {
      elements.agentChannelList.innerHTML = `<p class="meta-note">Nenhum canal carregado.</p>`;
      syncAgentChannelSelectors();
      return;
    }

    elements.agentChannelList.innerHTML = channelsCache.map((channel) => {
      const warning = channel.ambiguity_warning
        ? `<p class="meta-note" style="color: var(--danger); margin-top: 8px;">${channel.ambiguity_warning}</p>`
        : "";
      const protocol = channel.protocol_family
        ? `<div class="meta-note">protocolo: ${channel.protocol_family} ${channel.protocol_version || ""}</div>`
        : "";
      const accounts = (channel.accounts || []).map((account) => {
        const health = account.health_state?.status || "-";
        const session = account.session?.status || "-";
        const accountCapabilities = Array.isArray(account.capabilities) && account.capabilities.length
          ? `<div class="agent-skill-badges" style="margin-top: 8px;">${account.capabilities
              .map((capability) => `<span class="agent-skill-tag">${capability}</span>`)
              .join("")}</div>`
          : "";
        return `
          <div class="glass" style="padding: 12px; border-radius: 12px; margin-top: 8px;">
            <div style="display: flex; justify-content: space-between; gap: 8px; align-items: flex-start;">
              <div>
                <strong>${account.account_id}</strong>${account.is_default ? ' <span class="meta-note">(default)</span>' : ""}
                <div class="meta-note" style="margin-top: 4px;">health: ${health} • session: ${session}</div>
                <div class="meta-note">credencial: ${account.credentials_ref || "nao definida"}</div>
                ${protocol}
                ${accountCapabilities}
              </div>
              <div style="display: flex; gap: 6px; flex-wrap: wrap; justify-content: flex-end;">
                <button class="ghost-btn text-sm" type="button" data-channel-action="edit" data-channel="${channel.id}" data-account="${account.account_id}">Editar</button>
                <button class="ghost-btn text-sm" type="button" data-channel-action="login" data-channel="${channel.id}" data-account="${account.account_id}">Login</button>
                <button class="ghost-btn text-sm" type="button" data-channel-action="logout" data-channel="${channel.id}" data-account="${account.account_id}">Logout</button>
                <button class="ghost-btn text-sm" type="button" data-channel-action="default" data-channel="${channel.id}" data-account="${account.account_id}">Default</button>
                <button class="ghost-btn text-sm" type="button" data-channel-action="rename" data-channel="${channel.id}" data-account="${account.account_id}">Renomear</button>
                <button class="ghost-btn text-sm danger" type="button" data-channel-action="remove" data-channel="${channel.id}" data-account="${account.account_id}">Remover</button>
              </div>
            </div>
          </div>
        `;
      }).join("");

      return `
        <section class="glass" style="padding: 16px; border-radius: 16px; margin-top: 12px;">
          <div style="display: flex; justify-content: space-between; gap: 12px; align-items: baseline;">
            <div>
              <h4 style="margin: 0;">${channel.name}</h4>
              <p class="meta-note" style="margin-top: 4px;">aliases: ${(channel.aliases || []).join(", ") || "-"}</p>
              <p class="meta-note" style="margin-top: 4px;">family: ${channel.protocol_family} • version: ${channel.protocol_version}</p>
              <div class="agent-skill-badges" style="margin-top: 8px;">${(channel.capabilities || [])
                .map((capability) => `<span class="agent-skill-tag">${capability}</span>`)
                .join("")}</div>
            </div>
            <span class="meta-note">${(channel.accounts || []).length} conta(s)</span>
          </div>
          ${warning}
          ${accounts || '<p class="meta-note" style="margin-top: 12px;">Sem contas configuradas.</p>'}
        </section>
      `;
    }).join("");

    syncAgentChannelSelectors();
  }

  async function loadAgentChannelLogs() {
    if (!elements.agentChannelLogsList) {
      return;
    }
    const params = new URLSearchParams();
    if (elements.agentChannelLogsChannelSelect?.value) params.set("channel", elements.agentChannelLogsChannelSelect.value);
    if (elements.agentChannelLogsAccountSelect?.value) params.set("account_id", elements.agentChannelLogsAccountSelect.value);
    params.set("limit", "20");
    const entries = await fetchJson(`/agent/channels/logs?${params.toString()}`, {
      method: "GET",
      headers: protocolHeaders(),
    });
    const list = Array.isArray(entries) ? entries : [];
    if (!list.length) {
      elements.agentChannelLogsList.innerHTML = `<li class="meta-note">Sem logs de canais.</li>`;
      return;
    }
    elements.agentChannelLogsList.innerHTML = list.map((entry) => {
      const ts = entry.timestamp ? new Date(entry.timestamp).toLocaleString() : "-";
      const action = entry.action || entry.operation || "-";
      const result = entry.result || entry.status || "-";
      const errorCode = entry.error_code ? ` • ${entry.error_code}` : "";
      return `<li class="agent-audit-item">${ts} • ${entry.channel}:${entry.account_id} • ${action} • ${result}${errorCode}${entry.error ? ` • ${entry.error}` : ""}</li>`;
    }).join("");
  }

  async function loadAgentChannels() {
    const channels = await fetchJson("/agent/channels", {
      method: "GET",
      headers: protocolHeaders(),
    });
    channelsCache = Array.isArray(channels) ? channels : [];
    renderAgentChannels();
    await loadAgentChannelLogs();
  }

  async function executeChannelOperation(path, payload) {
    const response = await fetchJson(path, {
      method: "POST",
      headers: protocolHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify(payload),
    });
    await loadAgentChannels();
    return response;
  }

  async function saveAgentChannelAccount() {
    const channel = elements.agentChannelSelect?.value;
    const accountId = String(elements.agentChannelAccountIdInput?.value || "").trim();
    if (!channel || !accountId) {
      throw new Error("Selecione um canal e informe o account_id.");
    }

    const payload = {
      channel,
      account_id: accountId,
      enabled: Boolean(elements.agentChannelEnabledToggle?.checked),
      credentials: parseCredentialsInput(elements.agentChannelCredentialsInput?.value),
      metadata: parseOptionalJsonInput(elements.agentChannelMetadataInput?.value, {}),
      routing_defaults: parseOptionalJsonInput(elements.agentChannelRoutingDefaultsInput?.value, {}),
      set_as_default: Boolean(elements.agentChannelSetDefaultToggle?.checked),
      adapter_config: null,
    };

    await fetchJson("/agent/channels/upsert-account", {
      method: "POST",
      headers: protocolHeaders({ "Content-Type": "application/json" }),
      body: JSON.stringify(payload),
    });
    if (elements.agentChannelFormFeedback) {
      elements.agentChannelFormFeedback.textContent = `Conta ${channel}:${accountId} salva.`;
    }
    await loadAgentChannels();
  }

  async function renameAgentChannelAccount(channelId, accountId) {
    const channel = findChannelView(channelId);
    const account = channel?.accounts?.find((entry) => entry.account_id === accountId);
    if (!channel || !account) {
      throw new Error("Conta nao encontrada.");
    }
    const nextAccountId = await promptText({
      title: "Novo account_id",
      message: `Atualize o identificador de ${channelId}:${accountId}`,
      defaultValue: accountId,
      confirmLabel: "Renomear",
    });
    if (!nextAccountId || nextAccountId === accountId) {
      return;
    }

    await executeChannelOperation("/agent/channels/upsert-account", {
      channel: channelId,
      account_id: nextAccountId,
      enabled: account.enabled,
      credentials_ref: account.credentials_ref || null,
      metadata: account.metadata || {},
      routing_defaults: account.routing_defaults || {},
      set_as_default: account.is_default,
      adapter_config: account.adapter_config || null,
    });
    await executeChannelOperation("/agent/channels/remove-account", {
      channel: channelId,
      account_id: accountId,
    });
  }

  async function handleAgentChannelListAction(action, channelId, accountId) {
    if (action === "edit") {
      populateChannelForm(channelId, accountId);
      return;
    }
    if (action === "login") {
      const response = await executeChannelOperation("/agent/channels/login", {
        channel: channelId,
        account_id: accountId,
      });
      if (elements.agentChannelActionFeedback) {
        elements.agentChannelActionFeedback.textContent = `${response.channel}:${response.account_id} • ${response.status}`;
      }
      return;
    }
    if (action === "logout") {
      const response = await executeChannelOperation("/agent/channels/logout", {
        channel: channelId,
        account_id: accountId,
      });
      if (elements.agentChannelActionFeedback) {
        elements.agentChannelActionFeedback.textContent = `${response.channel}:${response.account_id} • ${response.status}`;
      }
      return;
    }
    if (action === "default") {
      const channel = findChannelView(channelId);
      const account = channel?.accounts?.find((entry) => entry.account_id === accountId);
      if (!account) {
        throw new Error("Conta nao encontrada.");
      }
      await executeChannelOperation("/agent/channels/upsert-account", {
        channel: channelId,
        account_id: accountId,
        enabled: account.enabled,
        credentials_ref: account.credentials_ref || null,
        metadata: account.metadata || {},
        routing_defaults: account.routing_defaults || {},
        set_as_default: true,
        adapter_config: account.adapter_config || null,
      });
      return;
    }
    if (action === "rename") {
      await renameAgentChannelAccount(channelId, accountId);
      return;
    }
    if (action === "remove") {
      const confirmed = await confirmAction({
        title: "Remover conta",
        message: `Remover ${channelId}:${accountId}?`,
        confirmLabel: "Remover",
        cancelLabel: "Cancelar",
        danger: true,
      });
      if (!confirmed) {
        return;
      }
      await executeChannelOperation("/agent/channels/remove-account", {
        channel: channelId,
        account_id: accountId,
      });
    }
  }

  async function sendAgentChannelTestMessage() {
    const channel = elements.agentSendChannelSelect?.value;
    const target = String(elements.agentSendTargetInput?.value || "").trim();
    const message = String(elements.agentSendMessageInput?.value || "").trim();
    if (!channel || !target || !message) {
      throw new Error("Canal, target e mensagem sao obrigatorios.");
    }
    const payload = await executeChannelOperation("/agent/message/send", {
      channel,
      account_id: elements.agentSendAccountSelect?.value || null,
      target,
      message,
    });
    if (elements.agentChannelActionFeedback) {
      elements.agentChannelActionFeedback.textContent = `Mensagem enviada via ${payload.channel}:${payload.account_id} (${payload.message_id})`;
    }
  }

  async function probeAgentChannel() {
    const channel = elements.agentSendChannelSelect?.value;
    if (!channel) {
      throw new Error("Selecione um canal.");
    }
    const payload = await executeChannelOperation("/agent/channels/probe", {
      channel,
      account_id: elements.agentSendAccountSelect?.value || null,
      all_accounts: !elements.agentSendAccountSelect?.value,
    });
    const summaries = (Array.isArray(payload) ? payload : []).map((entry) => `${entry.account_id}:${entry.status}`);
    if (elements.agentChannelActionFeedback) {
      elements.agentChannelActionFeedback.textContent = summaries.join(" • ") || "Probe concluido.";
    }
  }

  async function resolveAgentChannelTarget() {
    const channel = elements.agentSendChannelSelect?.value;
    const target = String(elements.agentSendTargetInput?.value || "").trim();
    if (!channel || !target) {
      throw new Error("Selecione canal e target.");
    }
    const payload = await executeChannelOperation("/agent/channels/resolve", {
      channel,
      account_id: elements.agentSendAccountSelect?.value || null,
      target,
    });
    if (elements.agentChannelActionFeedback) {
      elements.agentChannelActionFeedback.textContent = `Target resolvido por ${payload.account_id}: ${payload.resolved_target}`;
    }
  }

  function bindEvents() {
    if (elements.agentChannelsRefreshBtn) {
      elements.agentChannelsRefreshBtn.addEventListener("click", () => {
        void loadAgentChannels();
      });
    }

    if (elements.agentChannelSaveBtn) {
      elements.agentChannelSaveBtn.addEventListener("click", async () => {
        try {
          await saveAgentChannelAccount();
        } catch (error) {
          if (elements.agentChannelFormFeedback) {
            elements.agentChannelFormFeedback.textContent = `Falha ao salvar conta: ${error.message}`;
          }
        }
      });
    }

    if (elements.agentChannelClearBtn) {
      elements.agentChannelClearBtn.addEventListener("click", () => {
        clearChannelForm();
      });
    }

    if (elements.agentChannelSelect) {
      elements.agentChannelSelect.addEventListener("change", () => {
        syncAgentAccountOptions(elements.agentChannelSelect.value);
      });
    }

    if (elements.agentSendChannelSelect) {
      elements.agentSendChannelSelect.addEventListener("change", () => {
        syncAgentAccountOptions(elements.agentSendChannelSelect.value);
      });
    }

    if (elements.agentChannelLogsChannelSelect) {
      elements.agentChannelLogsChannelSelect.addEventListener("change", () => {
        syncAgentLogsAccountOptions(elements.agentChannelLogsChannelSelect.value);
        void loadAgentChannelLogs();
      });
    }

    if (elements.agentChannelLogsAccountSelect) {
      elements.agentChannelLogsAccountSelect.addEventListener("change", () => {
        void loadAgentChannelLogs();
      });
    }

    if (elements.agentChannelList) {
      elements.agentChannelList.addEventListener("click", async (event) => {
        const target = event.target instanceof HTMLElement ? event.target.closest("[data-channel-action]") : null;
        if (!target) {
          return;
        }
        try {
          await handleAgentChannelListAction(
            target.dataset.channelAction,
            target.dataset.channel,
            target.dataset.account,
          );
        } catch (error) {
          if (elements.agentChannelActionFeedback) {
            elements.agentChannelActionFeedback.textContent = `Falha: ${error.message}`;
          }
        }
      });
    }

    if (elements.agentSendTestBtn) {
      elements.agentSendTestBtn.addEventListener("click", async () => {
        try {
          await sendAgentChannelTestMessage();
        } catch (error) {
          if (elements.agentChannelActionFeedback) {
            elements.agentChannelActionFeedback.textContent = `Falha no envio: ${error.message}`;
          }
        }
      });
    }

    if (elements.agentProbeChannelBtn) {
      elements.agentProbeChannelBtn.addEventListener("click", async () => {
        try {
          await probeAgentChannel();
        } catch (error) {
          if (elements.agentChannelActionFeedback) {
            elements.agentChannelActionFeedback.textContent = `Falha no probe: ${error.message}`;
          }
        }
      });
    }

    if (elements.agentResolveTargetBtn) {
      elements.agentResolveTargetBtn.addEventListener("click", async () => {
        try {
          await resolveAgentChannelTarget();
        } catch (error) {
          if (elements.agentChannelActionFeedback) {
            elements.agentChannelActionFeedback.textContent = `Falha no resolve: ${error.message}`;
          }
        }
      });
    }

    if (elements.agentChannelLogsRefreshBtn) {
      elements.agentChannelLogsRefreshBtn.addEventListener("click", () => {
        void loadAgentChannelLogs();
      });
    }
  }

  bindEvents();

  return {
    clearChannelForm,
    loadChannels: loadAgentChannels,
    loadLogs: loadAgentChannelLogs,
  };
}
