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
  showChannelLoginDialog,
  renderQrCode,
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

  function findAccountView(channelId, accountId) {
    const channel = findChannelView(channelId);
    return channel?.accounts?.find((entry) => entry.account_id === accountId) || null;
  }

  function channelCapabilitySet(channel, account = null) {
    return new Set([
      ...(Array.isArray(channel?.capabilities) ? channel.capabilities : []),
      ...(Array.isArray(account?.capabilities) ? account.capabilities : []),
    ]);
  }

  function supportsQrLogin(channel, account) {
    return channelCapabilitySet(channel, account).has("qr-login");
  }

  function loginActionLabel(channel, account) {
    const capabilities = channelCapabilitySet(channel, account);
    const protocolFamily = String(channel?.protocol_family || "").toLowerCase();
    if (capabilities.has("qr-login") || channel?.id === "whatsapp") {
      return "Conectar QR";
    }
    if (
      capabilities.has("bot-token")
      || capabilities.has("access-token")
      || protocolFamily.includes("token_bot")
      || protocolFamily.includes("oauth")
    ) {
      return "Autenticar";
    }
    if (protocolFamily.includes("bridge")) {
      return "Ativar";
    }
    return "Login";
  }

  function nonEmptyDetails(details) {
    return Boolean(
      details
      && typeof details === "object"
      && !Array.isArray(details)
      && Object.keys(details).length,
    );
  }

  function extractAccountQrPayload(account = null) {
    const qrCode = typeof account?.session?.qr_code === "string" && account.session.qr_code.trim()
      ? account.session.qr_code.trim()
      : "";
    const qrDataUrl = typeof account?.session?.qr_image_data_url === "string"
      && account.session.qr_image_data_url.trim()
      ? account.session.qr_image_data_url.trim()
      : "";
    return { qrCode, qrDataUrl };
  }

  async function presentChannelLoginDialog(channelId, accountId, response) {
    if (typeof showChannelLoginDialog !== "function") {
      return;
    }
    const channel = findChannelView(channelId);
    const account = channel?.accounts?.find((entry) => entry.account_id === accountId) || null;
    const details = nonEmptyDetails(response?.details) ? response.details : null;
    const { qrCode: sessionQrCode, qrDataUrl: sessionQrDataUrl } = extractAccountQrPayload(account);
    const qrCode = typeof details?.qr_code === "string" && details.qr_code.trim()
      ? details.qr_code.trim()
      : sessionQrCode;
    const qrDataUrl = typeof details?.qr_image_data_url === "string" && details.qr_image_data_url.trim()
      ? details.qr_image_data_url.trim()
      : sessionQrDataUrl;

    if (!qrCode && !qrDataUrl && !details) {
      return;
    }

    await showChannelLoginDialog({
      title: `${channel?.name || channelId} • ${accountId}`,
      channelName: channel?.name || channelId,
      channelId,
      accountId,
      status: response?.status || account?.session?.status || "connected",
      message: response?.message || "",
      qrCode: qrCode || null,
      qrDataUrl: qrDataUrl || null,
      details,
      sessionState: account?.session || null,
    });
  }

  function nonEmptyObject(value) {
    return Boolean(
      value
      && typeof value === "object"
      && !Array.isArray(value)
      && Object.keys(value).length,
    );
  }

  function onboardingBlueprint(channel, account = null) {
    if (!channel) {
      return {
        title: "Selecione um canal para ver o fluxo recomendado",
        summary: "A UI adapta as instrucoes conforme QR local, token de bot ou bridge externa.",
        credentialLabel: "Credencial (token ou JSON)",
        credentialPlaceholder: '{"token":"..."} ou xoxb-...',
        credentialHint: "Cole um token simples ou JSON, dependendo do canal.",
        metadataLabel: "Metadata (JSON)",
        metadataPlaceholder: '{"workspace":"ops"}',
        routingLabel: "Routing defaults (JSON)",
        routingPlaceholder: '{"target":"#alerts"}',
        adapterLabel: "Adapter config (JSON)",
        adapterPlaceholder: '{"endpoint":"https://bridge.exemplo","token":"..."}',
        steps: [
          "Selecione um canal.",
          "Salve uma conta.",
          "Siga o fluxo de login indicado pelo canal.",
        ],
      };
    }

    const capabilities = channelCapabilitySet(channel, account);
    const protocolFamily = String(channel.protocol_family || "").toLowerCase();
    const docsSummary = channel.docs?.summary || channel.docs?.help || "Sem resumo disponivel para este canal.";
    const bridgeMode = protocolFamily.includes("bridge") || protocolFamily.includes("webhook") || capabilities.has("bridge-http");
    const qrMode = capabilities.has("qr-login") || channel.id === "whatsapp";
    const botTokenMode = capabilities.has("bot-token") || protocolFamily.includes("token_bot");
    const accessTokenMode = capabilities.has("access-token") || protocolFamily.includes("oauth");

    if (bridgeMode) {
      return {
        title: `${channel.name} • Bridge externa`,
        summary: `${docsSummary} Este conector depende de bridge/webhook real para ativacao completa.`,
        credentialLabel: "Credencial do bridge (opcional)",
        credentialPlaceholder: '{"token":"..."}',
        credentialHint: "Se o conector exigir endpoint, token, room ou base_url, prefira preencher em Adapter config.",
        metadataLabel: "Metadata operacional (JSON)",
        metadataPlaceholder: '{"workspace":"ops","owner":"team-messaging"}',
        routingLabel: "Routing defaults (JSON)",
        routingPlaceholder: '{"target":"#alerts"}',
        adapterLabel: "Adapter config do bridge (JSON)",
        adapterPlaceholder: '{"endpoint":"https://bridge.exemplo/api","token":"...","room":"#ops"}',
        steps: [
          "Provisione o bridge/webhook externo do canal.",
          "Preencha endpoint, token e dados de room em Adapter config.",
          "Salve a conta e execute Login ou Probe.",
          "Envie uma mensagem teste para validar o fluxo real.",
        ],
      };
    }

    if (qrMode) {
      return {
        title: `${channel.name} • Sessao local com QR`,
        summary: `${docsSummary} O onboarding principal acontece por sessao local e QRCode.`,
        credentialLabel: "Credencial (opcional para QR local)",
        credentialPlaceholder: "",
        credentialHint: "Para QR local, normalmente basta salvar a conta e clicar em Conectar QR.",
        metadataLabel: "Metadata da conta (JSON)",
        metadataPlaceholder: '{"profile":"pessoal"}',
        routingLabel: "Routing defaults (JSON)",
        routingPlaceholder: '{"target":"@cliente"}',
        adapterLabel: "Adapter config da sessao (JSON)",
        adapterPlaceholder: '{"backend":"embedded"}',
        steps: [
          "Salve a conta mesmo sem token.",
          "Clique em Conectar QR para abrir a sessao local.",
          "Escaneie o QR e use Ver QR se precisar reabrir o codigo.",
          "Rode Probe e depois Enviar teste para validar a conta.",
        ],
      };
    }

    if (botTokenMode || accessTokenMode) {
      return {
        title: `${channel.name} • Token de autenticacao`,
        summary: `${docsSummary} Este canal costuma ser ativado com token simples ou JSON de credencial.`,
        credentialLabel: botTokenMode ? "Token do bot ou JSON" : "Access token ou JSON",
        credentialPlaceholder: botTokenMode ? "123456:ABC-DEF..." : '{"access_token":"..."}',
        credentialHint: "Voce pode colar o token puro ou um JSON com token/access_token conforme o adapter esperar.",
        metadataLabel: "Metadata (JSON)",
        metadataPlaceholder: '{"workspace":"ops"}',
        routingLabel: "Routing defaults (JSON)",
        routingPlaceholder: '{"target":"#alerts"}',
        adapterLabel: "Adapter config adicional (JSON)",
        adapterPlaceholder: '{"base_url":"https://api.exemplo","workspace":"ops"}',
        steps: [
          "Cole o token no campo de credencial.",
          "Salve a conta e rode Login para validar o token.",
          "Use Probe para checar saude da integracao.",
          "Envie uma mensagem teste no target final.",
        ],
      };
    }

    return {
      title: `${channel.name} • Fluxo generico`,
      summary: docsSummary,
      credentialLabel: "Credencial (token ou JSON)",
      credentialPlaceholder: '{"token":"..."}',
      credentialHint: "Use JSON quando o adapter exigir mais de um campo.",
      metadataLabel: "Metadata (JSON)",
      metadataPlaceholder: '{"workspace":"ops"}',
      routingLabel: "Routing defaults (JSON)",
      routingPlaceholder: '{"target":"#alerts"}',
      adapterLabel: "Adapter config (JSON)",
      adapterPlaceholder: '{"endpoint":"https://..."}',
      steps: [
        "Salve a conta com a configuracao necessaria.",
        "Execute Login ou Probe para validar a conexao.",
        "Envie uma mensagem teste para fechar o onboarding.",
      ],
    };
  }

  function renderSelectedChannelOnboarding(channel, account = null) {
    const blueprint = onboardingBlueprint(channel, account);

    if (elements.agentChannelOnboardingTitle) {
      elements.agentChannelOnboardingTitle.textContent = blueprint.title;
    }
    if (elements.agentChannelOnboardingSummary) {
      elements.agentChannelOnboardingSummary.textContent = blueprint.summary;
    }
    if (elements.agentChannelOnboardingSteps) {
      elements.agentChannelOnboardingSteps.innerHTML = (blueprint.steps || [])
        .map((step) => `<li>${step}</li>`)
        .join("");
    }
    if (elements.agentChannelCredentialsLabel) {
      elements.agentChannelCredentialsLabel.textContent = blueprint.credentialLabel;
    }
    if (elements.agentChannelCredentialHint) {
      elements.agentChannelCredentialHint.textContent = blueprint.credentialHint;
    }
    if (elements.agentChannelCredentialsInput && !elements.agentChannelCredentialsInput.value) {
      elements.agentChannelCredentialsInput.placeholder = blueprint.credentialPlaceholder;
    }
    if (elements.agentChannelMetadataLabel) {
      elements.agentChannelMetadataLabel.textContent = blueprint.metadataLabel;
    }
    if (elements.agentChannelMetadataInput && !elements.agentChannelMetadataInput.value) {
      elements.agentChannelMetadataInput.placeholder = blueprint.metadataPlaceholder;
    }
    if (elements.agentChannelRoutingDefaultsLabel) {
      elements.agentChannelRoutingDefaultsLabel.textContent = blueprint.routingLabel;
    }
    if (elements.agentChannelRoutingDefaultsInput && !elements.agentChannelRoutingDefaultsInput.value) {
      elements.agentChannelRoutingDefaultsInput.placeholder = blueprint.routingPlaceholder;
    }
    if (elements.agentChannelAdapterConfigLabel) {
      elements.agentChannelAdapterConfigLabel.textContent = blueprint.adapterLabel;
    }
    if (elements.agentChannelAdapterConfigInput && !elements.agentChannelAdapterConfigInput.value) {
      elements.agentChannelAdapterConfigInput.placeholder = blueprint.adapterPlaceholder;
    }
  }

  function setSessionActionButton(button, {
    label = null,
    hidden = false,
    disabled = false,
  } = {}) {
    if (!button) {
      return;
    }
    if (label) {
      button.textContent = label;
    }
    button.hidden = Boolean(hidden);
    button.disabled = Boolean(disabled);
  }

  function renderSessionCapabilityBadges(capabilities) {
    if (!elements.agentChannelSessionCapabilities) {
      return;
    }
    const values = Array.from(capabilities || []).sort();
    elements.agentChannelSessionCapabilities.innerHTML = values.map(
      (capability) => `<span class="agent-skill-tag">${capability}</span>`,
    ).join("");
  }

  function clearSelectedAccountQr() {
    if (elements.agentChannelQrPanel) {
      elements.agentChannelQrPanel.hidden = true;
    }
    if (elements.agentChannelQrText) {
      elements.agentChannelQrText.textContent = "-";
    }
    const qrFrame = elements.agentChannelQrCanvas?.parentElement;
    if (qrFrame) {
      qrFrame.classList.remove("qr-fallback");
    }
    if (elements.agentChannelQrCanvas) {
      elements.agentChannelQrCanvas.hidden = false;
      const context = elements.agentChannelQrCanvas.getContext?.("2d");
      if (context) {
        context.clearRect(
          0,
          0,
          elements.agentChannelQrCanvas.width,
          elements.agentChannelQrCanvas.height,
        );
      }
    }
    if (elements.agentChannelQrImage) {
      elements.agentChannelQrImage.hidden = true;
      elements.agentChannelQrImage.removeAttribute("src");
    }
  }

  function renderSelectedAccountQr(qrCode, qrDataUrl = "") {
    if (!qrCode && !qrDataUrl) {
      clearSelectedAccountQr();
      return;
    }

    if (elements.agentChannelQrPanel) {
      elements.agentChannelQrPanel.hidden = false;
    }
    if (elements.agentChannelQrText) {
      elements.agentChannelQrText.textContent = qrDataUrl
        ? "QR image fornecido pelo backend."
        : qrCode;
    }
    const qrFrame = elements.agentChannelQrCanvas?.parentElement;
    qrFrame?.classList.remove("qr-fallback");

    if (qrDataUrl && elements.agentChannelQrImage) {
      if (elements.agentChannelQrCanvas) {
        elements.agentChannelQrCanvas.hidden = true;
      }
      elements.agentChannelQrImage.src = qrDataUrl;
      elements.agentChannelQrImage.hidden = false;
      return;
    }

    if (elements.agentChannelQrImage) {
      elements.agentChannelQrImage.hidden = true;
      elements.agentChannelQrImage.removeAttribute("src");
    }

    if (typeof renderQrCode === "function" && elements.agentChannelQrCanvas) {
      elements.agentChannelQrCanvas.hidden = false;
      renderQrCode(elements.agentChannelQrCanvas, qrCode, { width: 220 })
        .catch(() => {
          qrFrame?.classList.add("qr-fallback");
          if (elements.agentChannelQrCanvas) {
            elements.agentChannelQrCanvas.hidden = true;
          }
        });
      return;
    }

    if (qrFrame) {
      qrFrame.classList.add("qr-fallback");
    }
    if (elements.agentChannelQrCanvas) {
      elements.agentChannelQrCanvas.hidden = true;
    }
  }

  function selectedChannelId() {
    return elements.agentChannelSelect?.value || "";
  }

  function selectedAccountId() {
    return String(elements.agentChannelAccountIdInput?.value || "").trim();
  }

  function updateSelectedAccountUi() {
    const channelId = selectedChannelId();
    const accountId = selectedAccountId();
    const channel = findChannelView(channelId);
    const account = findAccountView(channelId, accountId);
    const capabilities = channelCapabilitySet(channel, account);
    const { qrCode, qrDataUrl } = extractAccountQrPayload(account);

    renderSessionCapabilityBadges(capabilities);
    renderSelectedChannelOnboarding(channel, account);

    if (elements.agentChannelSessionTitle) {
      elements.agentChannelSessionTitle.textContent = channel
        ? `${channel.name}${accountId ? ` • ${accountId}` : ""}`
        : "Selecione um canal e uma conta";
    }

    if (!channel) {
      if (elements.agentChannelSessionStatus) {
        elements.agentChannelSessionStatus.textContent = "Selecione um canal para configurar a conexao.";
      }
      if (elements.agentChannelSessionMeta) {
        elements.agentChannelSessionMeta.textContent = "-";
      }
      setSessionActionButton(elements.agentChannelLoginBtn, { hidden: false, disabled: true, label: "Login" });
      setSessionActionButton(elements.agentChannelLogoutBtn, { hidden: false, disabled: true });
      setSessionActionButton(elements.agentChannelShowQrBtn, { hidden: true, disabled: true });
      clearSelectedAccountQr();
      return;
    }

    if (!accountId) {
      if (elements.agentChannelSessionStatus) {
        elements.agentChannelSessionStatus.textContent = "Informe um account_id ou escolha uma conta existente para fazer login.";
      }
      if (elements.agentChannelSessionMeta) {
        elements.agentChannelSessionMeta.textContent = `Canal ${channel.name} • protocolo ${channel.protocol_family}`;
      }
      setSessionActionButton(elements.agentChannelLoginBtn, {
        hidden: false,
        disabled: true,
        label: loginActionLabel(channel, null),
      });
      setSessionActionButton(elements.agentChannelLogoutBtn, { hidden: false, disabled: true });
      setSessionActionButton(elements.agentChannelShowQrBtn, { hidden: true, disabled: true });
      clearSelectedAccountQr();
      return;
    }

    if (!account) {
      if (elements.agentChannelSessionStatus) {
        elements.agentChannelSessionStatus.textContent = "Salve a conta antes de usar login, logout ou QRCode.";
      }
      if (elements.agentChannelSessionMeta) {
        elements.agentChannelSessionMeta.textContent = `Canal ${channel.name} • protocolo ${channel.protocol_family} • conta ainda nao cadastrada`;
      }
      setSessionActionButton(elements.agentChannelLoginBtn, {
        hidden: false,
        disabled: true,
        label: loginActionLabel(channel, null),
      });
      setSessionActionButton(elements.agentChannelLogoutBtn, { hidden: false, disabled: true });
      setSessionActionButton(elements.agentChannelShowQrBtn, { hidden: true, disabled: true });
      clearSelectedAccountQr();
      return;
    }

    if (elements.agentChannelSessionStatus) {
      elements.agentChannelSessionStatus.textContent = `Sessao ${account.session?.status || "idle"} • health ${account.health_state?.status || "-"}`;
    }
    if (elements.agentChannelSessionMeta) {
      const parts = [
        `Canal ${channel.name}`,
        `protocolo ${channel.protocol_family}`,
        `credencial ${account.credentials_ref || "nao definida"}`,
      ];
      if (account.is_default) {
        parts.push("conta default");
      }
      if (account.session?.session_dir) {
        parts.push(`sessao ${account.session.session_dir}`);
      }
      if (nonEmptyObject(account.adapter_config)) {
        parts.push("adapter_config configurado");
      }
      elements.agentChannelSessionMeta.textContent = parts.join(" • ");
    }
    if (elements.agentChannelCredentialsInput && account.credentials_ref) {
      elements.agentChannelCredentialsInput.placeholder = `Credencial mantida em ${account.credentials_ref}`;
    }

    setSessionActionButton(elements.agentChannelLoginBtn, {
      hidden: false,
      disabled: false,
      label: loginActionLabel(channel, account),
    });
    setSessionActionButton(elements.agentChannelLogoutBtn, {
      hidden: false,
      disabled: !account.session?.status || account.session.status === "idle" || account.session.status === "logged_out",
    });
    setSessionActionButton(elements.agentChannelShowQrBtn, {
      hidden: !supportsQrLogin(channel, account) || (!qrCode && !qrDataUrl),
      disabled: !qrCode && !qrDataUrl,
    });

    renderSelectedAccountQr(qrCode, qrDataUrl);
  }

  function syncSelectedChannelAccount(channelId) {
    const currentAccountId = selectedAccountId();
    if (!channelId) {
      updateSelectedAccountUi();
      return;
    }

    if (currentAccountId) {
      const existingAccount = findAccountView(channelId, currentAccountId);
      if (existingAccount) {
        populateChannelForm(channelId, currentAccountId);
        return;
      }
      updateSelectedAccountUi();
      return;
    }

    const channel = findChannelView(channelId);
    const preferredAccountId = channel?.accounts?.find((entry) => entry.is_default)?.account_id
      || channel?.accounts?.[0]?.account_id
      || "";
    if (preferredAccountId) {
      populateChannelForm(channelId, preferredAccountId);
      return;
    }
    updateSelectedAccountUi();
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
    syncSelectedChannelAccount(elements.agentChannelSelect?.value || "");
  }

  function populateChannelForm(channelId, accountId) {
    const channel = findChannelView(channelId);
    const account = channel?.accounts?.find((entry) => entry.account_id === accountId);
    if (!channel || !account) {
      updateSelectedAccountUi();
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
    if (elements.agentChannelAdapterConfigInput) {
      elements.agentChannelAdapterConfigInput.value = JSON.stringify(account.adapter_config || {}, null, 2);
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
    updateSelectedAccountUi();
  }

  function clearChannelForm() {
    if (elements.agentChannelAccountIdInput) elements.agentChannelAccountIdInput.value = "";
    if (elements.agentChannelCredentialsInput) {
      elements.agentChannelCredentialsInput.value = "";
      elements.agentChannelCredentialsInput.placeholder = '{"token":"..."} ou xoxb-...';
    }
    if (elements.agentChannelMetadataInput) elements.agentChannelMetadataInput.value = "";
    if (elements.agentChannelRoutingDefaultsInput) elements.agentChannelRoutingDefaultsInput.value = "";
    if (elements.agentChannelAdapterConfigInput) elements.agentChannelAdapterConfigInput.value = "";
    if (elements.agentChannelEnabledToggle) elements.agentChannelEnabledToggle.checked = true;
    if (elements.agentChannelSetDefaultToggle) elements.agentChannelSetDefaultToggle.checked = false;
    updateSelectedAccountUi();
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
        ? `<p class="meta-note agent-inline-note-danger">${channel.ambiguity_warning}</p>`
        : "";
      const protocol = channel.protocol_family
        ? `<div class="meta-note">protocolo: ${channel.protocol_family} ${channel.protocol_version || ""}</div>`
        : "";
      const accounts = (channel.accounts || []).map((account) => {
        const health = account.health_state?.status || "-";
        const session = account.session?.status || "-";
        const loginLabel = loginActionLabel(channel, account);
        const { qrCode, qrDataUrl } = extractAccountQrPayload(account);
        const qrCodeReady = Boolean(qrCode || qrDataUrl);
        const adapterConfigured = nonEmptyObject(account.adapter_config);
        const accountCapabilities = Array.isArray(account.capabilities) && account.capabilities.length
          ? `<div class="agent-skill-badges">${account.capabilities
              .map((capability) => `<span class="agent-skill-tag">${capability}</span>`)
              .join("")}</div>`
          : "";
        const qrStatusTag = qrCodeReady
          ? `<div class="agent-skill-badges"><span class="agent-skill-tag">qr pronto</span></div>`
          : "";
        const qrAction = qrCodeReady
          ? `<button class="ghost-btn text-sm" type="button" data-channel-action="show-qr" data-channel="${channel.id}" data-account="${account.account_id}">Ver QR</button>`
          : "";
        return `
          <div class="agent-channel-account-card">
            <div class="agent-channel-account-head">
              <div>
                <strong>${account.account_id}</strong>${account.is_default ? ' <span class="meta-note">(default)</span>' : ""}
                <div class="meta-note">health: ${health} • session: ${session}</div>
                <div class="meta-note">credencial: ${account.credentials_ref || "nao definida"}</div>
                <div class="meta-note">adapter_config: ${adapterConfigured ? "configurado" : "vazio"}</div>
                ${protocol}
                ${qrStatusTag}
                ${accountCapabilities}
              </div>
              <div class="agent-channel-account-actions">
                <button class="ghost-btn text-sm" type="button" data-channel-action="edit" data-channel="${channel.id}" data-account="${account.account_id}">Editar</button>
                <button class="ghost-btn text-sm" type="button" data-channel-action="login" data-channel="${channel.id}" data-account="${account.account_id}">${loginLabel}</button>
                ${qrAction}
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
        <section class="glass agent-channel-card">
          <div class="agent-channel-card-head">
            <div class="agent-channel-card-meta">
              <h4>${channel.name}</h4>
              <p class="meta-note">aliases: ${(channel.aliases || []).join(", ") || "-"}</p>
              <p class="meta-note">family: ${channel.protocol_family} • version: ${channel.protocol_version}</p>
              <div class="agent-skill-badges">${(channel.capabilities || [])
                .map((capability) => `<span class="agent-skill-tag">${capability}</span>`)
                .join("")}</div>
            </div>
            <span class="meta-note">${(channel.accounts || []).length} conta(s)</span>
          </div>
          ${warning}
          ${accounts || '<p class="meta-note">Sem contas configuradas.</p>'}
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

    const adapterConfig = parseOptionalJsonInput(elements.agentChannelAdapterConfigInput?.value, {});
    const effectiveAdapterConfig = channel === "whatsapp"
      && adapterConfig
      && typeof adapterConfig === "object"
      && !Array.isArray(adapterConfig)
      && !Object.keys(adapterConfig).length
      ? { backend: "embedded" }
      : adapterConfig;

    const payload = {
      channel,
      account_id: accountId,
      enabled: Boolean(elements.agentChannelEnabledToggle?.checked),
      credentials: parseCredentialsInput(elements.agentChannelCredentialsInput?.value),
      metadata: parseOptionalJsonInput(elements.agentChannelMetadataInput?.value, {}),
      routing_defaults: parseOptionalJsonInput(elements.agentChannelRoutingDefaultsInput?.value, {}),
      adapter_config: effectiveAdapterConfig,
      set_as_default: Boolean(elements.agentChannelSetDefaultToggle?.checked),
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
        elements.agentChannelActionFeedback.textContent = `${response.channel}:${response.account_id} • ${response.status}${response.message ? ` • ${response.message}` : ""}`;
      }
      await presentChannelLoginDialog(channelId, accountId, response);
      return;
    }
    if (action === "show-qr") {
      const channel = findChannelView(channelId);
      const account = channel?.accounts?.find((entry) => entry.account_id === accountId);
      const { qrCode, qrDataUrl } = extractAccountQrPayload(account);
      if (!qrCode && !qrDataUrl) {
        throw new Error("Nenhum QR code disponivel para esta conta.");
      }
      await presentChannelLoginDialog(channelId, accountId, {
        status: account.session.status,
        message: "Escaneie o QR code para concluir a conexao.",
        details: {
          ...(qrCode ? { qr_code: qrCode } : {}),
          ...(qrDataUrl ? { qr_image_data_url: qrDataUrl } : {}),
        },
      });
      return;
    }
    if (action === "logout") {
      const response = await executeChannelOperation("/agent/channels/logout", {
        channel: channelId,
        account_id: accountId,
      });
      if (elements.agentChannelActionFeedback) {
        elements.agentChannelActionFeedback.textContent = `${response.channel}:${response.account_id} • ${response.status}${response.message ? ` • ${response.message}` : ""}`;
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

  async function handleSelectedAccountAction(action) {
    const channelId = selectedChannelId();
    const accountId = selectedAccountId();
    if (!channelId || !accountId) {
      throw new Error("Selecione um canal e uma conta.");
    }
    if (!findAccountView(channelId, accountId)) {
      throw new Error("Salve a conta antes de executar esta acao.");
    }
    await handleAgentChannelListAction(action, channelId, accountId);
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
        syncSelectedChannelAccount(elements.agentChannelSelect.value);
      });
    }

    if (elements.agentChannelAccountIdInput) {
      elements.agentChannelAccountIdInput.addEventListener("input", () => {
        updateSelectedAccountUi();
      });
      elements.agentChannelAccountIdInput.addEventListener("change", () => {
        const channelId = selectedChannelId();
        const accountId = selectedAccountId();
        if (findAccountView(channelId, accountId)) {
          populateChannelForm(channelId, accountId);
          return;
        }
        updateSelectedAccountUi();
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

    if (elements.agentChannelLoginBtn) {
      elements.agentChannelLoginBtn.addEventListener("click", async () => {
        try {
          await handleSelectedAccountAction("login");
        } catch (error) {
          if (elements.agentChannelActionFeedback) {
            elements.agentChannelActionFeedback.textContent = `Falha: ${error.message}`;
          }
        }
      });
    }

    if (elements.agentChannelLogoutBtn) {
      elements.agentChannelLogoutBtn.addEventListener("click", async () => {
        try {
          await handleSelectedAccountAction("logout");
        } catch (error) {
          if (elements.agentChannelActionFeedback) {
            elements.agentChannelActionFeedback.textContent = `Falha: ${error.message}`;
          }
        }
      });
    }

    if (elements.agentChannelShowQrBtn) {
      elements.agentChannelShowQrBtn.addEventListener("click", async () => {
        try {
          await handleSelectedAccountAction("show-qr");
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
