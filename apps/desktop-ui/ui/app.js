import { ParticleSystem } from "./particles.js";
import { createAgentChannelsController } from "./agent-channels.js";
import { createAgentSkillsController } from "./agent-skills.js";
import { createAgentControlPlaneController } from "./agent-control-plane.js";

const STORAGE_DAEMON_URL = "mlxPilotDaemonUrl";
const STORAGE_CHAT_THREADS = "mlxPilotChatThreadsV2";
const STORAGE_AGENT_OBSERVABILITY_PREFIX = "mlxPilotAgentObservabilityV2";
const STORAGE_CHAT_WEBSEARCH_ENABLED = "mlxPilotWebsearchEnabled";
const STORAGE_CHAT_AIRLLM_ENABLED = "mlxPilotChatAirllmEnabled";

const STREAM_CHARS_PER_TICK = 22;
const STREAM_TICK_MS = 20;
const CHAT_SCROLL_THRESHOLD_PX = 72;
const OPENCLAW_LOG_POLL_MS = 1500;
const OPENCLAW_LOG_MAX_CHARS = 120000;
const DAEMON_BOOT_TIMEOUT_MS = 45000;
const DAEMON_BOOT_POLL_MS = 500;
const CHAT_MAX_TOKENS_DEFAULT = 512;
const CHAT_MAX_TOKENS_THINKING = 1536;
const ENVIRONMENT_ENDPOINT_CANDIDATES = ["/environment", "/openclaw/environment"];

const appShell = document.getElementById("app-shell");
const splashScreen = document.getElementById("splash-screen");
const mobileMenuBtn = document.getElementById("mobile-menu-btn");
const chatSidebar = document.getElementById("chat-sidebar");
const statusPill = document.getElementById("status-pill");

const daemonInput = document.getElementById("daemon-url");
const saveUrlBtn = document.getElementById("save-url");

const tabButtons = Array.from(document.querySelectorAll(".tab-btn[data-tab]"));
const agentTabLabel = document.getElementById("agent-tab-label");
const panelChat = document.getElementById("panel-chat");
const panelDiscover = document.getElementById("panel-discover");
const panelOpenClaw = document.getElementById("panel-openclaw");
const panelSettings = document.getElementById("panel-settings");
const panelAiInteraction = document.getElementById("panel-ai-interaction");
const panelAgent = document.getElementById("panel-agent");
const chatEmptyHero = document.getElementById("chat-empty-hero");

const chatModelSwitcher = document.getElementById("chat-model-switcher");
const chatModelTrigger = document.getElementById("chat-model-trigger");
const chatModelCurrent = document.getElementById("chat-model-current");
const chatModelMenu = document.getElementById("chat-model-menu");
const chatModelSelect = document.getElementById("chat-model-select");
const chatAirllmToggleBtn = document.getElementById("chat-airllm-toggle");
const refreshModelsBtn = document.getElementById("refresh-models");
const newChatThreadTopBtn = document.getElementById("new-chat-thread-top");

const chatHistoryMeta = document.getElementById("chat-history-meta");
const chatHistoryList = document.getElementById("chat-history-list");
const newChatThreadButtons = Array.from(document.querySelectorAll('[data-action="new-chat"]'));

const selectedThreadLabel = document.getElementById("selected-thread-label");
const selectedModelLabel = document.getElementById("selected-model-label");
const chatForm = document.getElementById("chat-form");
const chatLog = document.getElementById("chat-log");
const messageInput = document.getElementById("message-input");
const chatWebsearchBtn = document.getElementById("chat-websearch-btn");
const chatWebsearchToggle = document.getElementById("chat-websearch-toggle");
const sendMessageBtn = chatForm.querySelector('button[type="submit"]');
const stopGenerationBtn = document.getElementById("stop-generation");
const cancelEditBtn = document.getElementById("cancel-edit");
const messageTemplate = document.getElementById("message-template");
const assistantStreamTemplate = document.getElementById("assistant-stream-template");

const catalogSource = document.getElementById("catalog-source");
const catalogQuery = document.getElementById("catalog-query");
const catalogSearchBtn = document.getElementById("catalog-search-btn");
const catalogMeta = document.getElementById("catalog-meta");
const remoteResults = document.getElementById("remote-results");
const discoverSubtabButtons = Array.from(document.querySelectorAll("[data-discover-subtab]"));
const discoverCatalogView = document.getElementById("discover-view-catalog");
const discoverInstalledView = document.getElementById("discover-view-installed");
const remoteCardTemplate = document.getElementById("remote-card-template");
const refreshDownloadsBtn = document.getElementById("refresh-downloads");
const downloadList = document.getElementById("download-list");
const downloadItemTemplate = document.getElementById("download-item-template");
const refreshInstalledModelsBtn = document.getElementById("refresh-installed-models");
const installedModelsMeta = document.getElementById("installed-models-meta");
const installedModelsList = document.getElementById("installed-models-list");

const openclawStatusText = document.getElementById("openclaw-status-text");
const openclawRuntimeMeta = document.getElementById("openclaw-runtime-meta");
const refreshOpenclawStatusBtn = document.getElementById("refresh-openclaw-status");
const openclawStartBtn = document.getElementById("openclaw-start-btn");
const openclawStopBtn = document.getElementById("openclaw-stop-btn");
const openclawRestartBtn = document.getElementById("openclaw-restart-btn");
const agentPanelEyebrow = document.getElementById("agent-panel-eyebrow");
const agentPanelTitle = document.getElementById("agent-panel-title");
const agentSubtabsLabel = document.getElementById("agent-subtabs-label");
const agentChatTitle = document.getElementById("agent-chat-title");
const agentLogsTitle = document.getElementById("agent-logs-title");
const agentObservabilityTitle = document.getElementById("agent-observability-title");
const agentConfigTitle = document.getElementById("agent-config-title");
const agentLogOptionGateway = document.getElementById("agent-log-option-gateway");
const agentLogOptionError = document.getElementById("agent-log-option-error");
const agentLogOptionSync = document.getElementById("agent-log-option-sync");

const openclawViewButtons = Array.from(document.querySelectorAll(".openclaw-view-btn"));
const openclawMultiViewToggle = document.getElementById("openclaw-multi-view");
const openclawPanelsRoot = document.getElementById("openclaw-panels");
const openclawPanelChat = document.getElementById("openclaw-panel-chat");
const openclawPanelLogs = document.getElementById("openclaw-panel-logs");
const openclawPanelObservability = document.getElementById("openclaw-panel-observability");
const openclawPanelConfig = document.getElementById("openclaw-panel-config");

const openclawChatForm = document.getElementById("openclaw-chat-form");
const openclawChatLog = document.getElementById("openclaw-chat-log");
const openclawMessageInput = document.getElementById("openclaw-message-input");
const openclawSendBtn = openclawChatForm.querySelector('button[type="submit"]');

const openclawLogStreamSelect = document.getElementById("openclaw-log-stream");
const refreshOpenclawLogBtn = document.getElementById("refresh-openclaw-log");
const clearOpenclawLogBtn = document.getElementById("clear-openclaw-log");
const openclawLogMeta = document.getElementById("openclaw-log-meta");
const openclawLogViewer = document.getElementById("openclaw-log-viewer");

const openclawProviderModel = document.getElementById("openclaw-provider-model");
const openclawUsage = document.getElementById("openclaw-usage");
const openclawSkills = document.getElementById("openclaw-skills");
const openclawTools = document.getElementById("openclaw-tools");

const openclawModelSource = document.getElementById("openclaw-model-source");
const openclawCloudPicker = document.getElementById("openclaw-cloud-picker");
const openclawLocalPicker = document.getElementById("openclaw-local-picker");
const nanobotModelPicker = document.getElementById("nanobot-model-picker");
const nanobotModelInput = document.getElementById("nanobot-model-input");
const openclawCloudModelSelect = document.getElementById("openclaw-cloud-model-select");
const openclawLocalModelSelect = document.getElementById("openclaw-local-model-select");
const refreshOpenclawModelsBtn = document.getElementById("refresh-openclaw-models");
const applyOpenclawModelBtn = document.getElementById("apply-openclaw-model");
const openclawModelCurrent = document.getElementById("openclaw-model-current");
const openclawConfigFeedback = document.getElementById("openclaw-config-feedback");
const openclawModelSourceLabel = document.getElementById("openclaw-model-source-label");
const openclawModelSourceCloudOption = document.getElementById("openclaw-model-source-cloud-option");
const openclawModelSourceLocalOption = document.getElementById("openclaw-model-source-local-option");
const openclawCloudLabel = document.getElementById("openclaw-cloud-label");
const openclawLocalLabel = document.getElementById("openclaw-local-label");

const settingModelsDir = document.getElementById("setting-models-dir");
const discoverModelsDir = document.getElementById("discover-models-dir");
const discoverModelsFeedback = document.getElementById("discover-models-feedback");
const settingOpenclawCli = document.getElementById("setting-openclaw-cli");
const settingOpenclawState = document.getElementById("setting-openclaw-state");
const saveSettingsBtn = document.getElementById("save-settings-btn");
const installOpenclawBtn = document.getElementById("install-openclaw-btn");
const installOpenclawFeedback = document.getElementById("install-openclaw-feedback");
const checkOpenclawStatusBtn = document.getElementById("check-openclaw-status-btn");
const openclawInstallStatusFeedback = document.getElementById("openclaw-install-status-feedback");
const openclawInstallStatusOutput = document.getElementById("openclaw-install-status-output");

const frameworkRadios = document.querySelectorAll('input[name="agent-framework"]');
const openclawSettingsGroup = document.getElementById("openclaw-settings-group");
const nanobotSettingsGroup = document.getElementById("nanobot-settings-group");
const settingNanobotCli = document.getElementById("setting-nanobot-cli");
const installNanobotBtn = document.getElementById("install-nanobot-btn");
const installNanobotFeedback = document.getElementById("install-nanobot-feedback");
const checkNanobotStatusBtn = document.getElementById("check-nanobot-status-btn");
const initNanobotBtn = document.getElementById("init-nanobot-btn");
const nanobotStatusFeedback = document.getElementById("nanobot-status-feedback");
const nanobotStatusOutput = document.getElementById("nanobot-status-output");
const refreshOpenclawEnvBtn = document.getElementById("refresh-openclaw-env-btn");
const saveOpenclawEnvBtn = document.getElementById("save-openclaw-env-btn");
const revealOpenclawEnvToggle = document.getElementById("reveal-openclaw-env");
const openclawEnvFeedback = document.getElementById("openclaw-env-feedback");
const openclawEnvPath = document.getElementById("openclaw-env-path");
const openclawEnvList = document.getElementById("openclaw-env-list");

const agentMeta = document.getElementById("agent-meta");
const agentRefreshBtn = document.getElementById("agent-refresh-btn");
const agentSaveConfigBtn = document.getElementById("agent-save-config-btn");
const agentPluginsRefreshBtn = document.getElementById("agent-plugins-refresh-btn");
const agentPluginsList = document.getElementById("agent-plugins-list");
const agentPluginDetailTitle = document.getElementById("agent-plugin-detail-title");
const agentPluginDetailMeta = document.getElementById("agent-plugin-detail-meta");
const agentPluginConfigEnabled = document.getElementById("agent-plugin-config-enabled");
const agentPluginConfigForm = document.getElementById("agent-plugin-config-form");
const agentPluginSaveBtn = document.getElementById("agent-plugin-save-btn");
const agentPluginResetBtn = document.getElementById("agent-plugin-reset-btn");
const agentPluginFeedback = document.getElementById("agent-plugin-feedback");
const agentProviderSelect = document.getElementById("agent-provider-select");
const agentModelSelect = document.getElementById("agent-model-select");
const agentApiKeyInput = document.getElementById("agent-api-key-input");
const agentBaseUrlInput = document.getElementById("agent-base-url-input");
const agentStreamingToggle = document.getElementById("agent-streaming-toggle");
const agentFallbackToggle = document.getElementById("agent-fallback-toggle");
const agentFallbackProviderSelect = document.getElementById("agent-fallback-provider-select");
const agentFallbackModelInput = document.getElementById("agent-fallback-model-input");
const agentExecutionModeSelect = document.getElementById("agent-execution-mode-select");
const agentApprovalModeSelect = document.getElementById("agent-approval-mode-select");
const agentMaxPromptInput = document.getElementById("agent-max-prompt-input");
const agentMaxHistoryInput = document.getElementById("agent-max-history-input");
const agentMaxToolsInput = document.getElementById("agent-max-tools-input");
const agentAggressiveToolsToggle = document.getElementById("agent-aggressive-tools-toggle");
const agentToolFallbackToggle = document.getElementById("agent-tool-fallback-toggle");
const agentToolProfileSelect = document.getElementById("agent-tool-profile-select");
const agentToolProfileApplyBtn = document.getElementById("agent-tool-profile-apply-btn");
const agentPolicyScopeSelect = document.getElementById("agent-policy-scope-select");
const agentPolicyAllowInput = document.getElementById("agent-policy-allow-input");
const agentPolicyDenyInput = document.getElementById("agent-policy-deny-input");
const agentPolicySaveBtn = document.getElementById("agent-policy-save-btn");
const agentPolicyResetBtn = document.getElementById("agent-policy-reset-btn");
const agentPolicyFeedback = document.getElementById("agent-policy-feedback");
const agentToolCatalogSummary = document.getElementById("agent-tool-catalog-summary");
const agentEffectivePolicyList = document.getElementById("agent-effective-policy-list");
const agentReloadSkillsBtn = document.getElementById("agent-reload-skills-btn");
const agentCheckSkillsBtn = document.getElementById("agent-check-skills-btn");
const agentConfigureSkillsBtn = document.getElementById("agent-configure-skills-btn");
const agentInstallSkillsBtn = document.getElementById("agent-install-skills-btn");
const agentNodeManagerSelect = document.getElementById("agent-node-manager-select");
const agentSkillsSummary = document.getElementById("agent-skills-summary");
const agentSkillsList = document.getElementById("agent-skills-list");
const agentToolsList = document.getElementById("agent-tools-list");
const agentEgressInput = document.getElementById("agent-egress-input");
const agentSensitivePathsInput = document.getElementById("agent-sensitive-paths-input");
const agentMaxPromptValue = document.getElementById("agent-max-prompt-value");
const agentMaxHistoryValue = document.getElementById("agent-max-history-value");
const agentMaxToolsValue = document.getElementById("agent-max-tools-value");
const agentMemoryLocalToggle = document.getElementById("agent-memory-local-toggle");
const agentMemoryBackendSelect = document.getElementById("agent-memory-backend-select");
const agentMemoryCompressionSelect = document.getElementById("agent-memory-compression-select");
const agentMemorySaveBtn = document.getElementById("agent-memory-save-btn");
const agentMemoryFeedback = document.getElementById("agent-memory-feedback");
const agentBudgetRefreshBtn = document.getElementById("agent-budget-refresh-btn");
const agentBudgetTelemetry = document.getElementById("agent-budget-telemetry");
const agentChannelsRefreshBtn = document.getElementById("agent-channels-refresh-btn");
const agentChannelSelect = document.getElementById("agent-channel-select");
const agentChannelAccountIdInput = document.getElementById("agent-channel-account-id");
const agentChannelCredentialsLabel = document.getElementById("agent-channel-credentials-label");
const agentChannelCredentialsInput = document.getElementById("agent-channel-credentials");
const agentChannelCredentialHint = document.getElementById("agent-channel-credential-hint");
const agentChannelMetadataLabel = document.getElementById("agent-channel-metadata-label");
const agentChannelMetadataInput = document.getElementById("agent-channel-metadata");
const agentChannelRoutingDefaultsLabel = document.getElementById("agent-channel-routing-defaults-label");
const agentChannelRoutingDefaultsInput = document.getElementById("agent-channel-routing-defaults");
const agentChannelAdapterConfigLabel = document.getElementById("agent-channel-adapter-config-label");
const agentChannelAdapterConfigInput = document.getElementById("agent-channel-adapter-config");
const agentChannelOnboardingTitle = document.getElementById("agent-channel-onboarding-title");
const agentChannelOnboardingSummary = document.getElementById("agent-channel-onboarding-summary");
const agentChannelOnboardingSteps = document.getElementById("agent-channel-onboarding-steps");
const agentChannelEnabledToggle = document.getElementById("agent-channel-enabled");
const agentChannelSetDefaultToggle = document.getElementById("agent-channel-set-default");
const agentChannelSaveBtn = document.getElementById("agent-channel-save-btn");
const agentChannelClearBtn = document.getElementById("agent-channel-clear-btn");
const agentChannelFormFeedback = document.getElementById("agent-channel-form-feedback");
const agentChannelSessionTitle = document.getElementById("agent-channel-session-title");
const agentChannelSessionStatus = document.getElementById("agent-channel-session-status");
const agentChannelSessionMeta = document.getElementById("agent-channel-session-meta");
const agentChannelSessionCapabilities = document.getElementById("agent-channel-session-capabilities");
const agentChannelLoginBtn = document.getElementById("agent-channel-login-btn");
const agentChannelLogoutBtn = document.getElementById("agent-channel-logout-btn");
const agentChannelShowQrBtn = document.getElementById("agent-channel-show-qr-btn");
const agentChannelQrPanel = document.getElementById("agent-channel-qr-panel");
const agentChannelQrCanvas = document.getElementById("agent-channel-qr-canvas");
const agentChannelQrText = document.getElementById("agent-channel-qr-text");
const agentSendChannelSelect = document.getElementById("agent-send-channel");
const agentSendAccountSelect = document.getElementById("agent-send-account");
const agentSendTargetInput = document.getElementById("agent-send-target");
const agentSendMessageInput = document.getElementById("agent-send-message");
const agentSendTestBtn = document.getElementById("agent-send-test-btn");
const agentProbeChannelBtn = document.getElementById("agent-probe-channel-btn");
const agentResolveTargetBtn = document.getElementById("agent-resolve-target-btn");
const agentChannelActionFeedback = document.getElementById("agent-channel-action-feedback");
const agentChannelList = document.getElementById("agent-channel-list");
const agentChannelLogsRefreshBtn = document.getElementById("agent-channel-logs-refresh-btn");
const agentChannelLogsChannelSelect = document.getElementById("agent-channel-logs-channel");
const agentChannelLogsAccountSelect = document.getElementById("agent-channel-logs-account");
const agentChannelLogsList = document.getElementById("agent-channel-logs-list");
const agentAuditList = document.getElementById("agent-audit-list");
const agentChatStatus = document.getElementById("agent-chat-status");
const agentChatLog = document.getElementById("agent-chat-log");
const agentChatForm = document.getElementById("agent-chat-form");
const agentMessageInput = document.getElementById("agent-message-input");
const agentSendBtn = document.getElementById("agent-send-btn");
const agentRuntimeRefreshBtn = document.getElementById("agent-runtime-refresh-btn");
const agentRuntimeSummary = document.getElementById("agent-runtime-summary");
const agentRuntimeFrameworkMeta = document.getElementById("agent-runtime-framework-meta");
const agentRuntimeDiagnosticsList = document.getElementById("agent-runtime-diagnostics-list");
const agentRuntimeLogMode = document.getElementById("agent-runtime-log-mode");
const agentRuntimeLogSource = document.getElementById("agent-runtime-log-source");
const agentRuntimeLogAccount = document.getElementById("agent-runtime-log-account");
const agentRuntimeLogList = document.getElementById("agent-runtime-log-list");

// Agent Observability Console (Audit Feed)
const auditFilterSession = document.getElementById("audit-filter-session");
const auditFilterEvent = document.getElementById("audit-filter-event");
const auditFilterStatus = document.getElementById("audit-filter-status");
const auditFilterTool = document.getElementById("audit-filter-tool");
const auditFollowToggle = document.getElementById("audit-follow-toggle");
const auditExportBtn = document.getElementById("audit-export-btn");
const agentObservabilityList = document.getElementById("agent-observability-list");

const auditDetailPanel = document.getElementById("audit-detail-panel");
const auditDetailContent = document.getElementById("audit-detail-content");
const auditDetailCloseBtn = document.getElementById("audit-detail-close");

let auditPollTimer = null;
let auditConsoleLastParams = "";
let auditActiveSessionFollowCache = null;

const agentSessionSelect = document.getElementById("agent-session-select");
const agentNewSessionBtn = document.getElementById("agent-new-session-btn");
const agentRenameSessionBtn = document.getElementById("agent-rename-session-btn");
const agentExportSessionBtn = document.getElementById("agent-export-session-btn");
const agentDeleteSessionBtn = document.getElementById("agent-delete-session-btn");

let daemonBaseUrl = localStorage.getItem(STORAGE_DAEMON_URL) || "http://127.0.0.1:11435";
let selectedModelId = null;
let localModels = [];
let chatAirllmEnabled = true;
let chatAirllmPersistInFlight = false;

let chatThreads = [];
let activeThreadId = null;
let openThreadMenuId = null;

let activeTab = "chat";
let activeDiscoverSubtab = "catalog";
let activeAgentFramework = "openclaw";
let chatModelMenuOpen = false;
let isGenerating = false;
let activeStreamController = null;
let streamFallbackNotified = false;
let pendingEditMessageIndex = null;

let agentSessions = [];
let activeAgentSessionId = null;

let lastDownloadsFingerprint = "";
let downloadsTimer = null;

const aiParticleInput = document.getElementById("ai-particle-input");
const aiParticleBtn = document.getElementById("ai-particle-btn");
const aiParticleStopBtn = document.getElementById("ai-particle-stop-btn");
const aiSceneStatus = document.getElementById("ai-scene-status");
const aiSceneSteps = document.getElementById("ai-scene-steps");
const aiSceneExampleButtons = Array.from(document.querySelectorAll(".ai-scene-example-btn"));

let aiSceneInFlight = false;
let aiSceneLastScript = null;
let aiSceneAnimating = false;
let aiSceneRequestToken = 0;
let openclawInstallInFlight = false;
let openclawStatusCheckInFlight = false;

let openclawStatusLoaded = false;
let openclawObservabilityLoaded = false;
let openclawChatInFlight = false;
let openclawLogsTimer = null;
let openclawActiveLogStream = "gateway";
let openclawLogCursor = 0;
let openclawRuntimeActionInFlight = false;
let openclawSelectedViews = new Set(["chat"]);
let openclawMultiView = false;
let openclawEnvironmentInFlight = false;
let resolvedEnvironmentEndpoint = null;
let openclawModelsCatalog = {
  cloud_models: [],
  local_models: [],
  current: null,
};

let agentProvidersCatalog = [];
let agentModelsByProvider = {};
let agentConfigCache = null;
let agentToolsCache = [];
let agentSkillsController = null;
let agentControlPlaneController = null;

daemonInput.value = daemonBaseUrl;

function setStatus(text, tone = "normal") {
  statusPill.textContent = text;
  statusPill.classList.remove("error", "running");
  if (tone === "error") {
    statusPill.classList.add("error");
  }
  if (tone === "running") {
    statusPill.classList.add("running");
  }
}

function createHttpError(status, message) {
  const error = new Error(message);
  error.status = status;
  return error;
}

async function parseErrorDetail(response) {
  let detail = `HTTP ${response.status}`;

  try {
    const body = await response.json();
    if (body?.error) {
      detail = body.error_code ? `${body.error_code}: ${body.error}` : body.error;
    }
  } catch {
    // ignore parse errors
  }

  return detail;
}

async function fetchJson(path, options = {}) {
  const response = await fetch(`${daemonBaseUrl}${path}`, options);
  if (!response.ok) {
    const detail = await parseErrorDetail(response);
    throw createHttpError(response.status, detail);
  }

  return response.json();
}

function delay(ms) {
  return new Promise((resolve) => {
    window.setTimeout(resolve, ms);
  });
}

function showTextPrompt({ title, message = "", defaultValue = "", confirmLabel = "Salvar", cancelLabel = "Cancelar" }) {
  if (!document?.body) {
    return Promise.resolve(window.prompt(title || "", defaultValue || ""));
  }

  return new Promise((resolve) => {
    const backdrop = document.createElement("div");
    backdrop.className = "app-dialog-backdrop";

    const dialog = document.createElement("div");
    dialog.className = "app-dialog-card";

    const heading = document.createElement("h3");
    heading.className = "app-dialog-title";
    heading.textContent = title || "Atualizar valor";

    const body = document.createElement("p");
    body.className = "app-dialog-message";
    body.textContent = message || "";

    const input = document.createElement("input");
    input.className = "input app-dialog-input";
    input.value = defaultValue || "";
    input.autocomplete = "off";

    const actions = document.createElement("div");
    actions.className = "app-dialog-actions";

    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "ghost-btn";
    cancelBtn.textContent = cancelLabel;

    const confirmBtn = document.createElement("button");
    confirmBtn.type = "button";
    confirmBtn.className = "primary-btn";
    confirmBtn.textContent = confirmLabel;

    const cleanup = (result) => {
      window.removeEventListener("keydown", onKeydown);
      backdrop.remove();
      resolve(result);
    };

    const onConfirm = () => cleanup(input.value);
    const onCancel = () => cleanup(null);
    const onKeydown = (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        onConfirm();
      } else if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
      }
    };

    backdrop.addEventListener("click", (event) => {
      if (event.target === backdrop) {
        onCancel();
      }
    });
    cancelBtn.addEventListener("click", onCancel);
    confirmBtn.addEventListener("click", onConfirm);
    window.addEventListener("keydown", onKeydown);

    actions.appendChild(cancelBtn);
    actions.appendChild(confirmBtn);
    dialog.appendChild(heading);
    if (message) {
      dialog.appendChild(body);
    }
    dialog.appendChild(input);
    dialog.appendChild(actions);
    backdrop.appendChild(dialog);
    document.body.appendChild(backdrop);

    input.focus();
    input.select();
  });
}

function showConfirmDialog({ title, message = "", confirmLabel = "Confirmar", cancelLabel = "Cancelar", danger = false }) {
  if (!document?.body) {
    return Promise.resolve(window.confirm(message || title || ""));
  }

  return new Promise((resolve) => {
    const backdrop = document.createElement("div");
    backdrop.className = "app-dialog-backdrop";

    const dialog = document.createElement("div");
    dialog.className = "app-dialog-card";

    const heading = document.createElement("h3");
    heading.className = "app-dialog-title";
    heading.textContent = title || "Confirmar";

    const body = document.createElement("p");
    body.className = "app-dialog-message";
    body.textContent = message || "";

    const actions = document.createElement("div");
    actions.className = "app-dialog-actions";

    const cancelBtn = document.createElement("button");
    cancelBtn.type = "button";
    cancelBtn.className = "ghost-btn";
    cancelBtn.textContent = cancelLabel;

    const confirmBtn = document.createElement("button");
    confirmBtn.type = "button";
    confirmBtn.className = danger ? "ghost-btn danger" : "primary-btn";
    confirmBtn.textContent = confirmLabel;

    const cleanup = (result) => {
      window.removeEventListener("keydown", onKeydown);
      backdrop.remove();
      resolve(result);
    };

    const onConfirm = () => cleanup(true);
    const onCancel = () => cleanup(false);
    const onKeydown = (event) => {
      if (event.key === "Enter") {
        event.preventDefault();
        onConfirm();
      } else if (event.key === "Escape") {
        event.preventDefault();
        onCancel();
      }
    };

    backdrop.addEventListener("click", (event) => {
      if (event.target === backdrop) {
        onCancel();
      }
    });
    cancelBtn.addEventListener("click", onCancel);
    confirmBtn.addEventListener("click", onConfirm);
    window.addEventListener("keydown", onKeydown);

    actions.appendChild(cancelBtn);
    actions.appendChild(confirmBtn);
    dialog.appendChild(heading);
    if (message) {
      dialog.appendChild(body);
    }
    dialog.appendChild(actions);
    backdrop.appendChild(dialog);
    document.body.appendChild(backdrop);

    confirmBtn.focus();
  });
}

let qrCodeLibraryPromise = null;

function loadQrCodeLibrary() {
  if (window.QRCode?.toCanvas) {
    return Promise.resolve(window.QRCode);
  }
  if (qrCodeLibraryPromise) {
    return qrCodeLibraryPromise;
  }

  qrCodeLibraryPromise = new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "https://cdn.jsdelivr.net/npm/qrcode@1.5.4/build/qrcode.min.js";
    script.async = true;
    script.onload = () => {
      if (window.QRCode?.toCanvas) {
        resolve(window.QRCode);
      } else {
        reject(new Error("Biblioteca de QR code indisponivel."));
      }
    };
    script.onerror = () => {
      reject(new Error("Falha ao carregar a biblioteca de QR code."));
    };
    document.head.appendChild(script);
  });

  return qrCodeLibraryPromise;
}

function renderQrCodeToCanvas(canvas, qrCode, options = {}) {
  if (!canvas || !qrCode) {
    return Promise.resolve();
  }

  return loadQrCodeLibrary()
    .then((QRCode) => QRCode.toCanvas(canvas, qrCode, {
      width: options.width || 260,
      margin: 1,
      color: {
        dark: "#10131a",
        light: "#ffffff",
      },
    }));
}

function sanitizeDialogDetails(details = {}, sessionState = null) {
  const merged = {
    ...(details && typeof details === "object" && !Array.isArray(details) ? details : {}),
  };

  if (sessionState?.session_dir && !merged.session_dir) {
    merged.session_dir = sessionState.session_dir;
  }
  if (sessionState?.connected_at_epoch_ms && !merged.connected_at_epoch_ms) {
    merged.connected_at_epoch_ms = sessionState.connected_at_epoch_ms;
  }
  if (sessionState?.status && !merged.session_status) {
    merged.session_status = sessionState.status;
  }

  return Object.fromEntries(
    Object.entries(merged).filter(([, value]) => value != null && value !== ""),
  );
}

function showChannelLoginDialog({
  title,
  channelName,
  channelId,
  accountId,
  status = "connected",
  message = "",
  qrCode = null,
  details = null,
  sessionState = null,
}) {
  if (!document?.body) {
    return Promise.resolve();
  }

  return new Promise((resolve) => {
    const backdrop = document.createElement("div");
    backdrop.className = "app-dialog-backdrop";

    const dialog = document.createElement("div");
    dialog.className = "app-dialog-card channel-auth-dialog";

    const heading = document.createElement("h3");
    heading.className = "app-dialog-title";
    heading.textContent = title || `${channelName || channelId || "Canal"} • ${accountId || "-"}`;

    const summary = document.createElement("div");
    summary.className = "channel-auth-summary";

    const subtitle = document.createElement("p");
    subtitle.className = "channel-auth-subtitle";
    subtitle.textContent = `${channelName || channelId || "canal"} • conta ${accountId || "-"}`;
    summary.appendChild(subtitle);

    const badges = document.createElement("div");
    badges.className = "channel-auth-badges";

    const statusBadge = document.createElement("span");
    statusBadge.className = `channel-auth-badge status-${String(status || "connected").toLowerCase()}`;
    statusBadge.textContent = status || "connected";
    badges.appendChild(statusBadge);

    if (qrCode) {
      const qrBadge = document.createElement("span");
      qrBadge.className = "channel-auth-badge";
      qrBadge.textContent = "qr-login";
      badges.appendChild(qrBadge);
    }

    summary.appendChild(badges);

    if (message) {
      const body = document.createElement("p");
      body.className = "app-dialog-message";
      body.textContent = message;
      summary.appendChild(body);
    }

    dialog.appendChild(heading);
    dialog.appendChild(summary);

    let qrRawText = null;
    if (qrCode) {
      const qrSection = document.createElement("section");
      qrSection.className = "channel-auth-qr-section";

      const qrLabel = document.createElement("p");
      qrLabel.className = "channel-auth-section-title";
      qrLabel.textContent = "Escaneie este QR Code";
      qrSection.appendChild(qrLabel);

      const qrFrame = document.createElement("div");
      qrFrame.className = "channel-auth-qr-frame";

      const qrCanvas = document.createElement("canvas");
      qrCanvas.className = "channel-auth-qr-canvas";
      qrFrame.appendChild(qrCanvas);
      qrSection.appendChild(qrFrame);

      qrRawText = document.createElement("code");
      qrRawText.className = "channel-auth-qr-raw";
      qrRawText.textContent = qrCode;
      qrSection.appendChild(qrRawText);

      dialog.appendChild(qrSection);

      renderQrCodeToCanvas(qrCanvas, qrCode, { width: 260 })
        .catch(() => {
          qrFrame.classList.add("qr-fallback");
          qrFrame.textContent = "Nao foi possivel renderizar o QR automaticamente. Use o codigo abaixo.";
        });
    }

    const normalizedDetails = sanitizeDialogDetails(details, sessionState);
    if (Object.keys(normalizedDetails).length) {
      const detailsSection = document.createElement("section");
      detailsSection.className = "channel-auth-details-section";

      const detailsLabel = document.createElement("p");
      detailsLabel.className = "channel-auth-section-title";
      detailsLabel.textContent = "Detalhes da conexao";
      detailsSection.appendChild(detailsLabel);

      const detailsPre = document.createElement("pre");
      detailsPre.className = "channel-auth-details";
      detailsPre.textContent = JSON.stringify(normalizedDetails, null, 2);
      detailsSection.appendChild(detailsPre);

      dialog.appendChild(detailsSection);
    }

    const actions = document.createElement("div");
    actions.className = "app-dialog-actions";

    const closeBtn = document.createElement("button");
    closeBtn.type = "button";
    closeBtn.className = "primary-btn";
    closeBtn.textContent = "Fechar";
    actions.appendChild(closeBtn);

    if (qrCode) {
      const copyBtn = document.createElement("button");
      copyBtn.type = "button";
      copyBtn.className = "ghost-btn";
      copyBtn.textContent = "Copiar codigo";
      copyBtn.addEventListener("click", async () => {
        try {
          await navigator.clipboard.writeText(qrCode);
          copyBtn.textContent = "Copiado";
          window.setTimeout(() => {
            copyBtn.textContent = "Copiar codigo";
          }, 1200);
        } catch {
          if (qrRawText) {
            const selection = window.getSelection();
            const range = document.createRange();
            range.selectNodeContents(qrRawText);
            selection.removeAllRanges();
            selection.addRange(range);
          }
        }
      });
      actions.appendChild(copyBtn);
    }

    const cleanup = () => {
      window.removeEventListener("keydown", onKeydown);
      backdrop.remove();
      resolve();
    };

    const onKeydown = (event) => {
      if (event.key === "Escape" || event.key === "Enter") {
        event.preventDefault();
        cleanup();
      }
    };

    closeBtn.addEventListener("click", cleanup);
    backdrop.addEventListener("click", (event) => {
      if (event.target === backdrop) {
        cleanup();
      }
    });
    window.addEventListener("keydown", onKeydown);

    dialog.appendChild(actions);
    backdrop.appendChild(dialog);
    document.body.appendChild(backdrop);
    closeBtn.focus();
  });
}

function hideSplash() {
  if (splashScreen) {
    splashScreen.classList.add("hidden");
  }
}

async function waitForDaemonReady() {
  const startedAt = Date.now();
  let lastError = "sem resposta";

  while (Date.now() - startedAt < DAEMON_BOOT_TIMEOUT_MS) {
    try {
      const response = await fetch(`${daemonBaseUrl}/health`, {
        method: "GET",
        cache: "no-store",
      });

      if (response.ok) {
        return true;
      }
      lastError = `HTTP ${response.status}`;
    } catch (error) {
      lastError = error?.message || "falha de conexao";
    }

    await delay(DAEMON_BOOT_POLL_MS);
  }

  addSystemMessage(`Daemon indisponivel em ${daemonBaseUrl}: ${lastError}`);
  return false;
}

function isNanobotActive() {
  return activeAgentFramework === "nanobot";
}

function activeAgentLabel() {
  return isNanobotActive() ? "NanoBot" : "OpenClaw";
}

function activeAgentEndpoint(path) {
  const normalized = String(path || "").replace(/^\/+/, "");
  const prefix = isNanobotActive() ? "/nanobot" : "/openclaw";
  return `${prefix}/${normalized}`;
}

function observabilityStorageKey() {
  return `${STORAGE_AGENT_OBSERVABILITY_PREFIX}:${activeAgentFramework}`;
}

function applyFrameworkGroupsVisibility() {
  openclawSettingsGroup.classList.toggle("hidden", isNanobotActive());
  nanobotSettingsGroup.classList.toggle("hidden", !isNanobotActive());
}

function applyAgentPanelCopy() {
  const label = activeAgentLabel();
  const isNanobot = isNanobotActive();

  if (agentTabLabel) {
    agentTabLabel.textContent = label;
  }
  if (agentPanelEyebrow) {
    agentPanelEyebrow.textContent = `${label} Integration`;
  }
  if (agentPanelTitle) {
    agentPanelTitle.textContent = isNanobot
      ? "Chat, runtime e configuracao do NanoBot"
      : "Chat, observabilidade e configuracao";
  }
  if (agentSubtabsLabel) {
    agentSubtabsLabel.setAttribute("aria-label", `${label} visualizacao`);
  }
  if (agentChatTitle) {
    agentChatTitle.textContent = `Chat com ${label}`;
  }
  if (agentLogsTitle) {
    agentLogsTitle.textContent = `Logs em tempo real (${label})`;
  }
  if (agentObservabilityTitle) {
    agentObservabilityTitle.textContent = `Skills e Tools da ultima resposta (${label})`;
  }
  if (agentConfigTitle) {
    agentConfigTitle.textContent = isNanobot
      ? "Configuracao de modelo NanoBot"
      : "Configuracao de modelo OpenClaw";
  }
  if (openclawMessageInput) {
    openclawMessageInput.placeholder = `Converse com o ${label} aqui...`;
  }
  if (openclawSendBtn) {
    openclawSendBtn.textContent = `Enviar para ${label}`;
  }
  if (refreshOpenclawStatusBtn) {
    refreshOpenclawStatusBtn.textContent = isNanobot ? "Atualizar status NanoBot" : "Atualizar status";
  }
  if (agentLogOptionGateway) {
    agentLogOptionGateway.textContent = "gateway.log";
  }
  if (agentLogOptionError) {
    agentLogOptionError.textContent = "gateway.err.log";
  }
  if (agentLogOptionSync) {
    agentLogOptionSync.textContent = isNanobot ? "agent.log" : "openclaw-mlx-sync.log";
  }
  if (openclawModelSourceLabel) {
    openclawModelSourceLabel.textContent = "Origem do modelo";
  }
  if (openclawModelSourceCloudOption) {
    openclawModelSourceCloudOption.textContent = isNanobot
      ? "Nuvem (catalogo compartilhado OpenClaw)"
      : "Nuvem (OpenClaw configurado)";
  }
  if (openclawModelSourceLocalOption) {
    openclawModelSourceLocalOption.textContent = "Local (MLX-Pilot)";
  }
  if (openclawCloudLabel) {
    openclawCloudLabel.textContent = isNanobot
      ? "Modelos cloud compartilhados"
      : "Modelos cloud";
  }
  if (openclawLocalLabel) {
    openclawLocalLabel.textContent = "Modelos locais compartilhados";
  }
  if (refreshOpenclawModelsBtn) {
    refreshOpenclawModelsBtn.textContent = "Atualizar modelos";
  }
  if (applyOpenclawModelBtn) {
    applyOpenclawModelBtn.textContent = isNanobot ? "Aplicar modelo NanoBot" : "Aplicar modelo";
  }
}

function applyAgentFramework(nextFramework, { syncRadio = false, refreshPanel = false } = {}) {
  const normalized = nextFramework === "nanobot" ? "nanobot" : "openclaw";
  const changed = normalized !== activeAgentFramework;
  activeAgentFramework = normalized;

  if (syncRadio) {
    frameworkRadios.forEach((radio) => {
      radio.checked = radio.value === normalized;
    });
  }

  applyFrameworkGroupsVisibility();
  applyAgentPanelCopy();
  toggleOpenClawSourceFields();

  if (changed) {
    openclawStatusLoaded = false;
    openclawObservabilityLoaded = false;
    resetOpenClawLogState();
    openclawRuntimeMeta.textContent = "runtime: verificando...";
  }

  if (normalized === "nanobot") {
    void loadNanobotStatus();
  } else {
    void loadOpenclawInstallStatus({ showLoading: false, syncInstallFeedback: false });
  }

  if (refreshPanel && activeTab === "openclaw") {
    onOpenClawTabSelected();
  }
}

function createParticleSystemFallback() {
  let frameTimer = null;
  let frameIndex = -1;
  let frames = [];
  let shouldLoop = false;

  const clearTimer = () => {
    if (frameTimer) {
      window.clearTimeout(frameTimer);
      frameTimer = null;
    }
  };

  const playNext = (callbacks) => {
    if (!frames.length) {
      callbacks.onComplete?.();
      return;
    }

    frameIndex += 1;
    if (frameIndex >= frames.length) {
      if (!shouldLoop) {
        callbacks.onComplete?.();
        return;
      }
      frameIndex = 0;
    }

    callbacks.onFrame?.(frameIndex, frames[frameIndex]);
    const durationMs = clampSceneNumber(frames[frameIndex]?.duration_ms, 900, 9000, 2200);
    frameTimer = window.setTimeout(() => playNext(callbacks), durationMs);
  };

  return {
    onWindowResize() {
      // no-op fallback
    },
    setParticleState() {
      // no-op fallback
    },
    formText() {
      // no-op fallback
    },
    stopScene() {
      clearTimer();
      frames = [];
      frameIndex = -1;
      shouldLoop = false;
    },
    playScript(script, options = {}) {
      clearTimer();
      frames = Array.isArray(script?.frames) ? script.frames : [];
      frameIndex = -1;
      shouldLoop = Boolean(options.loop);

      if (!frames.length) {
        options.onComplete?.();
        return;
      }
      playNext({
        onFrame: options.onFrame,
        onComplete: options.onComplete,
      });
    },
  };
}

function formatNumber(value) {
  return new Intl.NumberFormat("pt-BR", { notation: "compact" }).format(value || 0);
}

function formatBytes(value) {
  if (!value || value <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  let size = value;
  let index = 0;

  while (size >= 1024 && index < units.length - 1) {
    size /= 1024;
    index += 1;
  }

  return `${size.toFixed(size >= 10 || index === 0 ? 0 : 1)} ${units[index]}`;
}

function formatDate(dateRaw) {
  if (!dateRaw) {
    return "sem data";
  }

  const value = new Date(dateRaw);
  if (Number.isNaN(value.getTime())) {
    return "sem data";
  }

  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    year: "2-digit",
  }).format(value);
}

function formatEpoch(epochMs) {
  if (!epochMs) {
    return "";
  }

  const date = new Date(Number(epochMs));
  if (Number.isNaN(date.getTime())) {
    return "";
  }

  return new Intl.DateTimeFormat("pt-BR", {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function normalizeScenePrompt(value) {
  if (typeof value !== "string") {
    return "";
  }
  return value
    .replace(/\s+/g, " ")
    .replace(/\u00a0/g, " ")
    .trim()
    .slice(0, 320);
}

function truncateSceneText(value, max = 78) {
  const normalized = String(value || "").replace(/\s+/g, " ").trim();
  if (!normalized) {
    return "";
  }
  if (normalized.length <= max) {
    return normalized;
  }
  return `${normalized.slice(0, max - 3)}...`;
}

function clampSceneNumber(value, min, max, fallback) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    return fallback;
  }
  return Math.max(min, Math.min(max, parsed));
}

function normalizeSceneColor(value, fallback) {
  if (typeof value !== "string") {
    return fallback;
  }
  const normalized = value.trim();
  if (!normalized || normalized.length > 32) {
    return fallback;
  }
  return normalized;
}

function setAiSceneStatus(text) {
  if (!aiSceneStatus) {
    return;
  }
  aiSceneStatus.textContent = text;
}

function setAiSceneBusyState() {
  if (aiParticleBtn) {
    aiParticleBtn.disabled = aiSceneInFlight;
    aiParticleBtn.textContent = aiSceneInFlight ? "Montando cena..." : "Animar resposta";
  }
  if (aiParticleStopBtn) {
    aiParticleStopBtn.disabled = !aiSceneAnimating && !aiSceneInFlight;
  }
}

function renderAiSceneSteps(script, activeIndex = -1) {
  if (!aiSceneSteps) {
    return;
  }

  aiSceneSteps.innerHTML = "";
  const frames = Array.isArray(script?.frames) ? script.frames : [];
  if (!frames.length) {
    return;
  }

  frames.forEach((frame, index) => {
    const item = document.createElement("li");
    item.className = "ai-scene-step";
    if (index === activeIndex) {
      item.classList.add("active");
    }

    const caption = truncateSceneText(frame.caption || `Etapa ${index + 1}`, 42);
    const duration = clampSceneNumber(frame.duration_ms, 500, 9000, 2200);
    const seconds = (duration / 1000).toFixed(1).replace(".0", "");
    item.textContent = `${index + 1}. ${caption} (${seconds}s)`;
    aiSceneSteps.appendChild(item);
  });
}

function sanitizeSceneShape(shape) {
  if (!shape || typeof shape !== "object") {
    return null;
  }

  const type = String(shape.type || "").trim().toLowerCase();
  const allowed = new Set(["text", "line", "arrow", "circle", "ring", "rect", "box", "spiral", "wave", "dot", "point"]);
  if (!allowed.has(type)) {
    return null;
  }

  const normalized = { type };

  if (type === "text") {
    normalized.text = truncateSceneText(shape.text || "", 96);
    if (!normalized.text) {
      return null;
    }
    normalized.x = clampSceneNumber(shape.x, 0, 100, 50);
    normalized.y = clampSceneNumber(shape.y, 0, 100, 50);
    normalized.size = clampSceneNumber(shape.size, 10, 110, 32);
    normalized.weight = clampSceneNumber(shape.weight, 300, 900, 700);
    normalized.align = ["left", "center", "right"].includes(String(shape.align || "").toLowerCase())
      ? String(shape.align).toLowerCase()
      : "center";
    normalized.color = normalizeSceneColor(shape.color, "#bfe8ff");
    return normalized;
  }

  normalized.color = normalizeSceneColor(shape.color, "#72d5ff");
  normalized.width = clampSceneNumber(shape.width, 1, 18, 4);

  if (["line", "arrow"].includes(type)) {
    normalized.x1 = clampSceneNumber(shape.x1, 0, 100, 30);
    normalized.y1 = clampSceneNumber(shape.y1, 0, 100, 30);
    normalized.x2 = clampSceneNumber(shape.x2, 0, 100, 70);
    normalized.y2 = clampSceneNumber(shape.y2, 0, 100, 70);
    if (type === "arrow") {
      normalized.head = clampSceneNumber(shape.head, 6, 34, 16);
    }
    return normalized;
  }

  if (["circle", "ring", "dot", "point"].includes(type)) {
    normalized.x = clampSceneNumber(shape.x, 0, 100, 50);
    normalized.y = clampSceneNumber(shape.y, 0, 100, 50);
    normalized.r = clampSceneNumber(shape.r, 2, 320, type === "dot" || type === "point" ? 8 : 70);
    if (type === "circle") {
      normalized.fill = normalizeSceneColor(shape.fill, `${normalized.color}22`);
    }
    return normalized;
  }

  if (["rect", "box"].includes(type)) {
    normalized.x = clampSceneNumber(shape.x, 0, 100, 50);
    normalized.y = clampSceneNumber(shape.y, 0, 100, 50);
    normalized.w = clampSceneNumber(shape.w, 6, 660, 180);
    normalized.h = clampSceneNumber(shape.h, 6, 420, 100);
    normalized.fill = normalizeSceneColor(shape.fill, "transparent");
    return normalized;
  }

  if (type === "spiral") {
    normalized.x = clampSceneNumber(shape.x, 0, 100, 50);
    normalized.y = clampSceneNumber(shape.y, 0, 100, 50);
    normalized.r = clampSceneNumber(shape.r, 8, 340, 120);
    normalized.turns = clampSceneNumber(shape.turns, 1, 12, 4);
    return normalized;
  }

  if (type === "wave") {
    normalized.x = clampSceneNumber(shape.x, 0, 100, 50);
    normalized.y = clampSceneNumber(shape.y, 0, 100, 50);
    normalized.length = clampSceneNumber(shape.length, 20, 860, 360);
    normalized.amp = clampSceneNumber(shape.amp, 4, 120, 26);
    normalized.cycles = clampSceneNumber(shape.cycles, 1, 16, 3);
    return normalized;
  }

  return null;
}

function sanitizeSceneFrame(frame, index = 0) {
  if (!frame || typeof frame !== "object") {
    return null;
  }

  const shapes = Array.isArray(frame.shapes)
    ? frame.shapes.map((shape) => sanitizeSceneShape(shape)).filter(Boolean)
    : [];

  const caption = truncateSceneText(frame.caption || `Etapa ${index + 1}`, 64);
  if (!shapes.length) {
    shapes.push({
      type: "text",
      text: caption || `Etapa ${index + 1}`,
      x: 50,
      y: 50,
      size: 34,
      weight: 700,
      align: "center",
      color: "#bfe8ff",
    });
  }

  const backgroundRaw = frame.background && typeof frame.background === "object" ? frame.background : {};

  return {
    caption: caption || `Etapa ${index + 1}`,
    duration_ms: clampSceneNumber(frame.duration_ms, 900, 9000, 2200),
    background: {
      color: normalizeSceneColor(backgroundRaw.color, "#050816"),
      glow: normalizeSceneColor(backgroundRaw.glow, "#214f95"),
    },
    shapes: shapes.slice(0, 18),
  };
}

function sanitizeParticleScript(rawScript, prompt) {
  const script = rawScript && typeof rawScript === "object" ? rawScript : {};
  const rawFrames = Array.isArray(script.frames) ? script.frames : [];
  const frames = rawFrames
    .slice(0, 8)
    .map((frame, index) => sanitizeSceneFrame(frame, index))
    .filter(Boolean);

  if (!frames.length) {
    return buildGenericSceneScript(prompt);
  }

  return {
    title: truncateSceneText(script.title || prompt || "Cena IA", 80),
    frames,
  };
}

function extractJsonPayload(rawText) {
  const raw = String(rawText || "").trim();
  if (!raw) {
    return null;
  }

  const direct = (() => {
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  })();
  if (direct) {
    return direct;
  }

  const fenced = raw.match(/```(?:json)?\\s*([\\s\\S]*?)```/i);
  if (fenced?.[1]) {
    try {
      return JSON.parse(fenced[1].trim());
    } catch {
      // continue
    }
  }

  const first = raw.indexOf("{");
  const last = raw.lastIndexOf("}");
  if (first >= 0 && last > first) {
    const chunk = raw.slice(first, last + 1);
    try {
      return JSON.parse(chunk);
    } catch {
      return null;
    }
  }

  return null;
}

async function requestAiSceneScript(prompt) {
  const plannerPrompt = [
    "Voce e um diretor de animacao por particulas para uma interface sem texto corrido.",
    "Retorne SOMENTE JSON valido, sem markdown e sem comentarios.",
    "Formato obrigatorio:",
    "{",
    "  \"title\": \"resumo curto\",",
    "  \"frames\": [",
    "    {",
    "      \"caption\": \"etapa\",",
    "      \"duration_ms\": 2200,",
    "      \"background\": {\"color\": \"#050816\", \"glow\": \"#2a66b0\"},",
    "      \"shapes\": [",
    "        {\"type\":\"text\",\"text\":\"...\",\"x\":50,\"y\":20,\"size\":40,\"color\":\"#bfe8ff\"},",
    "        {\"type\":\"arrow\",\"x1\":20,\"y1\":30,\"x2\":80,\"y2\":30,\"color\":\"#7edbff\",\"width\":4},",
    "        {\"type\":\"line\"|\"circle\"|\"ring\"|\"rect\"|\"spiral\"|\"wave\"|\"dot\", ...}",
    "      ]",
    "    }",
    "  ]",
    "}",
    "Use x/y em percentual (0..100).",
    "Duracao de cada frame entre 1200 e 5000 ms.",
    "No maximo 6 frames e 12 shapes por frame.",
    "Objetivo: responder visualmente ao pedido do usuario.",
    `Pedido do usuario: ${prompt}`,
  ].join("\\n");

  const payload = await fetchJson(activeAgentEndpoint("chat"), {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ message: plannerPrompt }),
  });

  const raw = [payload?.reply, Array.isArray(payload?.payloads) ? payload.payloads.join("\\n") : ""]
    .filter(Boolean)
    .join("\\n")
    .trim();

  const jsonPayload = extractJsonPayload(raw);
  if (!jsonPayload) {
    throw new Error("resposta sem JSON valido para cena");
  }
  return sanitizeParticleScript(jsonPayload, prompt);
}

function extractPromptKeywords(prompt) {
  const cleaned = String(prompt || "")
    .toLowerCase()
    .replace(/[^a-z0-9\\u00c0-\\u017f\\s]/g, " ")
    .replace(/\\s+/g, " ")
    .trim();

  if (!cleaned) {
    return [];
  }

  const stopWords = new Set([
    "como", "fazer", "faço", "faco", "voce", "você", "sabe", "seria", "sobre", "para", "com", "sem",
    "que", "isso", "essa", "esse", "uma", "um", "das", "dos", "de", "da", "do", "e", "a", "o", "as",
    "os", "me", "mostrar", "mostra", "consegue", "explicar", "imagine", "imagina", "quero", "pode",
  ]);

  const unique = [];
  for (const token of cleaned.split(" ")) {
    if (!token || token.length < 3 || stopWords.has(token)) {
      continue;
    }
    if (!unique.includes(token)) {
      unique.push(token);
    }
  }
  return unique.slice(0, 5);
}

function buildBhaskaraSceneScript() {
  return {
    title: "Bhaskara em etapas",
    frames: [
      {
        caption: "Equacao de segundo grau",
        duration_ms: 2300,
        background: { color: "#050816", glow: "#1e4f89" },
        shapes: [
          { type: "text", text: "ax^2 + bx + c = 0", x: 50, y: 20, size: 48, color: "#bfe8ff" },
          { type: "rect", x: 50, y: 40, w: 520, h: 96, color: "#69ccff", width: 4 },
          { type: "text", text: "Exemplo: 2x^2 + 5x - 3 = 0", x: 50, y: 40, size: 34, color: "#d9f2ff" },
          { type: "arrow", x1: 18, y1: 62, x2: 42, y2: 62, color: "#72d6ff", width: 4 },
          { type: "arrow", x1: 82, y1: 62, x2: 58, y2: 62, color: "#72d6ff", width: 4 },
        ],
      },
      {
        caption: "Calculando o delta",
        duration_ms: 2500,
        background: { color: "#060d20", glow: "#266cbf" },
        shapes: [
          { type: "text", text: "Delta = b^2 - 4ac", x: 50, y: 22, size: 46, color: "#bfe8ff" },
          { type: "text", text: "Delta = 5^2 - 4*2*(-3) = 49", x: 50, y: 46, size: 34, color: "#d9f4ff" },
          { type: "circle", x: 50, y: 68, r: 90, color: "#81dcff", width: 5, fill: "#58b6ff22" },
          { type: "text", text: "Delta > 0", x: 50, y: 68, size: 36, color: "#e7f8ff" },
        ],
      },
      {
        caption: "Aplicando a formula",
        duration_ms: 2600,
        background: { color: "#050a18", glow: "#2f6bc0" },
        shapes: [
          { type: "text", text: "x = (-b +- sqrt(Delta)) / 2a", x: 50, y: 22, size: 40, color: "#cbecff" },
          { type: "arrow", x1: 20, y1: 40, x2: 80, y2: 40, color: "#79dbff", width: 4 },
          { type: "text", text: "x1 = (-5 + 7) / 4 = 0.5", x: 50, y: 58, size: 34, color: "#dff4ff" },
          { type: "text", text: "x2 = (-5 - 7) / 4 = -3", x: 50, y: 74, size: 34, color: "#dff4ff" },
        ],
      },
      {
        caption: "Resultado final",
        duration_ms: 2100,
        background: { color: "#040611", glow: "#225089" },
        shapes: [
          { type: "ring", x: 34, y: 56, r: 72, color: "#8ee3ff", width: 5 },
          { type: "ring", x: 66, y: 56, r: 72, color: "#8ee3ff", width: 5 },
          { type: "text", text: "x1 = 0.5", x: 34, y: 56, size: 34, color: "#f0fbff" },
          { type: "text", text: "x2 = -3", x: 66, y: 56, size: 34, color: "#f0fbff" },
        ],
      },
    ],
  };
}

function buildGojoSceneScript() {
  return {
    title: "Expansao imaginada",
    frames: [
      {
        caption: "O espaco comeca a se fechar",
        duration_ms: 2200,
        background: { color: "#050a1b", glow: "#2e65c1" },
        shapes: [
          { type: "ring", x: 50, y: 50, r: 180, color: "#8adfff", width: 4 },
          { type: "text", text: "Limite se formando", x: 50, y: 18, size: 28, color: "#bfe8ff" },
          { type: "spiral", x: 50, y: 50, r: 120, turns: 4, color: "#77d8ff", width: 3 },
        ],
      },
      {
        caption: "Camadas infinitas comprimindo",
        duration_ms: 2600,
        background: { color: "#030613", glow: "#3a79d4" },
        shapes: [
          { type: "ring", x: 50, y: 50, r: 190, color: "#7dd9ff", width: 3 },
          { type: "ring", x: 50, y: 50, r: 145, color: "#7dd9ff", width: 3 },
          { type: "ring", x: 50, y: 50, r: 100, color: "#7dd9ff", width: 3 },
          { type: "ring", x: 50, y: 50, r: 60, color: "#7dd9ff", width: 3 },
          { type: "text", text: "Informacao sem fim", x: 50, y: 82, size: 28, color: "#dff4ff" },
        ],
      },
      {
        caption: "Centro absoluto",
        duration_ms: 2600,
        background: { color: "#02040d", glow: "#4a90e8" },
        shapes: [
          { type: "circle", x: 50, y: 50, r: 62, color: "#8fe4ff", width: 5, fill: "#80d6ff2a" },
          { type: "dot", x: 50, y: 50, r: 16, color: "#ecf9ff" },
          { type: "wave", x: 50, y: 72, length: 420, amp: 18, cycles: 3, color: "#8fe2ff", width: 3 },
          { type: "text", text: "Tudo converge para um unico ponto", x: 50, y: 20, size: 30, color: "#d5f1ff" },
        ],
      },
      {
        caption: "Dominio completo",
        duration_ms: 2300,
        background: { color: "#030713", glow: "#2c68c2" },
        shapes: [
          { type: "text", text: "Expansao concluida", x: 50, y: 22, size: 34, color: "#e8f8ff" },
          { type: "spiral", x: 50, y: 52, r: 168, turns: 5, color: "#85deff", width: 3 },
          { type: "arrow", x1: 20, y1: 78, x2: 45, y2: 58, color: "#7ed8ff", width: 4 },
          { type: "arrow", x1: 80, y1: 78, x2: 55, y2: 58, color: "#7ed8ff", width: 4 },
        ],
      },
    ],
  };
}

function buildGenericSceneScript(prompt) {
  const clipped = truncateSceneText(prompt || "Explique visualmente", 74);
  const keywords = extractPromptKeywords(prompt);
  const keywordShapes = [];
  const angles = [220, 300, 20, 80, 150];

  keywords.forEach((keyword, index) => {
    const angle = (angles[index % angles.length] * Math.PI) / 180;
    const x = 50 + Math.cos(angle) * 27;
    const y = 52 + Math.sin(angle) * 27;
    keywordShapes.push({ type: "arrow", x1: 50, y1: 52, x2: x, y2: y, color: "#78d8ff", width: 3 });
    keywordShapes.push({ type: "text", text: truncateSceneText(keyword, 14), x, y, size: 24, color: "#ddf4ff" });
  });

  return {
    title: "Cena visual",
    frames: [
      {
        caption: "Interpretando sua pergunta",
        duration_ms: 2100,
        background: { color: "#050816", glow: "#1e518d" },
        shapes: [
          { type: "text", text: clipped, x: 50, y: 28, size: 30, color: "#ccecff" },
          { type: "ring", x: 50, y: 58, r: 130, color: "#77d6ff", width: 4 },
          { type: "dot", x: 50, y: 58, r: 11, color: "#ebf9ff" },
        ],
      },
      {
        caption: "Ligando os conceitos",
        duration_ms: 2400,
        background: { color: "#040712", glow: "#2e6dc2" },
        shapes: [
          { type: "text", text: "Mapa de ideias", x: 50, y: 18, size: 30, color: "#d9f3ff" },
          { type: "circle", x: 50, y: 52, r: 88, color: "#74d8ff", width: 4, fill: "#74d8ff18" },
          { type: "text", text: "Tema", x: 50, y: 52, size: 26, color: "#f0fbff" },
          ...keywordShapes,
        ],
      },
      {
        caption: "Resumo visual",
        duration_ms: 2200,
        background: { color: "#040713", glow: "#21599e" },
        shapes: [
          { type: "text", text: "Resposta montada por etapas", x: 50, y: 28, size: 30, color: "#d9f3ff" },
          { type: "wave", x: 50, y: 54, length: 420, amp: 16, cycles: 4, color: "#84ddff", width: 3 },
          { type: "spiral", x: 50, y: 68, r: 90, turns: 3, color: "#79d7ff", width: 3 },
        ],
      },
    ],
  };
}

function buildFallbackParticleScript(prompt) {
  const normalized = String(prompt || "").toLowerCase();
  if (/(bhaskara|baskara|bascara)/i.test(normalized)) {
    return buildBhaskaraSceneScript();
  }
  if (/(gojo|expansao|expansão|dominio|domínio)/i.test(normalized)) {
    return buildGojoSceneScript();
  }
  return buildGenericSceneScript(prompt);
}

function stopAiScenePlayback({ keepTimeline = true } = {}) {
  aiSceneRequestToken += 1;
  if (window.particleSystem) {
    window.particleSystem.stopScene();
  }
  aiSceneInFlight = false;
  aiSceneAnimating = false;
  setAiSceneBusyState();

  if (!keepTimeline) {
    renderAiSceneSteps(null);
  }
}

async function applyParticleTextFromInput() {
  if (!window.particleSystem) {
    setStatus("playground de particulas indisponivel", "error");
    return;
  }

  if (aiSceneInFlight) {
    return;
  }

  const raw = aiParticleInput ? aiParticleInput.value : "";
  const prompt = normalizeScenePrompt(raw);
  if (aiParticleInput) {
    aiParticleInput.value = prompt;
  }

  if (!prompt) {
    stopAiScenePlayback({ keepTimeline: false });
    window.particleSystem.setParticleState("neutral");
    setAiSceneStatus("Escreva uma pergunta para montar a cena.");
    setStatus("particulas em modo neutro");
    return;
  }

  aiSceneInFlight = true;
  aiSceneAnimating = false;
  const requestToken = aiSceneRequestToken + 1;
  aiSceneRequestToken = requestToken;
  setAiSceneBusyState();
  setAiSceneStatus(`Planejando cena com ${activeAgentLabel()}...`);
  setStatus("ia visual preparando", "running");

  let script = null;
  let usedFallback = false;

  try {
    script = await requestAiSceneScript(prompt);
  } catch (error) {
    usedFallback = true;
    script = buildFallbackParticleScript(prompt);
    setAiSceneStatus(`Modo visual local: ${error.message}`);
  } finally {
    if (requestToken === aiSceneRequestToken) {
      aiSceneInFlight = false;
      setAiSceneBusyState();
    }
  }

  if (requestToken !== aiSceneRequestToken) {
    return;
  }

  if (!script || !Array.isArray(script.frames) || !script.frames.length) {
    setAiSceneStatus("Nao foi possivel montar a cena.");
    setStatus("erro cena visual", "error");
    return;
  }

  aiSceneLastScript = script;
  aiSceneAnimating = true;
  setAiSceneBusyState();
  renderAiSceneSteps(script, 0);
  setAiSceneStatus(
    usedFallback
      ? "Cena em execucao (fallback visual local)."
      : `Cena em execucao com ${script.frames.length} etapa(s).`
  );

  window.particleSystem.playScript(script, {
    loop: false,
    onFrame: (index) => {
      renderAiSceneSteps(script, index);
    },
    onComplete: () => {
      aiSceneAnimating = false;
      setAiSceneBusyState();
      renderAiSceneSteps(script, script.frames.length - 1);
      setAiSceneStatus("Cena concluida. Envie outra pergunta para uma nova animacao.");
      setStatus("cena visual concluida");
    },
  });
}

function isMeaningfulCatalogSummary(value) {
  if (typeof value !== "string") {
    return false;
  }

  const normalized = value.trim();
  if (!normalized) {
    return false;
  }

  const lowered = normalized.toLowerCase();
  const placeholders = new Set([
    "sem descricao detalhada no catalogo.",
    "sem descricao",
    "sem descrição",
    "no description",
    "no description provided",
    "no detailed description",
    "n/a",
  ]);

  return !placeholders.has(lowered);
}

function threadStorageSafeParse(raw) {
  if (!raw) {
    return [];
  }

  try {
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return [];
    }

    return parsed
      .filter((entry) => entry && typeof entry === "object")
      .map((entry) => {
        const messages = Array.isArray(entry.messages)
          ? entry.messages
            .filter((item) => item && typeof item.role === "string" && typeof item.content === "string")
            .map((item) => {
              const normalized = {
                role: item.role,
                content: item.content,
              };

              if (normalized.role === "assistant") {
                const thinking = typeof item.thinking === "string" ? item.thinking.trim() : "";
                if (thinking) {
                  normalized.thinking = thinking;
                }

                const metrics = normalizeAssistantMetrics(item.metrics);
                if (metrics) {
                  normalized.metrics = metrics;
                }
              }

              return normalized;
            })
          : [];

        return {
          id: String(entry.id || crypto.randomUUID()),
          title: String(entry.title || "Nova conversa"),
          modelId: typeof entry.modelId === "string" ? entry.modelId : null,
          updatedAt: Number(entry.updatedAt || Date.now()),
          messages,
        };
      });
  } catch {
    return [];
  }
}

function persistThreads() {
  localStorage.setItem(STORAGE_CHAT_THREADS, JSON.stringify(chatThreads));
}

function createThread({ title = "Nova conversa", modelId = selectedModelId } = {}) {
  return {
    id: crypto.randomUUID(),
    title,
    modelId: modelId || null,
    updatedAt: Date.now(),
    messages: [],
  };
}

function loadThreads() {
  chatThreads = threadStorageSafeParse(localStorage.getItem(STORAGE_CHAT_THREADS));
  if (!chatThreads.length) {
    const first = createThread();
    chatThreads = [first];
    activeThreadId = first.id;
    persistThreads();
    return;
  }

  if (!chatThreads.some((entry) => entry.id === activeThreadId)) {
    const [latest] = [...chatThreads].sort((a, b) => Number(b.updatedAt) - Number(a.updatedAt));
    activeThreadId = latest?.id || chatThreads[0].id;
  }
}

function getActiveThread() {
  return chatThreads.find((entry) => entry.id === activeThreadId) || null;
}

function deriveThreadTitle(content) {
  const text = String(content || "").trim();
  if (!text) {
    return "Nova conversa";
  }

  if (text.length <= 52) {
    return text;
  }

  return `${text.slice(0, 52)}...`;
}

async function renameThread(threadId) {
  if (isGenerating) {
    addSystemMessage("Pare a geracao atual antes de renomear uma conversa.");
    return;
  }

  openThreadMenuId = null;

  const thread = chatThreads.find((entry) => entry.id === threadId);
  if (!thread) {
    return;
  }

  const proposed = await showTextPrompt({
    title: "Renomear conversa",
    message: "Defina um novo nome para esse chat.",
    defaultValue: thread.title || "Nova conversa",
    confirmLabel: "Salvar",
  });
  if (proposed === null) {
    return;
  }

  const normalized = proposed.trim();
  if (!normalized) {
    addSystemMessage("Nome invalido. Informe ao menos um caractere.");
    return;
  }

  thread.title = normalized;
  thread.updatedAt = Date.now();
  persistThreads();
  renderThreadList();
  renderSelectedThreadMeta();
  setStatus("conversa renomeada");
}

async function deleteThread(threadId) {
  if (isGenerating) {
    addSystemMessage("Pare a geracao atual antes de apagar uma conversa.");
    return;
  }

  openThreadMenuId = null;

  const index = chatThreads.findIndex((entry) => entry.id === threadId);
  if (index === -1) {
    return;
  }

  const thread = chatThreads[index];

  if (chatThreads.length === 1) {
    chatThreads[0] = createThread({ title: "Nova conversa", modelId: selectedModelId });
    activeThreadId = chatThreads[0].id;
    persistThreads();
    renderThreadList();
    rebuildChatFromThread();
    setStatus("conversa limpa");
    return;
  }

  const confirmed = await showConfirmDialog({
    title: "Apagar conversa",
    message: `Tem certeza que deseja apagar "${thread.title || "Nova conversa"}"?`,
    confirmLabel: "Apagar",
    danger: true,
  });
  if (!confirmed) {
    return;
  }

  const wasActive = activeThreadId === thread.id;
  chatThreads.splice(index, 1);

  if (wasActive) {
    activeThreadId = sortThreadsForView()[0]?.id || chatThreads[0]?.id || null;
    syncModelWithActiveThread();
    rebuildChatFromThread();
  }

  persistThreads();
  renderThreadList();
  renderSelectedThreadMeta();
  setStatus("conversa apagada");
}

function ensureActiveThread() {
  if (getActiveThread()) {
    return;
  }

  const thread = createThread();
  chatThreads.unshift(thread);
  activeThreadId = thread.id;
  persistThreads();
}

function sortThreadsForView() {
  return [...chatThreads].sort((left, right) => Number(right.updatedAt) - Number(left.updatedAt));
}

function closeThreadMenu({ rerender = false } = {}) {
  if (!openThreadMenuId) {
    return;
  }

  openThreadMenuId = null;
  if (rerender) {
    renderThreadList();
  }
}

function toggleThreadMenu(threadId) {
  if (!threadId) {
    return;
  }

  openThreadMenuId = openThreadMenuId === threadId ? null : threadId;
  renderThreadList();
}

function createNewChatThread() {
  if (isGenerating) {
    addSystemMessage("Pare a geracao atual antes de iniciar nova conversa.");
    return;
  }

  closeThreadMenu();
  setEditMode(null);
  const thread = createThread({ modelId: selectedModelId });
  chatThreads.unshift(thread);
  activeThreadId = thread.id;
  persistThreads();
  renderThreadList();
  rebuildChatFromThread();
  messageInput.focus();
}

function getModelLabelById(modelId) {
  const model = localModels.find((entry) => entry.id === modelId);
  return model ? model.name : "modelo n/d";
}

function setChatModelMenuOpen(nextState) {
  chatModelMenuOpen = Boolean(nextState);
  if (!chatModelMenu) {
    return;
  }

  chatModelMenu.classList.toggle("hidden", !chatModelMenuOpen);
  if (chatModelTrigger) {
    chatModelTrigger.setAttribute("aria-expanded", chatModelMenuOpen ? "true" : "false");
  }
  chatModelSwitcher.classList.toggle("menu-open", chatModelMenuOpen);
}

function updateChatModelPickerLabel() {
  if (!chatModelCurrent) {
    return;
  }

  if (!selectedModelId) {
    chatModelCurrent.textContent = "Selecionar modelo";
    return;
  }

  const activeModel = localModels.find((entry) => entry.id === selectedModelId);
  chatModelCurrent.textContent = activeModel ? activeModel.name : "Selecionar modelo";
}

function renderChatModelPickerMenu() {
  if (!chatModelMenu) {
    return;
  }

  chatModelMenu.innerHTML = "";

  if (!localModels.length) {
    const empty = document.createElement("p");
    empty.className = "model-picker-empty";
    empty.textContent = "Nenhum modelo local disponivel.";
    chatModelMenu.appendChild(empty);
    updateChatModelPickerLabel();
    return;
  }

  localModels.forEach((model) => {
    const option = document.createElement("button");
    option.type = "button";
    option.className = "model-picker-item";
    if (selectedModelId === model.id) {
      option.classList.add("active");
    }
    option.setAttribute("role", "option");
    option.setAttribute("aria-selected", selectedModelId === model.id ? "true" : "false");

    const name = document.createElement("span");
    name.className = "model-picker-item-name";
    name.textContent = model.name;

    const meta = document.createElement("span");
    meta.className = "model-picker-item-meta";
    meta.textContent = `${model.provider || "local"} • ${model.id}`;

    option.appendChild(name);
    option.appendChild(meta);
    option.addEventListener("click", () => {
      chatModelSelect.value = model.id;
      chatModelSelect.dispatchEvent(new Event("change"));
      setChatModelMenuOpen(false);
    });

    chatModelMenu.appendChild(option);
  });

  updateChatModelPickerLabel();
}

function renderThreadList() {
  chatHistoryList.innerHTML = "";

  const sorted = sortThreadsForView();
  if (openThreadMenuId && !sorted.some((thread) => thread.id === openThreadMenuId)) {
    openThreadMenuId = null;
  }
  chatHistoryMeta.textContent = `${sorted.length} conversa(s)`;

  sorted.forEach((thread) => {
    const li = document.createElement("li");
    li.className = "history-item";

    const row = document.createElement("div");
    row.className = "history-item-row";

    const button = document.createElement("button");
    button.className = "history-select-btn";
    if (thread.id === activeThreadId) {
      button.classList.add("active");
    }

    const title = document.createElement("p");
    title.className = "history-title";
    title.textContent = thread.title || "Nova conversa";

    const meta = document.createElement("p");
    meta.className = "history-meta";
    meta.textContent = `${getModelLabelById(thread.modelId)} • ${thread.messages.length} msg • ${formatEpoch(thread.updatedAt)}`;

    button.appendChild(title);
    button.appendChild(meta);

    button.addEventListener("click", () => {
      if (isGenerating) {
        addSystemMessage("Pare a geracao atual antes de trocar de conversa.");
        return;
      }
      closeThreadMenu();
      setEditMode(null);
      activeThreadId = thread.id;
      syncModelWithActiveThread();
      renderThreadList();
      rebuildChatFromThread();
    });

    const actions = document.createElement("div");
    actions.className = "history-item-actions";

    const menuTrigger = document.createElement("button");
    menuTrigger.type = "button";
    menuTrigger.className = "history-menu-trigger";
    menuTrigger.textContent = "⋯";
    const isMenuOpen = openThreadMenuId === thread.id;
    menuTrigger.setAttribute("aria-label", "Acoes da conversa");
    menuTrigger.setAttribute("aria-expanded", isMenuOpen ? "true" : "false");
    menuTrigger.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      toggleThreadMenu(thread.id);
    });

    actions.appendChild(menuTrigger);

    if (isMenuOpen) {
      const menu = document.createElement("div");
      menu.className = "history-item-menu";

      const renameBtn = document.createElement("button");
      renameBtn.type = "button";
      renameBtn.className = "history-menu-item";
      renameBtn.textContent = "Renomear";
      renameBtn.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        void renameThread(thread.id);
      });

      const deleteBtn = document.createElement("button");
      deleteBtn.type = "button";
      deleteBtn.className = "history-menu-item danger";
      deleteBtn.textContent = "Apagar";
      deleteBtn.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        void deleteThread(thread.id);
      });

      menu.appendChild(renameBtn);
      menu.appendChild(deleteBtn);
      actions.appendChild(menu);
    }

    row.appendChild(button);
    row.appendChild(actions);
    li.appendChild(row);
    chatHistoryList.appendChild(li);
  });
}

function syncModelWithActiveThread() {
  const active = getActiveThread();
  if (!active) {
    return;
  }

  if (active.modelId && localModels.some((entry) => entry.id === active.modelId)) {
    selectedModelId = active.modelId;
  } else if (selectedModelId && localModels.some((entry) => entry.id === selectedModelId)) {
    active.modelId = selectedModelId;
    active.updatedAt = Date.now();
    persistThreads();
  } else if (localModels.length) {
    selectedModelId = localModels[0].id;
    active.modelId = selectedModelId;
    active.updatedAt = Date.now();
    persistThreads();
  }

  renderChatModelSelect();
  renderSelectedThreadMeta();
}

function renderChatModelSelect() {
  const previous = chatModelSelect.value;
  chatModelSelect.innerHTML = "";

  if (!localModels.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "Sem modelos locais";
    chatModelSelect.appendChild(option);
    chatModelSelect.disabled = true;
    selectedModelId = null;
    renderChatModelPickerMenu();
    renderSelectedThreadMeta();
    return;
  }

  chatModelSelect.disabled = false;
  localModels.forEach((model) => {
    const option = document.createElement("option");
    option.value = model.id;
    option.textContent = model.name;
    chatModelSelect.appendChild(option);
  });

  const preferred = selectedModelId || previous;
  if (preferred && localModels.some((entry) => entry.id === preferred)) {
    chatModelSelect.value = preferred;
    selectedModelId = preferred;
  } else {
    chatModelSelect.value = localModels[0].id;
    selectedModelId = localModels[0].id;
  }

  const active = getActiveThread();
  if (active) {
    active.modelId = selectedModelId;
    active.updatedAt = Date.now();
    persistThreads();
  }

  renderChatModelPickerMenu();
  renderSelectedThreadMeta();
}

function renderSelectedThreadMeta() {
  const active = getActiveThread();
  if (!active) {
    selectedThreadLabel.textContent = "Nova conversa";
    selectedModelLabel.textContent = "Nenhum modelo selecionado";
    updateChatModelPickerLabel();
    return;
  }

  selectedThreadLabel.textContent = active.title || "Nova conversa";
  const model = localModels.find((entry) => entry.id === selectedModelId);
  selectedModelLabel.textContent = model ? `${model.name} (${model.provider})` : "Modelo nao selecionado";
  updateChatModelPickerLabel();
}

function roleClassFromValue(role) {
  const normalized = String(role || "").toLowerCase();
  if (["user", "assistant", "system", "tool"].includes(normalized)) {
    return normalized;
  }
  return "assistant";
}

function escapeHtml(value) {
  return String(value || "")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function sanitizeMarkdownLinkUrl(rawValue) {
  const value = String(rawValue || "").trim();
  if (!value) {
    return "#";
  }

  if (value.startsWith("#")) {
    return value;
  }

  const hasExplicitScheme = /^[a-zA-Z][a-zA-Z\d+.-]*:/.test(value);
  if (!hasExplicitScheme && !value.startsWith("//")) {
    return value;
  }

  try {
    const parsed = new URL(value, "https://mlx-pilot.local");
    if (["http:", "https:", "mailto:"].includes(parsed.protocol)) {
      return parsed.href;
    }
  } catch {
    // ignore invalid URL
  }

  return "#";
}

function renderInlineMarkdown(rawValue) {
  let html = escapeHtml(rawValue);

  html = html.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_match, label, href) => {
    const safeUrl = escapeHtml(sanitizeMarkdownLinkUrl(href));
    return `<a href="${safeUrl}" target="_blank" rel="noopener noreferrer">${label}</a>`;
  });

  html = html.replace(/`([^`]+)`/g, (_match, code) => `<code>${code}</code>`);
  html = html.replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>");
  html = html.replace(/\*([^*\n]+)\*/g, "<em>$1</em>");

  return html;
}

function renderMarkdownToHtml(rawMarkdown) {
  const source = String(rawMarkdown || "").replace(/\r\n/g, "\n");
  if (!source.trim()) {
    return "";
  }

  const lines = source.split("\n");
  const html = [];

  let inCodeBlock = false;
  let codeLang = "";
  let codeLines = [];
  let inUnorderedList = false;
  let inOrderedList = false;
  let paragraphLines = [];
  let quoteLines = [];

  const closeLists = () => {
    if (inUnorderedList) {
      html.push("</ul>");
      inUnorderedList = false;
    }
    if (inOrderedList) {
      html.push("</ol>");
      inOrderedList = false;
    }
  };

  const flushParagraph = () => {
    if (!paragraphLines.length) {
      return;
    }
    const rendered = paragraphLines
      .map((line) => renderInlineMarkdown(line.trim()))
      .join("<br />");
    html.push(`<p>${rendered}</p>`);
    paragraphLines = [];
  };

  const flushQuote = () => {
    if (!quoteLines.length) {
      return;
    }
    const rendered = quoteLines
      .map((line) => renderInlineMarkdown(line.trim()))
      .join("<br />");
    html.push(`<blockquote>${rendered}</blockquote>`);
    quoteLines = [];
  };

  const flushCodeBlock = () => {
    if (!codeLines.length) {
      html.push("<pre><code></code></pre>");
      return;
    }

    const languageClass = codeLang
      ? ` class="language-${escapeHtml(codeLang.replace(/[^\w-]/g, ""))}"`
      : "";
    html.push(`<pre><code${languageClass}>${escapeHtml(codeLines.join("\n"))}</code></pre>`);
    codeLines = [];
  };

  for (const line of lines) {
    const trimmed = line.trim();

    if (inCodeBlock) {
      if (trimmed.startsWith("```")) {
        flushCodeBlock();
        inCodeBlock = false;
        codeLang = "";
      } else {
        codeLines.push(line);
      }
      continue;
    }

    if (trimmed.startsWith("```")) {
      flushParagraph();
      flushQuote();
      closeLists();
      inCodeBlock = true;
      codeLang = trimmed.slice(3).trim();
      codeLines = [];
      continue;
    }

    if (!trimmed) {
      flushParagraph();
      flushQuote();
      closeLists();
      continue;
    }

    if (/^(-{3,}|\*{3,}|_{3,})$/.test(trimmed)) {
      flushParagraph();
      flushQuote();
      closeLists();
      html.push("<hr />");
      continue;
    }

    const headingMatch = trimmed.match(/^(#{1,6})\s+(.+)$/);
    if (headingMatch) {
      flushParagraph();
      flushQuote();
      closeLists();
      const level = headingMatch[1].length;
      html.push(`<h${level}>${renderInlineMarkdown(headingMatch[2])}</h${level}>`);
      continue;
    }

    const quoteMatch = line.match(/^\s*>\s?(.*)$/);
    if (quoteMatch) {
      flushParagraph();
      closeLists();
      quoteLines.push(quoteMatch[1]);
      continue;
    }

    const unorderedMatch = line.match(/^\s*[-*+]\s*(.+)$/);
    if (unorderedMatch) {
      flushParagraph();
      flushQuote();
      if (!inUnorderedList) {
        if (inOrderedList) {
          html.push("</ol>");
          inOrderedList = false;
        }
        html.push("<ul>");
        inUnorderedList = true;
      }
      html.push(`<li>${renderInlineMarkdown(unorderedMatch[1])}</li>`);
      continue;
    }

    const orderedMatch = line.match(/^\s*\d+\.\s*(.+)$/);
    if (orderedMatch) {
      flushParagraph();
      flushQuote();
      if (!inOrderedList) {
        if (inUnorderedList) {
          html.push("</ul>");
          inUnorderedList = false;
        }
        html.push("<ol>");
        inOrderedList = true;
      }
      html.push(`<li>${renderInlineMarkdown(orderedMatch[1])}</li>`);
      continue;
    }

    flushQuote();
    closeLists();
    paragraphLines.push(line);
  }

  flushParagraph();
  flushQuote();
  closeLists();

  if (inCodeBlock) {
    flushCodeBlock();
  }

  return html.join("\n");
}

function renderMarkdownInto(element, markdownText) {
  if (!element) {
    return;
  }

  element.innerHTML = renderMarkdownToHtml(markdownText);
}

function normalizeAssistantMetrics(value) {
  if (!value || typeof value !== "object") {
    return null;
  }

  const normalized = {};
  const integerKeys = ["prompt_tokens", "completion_tokens", "total_tokens", "latency_ms"];
  const floatKeys = ["prompt_tps", "generation_tps", "peak_memory_gb"];

  integerKeys.forEach((key) => {
    const parsed = Number(value[key]);
    if (Number.isFinite(parsed)) {
      normalized[key] = Math.round(parsed);
    }
  });

  floatKeys.forEach((key) => {
    const parsed = Number(value[key]);
    if (Number.isFinite(parsed)) {
      normalized[key] = parsed;
    }
  });

  if (typeof value.raw_metrics === "string" && value.raw_metrics.trim()) {
    normalized.raw_metrics = value.raw_metrics.trim();
  }

  if (value.airllm_required != null) {
    normalized.airllm_required = Boolean(value.airllm_required);
  }

  if (value.airllm_used != null) {
    normalized.airllm_used = Boolean(value.airllm_used);
  }

  if (Array.isArray(value.airllm_logs)) {
    const logs = value.airllm_logs
      .map((entry) => String(entry || "").trim())
      .filter((entry) => entry.length > 0);
    if (logs.length) {
      normalized.airllm_logs = logs.slice(-40);
    }
  }

  return Object.keys(normalized).length ? normalized : null;
}

function addSystemMessage(text) {
  const node = messageTemplate.content.firstElementChild.cloneNode(true);
  node.classList.add("role-system");
  node.querySelector(".message-role").textContent = "system";
  node.querySelector(".message-content").textContent = text;
  const actions = node.querySelector(".message-actions");
  if (actions) {
    actions.remove();
  }
  chatLog.appendChild(node);
  scrollChatToBottom(true);
}

function isChatNearBottom() {
  const distanceFromBottom = chatLog.scrollHeight - chatLog.scrollTop - chatLog.clientHeight;
  return distanceFromBottom <= CHAT_SCROLL_THRESHOLD_PX;
}

function scrollChatToBottom(force = false) {
  if (force || isChatNearBottom()) {
    chatLog.scrollTop = chatLog.scrollHeight;
  }
}

function addMessageCard(role, content, { editable = false, messageIndex = null, forceScroll = true } = {}) {
  const node = messageTemplate.content.firstElementChild.cloneNode(true);
  const roleClass = roleClassFromValue(role);
  node.classList.add(`role-${roleClass}`);
  node.querySelector(".message-role").textContent = role;
  const contentNode = node.querySelector(".message-content");
  if (roleClass === "assistant") {
    contentNode.classList.add("markdown-view");
    renderMarkdownInto(contentNode, content);
  } else {
    contentNode.textContent = content;
  }

  const actions = node.querySelector(".message-actions");
  const editBtn = node.querySelector(".edit-message-btn");
  if (editable && Number.isInteger(messageIndex)) {
    actions.classList.remove("hidden");
    editBtn.addEventListener("click", () => {
      void editMessageAndRegenerate(messageIndex);
    });
  } else {
    actions.remove();
  }

  chatLog.appendChild(node);
  scrollChatToBottom(forceScroll);
}

function updateChatEmptyState() {
  const active = getActiveThread();
  const hasMessages = Boolean(active && Array.isArray(active.messages) && active.messages.length > 0);
  panelChat.classList.toggle("chat-empty-state", !hasMessages);
  if (chatEmptyHero) {
    chatEmptyHero.classList.toggle("hidden", hasMessages);
  }
}

function rebuildChatFromThread() {
  chatLog.innerHTML = "";
  const active = getActiveThread();

  if (!active) {
    setEditMode(null);
    renderSelectedThreadMeta();
    updateChatEmptyState();
    return;
  }

  if (isEditingMessage()) {
    const target = active.messages[pendingEditMessageIndex];
    if (!target || target.role !== "user") {
      setEditMode(null);
    }
  }

  active.messages.forEach((message, index) => {
    if (message.role === "user") {
      addMessageCard("user", message.content, {
        editable: true,
        messageIndex: index,
        forceScroll: false,
      });
      return;
    }

    if (message.role === "assistant" && (message.thinking || message.metrics)) {
      addAssistantHistoryCard(message, { forceScroll: false });
      return;
    }

    addMessageCard(message.role, message.content, { forceScroll: false });
  });

  scrollChatToBottom(true);
  renderSelectedThreadMeta();
  updateChatEmptyState();
}

function appendMessageToActiveThread(role, content, extra = {}) {
  const active = getActiveThread();
  if (!active) {
    return;
  }

  const message = { role, content };
  if (role === "assistant") {
    const thinking = typeof extra.thinking === "string" ? extra.thinking.trim() : "";
    if (thinking) {
      message.thinking = thinking;
    }

    const metrics = normalizeAssistantMetrics(extra.metrics);
    if (metrics) {
      message.metrics = metrics;
    }
  }

  active.messages.push(message);
  active.updatedAt = Date.now();

  if (role === "user" && (active.title === "Nova conversa" || !active.title?.trim())) {
    active.title = deriveThreadTitle(content);
  }

  persistThreads();
  renderThreadList();
  renderSelectedThreadMeta();
  updateChatEmptyState();
}

function setGeneratingState(nextState) {
  isGenerating = nextState;
  sendMessageBtn.disabled = nextState;
  messageInput.disabled = nextState;
  stopGenerationBtn.disabled = !nextState;
}

function isEditingMessage() {
  return Number.isInteger(pendingEditMessageIndex);
}

function setEditMode(messageIndex = null) {
  pendingEditMessageIndex = Number.isInteger(messageIndex) ? messageIndex : null;
  const editing = isEditingMessage();

  chatForm.classList.toggle("edit-mode", editing);
  cancelEditBtn.classList.toggle("hidden", !editing);
  sendMessageBtn.textContent = editing ? "Regenerar" : "Enviar";
}

function applyEditedMessageAndTrim(messageIndex, nextContent) {
  const active = getActiveThread();
  if (!active) {
    return false;
  }

  const target = active.messages[messageIndex];
  if (!target || target.role !== "user") {
    return false;
  }

  active.messages = [...active.messages.slice(0, messageIndex), { role: "user", content: nextContent }];
  active.updatedAt = Date.now();
  active.title = deriveThreadTitle(active.messages.find((entry) => entry.role === "user")?.content || "");
  persistThreads();
  rebuildChatFromThread();
  renderThreadList();
  renderSelectedThreadMeta();
  return true;
}

function createAssistantStreamCard({ forceScroll = true } = {}) {
  const node = assistantStreamTemplate.content.firstElementChild.cloneNode(true);
  node.classList.add("role-assistant");
  const ui = {
    node,
    stateLabel: node.querySelector(".assistant-state-label"),
    runtimeBadge: node.querySelector(".assistant-runtime-badge"),
    typingIndicator: node.querySelector(".typing-indicator"),
    thinkingSection: node.querySelector(".assistant-thinking"),
    thinkingText: node.querySelector(".assistant-thinking-text"),
    answerSection: node.querySelector(".assistant-answer"),
    answerText: node.querySelector(".assistant-answer-text"),
    metricsSection: node.querySelector(".assistant-metrics"),
    metricsText: node.querySelector(".assistant-metrics-text"),
    finalAnswer: "",
    answerRaw: "",
    thinkingQueue: "",
    answerQueue: "",
    flushTimer: null,
    latestMetrics: null,
    airllmRequired: false,
    airllmUsed: false,
    airllmLogs: [],
  };

  chatLog.appendChild(node);
  scrollChatToBottom(forceScroll);
  return ui;
}

function addAssistantHistoryCard(message, { forceScroll = true } = {}) {
  const ui = createAssistantStreamCard({ forceScroll: false });
  setAssistantState(ui, "completed");

  const savedThinking = typeof message?.thinking === "string" ? message.thinking.trim() : "";
  const savedAnswer = String(message?.content || "").trim();
  const savedMetrics = normalizeAssistantMetrics(message?.metrics);

  setAssistantRuntimeBadge(ui, {
    airllmRequired: Boolean(savedMetrics?.airllm_required),
    airllmUsed: Boolean(savedMetrics?.airllm_used),
  });
  if (Array.isArray(savedMetrics?.airllm_logs)) {
    ui.airllmLogs = savedMetrics.airllm_logs.slice(-40);
    updateAssistantRuntimeBadgeTooltip(ui);
  }

  if (savedThinking) {
    ui.thinkingSection.classList.remove("hidden");
    ui.thinkingText.textContent = savedThinking;
  }

  if (savedAnswer) {
    ui.answerSection.classList.remove("hidden");
    ui.answerRaw = message.content;
    ui.finalAnswer = message.content;
    renderMarkdownInto(ui.answerText, ui.answerRaw);
  }

  if (savedMetrics) {
    renderAssistantMetrics(ui, savedMetrics);
  }

  if (!savedThinking && !savedAnswer && !savedMetrics) {
    ui.answerSection.classList.remove("hidden");
    ui.answerRaw = "(sem resposta textual)";
    ui.finalAnswer = ui.answerRaw;
    renderMarkdownInto(ui.answerText, ui.answerRaw);
  }

  scrollChatToBottom(forceScroll);
}

function setAssistantState(ui, status) {
  const labels = {
    waiting: "aguardando modelo",
    airllm_required: "AIRLLM necessario",
    fallback_airllm: "AIRLLM em uso",
    thinking: "thinking",
    answering: "respondendo",
    completed: "finalizado",
    cancelled: "interrompido",
    error: "erro",
  };

  ui.stateLabel.textContent = labels[status] || status;

  if (["waiting", "airllm_required", "fallback_airllm"].includes(status)) {
    ui.typingIndicator.classList.remove("hidden");
  } else {
    ui.typingIndicator.classList.add("hidden");
  }

  if (status === "thinking") {
    ui.thinkingSection.classList.remove("hidden");
  }

  if (["answering", "completed", "cancelled"].includes(status)) {
    ui.answerSection.classList.remove("hidden");
  }
}

function setAssistantRuntimeBadge(ui, { airllmRequired = false, airllmUsed = false } = {}) {
  if (!ui?.runtimeBadge) {
    return;
  }

  ui.airllmRequired = Boolean(airllmRequired);
  ui.airllmUsed = Boolean(airllmUsed);
  ui.runtimeBadge.classList.remove("hidden", "airllm-used");

  if (!ui.airllmRequired && !ui.airllmUsed) {
    ui.runtimeBadge.classList.add("hidden");
    ui.runtimeBadge.textContent = "";
    ui.runtimeBadge.removeAttribute("title");
    ui.runtimeBadge.removeAttribute("aria-label");
    return;
  }

  if (ui.airllmUsed) {
    ui.runtimeBadge.textContent = "AIRLLM em uso";
    ui.runtimeBadge.classList.add("airllm-used");
    updateAssistantRuntimeBadgeTooltip(ui);
    return;
  }

  ui.runtimeBadge.textContent = "AIRLLM necessario";
  updateAssistantRuntimeBadgeTooltip(ui);
}

function updateAssistantRuntimeBadgeTooltip(ui) {
  if (!ui?.runtimeBadge) {
    return;
  }

  if (!ui.airllmRequired && !ui.airllmUsed) {
    ui.runtimeBadge.removeAttribute("title");
    ui.runtimeBadge.removeAttribute("aria-label");
    ui.runtimeBadge.removeAttribute("data-airllm-tooltip");
    return;
  }

  const lines = [];
  if (ui.airllmUsed) {
    lines.push("AIRLLM em uso.");
  } else if (ui.airllmRequired) {
    lines.push("AIRLLM necessario.");
  }

  if (Array.isArray(ui.airllmLogs) && ui.airllmLogs.length) {
    lines.push("", "Logs AIRLLM (mais recentes):");
    lines.push(...ui.airllmLogs.slice(-20));
  } else {
    lines.push("", "Sem logs do AIRLLM ainda.");
  }

  const tooltip = lines.join("\n").trim();
  if (tooltip) {
    ui.runtimeBadge.title = tooltip;
    ui.runtimeBadge.setAttribute("aria-label", tooltip);
    ui.runtimeBadge.setAttribute("data-airllm-tooltip", tooltip);
  } else {
    ui.runtimeBadge.removeAttribute("title");
    ui.runtimeBadge.removeAttribute("aria-label");
    ui.runtimeBadge.removeAttribute("data-airllm-tooltip");
  }
}

function appendAirllmLog(ui, message) {
  if (!ui) {
    return;
  }

  const clean = String(message || "").trim();
  if (!clean) {
    return;
  }

  const timestamp = new Date().toLocaleTimeString("pt-BR", { hour12: false });
  const lastRaw = ui.airllmLastLogRaw || "";
  if (lastRaw === clean) {
    return;
  }
  ui.airllmLastLogRaw = clean;

  if (!Array.isArray(ui.airllmLogs)) {
    ui.airllmLogs = [];
  }
  ui.airllmLogs.push(`[${timestamp}] ${clean}`);
  if (ui.airllmLogs.length > 40) {
    ui.airllmLogs = ui.airllmLogs.slice(-40);
  }

  if (ui.latestMetrics && typeof ui.latestMetrics === "object") {
    ui.latestMetrics.airllm_logs = ui.airllmLogs.slice(-40);
  }

  updateAssistantRuntimeBadgeTooltip(ui);
  renderAirllmLiveLogs(ui);
}

function renderAirllmLiveLogs(ui) {
  if (!ui?.metricsSection || !ui?.metricsText) {
    return;
  }
  if (!Array.isArray(ui.airllmLogs) || !ui.airllmLogs.length) {
    return;
  }

  const lines = [];
  if (ui.airllmUsed) {
    lines.push("AIRLLM em uso.");
  } else if (ui.airllmRequired) {
    lines.push("AIRLLM necessario.");
  }
  lines.push("", "AIRLLM logs (ao vivo):");
  lines.push(...ui.airllmLogs.slice(-12));

  ui.metricsText.textContent = lines.join("\n");
  ui.metricsSection.classList.remove("hidden");
  scrollChatToBottom();
}

function flushAssistantQueues(ui) {
  let changed = false;

  if (ui.thinkingQueue.length) {
    const chunk = ui.thinkingQueue.slice(0, STREAM_CHARS_PER_TICK);
    ui.thinkingQueue = ui.thinkingQueue.slice(chunk.length);
    ui.thinkingSection.classList.remove("hidden");
    ui.thinkingText.textContent += chunk;
    changed = true;
  }

  if (ui.answerQueue.length) {
    const chunk = ui.answerQueue.slice(0, STREAM_CHARS_PER_TICK);
    ui.answerQueue = ui.answerQueue.slice(chunk.length);
    ui.answerSection.classList.remove("hidden");
    ui.answerRaw += chunk;
    renderMarkdownInto(ui.answerText, ui.answerRaw);
    ui.finalAnswer = ui.answerRaw;
    changed = true;
  }

  if (changed) {
    scrollChatToBottom();
  }

  if (!ui.thinkingQueue.length && !ui.answerQueue.length && ui.flushTimer !== null) {
    window.clearInterval(ui.flushTimer);
    ui.flushTimer = null;
  }
}

function scheduleAssistantFlush(ui) {
  if (ui.flushTimer !== null) {
    return;
  }

  ui.flushTimer = window.setInterval(() => {
    flushAssistantQueues(ui);
  }, STREAM_TICK_MS);
}

async function waitForAssistantFlush(ui) {
  if (!ui.thinkingQueue.length && !ui.answerQueue.length && ui.flushTimer === null) {
    return;
  }

  await new Promise((resolve) => {
    const pollTimer = window.setInterval(() => {
      if (!ui.thinkingQueue.length && !ui.answerQueue.length && ui.flushTimer === null) {
        window.clearInterval(pollTimer);
        resolve();
      }
    }, STREAM_TICK_MS);
  });
}

function appendThinking(ui, delta) {
  if (!delta) {
    return;
  }

  ui.thinkingQueue += delta;
  scheduleAssistantFlush(ui);
}

function appendAnswer(ui, delta) {
  if (!delta) {
    return;
  }

  ui.answerQueue += delta;
  scheduleAssistantFlush(ui);
}

function extractRawMetrics(rawOutput) {
  if (typeof rawOutput !== "string" || !rawOutput.trim()) {
    return null;
  }

  const normalized = rawOutput.replace(/\r\n/g, "\n");
  const marker = "==========";
  const firstMarker = normalized.indexOf(marker);
  if (firstMarker === -1) {
    return null;
  }

  const afterFirst = normalized.slice(firstMarker + marker.length).replace(/^\n+/, "");
  const secondMarker = afterFirst.indexOf(marker);
  if (secondMarker === -1) {
    return null;
  }

  const metrics = afterFirst.slice(secondMarker + marker.length).trim();
  return metrics || null;
}

function extractMlxPilotRuntimeMeta(rawOutput) {
  if (typeof rawOutput !== "string" || !rawOutput.trim()) {
    return null;
  }

  const firstLine = rawOutput.replace(/\r\n/g, "\n").split("\n", 1)[0]?.trim() || "";
  if (!firstLine.startsWith("[[MLX-PILOT-META")) {
    return null;
  }

  const requiredMatch = firstLine.match(/airllm_required=(\d+)/i);
  const usedMatch = firstLine.match(/airllm_used=(\d+)/i);

  return {
    airllm_required: requiredMatch ? requiredMatch[1] === "1" : false,
    airllm_used: usedMatch ? usedMatch[1] === "1" : false,
  };
}

function splitThinkingAndAnswer(content) {
  if (!content) {
    return { thinking: "", answer: "" };
  }

  const text = String(content);
  const openTag = "<think>";
  const closeTag = "</think>";
  const openIndex = text.indexOf(openTag);
  if (openIndex === -1) {
    return { thinking: "", answer: text.trimStart() };
  }

  const afterOpen = text.slice(openIndex + openTag.length);
  const closeIndex = afterOpen.indexOf(closeTag);
  if (closeIndex === -1) {
    return { thinking: afterOpen.trimStart(), answer: "" };
  }

  return {
    thinking: afterOpen.slice(0, closeIndex).trimStart(),
    answer: afterOpen.slice(closeIndex + closeTag.length).trimStart(),
  };
}

function isThinkingModel(modelId) {
  const normalized = String(modelId || "").trim().toLowerCase();
  if (!normalized) {
    return false;
  }

  return (
    normalized.includes("deepseek-r1")
    || normalized.includes("reason")
    || normalized.includes("thinking")
    || normalized.includes("qwq")
    || /(?:^|[-_/])r1(?:$|[-_/])/.test(normalized)
  );
}

function resolveChatMaxTokens(modelId) {
  return isThinkingModel(modelId) ? CHAT_MAX_TOKENS_THINKING : CHAT_MAX_TOKENS_DEFAULT;
}

function promoteThinkingToAnswerIfNeeded(ui) {
  if (!ui || String(ui.finalAnswer || "").trim()) {
    return false;
  }

  const fallbackAnswer = String(ui.thinkingText?.textContent || "").trim();
  if (!fallbackAnswer) {
    return false;
  }

  ui.thinkingText.textContent = "";
  ui.thinkingSection.classList.add("hidden");
  setAssistantState(ui, "answering");
  appendAnswer(ui, fallbackAnswer);
  return true;
}

function renderAssistantMetrics(ui, event) {
  const normalizedEvent = normalizeAssistantMetrics(event);
  if (!normalizedEvent) {
    return;
  }

  const airllmRequired = normalizedEvent.airllm_required != null
    ? Boolean(normalizedEvent.airllm_required)
    : Boolean(ui.airllmRequired);
  const airllmUsed = normalizedEvent.airllm_used != null
    ? Boolean(normalizedEvent.airllm_used)
    : Boolean(ui.airllmUsed);

  setAssistantRuntimeBadge(ui, {
    airllmRequired,
    airllmUsed,
  });

  ui.latestMetrics = {
    ...normalizedEvent,
    airllm_required: airllmRequired,
    airllm_used: airllmUsed,
    airllm_logs: Array.isArray(normalizedEvent.airllm_logs)
      ? normalizedEvent.airllm_logs.slice(-40)
      : Array.isArray(ui.airllmLogs)
        ? ui.airllmLogs.slice(-40)
        : undefined,
  };
  if (Array.isArray(ui.latestMetrics.airllm_logs)) {
    ui.airllmLogs = ui.latestMetrics.airllm_logs.slice(-40);
    updateAssistantRuntimeBadgeTooltip(ui);
  }

  const lines = [];
  const hasRawMetrics = typeof normalizedEvent.raw_metrics === "string" && normalizedEvent.raw_metrics.trim().length > 0;

  if (!hasRawMetrics && normalizedEvent.prompt_tokens != null) {
    lines.push(`Prompt: ${normalizedEvent.prompt_tokens} tokens`);
  }
  if (!hasRawMetrics && normalizedEvent.completion_tokens != null) {
    lines.push(`Generation: ${normalizedEvent.completion_tokens} tokens`);
  }
  if (normalizedEvent.total_tokens != null) {
    lines.push(`Total: ${normalizedEvent.total_tokens} tokens`);
  }
  if (!hasRawMetrics && normalizedEvent.prompt_tps != null) {
    lines.push(`Prompt rate: ${Number(normalizedEvent.prompt_tps).toFixed(3)} tokens/sec`);
  }
  if (!hasRawMetrics && normalizedEvent.generation_tps != null) {
    lines.push(`Generation rate: ${Number(normalizedEvent.generation_tps).toFixed(3)} tokens/sec`);
  }
  if (!hasRawMetrics && normalizedEvent.peak_memory_gb != null) {
    lines.push(`Peak memory: ${Number(normalizedEvent.peak_memory_gb).toFixed(3)} GB`);
  }
  if (normalizedEvent.latency_ms != null) {
    lines.push(`Latency: ${normalizedEvent.latency_ms} ms`);
  }
  if (airllmRequired) {
    lines.push(`AIRLLM necessario: sim`);
  }
  if (airllmUsed) {
    lines.push(`AIRLLM usado: sim`);
  }
  if (Array.isArray(ui.airllmLogs) && ui.airllmLogs.length) {
    lines.push("");
    lines.push("AIRLLM logs:");
    lines.push(...ui.airllmLogs.slice(-8));
  }

  if (hasRawMetrics) {
    lines.unshift(normalizedEvent.raw_metrics.trim());
  }

  if (lines.length) {
    ui.metricsText.textContent = lines.join("\n");
    ui.metricsSection.classList.remove("hidden");
    scrollChatToBottom();
  }
}

function isAbortError(error) {
  return (
    error?.name === "AbortError" ||
    String(error?.message || "").toLowerCase().includes("aborted")
  );
}

function setWebsearchToggleState(nextState, { persist = true } = {}) {
  const enabled = Boolean(nextState);
  chatWebsearchToggle.checked = enabled;
  if (chatWebsearchBtn) {
    chatWebsearchBtn.classList.toggle("active", enabled);
    chatWebsearchBtn.setAttribute("aria-pressed", enabled ? "true" : "false");
  }
  if (persist) {
    localStorage.setItem(STORAGE_CHAT_WEBSEARCH_ENABLED, enabled ? "1" : "0");
  }
}

function setChatAirllmToggleState(nextState, { persist = true } = {}) {
  const enabled = Boolean(nextState);
  chatAirllmEnabled = enabled;
  if (chatAirllmToggleBtn) {
    chatAirllmToggleBtn.classList.toggle("enabled", enabled);
    chatAirllmToggleBtn.setAttribute("aria-pressed", enabled ? "true" : "false");
    chatAirllmToggleBtn.textContent = enabled ? "AIRLLM ON" : "AIRLLM OFF";
  }
  if (persist) {
    localStorage.setItem(STORAGE_CHAT_AIRLLM_ENABLED, enabled ? "1" : "0");
  }
}

async function persistChatAirllmToggle(nextState) {
  if (chatAirllmPersistInFlight) {
    return;
  }

  chatAirllmPersistInFlight = true;
  if (chatAirllmToggleBtn) {
    chatAirllmToggleBtn.disabled = true;
  }

  try {
    const payload = await fetchJson("/config", { method: "GET" });
    payload.mlx_airllm_enabled = Boolean(nextState);

    await fetchJson("/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
  } catch (error) {
    console.error("Falha ao persistir toggle AIRLLM", error);
    addSystemMessage(
      `AIRLLM mudou localmente, mas nao foi possivel salvar no backend: ${error.message}`,
    );
  } finally {
    chatAirllmPersistInFlight = false;
    if (chatAirllmToggleBtn) {
      chatAirllmToggleBtn.disabled = false;
    }
  }
}

function getEnvironmentValue(key) {
  if (!openclawEnvList) {
    return "";
  }
  const normalized = String(key || "").trim().toUpperCase();
  if (!normalized) {
    return "";
  }

  const envInput = Array.from(openclawEnvList.querySelectorAll("input[data-env-key]")).find(
    (input) => String(input.dataset.envKey || "").trim().toUpperCase() === normalized
  );
  return (envInput?.value || "").trim();
}

function getBraveApiKey() {
  return getEnvironmentValue("BRAVE_API_KEY");
}

function buildWebsearchSkeletonPrompt({ apiKeyConfigured, searchSummary }) {
  const keyStatus = apiKeyConfigured ? "configured" : "missing";
  const parts = [
    "OpenClaw WebSearch skeleton (Brave API) enabled for this turn.",
    `Brave API key status: ${keyStatus}.`,
    "When relevant, use web evidence to improve factuality and cite source URLs.",
    "Reference skeleton:",
    "1) GET https://api.search.brave.com/res/v1/web/search?q=<query>&count=5",
    "2) Header: X-Subscription-Token: <BRAVE_API_KEY>",
    "3) Read web.results[] and synthesize a concise answer with links.",
  ];

  if (searchSummary) {
    parts.push("");
    parts.push("Pre-fetched Brave web results for this user query:");
    parts.push(searchSummary);
  } else if (!apiKeyConfigured) {
    parts.push("");
    parts.push("No Brave key configured; answer without live web retrieval.");
  }

  return parts.join("\n");
}

function formatBraveSearchSummary(results) {
  if (!Array.isArray(results) || !results.length) {
    return "No Brave results returned.";
  }

  return results
    .slice(0, 5)
    .map((entry, index) => {
      const title = String(entry.title || "sem titulo").trim();
      const url = String(entry.url || "").trim();
      const description = String(entry.description || "").trim();
      return `${index + 1}. ${title}\nURL: ${url}\nResumo: ${description}`;
    })
    .join("\n\n");
}

async function maybeInjectWebsearchContext(messages) {
  if (!chatWebsearchToggle.checked) {
    return messages;
  }

  let apiKey = getBraveApiKey();
  const hasEnvironmentSnapshot = Boolean(openclawEnvList?.querySelector("input[data-env-key]"));
  if (!apiKey && !hasEnvironmentSnapshot) {
    await loadOpenclawEnvironment({ showLoading: false });
    apiKey = getBraveApiKey();
  }
  const latestUserMessage = [...messages]
    .reverse()
    .find((entry) => entry.role === "user")
    ?.content?.trim();

  let searchSummary = "";
  let keyConfigured = Boolean(apiKey);
  if (latestUserMessage) {
    try {
      const payload = {
        query: latestUserMessage,
        max_results: 5,
      };
      if (apiKey) {
        payload.api_key = apiKey;
      }
      const response = await fetchJson("/web/brave/search", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload),
      });
      searchSummary = formatBraveSearchSummary(response.results);
      keyConfigured = keyConfigured || Boolean(response?.key_source);
    } catch (error) {
      addSystemMessage(`WebSearch Brave indisponivel: ${error.message}`);
    }
  }

  const systemPrompt = buildWebsearchSkeletonPrompt({
    apiKeyConfigured: keyConfigured,
    searchSummary,
  });

  return [{ role: "system", content: systemPrompt }, ...messages];
}

async function buildChatPayload(messages) {
  const promptMessages = Array.isArray(messages)
    ? messages
      .filter((entry) => entry && typeof entry.role === "string" && typeof entry.content === "string")
      .map((entry) => ({ role: entry.role, content: entry.content }))
    : [];

  const payloadMessages = await maybeInjectWebsearchContext(promptMessages);
  return {
    model_id: selectedModelId,
    messages: payloadMessages,
    options: {
      temperature: 0.2,
      max_tokens: resolveChatMaxTokens(selectedModelId),
      airllm_enabled: chatAirllmEnabled,
    },
  };
}

async function consumeChatClassic(payload, ui, signal) {
  const response = await fetch(`${daemonBaseUrl}/chat`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
    signal,
  });

  if (!response.ok) {
    const detail = await parseErrorDetail(response);
    throw createHttpError(response.status, detail);
  }

  const body = await response.json();
  const split = splitThinkingAndAnswer(body?.message?.content || "");
  const runtimeMeta = extractMlxPilotRuntimeMeta(body?.raw_output);

  if (split.thinking) {
    setAssistantState(ui, "thinking");
    appendThinking(ui, split.thinking);
  }

  if (split.answer) {
    setAssistantState(ui, "answering");
    appendAnswer(ui, split.answer);
  }

  await waitForAssistantFlush(ui);
  if (promoteThinkingToAnswerIfNeeded(ui)) {
    await waitForAssistantFlush(ui);
  }

  setAssistantRuntimeBadge(ui, {
    airllmRequired: Boolean(runtimeMeta?.airllm_required),
    airllmUsed: Boolean(runtimeMeta?.airllm_used),
  });

  setAssistantState(ui, "completed");
  renderAssistantMetrics(ui, {
    prompt_tokens: body?.usage?.prompt_tokens,
    completion_tokens: body?.usage?.completion_tokens,
    total_tokens: body?.usage?.total_tokens,
    latency_ms: body?.latency_ms,
    raw_metrics: extractRawMetrics(body?.raw_output),
    airllm_required: runtimeMeta?.airllm_required,
    airllm_used: runtimeMeta?.airllm_used,
  });
}

async function consumeChatStream(payload, ui, signal) {
  const response = await fetch(`${daemonBaseUrl}/chat/stream`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
    signal,
  });

  if (!response.ok) {
    if ((response.status === 404 || response.status === 405) && !signal?.aborted) {
      if (!streamFallbackNotified) {
        addSystemMessage(
          "Daemon sem suporte a /chat/stream detectado. Usando modo de compatibilidade em /chat.",
        );
        streamFallbackNotified = true;
      }
      await consumeChatClassic(payload, ui, signal);
      return;
    }

    const detail = await parseErrorDetail(response);
    throw createHttpError(response.status, detail);
  }

  if (!response.body) {
    throw new Error("Resposta de stream sem corpo");
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder("utf-8");
  let buffer = "";
  let doneEvent = null;

  outer: while (true) {
    const { done, value } = await reader.read();
    if (done) {
      buffer += decoder.decode();
      break;
    }

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split("\n");
    buffer = lines.pop() || "";

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed) {
        continue;
      }

      let event;
      try {
        event = JSON.parse(trimmed);
      } catch {
        continue;
      }

      if (event.event === "status") {
        const streamStatus = event.status || "waiting";
        if (streamStatus === "airllm_required") {
          setAssistantRuntimeBadge(ui, {
            airllmRequired: true,
            airllmUsed: Boolean(ui.airllmUsed),
          });
        }
        if (streamStatus === "fallback_airllm") {
          setAssistantRuntimeBadge(ui, {
            airllmRequired: true,
            airllmUsed: true,
          });
        }
        setAssistantState(ui, streamStatus);
        continue;
      }

      if (event.event === "thinking_delta") {
        setAssistantState(ui, "thinking");
        appendThinking(ui, event.delta || "");
        continue;
      }

      if (event.event === "answer_delta") {
        setAssistantState(ui, "answering");
        appendAnswer(ui, event.delta || "");
        continue;
      }

      if (event.event === "metrics") {
        ui.latestMetrics = event;
        continue;
      }

      if (event.event === "airllm_log") {
        appendAirllmLog(ui, event.message || "");
        continue;
      }

      if (event.event === "done") {
        if (event.airllm_required != null || event.airllm_used != null) {
          setAssistantRuntimeBadge(ui, {
            airllmRequired: Boolean(event.airllm_required),
            airllmUsed: Boolean(event.airllm_used),
          });
        }
        doneEvent = event;
        break outer;
      }

      if (event.event === "error") {
        throw createHttpError(500, event.message || "Erro na geracao");
      }
    }
  }

  if (!doneEvent && buffer.trim()) {
    try {
      const trailing = JSON.parse(buffer.trim());
      if (trailing.event === "done") {
        if (trailing.airllm_required != null || trailing.airllm_used != null) {
          setAssistantRuntimeBadge(ui, {
            airllmRequired: Boolean(trailing.airllm_required),
            airllmUsed: Boolean(trailing.airllm_used),
          });
        }
        doneEvent = trailing;
      } else if (trailing.event === "metrics") {
        ui.latestMetrics = trailing;
      } else if (trailing.event === "error") {
        throw createHttpError(500, trailing.message || "Erro na geracao");
      }
    } catch {
      // ignore trailing parse errors
    }
  }

  if (!doneEvent) {
    throw new Error("Stream encerrado sem evento final");
  }

  await waitForAssistantFlush(ui);
  if (promoteThinkingToAnswerIfNeeded(ui)) {
    await waitForAssistantFlush(ui);
  }
  setAssistantState(ui, "completed");
  renderAssistantMetrics(ui, doneEvent || ui.latestMetrics);
}

async function runAssistantGeneration() {
  const thread = getActiveThread();
  if (!thread) {
    addSystemMessage("Nenhuma conversa ativa.");
    return;
  }

  const assistantUi = createAssistantStreamCard();
  setAssistantState(assistantUi, "waiting");
  setStatus("gerando resposta", "running");

  activeStreamController = new AbortController();
  setGeneratingState(true);

  try {
    const payload = await buildChatPayload(thread.messages);
    await consumeChatStream(payload, assistantUi, activeStreamController.signal);

    const finalAnswer = assistantUi.finalAnswer.trim();
    const finalThinking = String(assistantUi.thinkingText?.textContent || "").trim();
    const finalMetrics = normalizeAssistantMetrics({
      ...(assistantUi.latestMetrics || {}),
      airllm_required: assistantUi.airllmRequired,
      airllm_used: assistantUi.airllmUsed,
      airllm_logs: Array.isArray(assistantUi.airllmLogs) ? assistantUi.airllmLogs.slice(-40) : undefined,
    });
    if (finalAnswer || finalThinking || finalMetrics) {
      appendMessageToActiveThread(
        "assistant",
        finalAnswer || "(sem resposta textual)",
        {
          thinking: finalThinking,
          metrics: finalMetrics,
        },
      );
    }

    setStatus("resposta concluida");
  } catch (error) {
    if (isAbortError(error)) {
      setAssistantState(assistantUi, "cancelled");
      assistantUi.metricsSection.classList.remove("hidden");
      assistantUi.metricsText.textContent = "Geracao interrompida pelo usuario.";
      const partialAnswer = assistantUi.finalAnswer.trim();
      const partialThinking = String(assistantUi.thinkingText?.textContent || "").trim();
      const partialMetrics = normalizeAssistantMetrics({
        ...(assistantUi.latestMetrics || {}),
        airllm_required: assistantUi.airllmRequired,
        airllm_used: assistantUi.airllmUsed,
        airllm_logs: Array.isArray(assistantUi.airllmLogs) ? assistantUi.airllmLogs.slice(-40) : undefined,
      });
      if (partialAnswer || partialThinking || partialMetrics) {
        appendMessageToActiveThread(
          "assistant",
          partialAnswer || "(geracao interrompida sem resposta final)",
          {
            thinking: partialThinking,
            metrics: partialMetrics,
          },
        );
      }
      setStatus("geracao interrompida");
      scrollChatToBottom(true);
      return;
    }

    setAssistantState(assistantUi, "error");
    assistantUi.metricsSection.classList.remove("hidden");
    assistantUi.metricsText.textContent = `Erro: ${error.message}`;
    scrollChatToBottom(true);
    addSystemMessage(`Erro no chat: ${error.message}`);
    setStatus("erro chat", "error");
  } finally {
    activeStreamController = null;
    setGeneratingState(false);
    messageInput.focus();
    renderThreadList();
  }
}

async function editMessageAndRegenerate(messageIndex) {
  const active = getActiveThread();
  if (!active) {
    return;
  }

  const entry = active.messages[messageIndex];
  if (!entry || entry.role !== "user") {
    return;
  }

  if (isGenerating) {
    addSystemMessage("Pare a geracao atual antes de editar uma mensagem.");
    return;
  }
  setEditMode(messageIndex);
  messageInput.value = entry.content;
  messageInput.focus();
  setStatus("edicao pronta");
}

async function loadModels() {
  setStatus("carregando modelos", "running");
  try {
    const models = await fetchJson("/models");
    localModels = Array.isArray(models) ? models : [];

    if (!localModels.some((entry) => entry.id === selectedModelId)) {
      selectedModelId = localModels[0]?.id || null;
    }

    ensureActiveThread();
    syncModelWithActiveThread();
    renderThreadList();
    renderSelectedThreadMeta();
    renderOpenClawModelSelectors();
    renderInstalledModelsList();

    setStatus("pronto");
  } catch (error) {
    setStatus("erro modelos", "error");
    addSystemMessage(`Falha ao carregar modelos: ${error.message}`);
  }
}

function renderInstalledModelsList() {
  if (!installedModelsList || !installedModelsMeta) {
    return;
  }

  installedModelsList.innerHTML = "";
  const installed = localModels.filter((entry) => String(entry.provider || "").toLowerCase() === "mlx");
  installedModelsMeta.textContent = `${installed.length} modelo(s) local(is) instalado(s).`;

  if (!installed.length) {
    const empty = document.createElement("li");
    empty.className = "installed-model-empty";
    empty.textContent = "Nenhum modelo local instalado encontrado no diretorio atual.";
    installedModelsList.appendChild(empty);
    return;
  }

  installed.forEach((model) => {
    const item = document.createElement("li");
    item.className = "installed-model-item";

    const main = document.createElement("div");
    main.className = "installed-model-main";

    const name = document.createElement("p");
    name.className = "installed-model-name";
    name.textContent = model.name || model.id || "-";

    const meta = document.createElement("p");
    meta.className = "installed-model-meta";
    meta.textContent = `${model.id || "-"} • ${model.path || "-"}`;

    main.appendChild(name);
    main.appendChild(meta);

    const actions = document.createElement("div");
    actions.className = "installed-model-actions";

    const renameBtn = document.createElement("button");
    renameBtn.type = "button";
    renameBtn.className = "ghost-btn";
    renameBtn.textContent = "Renomear";
    renameBtn.addEventListener("click", () => {
      void renameInstalledModel(model.id);
    });

    const deleteBtn = document.createElement("button");
    deleteBtn.type = "button";
    deleteBtn.className = "ghost-btn danger";
    deleteBtn.textContent = "Apagar";
    deleteBtn.addEventListener("click", () => {
      void deleteInstalledModel(model.id);
    });

    actions.appendChild(renameBtn);
    actions.appendChild(deleteBtn);
    item.appendChild(main);
    item.appendChild(actions);
    installedModelsList.appendChild(item);
  });
}

function normalizeMlxModelSelectionId(modelId) {
  const raw = String(modelId || "").trim();
  if (!raw) {
    return "";
  }
  if (raw.toLowerCase().startsWith("mlx::")) {
    return raw.slice(5).trim();
  }
  return raw;
}

function buildMlxModelSelectionId(rawLocalId, sampleSelectionId) {
  const normalizedLocalId = normalizeMlxModelSelectionId(rawLocalId);
  const sample = String(sampleSelectionId || "").trim().toLowerCase();
  if (sample.startsWith("mlx::")) {
    return `mlx::${normalizedLocalId}`;
  }
  return normalizedLocalId;
}

function switchDiscoverSubtab(nextSubtab) {
  const normalized = nextSubtab === "installed" ? "installed" : "catalog";
  activeDiscoverSubtab = normalized;

  discoverSubtabButtons.forEach((button) => {
    const isActive = button.dataset.discoverSubtab === normalized;
    button.classList.toggle("active", isActive);
    button.setAttribute("aria-selected", isActive ? "true" : "false");
  });

  if (discoverCatalogView) {
    discoverCatalogView.classList.toggle("active", normalized === "catalog");
  }
  if (discoverInstalledView) {
    discoverInstalledView.classList.toggle("active", normalized === "installed");
  }

  if (normalized === "installed") {
    renderInstalledModelsList();
  }
}

async function renameInstalledModel(modelId) {
  const current = localModels.find((entry) => entry.id === modelId);
  if (!current) {
    addSystemMessage("Modelo nao encontrado para renomear.");
    return;
  }
  const currentLocalId = normalizeMlxModelSelectionId(current.id);

  const nextName = await showTextPrompt({
    title: "Renomear modelo instalado",
    message: "Informe o novo nome da pasta do modelo local.",
    defaultValue: currentLocalId,
    confirmLabel: "Renomear",
  });
  if (nextName === null) {
    return;
  }

  const normalized = normalizeMlxModelSelectionId(nextName);
  if (!normalized || normalized === currentLocalId) {
    return;
  }

  try {
    await fetchJson("/models/rename", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ current_id: currentLocalId, new_id: normalized }),
    });
    const nextSelectionId = buildMlxModelSelectionId(normalized, current.id);
    chatThreads.forEach((thread) => {
      const threadLocalId = normalizeMlxModelSelectionId(thread.modelId);
      if (thread.modelId === current.id || threadLocalId === currentLocalId) {
        thread.modelId = nextSelectionId;
        thread.updatedAt = Date.now();
      }
    });
    if (
      selectedModelId === current.id
      || normalizeMlxModelSelectionId(selectedModelId) === currentLocalId
    ) {
      selectedModelId = nextSelectionId;
    }
    persistThreads();
    await loadModels();
    setStatus("modelo renomeado");
  } catch (error) {
    addSystemMessage(`Falha ao renomear modelo: ${error.message}`);
    setStatus("erro ao renomear modelo", "error");
  }
}

async function deleteInstalledModel(modelId) {
  const current = localModels.find((entry) => entry.id === modelId);
  if (!current) {
    addSystemMessage("Modelo nao encontrado para apagar.");
    return;
  }
  const currentLocalId = normalizeMlxModelSelectionId(current.id);

  const confirmed = await showConfirmDialog({
    title: "Apagar modelo instalado",
    message: `Deseja apagar o modelo "${currentLocalId}"? Essa acao remove os arquivos do disco.`,
    confirmLabel: "Apagar",
    danger: true,
  });
  if (!confirmed) {
    return;
  }

  try {
    await fetchJson(`/models/${encodeURIComponent(currentLocalId)}`, {
      method: "DELETE",
    });
    chatThreads.forEach((thread) => {
      const threadLocalId = normalizeMlxModelSelectionId(thread.modelId);
      if (thread.modelId === current.id || threadLocalId === currentLocalId) {
        thread.modelId = null;
        thread.updatedAt = Date.now();
      }
    });
    if (
      selectedModelId === current.id
      || normalizeMlxModelSelectionId(selectedModelId) === currentLocalId
    ) {
      selectedModelId = null;
    }
    persistThreads();
    await loadModels();
    setStatus("modelo apagado");
  } catch (error) {
    addSystemMessage(`Falha ao apagar modelo: ${error.message}`);
    setStatus("erro ao apagar modelo", "error");
  }
}

async function loadCatalogSources() {
  try {
    const sources = await fetchJson("/catalog/sources");

    catalogSource.innerHTML = "";
    sources.forEach((source) => {
      const option = document.createElement("option");
      option.value = source.id;
      option.textContent = source.name;
      catalogSource.appendChild(option);
    });

    if (!catalogSource.value && sources[0]) {
      catalogSource.value = sources[0].id;
    }
  } catch (error) {
    catalogSource.innerHTML = "";
    const fallback = document.createElement("option");
    fallback.value = "huggingface";
    fallback.textContent = "Hugging Face";
    catalogSource.appendChild(fallback);
    catalogSource.value = "huggingface";
    catalogMeta.textContent = `Catalogo indisponivel: ${error.message}`;
  }
}

function renderCatalogModels(models) {
  remoteResults.innerHTML = "";

  if (!models.length) {
    catalogMeta.textContent = "Nenhum modelo encontrado para esse filtro.";
    return;
  }

  catalogMeta.textContent = `${models.length} modelo(s) encontrado(s)`;

  models.forEach((model) => {
    const node = remoteCardTemplate.content.firstElementChild.cloneNode(true);

    node.querySelector(".remote-name").textContent = model.name;
    node.querySelector(".remote-subtitle").textContent = `${model.author} / ${model.model_id}`;
    node.querySelector(".remote-task").textContent = model.task || "task n/d";
    const summaryNode = node.querySelector(".remote-summary");
    if (isMeaningfulCatalogSummary(model.summary)) {
      summaryNode.textContent = String(model.summary).trim();
    } else {
      summaryNode.remove();
    }
    node.querySelector(".remote-size").textContent = formatBytes(model.size_bytes);
    node.querySelector(".remote-downloads").textContent = `${formatNumber(model.downloads)} downloads`;
    node.querySelector(".remote-likes").textContent = `${formatNumber(model.likes)} likes`;

    const tagsContainer = node.querySelector(".remote-tags");
    const visibleTags = Array.isArray(model.tags) ? model.tags.slice(0, 4) : [];
    visibleTags.forEach((tag) => {
      const chip = document.createElement("span");
      chip.textContent = tag;
      tagsContainer.appendChild(chip);
    });

    const dateChip = document.createElement("span");
    dateChip.textContent = `updated ${formatDate(model.last_modified)}`;
    tagsContainer.appendChild(dateChip);

    const downloadBtn = node.querySelector(".remote-download-btn");
    downloadBtn.addEventListener("click", async () => {
      downloadBtn.disabled = true;
      downloadBtn.textContent = "Iniciando...";

      try {
        const payload = {
          source: model.source,
          model_id: model.model_id,
          allow_patterns: [],
        };

        const created = await fetchJson("/catalog/downloads", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(payload),
        });

        setStatus("download iniciado", "running");
        await loadDownloads(true);
        addSystemMessage(`Download iniciado para ${created.model_id}. Pasta: ${created.destination}`);
      } catch (error) {
        setStatus("erro download", "error");
        addSystemMessage(`Falha ao iniciar download: ${error.message}`);
      } finally {
        downloadBtn.disabled = false;
        downloadBtn.textContent = "Baixar";
      }
    });

    remoteResults.appendChild(node);
  });
}

async function searchCatalogModels() {
  const source = catalogSource.value || "huggingface";
  const query = catalogQuery.value.trim();

  setStatus("buscando catalogo", "running");
  catalogMeta.textContent = "Consultando catalogo remoto...";

  try {
    const queryString = new URLSearchParams({
      source,
      limit: "16",
    });

    if (query) {
      queryString.set("query", query);
    }

    const models = await fetchJson(`/catalog/models?${queryString.toString()}`);
    renderCatalogModels(models);
    setStatus("catalogo pronto");
  } catch (error) {
    catalogMeta.textContent = `Erro ao buscar catalogo: ${error.message}`;
    setStatus("erro catalogo", "error");
  }
}

async function cancelDownload(jobId) {
  try {
    await fetchJson(`/catalog/downloads/${encodeURIComponent(jobId)}/cancel`, {
      method: "POST",
    });
    setStatus("cancelamento solicitado", "running");
    await loadDownloads();
  } catch (error) {
    setStatus("erro cancelamento", "error");
    addSystemMessage(`Falha ao cancelar download: ${error.message}`);
  }
}

function renderDownloads(jobs) {
  downloadList.innerHTML = "";

  if (!jobs.length) {
    const empty = document.createElement("li");
    empty.textContent = "Nenhum download registrado.";
    downloadList.appendChild(empty);
    return;
  }

  jobs.forEach((job) => {
    const node = downloadItemTemplate.content.firstElementChild.cloneNode(true);

    node.querySelector(".download-model").textContent = `${job.model_id}`;
    node.querySelector(".download-destination").textContent = job.current_file
      ? `${job.destination} | arquivo: ${job.current_file}`
      : job.destination;

    const progress = Math.max(0, Math.min(100, Number(job.progress_percent || 0)));
    node.querySelector(".download-progress-fill").style.width = `${progress.toFixed(1)}%`;

    const progressLabel = node.querySelector(".download-progress-label");
    if (job.status === "queued") {
      progressLabel.textContent = "Na fila para iniciar download...";
    } else {
      progressLabel.textContent = `${progress.toFixed(1)}% • ${formatBytes(job.bytes_downloaded)} / ${formatBytes(job.bytes_total)} • ${job.completed_files || 0}/${job.total_files || 0} arquivos`;
    }

    const status = node.querySelector(".download-status");
    status.textContent = job.status;
    status.classList.add(job.status);

    const when = job.finished_at || job.started_at || job.created_at;
    node.querySelector(".download-time").textContent = formatEpoch(when);

    const cancelBtn = node.querySelector(".download-cancel-btn");
    if (job.can_cancel) {
      cancelBtn.disabled = job.status === "cancelling";
      cancelBtn.textContent = job.status === "cancelling" ? "Cancelando..." : "Cancelar";
      cancelBtn.addEventListener("click", () => {
        void cancelDownload(job.id);
      });
    } else {
      cancelBtn.remove();
    }

    if (job.error) {
      const errorInfo = document.createElement("p");
      errorInfo.className = "download-progress-label";
      errorInfo.textContent = `Erro: ${job.error}`;
      node.querySelector(".download-main").appendChild(errorInfo);
    }

    downloadList.appendChild(node);
  });
}

async function loadDownloads(forceRefreshModels = false) {
  try {
    const jobs = await fetchJson("/catalog/downloads");
    renderDownloads(jobs);

    const fingerprint = jobs.map((job) => `${job.id}:${job.status}:${job.finished_at || ""}`).join("|");
    const completedChanged = fingerprint !== lastDownloadsFingerprint;
    lastDownloadsFingerprint = fingerprint;

    const hasRunning = jobs.some((job) => ["running", "queued", "cancelling"].includes(job.status));
    if (hasRunning) {
      setStatus("download em andamento", "running");
    }

    if (
      (forceRefreshModels || completedChanged) &&
      jobs.some((job) => ["completed", "cancelled"].includes(job.status))
    ) {
      await loadModels();
    }
  } catch {
    setStatus("erro downloads", "error");
  }
}

function clearChipList(container, fallbackText) {
  container.innerHTML = "";
  const item = document.createElement("li");
  item.className = "chip-empty";
  item.textContent = fallbackText;
  container.appendChild(item);
}

function renderChipList(container, values, fallbackText) {
  container.innerHTML = "";
  if (!Array.isArray(values) || values.length === 0) {
    clearChipList(container, fallbackText);
    return;
  }

  values.forEach((value) => {
    const item = document.createElement("li");
    item.className = "chip-item";
    item.textContent = value;
    container.appendChild(item);
  });
}

function renderOpenClawUsage(usage) {
  if (!usage) {
    openclawUsage.textContent = "-";
    return;
  }

  const parts = [];
  if (usage.input != null) {
    parts.push(`input ${usage.input}`);
  }
  if (usage.output != null) {
    parts.push(`output ${usage.output}`);
  }
  if (usage.total != null) {
    parts.push(`total ${usage.total}`);
  }
  if (usage.cache_read != null) {
    parts.push(`cache read ${usage.cache_read}`);
  }
  if (usage.cache_write != null) {
    parts.push(`cache write ${usage.cache_write}`);
  }

  openclawUsage.textContent = parts.length ? parts.join(" • ") : "-";
}

function normalizeOpenClawObservability(response = {}) {
  const skills = Array.isArray(response.skills)
    ? response.skills
      .map((value) => String(value || "").trim())
      .filter(Boolean)
    : [];
  const tools = Array.isArray(response.tools)
    ? response.tools
      .map((value) => String(value || "").trim())
      .filter(Boolean)
    : [];

  return {
    provider: typeof response.provider === "string" ? response.provider.trim() : "",
    model: typeof response.model === "string" ? response.model.trim() : "",
    usage: response.usage && typeof response.usage === "object" ? response.usage : null,
    skills,
    tools,
    updated_at: Number.isFinite(Number(response.updated_at))
      ? Number(response.updated_at)
      : null,
  };
}

function persistOpenClawObservability(snapshot) {
  localStorage.setItem(observabilityStorageKey(), JSON.stringify(snapshot));
}

function applyOpenClawObservability(snapshot, { persist = true } = {}) {
  const normalized = normalizeOpenClawObservability(snapshot);
  const provider = normalized.provider || "provider n/d";
  const model = normalized.model || "model n/d";

  openclawProviderModel.textContent = `${provider} • ${model}`;
  renderOpenClawUsage(normalized.usage);
  renderChipList(openclawSkills, normalized.skills, "Nenhuma skill reportada");
  renderChipList(openclawTools, normalized.tools, "Nenhuma tool reportada");

  if (persist) {
    persistOpenClawObservability(normalized);
  }
}

function restoreOpenClawObservabilityFromStorage() {
  try {
    let raw = localStorage.getItem(observabilityStorageKey());
    if (!raw && !isNanobotActive()) {
      raw = localStorage.getItem("mlxPilotOpenClawObservabilityV1");
    }
    if (!raw) {
      return false;
    }

    const parsed = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object") {
      return false;
    }

    applyOpenClawObservability(parsed, { persist: false });
    return true;
  } catch {
    return false;
  }
}

function updateOpenClawObservability(response) {
  applyOpenClawObservability(response);
}

async function loadOpenClawObservability() {
  try {
    const payload = await fetchJson(activeAgentEndpoint("observability"));
    applyOpenClawObservability(payload);
    openclawObservabilityLoaded = true;
  } catch (error) {
    if (!openclawObservabilityLoaded) {
      const restored = restoreOpenClawObservabilityFromStorage();
      if (!restored) {
        clearChipList(openclawSkills, "Nenhuma skill reportada");
        clearChipList(openclawTools, "Nenhuma tool reportada");
      }
    }
  }
}

function setOpenClawRuntimeButtons(serviceStatus = "") {
  const normalized = String(serviceStatus || "").toLowerCase();
  const running = normalized === "running" || normalized === "active";

  openclawStartBtn.disabled = openclawRuntimeActionInFlight || running;
  openclawStopBtn.disabled = openclawRuntimeActionInFlight || !running;
  openclawRestartBtn.disabled = openclawRuntimeActionInFlight || !running;
}

function renderOpenClawRuntimeState(runtime) {
  if (!runtime || typeof runtime !== "object") {
    openclawRuntimeMeta.textContent = "runtime: n/d";
    setOpenClawRuntimeButtons("");
    return;
  }

  const status = runtime.service_status || "unknown";
  const state = runtime.service_state || "unknown";
  const pid = runtime.pid != null ? `pid ${runtime.pid}` : "sem pid";
  const rpc = runtime.rpc_ok ? "rpc ok" : "rpc indisponivel";
  const issues = Array.isArray(runtime.issues) ? runtime.issues.filter(Boolean) : [];

  let text = `runtime: ${status}/${state} • ${pid} • ${rpc}`;
  if (issues.length) {
    text += ` • ${issues[0]}`;
  }

  openclawRuntimeMeta.textContent = text;
  setOpenClawRuntimeButtons(status);
}

async function loadOpenClawRuntimeStatus() {
  try {
    const runtime = await fetchJson(activeAgentEndpoint("runtime"));
    renderOpenClawRuntimeState(runtime);
  } catch (error) {
    openclawRuntimeMeta.textContent = `runtime indisponivel • ${error.message}`;
    setOpenClawRuntimeButtons("");
  }
}

async function runOpenClawRuntimeAction(action) {
  if (openclawRuntimeActionInFlight) {
    return;
  }

  const agentLabel = activeAgentLabel().toLowerCase();
  openclawRuntimeActionInFlight = true;
  setOpenClawRuntimeButtons("");
  setStatus(`${agentLabel} ${action}`, "running");

  try {
    const payload = await fetchJson(activeAgentEndpoint("runtime"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action }),
    });

    renderOpenClawRuntimeState(payload.runtime);
    await loadOpenClawStatus();
    await loadOpenClawObservability();
    setStatus(`${agentLabel} ${action} ok`);
  } catch (error) {
    setStatus(`erro ${agentLabel} ${action}`, "error");
    openclawRuntimeMeta.textContent = `falha ${action} • ${error.message}`;
  } finally {
    openclawRuntimeActionInFlight = false;
    await loadOpenClawRuntimeStatus();
  }
}

function addOpenClawChatMessage(role, content, meta = "") {
  const node = document.createElement("article");
  node.className = "message-card";
  const roleClass = roleClassFromValue(role);
  node.classList.add(`role-${roleClass}`);

  const roleNode = document.createElement("header");
  roleNode.className = "message-role";
  roleNode.textContent = role;

  const contentNode = document.createElement("div");
  contentNode.className = "message-content";
  if (roleClass === "assistant") {
    contentNode.classList.add("markdown-view");
    renderMarkdownInto(contentNode, content);
  } else {
    contentNode.textContent = content;
  }

  node.appendChild(roleNode);
  node.appendChild(contentNode);

  if (meta) {
    const metaNode = document.createElement("p");
    metaNode.className = "openclaw-message-meta";
    metaNode.textContent = meta;
    node.appendChild(metaNode);
  }

  openclawChatLog.appendChild(node);
  openclawChatLog.scrollTop = openclawChatLog.scrollHeight;
}

function createOpenClawAssistantStreamCard() {
  const node = assistantStreamTemplate.content.firstElementChild.cloneNode(true);
  node.classList.add("role-assistant");

  const ui = {
    node,
    stateLabel: node.querySelector(".assistant-state-label"),
    typingIndicator: node.querySelector(".typing-indicator"),
    thinkingSection: node.querySelector(".assistant-thinking"),
    thinkingText: node.querySelector(".assistant-thinking-text"),
    answerSection: node.querySelector(".assistant-answer"),
    answerText: node.querySelector(".assistant-answer-text"),
    metricsSection: node.querySelector(".assistant-metrics"),
    metricsText: node.querySelector(".assistant-metrics-text"),
    finalAnswer: "",
    answerRaw: "",
    thinkingQueue: "",
    answerQueue: "",
    flushTimer: null,
  };

  openclawChatLog.appendChild(node);
  openclawChatLog.scrollTop = openclawChatLog.scrollHeight;
  return ui;
}

function setOpenClawAssistantState(ui, status) {
  const labels = {
    waiting: "aguardando modelo",
    thinking: "thinking",
    answering: "respondendo",
    completed: "finalizado",
    error: "erro",
  };
  ui.stateLabel.textContent = labels[status] || status;
  ui.typingIndicator.classList.toggle("hidden", status !== "waiting");
  if (status === "thinking") {
    ui.thinkingSection.classList.remove("hidden");
  }
  if (status === "answering" || status === "completed") {
    ui.answerSection.classList.remove("hidden");
  }
}

function flushOpenClawAssistantQueues(ui) {
  let changed = false;

  if (ui.thinkingQueue.length) {
    const chunk = ui.thinkingQueue.slice(0, STREAM_CHARS_PER_TICK);
    ui.thinkingQueue = ui.thinkingQueue.slice(chunk.length);
    ui.thinkingSection.classList.remove("hidden");
    ui.thinkingText.textContent += chunk;
    changed = true;
  }

  if (ui.answerQueue.length) {
    const chunk = ui.answerQueue.slice(0, STREAM_CHARS_PER_TICK);
    ui.answerQueue = ui.answerQueue.slice(chunk.length);
    ui.answerSection.classList.remove("hidden");
    ui.answerRaw += chunk;
    renderMarkdownInto(ui.answerText, ui.answerRaw);
    ui.finalAnswer = ui.answerRaw;
    changed = true;
  }

  if (changed) {
    openclawChatLog.scrollTop = openclawChatLog.scrollHeight;
  }

  if (!ui.thinkingQueue.length && !ui.answerQueue.length && ui.flushTimer !== null) {
    window.clearInterval(ui.flushTimer);
    ui.flushTimer = null;
  }
}

function scheduleOpenClawAssistantFlush(ui) {
  if (ui.flushTimer !== null) {
    return;
  }
  ui.flushTimer = window.setInterval(() => {
    flushOpenClawAssistantQueues(ui);
  }, STREAM_TICK_MS);
}

async function waitForOpenClawAssistantFlush(ui) {
  if (!ui.thinkingQueue.length && !ui.answerQueue.length && ui.flushTimer === null) {
    return;
  }
  await new Promise((resolve) => {
    const pollTimer = window.setInterval(() => {
      if (!ui.thinkingQueue.length && !ui.answerQueue.length && ui.flushTimer === null) {
        window.clearInterval(pollTimer);
        resolve();
      }
    }, STREAM_TICK_MS);
  });
}

function appendOpenClawThinking(ui, delta) {
  if (!delta) {
    return;
  }
  ui.thinkingQueue += delta;
  scheduleOpenClawAssistantFlush(ui);
}

function appendOpenClawAnswer(ui, delta) {
  if (!delta) {
    return;
  }
  ui.answerQueue += delta;
  scheduleOpenClawAssistantFlush(ui);
}

function sanitizeAgentReplyText(rawReply) {
  const text = String(rawReply || "").replace(/\r/g, "");
  const filtered = text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .filter((line) => !/^={4,}$/.test(line))
    .filter((line) => !/^Prompt:\s/i.test(line))
    .filter((line) => !/^Generation:\s/i.test(line))
    .filter((line) => !/^Peak memory:\s/i.test(line))
    .filter((line) => !/^completed\s*•/i.test(line))
    .filter((line) => !/tokens-per-sec/i.test(line));
  return filtered.join("\n").trim();
}

function normalizeAgentReplySplit(split) {
  const thinking = String(split?.thinking || "").trim();
  const answer = String(split?.answer || "").trim();
  if (!answer && thinking) {
    return { thinking: "", answer: thinking };
  }
  return { thinking, answer };
}

function renderOpenClawAssistantMeta(ui, response) {
  const lines = [];
  if (response?.status) {
    lines.push(`Status: ${response.status}`);
  }
  if (response?.summary) {
    lines.push(response.summary);
  }
  if (response?.duration_ms != null) {
    lines.push(`Latencia: ${response.duration_ms} ms`);
  }
  if (response?.run_id) {
    lines.push(`Run: ${response.run_id}`);
  }
  if (response?.provider || response?.model) {
    lines.push(`Modelo: ${response.provider || "-"} • ${response.model || "-"}`);
  }

  const usage = response?.usage || {};
  const promptTokens = usage.prompt ?? usage.prompt_tokens ?? usage.input;
  const completionTokens = usage.completion ?? usage.completion_tokens ?? usage.output;
  const totalTokens = usage.total ?? usage.total_tokens;
  const usageLine = [
    promptTokens != null ? `Prompt ${promptTokens}` : "",
    completionTokens != null ? `Generation ${completionTokens}` : "",
    totalTokens != null ? `Total ${totalTokens}` : "",
  ]
    .filter(Boolean)
    .join(" • ");
  if (usageLine) {
    lines.push(usageLine);
  }

  if (!lines.length) {
    return;
  }

  ui.metricsSection.classList.remove("hidden");
  ui.metricsText.textContent = lines.join("\n");
  openclawChatLog.scrollTop = openclawChatLog.scrollHeight;
}

function setOpenClawSendingState(nextState) {
  openclawChatInFlight = nextState;
  openclawSendBtn.disabled = nextState;
  openclawMessageInput.disabled = nextState;
}

async function loadOpenClawStatus() {
  try {
    const status = await fetchJson(activeAgentEndpoint("status"));
    openclawStatusLoaded = true;

    if (isNanobotActive()) {
      if (status.installed) {
        const configBadge = status.config_exists ? "config ok" : "sem config";
        openclawStatusText.textContent = `online • ${configBadge} • ${status.version || "versao n/d"}`;
      } else {
        openclawStatusText.textContent = status.message || "offline";
      }
      return;
    }

    if (status.available) {
      openclawStatusText.textContent = `online • session ${status.session_key}`;
      if (status.health?.ok || status.health?.result?.ok) {
        openclawStatusText.textContent += " • gateway ok";
      }
    } else {
      openclawStatusText.textContent = status.error ? `offline • ${status.error}` : "offline";
    }
  } catch (error) {
    openclawStatusText.textContent = `erro status • ${error.message}`;
  }
}

function resetOpenClawLogState() {
  openclawLogCursor = 0;
  openclawLogViewer.textContent = "";
  openclawLogMeta.textContent = "Aguardando logs...";
}

function appendOpenClawLog(content) {
  if (!content) {
    return;
  }

  openclawLogViewer.textContent += content;
  if (openclawLogViewer.textContent.length > OPENCLAW_LOG_MAX_CHARS) {
    openclawLogViewer.textContent = openclawLogViewer.textContent.slice(-OPENCLAW_LOG_MAX_CHARS);
  }
  openclawLogViewer.scrollTop = openclawLogViewer.scrollHeight;
}

async function pollOpenClawLogs({ reset = false } = {}) {
  const selectedStream = openclawLogStreamSelect.value || "gateway";
  if (reset || selectedStream !== openclawActiveLogStream) {
    openclawActiveLogStream = selectedStream;
    resetOpenClawLogState();
  }

  const params = new URLSearchParams({
    stream: openclawActiveLogStream,
    cursor: String(openclawLogCursor),
    max_bytes: "65536",
  });

  try {
    const chunk = await fetchJson(`${activeAgentEndpoint("logs")}?${params.toString()}`);

    if (!chunk.exists) {
      openclawLogMeta.textContent = `arquivo nao encontrado: ${chunk.path}`;
      return;
    }

    if (chunk.truncated && openclawLogCursor > 0) {
      appendOpenClawLog("\n[log rotacionado, cursor reiniciado]\n");
    }

    openclawLogCursor = chunk.next_cursor || openclawLogCursor;
    appendOpenClawLog(chunk.content || "");
    openclawLogMeta.textContent = `stream ${chunk.stream} • ${formatBytes(chunk.file_size)} • cursor ${openclawLogCursor}`;
  } catch (error) {
    openclawLogMeta.textContent = `erro ao ler logs: ${error.message}`;
  }
}

function stopOpenClawLogPolling() {
  if (!openclawLogsTimer) {
    return;
  }

  window.clearInterval(openclawLogsTimer);
  openclawLogsTimer = null;
}

function startOpenClawLogPolling() {
  stopOpenClawLogPolling();
  void pollOpenClawLogs({ reset: true });
  openclawLogsTimer = window.setInterval(() => {
    void pollOpenClawLogs();
  }, OPENCLAW_LOG_POLL_MS);
}

async function sendOpenClawMessage() {
  if (openclawChatInFlight) {
    return;
  }

  const message = openclawMessageInput.value.trim();
  if (!message) {
    return;
  }

  openclawMessageInput.value = "";
  addOpenClawChatMessage("user", message);
  setOpenClawSendingState(true);
  setStatus(`consultando ${activeAgentLabel().toLowerCase()}`, "running");

  try {
    const response = await fetchJson(activeAgentEndpoint("chat"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message }),
    });

    const streamUi = createOpenClawAssistantStreamCard();
    setOpenClawAssistantState(streamUi, "waiting");
    const sanitizedReply = sanitizeAgentReplyText(response.reply || "");
    const split = normalizeAgentReplySplit(splitThinkingAndAnswer(sanitizedReply));

    if (split.thinking) {
      setOpenClawAssistantState(streamUi, "thinking");
      appendOpenClawThinking(streamUi, split.thinking);
    }

    if (split.answer) {
      setOpenClawAssistantState(streamUi, "answering");
      appendOpenClawAnswer(streamUi, split.answer);
    }

    if (!split.thinking && !split.answer) {
      setOpenClawAssistantState(streamUi, "answering");
      appendOpenClawAnswer(streamUi, "(sem resposta textual)");
    }

    await waitForOpenClawAssistantFlush(streamUi);
    setOpenClawAssistantState(streamUi, "completed");
    renderOpenClawAssistantMeta(streamUi, response);
    updateOpenClawObservability(response);
    setStatus(`${activeAgentLabel().toLowerCase()} respondeu`);
  } catch (error) {
    addOpenClawChatMessage("system", `Erro no ${activeAgentLabel()}: ${error.message}`);
    setStatus(`erro ${activeAgentLabel().toLowerCase()}`, "error");
  } finally {
    setOpenClawSendingState(false);
    openclawMessageInput.focus();
  }
}

function renderOpenClawModelSelectors() {
  const cloud = Array.isArray(openclawModelsCatalog.cloud_models)
    ? openclawModelsCatalog.cloud_models
    : [];

  const locals = Array.isArray(openclawModelsCatalog.local_models)
    ? openclawModelsCatalog.local_models
    : [];

  openclawCloudModelSelect.innerHTML = "";
  if (!cloud.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "Nenhum modelo cloud configurado";
    openclawCloudModelSelect.appendChild(option);
    openclawCloudModelSelect.disabled = true;
  } else {
    openclawCloudModelSelect.disabled = false;
    cloud.forEach((entry) => {
      const option = document.createElement("option");
      option.value = entry.reference;
      option.textContent = entry.label || entry.reference;
      openclawCloudModelSelect.appendChild(option);
    });
  }

  openclawLocalModelSelect.innerHTML = "";
  if (!locals.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "Nenhum modelo local disponivel";
    openclawLocalModelSelect.appendChild(option);
    openclawLocalModelSelect.disabled = true;
  } else {
    openclawLocalModelSelect.disabled = false;
    locals.forEach((entry) => {
      const option = document.createElement("option");
      option.value = entry.id;
      option.textContent = entry.name;
      openclawLocalModelSelect.appendChild(option);
    });
  }

  const current = openclawModelsCatalog.current;
  if (current) {
    openclawModelCurrent.textContent = `Modelo atual: ${current.label} (${current.source})`;

    if (current.source === "cloud") {
      openclawModelSource.value = "cloud";
      if (cloud.some((entry) => entry.reference === current.reference)) {
        openclawCloudModelSelect.value = current.reference;
      }
    }

    if (current.source === "local") {
      openclawModelSource.value = "local";
      const match = locals.find((entry) => current.model === entry.path || current.reference === `openai/${entry.path}`);
      if (match) {
        openclawLocalModelSelect.value = match.id;
      }
    }
  } else {
    openclawModelCurrent.textContent = "Modelo atual: -";
  }

  toggleOpenClawSourceFields();
}

function toggleOpenClawSourceFields() {
  if (openclawModelSource?.parentElement) {
    openclawModelSource.parentElement.classList.remove("hidden");
  }
  if (nanobotModelPicker) {
    nanobotModelPicker.classList.add("hidden");
  }

  const source = openclawModelSource.value || "cloud";
  openclawCloudPicker.classList.toggle("hidden", source !== "cloud");
  openclawLocalPicker.classList.toggle("hidden", source !== "local");
}

async function loadOpenClawModelCatalog() {
  try {
    const payload = await fetchJson(activeAgentEndpoint("models"));
    openclawModelsCatalog = {
      cloud_models: Array.isArray(payload.cloud_models) ? payload.cloud_models : [],
      local_models: Array.isArray(payload.local_models) ? payload.local_models : [],
      current: payload.current || null,
    };

    renderOpenClawModelSelectors();
    openclawConfigFeedback.textContent = isNanobotActive()
      ? "Modelos do NanoBot carregados."
      : "Modelos carregados.";
  } catch (error) {
    if (isNanobotActive()) {
      try {
        const current = await fetchJson(activeAgentEndpoint("model"));
        openclawModelsCatalog = {
          cloud_models: [],
          local_models: [],
          current: current || null,
        };
        renderOpenClawModelSelectors();
        openclawConfigFeedback.textContent = current?.model
          ? "Modelo NanoBot carregado (fallback)."
          : "Nenhum modelo NanoBot definido.";
        return;
      } catch {
        // fallback falhou, manter erro original
      }
    }
    openclawConfigFeedback.textContent = `Erro ao carregar modelos: ${error.message}`;
  }
}

async function applyOpenClawModelSelection() {
  const source = openclawModelSource.value || "cloud";
  const frameworkLabel = isNanobotActive() ? "nanobot" : "openclaw";
  openclawConfigFeedback.textContent = isNanobotActive()
    ? "Aplicando modelo NanoBot..."
    : "Aplicando modelo...";
  setStatus(`aplicando modelo ${frameworkLabel}`, "running");

  const payload = { source };

  if (source === "cloud") {
    const reference = (openclawCloudModelSelect.value || "").trim();
    if (!reference) {
      openclawConfigFeedback.textContent = "Escolha um modelo cloud.";
      return;
    }
    payload.model_reference = reference;
  }

  if (source === "local") {
    const localId = (openclawLocalModelSelect.value || "").trim();
    if (!localId) {
      openclawConfigFeedback.textContent = "Escolha um modelo local.";
      return;
    }
    payload.local_model_id = localId;
  }

  try {
    const current = await fetchJson(activeAgentEndpoint("model"), {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });

    openclawModelsCatalog.current = current;
    renderOpenClawModelSelectors();
    openclawConfigFeedback.textContent = `Modelo aplicado: ${current.label}`;
    setStatus(`modelo ${frameworkLabel} atualizado`);
  } catch (error) {
    openclawConfigFeedback.textContent = `Falha ao aplicar modelo: ${error.message}`;
    setStatus(`erro modelo ${frameworkLabel}`, "error");
  }
}

function renderOpenClawViews() {
  if (!openclawSelectedViews.size) {
    openclawSelectedViews = new Set(["chat"]);
  }

  const selected = [...openclawSelectedViews];
  if (!openclawMultiView && selected.length > 1) {
    openclawSelectedViews = new Set([selected[0]]);
  }

  openclawPanelsRoot.classList.toggle("single-mode", !openclawMultiView);
  openclawPanelsRoot.classList.toggle("multi-mode", openclawMultiView);

  const isActive = (name) => openclawSelectedViews.has(name);

  openclawPanelChat.classList.toggle("active", isActive("chat"));
  openclawPanelLogs.classList.toggle("active", isActive("logs"));
  openclawPanelObservability.classList.toggle("active", isActive("observability"));
  openclawPanelConfig.classList.toggle("active", isActive("config"));

  openclawViewButtons.forEach((button) => {
    const view = button.dataset.openclawView;
    button.classList.toggle("active", isActive(view));
  });
}

function toggleOpenClawView(view) {
  if (!view) {
    return;
  }

  if (!openclawMultiView) {
    openclawSelectedViews = new Set([view]);
    renderOpenClawViews();
    return;
  }

  if (openclawSelectedViews.has(view)) {
    if (openclawSelectedViews.size > 1) {
      openclawSelectedViews.delete(view);
    }
  } else {
    openclawSelectedViews.add(view);
  }

  renderOpenClawViews();
}

function onOpenClawTabSelected() {
  startOpenClawLogPolling();

  if (!openclawObservabilityLoaded) {
    restoreOpenClawObservabilityFromStorage();
  }

  void loadOpenClawStatus();
  void loadOpenClawRuntimeStatus();
  void loadOpenClawObservability();
  void loadOpenClawModelCatalog();
}

function switchTab(nextTab) {
  activeTab = nextTab;

  if (nextTab !== "chat") {
    setEditMode(null);
    closeThreadMenu();
    setChatModelMenuOpen(false);
  }

  tabButtons.forEach((button) => {
    const active = button.dataset.tab === nextTab;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", active ? "true" : "false");
  });

  panelChat.classList.toggle("active", nextTab === "chat");
  panelDiscover.classList.toggle("active", nextTab === "discover");
  panelOpenClaw.classList.toggle("active", nextTab === "openclaw");
  if (panelAgent) panelAgent.classList.toggle("active", nextTab === "agent");
  panelSettings.classList.toggle("active", nextTab === "settings");
  if (panelAiInteraction) panelAiInteraction.classList.toggle("active", nextTab === "ai-interaction");

  appShell.classList.toggle("chat-mode", nextTab === "chat");
  chatModelSwitcher.classList.toggle("hidden", nextTab !== "chat");
  if (newChatThreadTopBtn) {
    newChatThreadTopBtn.classList.toggle("hidden", nextTab !== "chat");
  }

  if (nextTab === "discover") {
    switchDiscoverSubtab(activeDiscoverSubtab);
    void searchCatalogModels();
    void loadDownloads();
  }

  if (window.particleSystem) {
    if (nextTab === "ai-interaction") {
      window.particleSystem.onWindowResize();
      if (!aiSceneAnimating) {
        window.particleSystem.setParticleState("neutral");
      }
      if (!aiSceneLastScript) {
        setAiSceneStatus("Pronto para montar uma cena.");
      }
    } else {
      stopAiScenePlayback({ keepTimeline: true });
      window.particleSystem.setParticleState("none");
    }
  }

  if (nextTab === "openclaw") {
    onOpenClawTabSelected();
  } else if (nextTab === "agent") {
    void onAgentTabSelected();
    startAuditPolling();
  } else {
    stopOpenClawLogPolling();
    stopAuditPolling();
  }
}

saveUrlBtn.addEventListener("click", () => {
  daemonBaseUrl = daemonInput.value.trim().replace(/\/$/, "");
  localStorage.setItem(STORAGE_DAEMON_URL, daemonBaseUrl);
  openclawStatusLoaded = false;
  openclawObservabilityLoaded = false;
  resetOpenClawLogState();
  openclawRuntimeMeta.textContent = "runtime: verificando...";
  setStatus("url salva");
  void bootstrap();
});

newChatThreadButtons.forEach((button) => {
  button.addEventListener("click", createNewChatThread);
});

refreshModelsBtn.addEventListener("click", () => {
  setChatModelMenuOpen(false);
  void loadModels();
});

if (chatModelTrigger) {
  chatModelTrigger.addEventListener("click", (event) => {
    event.preventDefault();
    if (activeTab !== "chat") {
      return;
    }
    setChatModelMenuOpen(!chatModelMenuOpen);
  });
}

chatModelSelect.addEventListener("change", () => {
  const modelId = chatModelSelect.value;
  if (!modelId) {
    renderChatModelPickerMenu();
    return;
  }

  selectedModelId = modelId;
  const active = getActiveThread();
  if (active) {
    active.modelId = modelId;
    active.updatedAt = Date.now();
    persistThreads();
    renderThreadList();
  }

  renderChatModelPickerMenu();
  renderSelectedThreadMeta();
});

chatForm.addEventListener("submit", async (event) => {
  event.preventDefault();

  if (isGenerating) {
    return;
  }

  if (!selectedModelId) {
    addSystemMessage("Selecione um modelo antes de enviar mensagem.");
    return;
  }

  const userText = messageInput.value.trim();
  if (!userText) {
    return;
  }

  const editIndex = pendingEditMessageIndex;
  messageInput.value = "";

  if (Number.isInteger(editIndex)) {
    const applied = applyEditedMessageAndTrim(editIndex, userText);
    setEditMode(null);

    if (!applied) {
      addSystemMessage("Nao foi possivel localizar a mensagem para edicao.");
      return;
    }

    setStatus("regenerando", "running");
    await runAssistantGeneration();
    return;
  }

  appendMessageToActiveThread("user", userText);

  const active = getActiveThread();
  const index = active ? active.messages.length - 1 : null;
  addMessageCard("user", userText, {
    editable: Number.isInteger(index),
    messageIndex: index,
  });

  await runAssistantGeneration();
});

if (chatWebsearchBtn) {
  chatWebsearchBtn.addEventListener("click", () => {
    setWebsearchToggleState(!chatWebsearchToggle.checked);
  });
}

if (chatAirllmToggleBtn) {
  chatAirllmToggleBtn.addEventListener("click", () => {
    const nextState = !chatAirllmEnabled;
    setChatAirllmToggleState(nextState);
    void persistChatAirllmToggle(nextState);
  });
}

stopGenerationBtn.addEventListener("click", () => {
  if (!activeStreamController || !isGenerating) {
    return;
  }

  stopGenerationBtn.disabled = true;
  setStatus("interrompendo geracao", "running");
  activeStreamController.abort();
});

cancelEditBtn.addEventListener("click", () => {
  if (isGenerating) {
    return;
  }

  setEditMode(null);
  messageInput.value = "";
  messageInput.focus();
  setStatus("edicao cancelada");
});

catalogSearchBtn.addEventListener("click", () => {
  void searchCatalogModels();
});

catalogSource.addEventListener("change", () => {
  void searchCatalogModels();
});

catalogQuery.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    void searchCatalogModels();
  }
});

refreshDownloadsBtn.addEventListener("click", () => {
  void loadDownloads(true);
});

if (discoverSubtabButtons.length) {
  discoverSubtabButtons.forEach((button) => {
    button.addEventListener("click", () => {
      const nextSubtab = button.dataset.discoverSubtab || "catalog";
      switchDiscoverSubtab(nextSubtab);
      if (nextSubtab === "installed") {
        void loadModels();
      }
    });
  });
}

if (refreshInstalledModelsBtn) {
  refreshInstalledModelsBtn.addEventListener("click", () => {
    void loadModels();
  });
}

tabButtons.forEach((button) => {
  button.addEventListener("click", () => {
    switchTab(button.dataset.tab);
  });
});

if (mobileMenuBtn) {
  mobileMenuBtn.addEventListener("click", () => {
    appShell.classList.toggle("sidebar-open");
  });
}

function autoResizeTextarea(el) {
  el.style.height = "auto";
  el.style.height = el.scrollHeight + "px";
}

if (messageInput) {
  messageInput.addEventListener("input", () => autoResizeTextarea(messageInput));
}
if (openclawMessageInput) {
  openclawMessageInput.addEventListener("input", () => autoResizeTextarea(openclawMessageInput));
}
if (aiParticleInput) {
  aiParticleInput.addEventListener("input", () => {
    if (aiParticleInput.value.length > 320) {
      aiParticleInput.value = aiParticleInput.value.slice(0, 320);
    }
  });
  aiParticleInput.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void applyParticleTextFromInput();
    }
  });
  aiParticleInput.addEventListener("blur", () => {
    aiParticleInput.value = normalizeScenePrompt(aiParticleInput.value);
  });
}
if (aiParticleBtn) {
  aiParticleBtn.addEventListener("click", () => {
    void applyParticleTextFromInput();
    if (aiParticleInput) {
      aiParticleInput.focus();
    }
  });
}
if (aiParticleStopBtn) {
  aiParticleStopBtn.addEventListener("click", () => {
    stopAiScenePlayback({ keepTimeline: true });
    setAiSceneStatus("Cena interrompida.");
    setStatus("cena visual interrompida");
  });
}
if (aiSceneExampleButtons.length) {
  aiSceneExampleButtons.forEach((button) => {
    button.addEventListener("click", () => {
      const prompt = normalizeScenePrompt(button.dataset.prompt || "");
      if (!prompt || !aiParticleInput) {
        return;
      }
      aiParticleInput.value = prompt;
      void applyParticleTextFromInput();
      aiParticleInput.focus();
    });
  });
}

document.addEventListener("click", (event) => {
  const target = event.target;
  const inHistoryMenu =
    target instanceof Element && Boolean(target.closest(".history-item-actions"));
  if (openThreadMenuId && !inHistoryMenu) {
    closeThreadMenu({ rerender: true });
  }

  const inModelSwitcher =
    target instanceof Element && Boolean(target.closest("#chat-model-switcher"));
  if (chatModelMenuOpen && !inModelSwitcher) {
    setChatModelMenuOpen(false);
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") {
    return;
  }

  if (chatModelMenuOpen) {
    setChatModelMenuOpen(false);
  }

  if (openThreadMenuId) {
    closeThreadMenu({ rerender: true });
  }
});

refreshOpenclawStatusBtn.addEventListener("click", () => {
  void loadOpenClawStatus();
  void loadOpenClawRuntimeStatus();
  void loadOpenClawObservability();
});

openclawStartBtn.addEventListener("click", () => {
  void runOpenClawRuntimeAction("start");
});

openclawStopBtn.addEventListener("click", () => {
  void runOpenClawRuntimeAction("stop");
});

openclawRestartBtn.addEventListener("click", () => {
  void runOpenClawRuntimeAction("restart");
});

openclawViewButtons.forEach((button) => {
  button.addEventListener("click", () => {
    toggleOpenClawView(button.dataset.openclawView);
  });
});

openclawMultiViewToggle.addEventListener("change", () => {
  openclawMultiView = openclawMultiViewToggle.checked;
  if (!openclawMultiView) {
    const first = [...openclawSelectedViews][0] || "chat";
    openclawSelectedViews = new Set([first]);
  }
  renderOpenClawViews();
});

openclawChatForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  await sendOpenClawMessage();
});

openclawLogStreamSelect.addEventListener("change", () => {
  void pollOpenClawLogs({ reset: true });
});

refreshOpenclawLogBtn.addEventListener("click", () => {
  void pollOpenClawLogs({ reset: true });
});

clearOpenclawLogBtn.addEventListener("click", () => {
  resetOpenClawLogState();
});

openclawModelSource.addEventListener("change", () => {
  toggleOpenClawSourceFields();
});

refreshOpenclawModelsBtn.addEventListener("click", () => {
  void loadOpenClawModelCatalog();
});

applyOpenclawModelBtn.addEventListener("click", () => {
  void applyOpenClawModelSelection();
});

saveSettingsBtn.addEventListener("click", async () => {
  try {
    const btn = saveSettingsBtn;
    const oldText = btn.textContent;
    btn.textContent = "Salvando...";
    btn.disabled = true;

    const payload = await fetchJson("/config", { method: "GET" });
    payload.models_dir = settingModelsDir.value;
    payload.openclaw_cli_path = settingOpenclawCli.value;
    payload.openclaw_state_dir = settingOpenclawState.value;
    payload.nanobot_cli_path = settingNanobotCli.value;

    const checkedFramework = document.querySelector('input[name="agent-framework"]:checked');
    if (checkedFramework) {
      payload.active_agent_framework = checkedFramework.value;
    }

    await fetchJson("/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload)
    });

    await loadOpenclawInstallStatus({ showLoading: false });
    if (isNanobotActive()) {
      await loadNanobotStatus();
    }

    btn.textContent = "Salvo!";
    setTimeout(() => {
      btn.textContent = oldText;
      btn.disabled = false;
    }, 2000);
  } catch (error) {
    alert(`Erro ao salvar configuracoes: ${error.message}`);
    saveSettingsBtn.textContent = "Salvar Configuracoes";
    saveSettingsBtn.disabled = false;
  }
});

async function loadConfig() {
  try {
    const cfg = await fetchJson("/config", { method: "GET" });
    if (settingModelsDir) settingModelsDir.value = cfg.models_dir || "";
    if (discoverModelsDir) discoverModelsDir.value = cfg.models_dir || "";
    if (settingOpenclawCli) settingOpenclawCli.value = cfg.openclaw_cli_path || "";
    if (settingOpenclawState) settingOpenclawState.value = cfg.openclaw_state_dir || "";
    if (settingNanobotCli) settingNanobotCli.value = cfg.nanobot_cli_path || "";
    if (cfg?.mlx_airllm_enabled != null) {
      setChatAirllmToggleState(Boolean(cfg.mlx_airllm_enabled));
    }

    const activeFramework = cfg.active_agent_framework === "nanobot" ? "nanobot" : "openclaw";
    applyAgentFramework(activeFramework, { syncRadio: true, refreshPanel: false });
    await loadOpenclawInstallStatus({ showLoading: false });
    await loadOpenclawEnvironment({ showLoading: false });

  } catch (err) {
    console.error("Failed to load config from backend", err);
  }
}

function yesNo(value) {
  return value ? "sim" : "nao";
}

function setOpenclawInstallButtonState({ installed = false, inFlight = false } = {}) {
  if (!installOpenclawBtn) {
    return;
  }

  if (inFlight) {
    installOpenclawBtn.disabled = true;
    installOpenclawBtn.textContent = "Instalando... isso pode demorar um pouco.";
    return;
  }

  if (installed) {
    installOpenclawBtn.disabled = true;
    installOpenclawBtn.textContent = "Instalado";
    return;
  }

  installOpenclawBtn.disabled = false;
  installOpenclawBtn.textContent = "Instalar OpenClaw agora";
}

function renderOpenclawInstallStatus({ status = null, runtime = null, runtimeError = null } = {}) {
  if (!openclawInstallStatusOutput) {
    return;
  }

  if (!status) {
    openclawInstallStatusOutput.textContent = "-";
    return;
  }

  const healthOk = status.health?.ok ?? status.health?.result?.ok;
  const lines = [
    `CLI configurada: ${status.cli_path || "-"}`,
    `CLI encontrada: ${yesNo(status.cli_exists)}`,
    `State dir: ${status.state_dir || "-"}`,
    `State dir existe: ${yesNo(status.state_dir_exists)}`,
    `Disponivel: ${yesNo(status.available)}`,
    `Health RPC: ${typeof healthOk === "boolean" ? (healthOk ? "ok" : "falha") : "-"}`,
    `Sessao: ${status.session_key || "-"}`,
    `Gateway log: ${status.gateway_log || "-"}`,
    `Error log: ${status.error_log || "-"}`,
    `Sync log: ${status.sync_log || "-"}`,
  ];

  if (status.error) {
    lines.push(`Erro status: ${status.error}`);
  }

  if (runtime) {
    lines.push(
      "",
      "Runtime gateway:",
      `Service status: ${runtime.service_status || "-"}`,
      `Service state: ${runtime.service_state || "-"}`,
      `PID: ${runtime.pid || "-"}`,
      `RPC ok: ${yesNo(runtime.rpc_ok)}`,
      `Porta: ${runtime.port_status || "-"}`,
      `Issues: ${Array.isArray(runtime.issues) && runtime.issues.length ? runtime.issues.join(" | ") : "-"}`
    );
  } else if (runtimeError) {
    lines.push("", `Runtime: indisponivel (${runtimeError})`);
  }

  openclawInstallStatusOutput.textContent = lines.join("\n");
}

async function loadOpenclawInstallStatus({ showLoading = true, syncInstallFeedback = true } = {}) {
  if (openclawStatusCheckInFlight) {
    return null;
  }

  openclawStatusCheckInFlight = true;
  if (checkOpenclawStatusBtn) {
    checkOpenclawStatusBtn.disabled = true;
  }

  if (showLoading && openclawInstallStatusFeedback) {
    openclawInstallStatusFeedback.textContent = "Checando status do OpenClaw...";
  }

  try {
    const status = await fetchJson("/openclaw/status", { method: "GET" });
    const installed = Boolean(status.cli_exists);
    setOpenclawInstallButtonState({ installed, inFlight: openclawInstallInFlight });

    let runtime = null;
    let runtimeError = null;
    try {
      runtime = await fetchJson("/openclaw/runtime", { method: "GET" });
    } catch (error) {
      runtimeError = error.message;
    }

    renderOpenclawInstallStatus({ status, runtime, runtimeError });

    if (openclawInstallStatusFeedback) {
      if (installed) {
        const suffix = status.available
          ? "Gateway respondeu ao healthcheck."
          : status.error
            ? `Gateway indisponivel: ${status.error}`
            : "Gateway indisponivel.";
        openclawInstallStatusFeedback.textContent = `OpenClaw detectado. ${suffix}`;
      } else {
        openclawInstallStatusFeedback.textContent =
          "OpenClaw nao detectado no caminho configurado. Ajuste o caminho ou rode a instalacao.";
      }
    }

    if (syncInstallFeedback && installOpenclawFeedback && !openclawInstallInFlight) {
      installOpenclawFeedback.textContent = installed
        ? "OpenClaw ja instalado no caminho configurado."
        : "OpenClaw nao detectado. Execute a instalacao automatizada.";
    }

    return status;
  } catch (error) {
    setOpenclawInstallButtonState({ installed: false, inFlight: openclawInstallInFlight });
    if (openclawInstallStatusFeedback) {
      openclawInstallStatusFeedback.textContent = `Falha ao consultar status: ${error.message}`;
    }
    if (openclawInstallStatusOutput) {
      openclawInstallStatusOutput.textContent = "-";
    }
    if (syncInstallFeedback && installOpenclawFeedback && !openclawInstallInFlight) {
      installOpenclawFeedback.textContent = `Falha ao validar instalacao: ${error.message}`;
    }
    return null;
  } finally {
    openclawStatusCheckInFlight = false;
    if (checkOpenclawStatusBtn) {
      checkOpenclawStatusBtn.disabled = openclawInstallInFlight;
    }
  }
}

function formatOpenclawEnvironmentSource(source) {
  if (source === "env_file") {
    return "arquivo";
  }
  if (source === "process_env") {
    return "processo";
  }
  if (source === "env_example") {
    return "exemplo";
  }
  return "catalogo";
}

function setOpenclawEnvironmentButtonsDisabled(disabled) {
  if (refreshOpenclawEnvBtn) {
    refreshOpenclawEnvBtn.disabled = disabled;
  }
  if (saveOpenclawEnvBtn) {
    saveOpenclawEnvBtn.disabled = disabled;
  }
}

function setOpenclawEnvironmentInputVisibility() {
  if (!openclawEnvList) {
    return;
  }
  const reveal = revealOpenclawEnvToggle?.checked ?? true;
  openclawEnvList.querySelectorAll("input[data-env-key]").forEach((input) => {
    input.type = reveal ? "text" : "password";
  });
}

function renderOpenclawEnvironment(payload) {
  if (!openclawEnvList) {
    return;
  }

  const variables = Array.isArray(payload?.variables) ? payload.variables : [];
  if (openclawEnvPath) {
    const path = payload?.env_path || "-";
    const envBadge = payload?.env_exists ? "ok" : "faltando";
    const examplePath = payload?.env_example_path || "-";
    const exampleBadge = payload?.env_example_exists ? "ok" : "faltando";
    openclawEnvPath.textContent = `env: ${path} (${envBadge}) • exemplo: ${examplePath} (${exampleBadge})`;
  }

  openclawEnvList.innerHTML = "";
  if (!variables.length) {
    const empty = document.createElement("p");
    empty.className = "meta-note";
    empty.textContent = "Nenhuma variavel de API encontrada.";
    openclawEnvList.appendChild(empty);
    return;
  }

  variables.forEach((entry) => {
    const item = document.createElement("article");
    item.className = "openclaw-env-item";

    const head = document.createElement("div");
    head.className = "openclaw-env-head";

    const keyNode = document.createElement("p");
    keyNode.className = "openclaw-env-key";
    keyNode.textContent = entry.key || "-";

    const sourceNode = document.createElement("span");
    sourceNode.className = "openclaw-env-source";
    sourceNode.textContent = formatOpenclawEnvironmentSource(entry.source);

    head.appendChild(keyNode);
    head.appendChild(sourceNode);
    item.appendChild(head);

    const input = document.createElement("input");
    input.className = "input";
    input.dataset.envKey = entry.key || "";
    input.value = entry.value || "";
    input.placeholder = entry.masked && entry.masked !== "-" ? entry.masked : "";
    input.type = (revealOpenclawEnvToggle?.checked ?? true) ? "text" : "password";
    input.autocomplete = "off";
    item.appendChild(input);

    const meta = document.createElement("p");
    meta.className = "openclaw-env-meta";
    const status = entry.present ? "configurado" : "vazio";
    meta.textContent = `${entry.label || entry.key || "-"} • ${status}`;
    item.appendChild(meta);

    openclawEnvList.appendChild(item);
  });
}

function isNotFoundError(error) {
  return Number(error?.status) === 404;
}

async function fetchEnvironmentWithFallback({ method = "GET", reveal = false, values = null } = {}) {
  const endpoints = resolvedEnvironmentEndpoint
    ? [resolvedEnvironmentEndpoint, ...ENVIRONMENT_ENDPOINT_CANDIDATES.filter((item) => item !== resolvedEnvironmentEndpoint)]
    : [...ENVIRONMENT_ENDPOINT_CANDIDATES];
  let lastError = null;

  for (const endpoint of endpoints) {
    const query = method === "GET" ? `?reveal=${reveal ? "true" : "false"}` : "";
    try {
      const payload = await fetchJson(`${endpoint}${query}`, {
        method,
        headers: method === "POST" ? { "Content-Type": "application/json" } : undefined,
        body: method === "POST" ? JSON.stringify({ values: values || {} }) : undefined,
      });
      resolvedEnvironmentEndpoint = endpoint;
      return payload;
    } catch (error) {
      lastError = error;
      if (!isNotFoundError(error)) {
        throw error;
      }
    }
  }

  throw lastError || new Error("endpoint de environment indisponivel");
}

async function loadOpenclawEnvironment({ showLoading = true } = {}) {
  if (!openclawEnvList || openclawEnvironmentInFlight) {
    return null;
  }

  openclawEnvironmentInFlight = true;
  setOpenclawEnvironmentButtonsDisabled(true);

  if (showLoading && openclawEnvFeedback) {
    openclawEnvFeedback.textContent = "Carregando environment global...";
  }

  try {
    const payload = await fetchEnvironmentWithFallback({
      method: "GET",
      reveal: true,
    });
    renderOpenclawEnvironment(payload);
    if (openclawEnvFeedback) {
      const count = Array.isArray(payload?.variables) ? payload.variables.length : 0;
      openclawEnvFeedback.textContent = `Environment global carregado (${count} variaveis).`;
    }
    setOpenclawEnvironmentInputVisibility();
    return payload;
  } catch (error) {
    if (openclawEnvFeedback) {
      openclawEnvFeedback.textContent = `Falha ao carregar environment: ${error.message}`;
    }
    return null;
  } finally {
    openclawEnvironmentInFlight = false;
    setOpenclawEnvironmentButtonsDisabled(false);
  }
}

function collectOpenclawEnvironmentValues() {
  const values = {};
  if (!openclawEnvList) {
    return values;
  }

  openclawEnvList.querySelectorAll("input[data-env-key]").forEach((input) => {
    const key = (input.dataset.envKey || "").trim();
    if (!key) {
      return;
    }
    values[key] = input.value || "";
  });
  return values;
}

async function saveOpenclawEnvironment() {
  if (!openclawEnvList || openclawEnvironmentInFlight) {
    return;
  }

  const values = collectOpenclawEnvironmentValues();
  if (!Object.keys(values).length) {
    if (openclawEnvFeedback) {
      openclawEnvFeedback.textContent = "Nenhuma variavel disponivel para salvar.";
    }
    return;
  }

  openclawEnvironmentInFlight = true;
  setOpenclawEnvironmentButtonsDisabled(true);
  if (openclawEnvFeedback) {
    openclawEnvFeedback.textContent = "Salvando environment global...";
  }

  try {
    await fetchEnvironmentWithFallback({
      method: "POST",
      values,
    });
    if (openclawEnvFeedback) {
      openclawEnvFeedback.textContent = "Environment global salvo.";
    }
    await loadOpenclawEnvironment({ showLoading: false });
  } catch (error) {
    if (openclawEnvFeedback) {
      openclawEnvFeedback.textContent = `Falha ao salvar environment: ${error.message}`;
    }
  } finally {
    openclawEnvironmentInFlight = false;
    setOpenclawEnvironmentButtonsDisabled(false);
  }
}

function renderNanobotStatus(payload) {
  if (!nanobotStatusOutput) {
    return;
  }

  const lines = [
    `Comando: ${payload.command || "-"}`,
    `Instalado: ${payload.installed ? "sim" : "nao"}`,
    `Versao: ${payload.version || "-"}`,
    `Config: ${payload.config_path || "-"} (${payload.config_exists ? "ok" : "faltando"})`,
    `Workspace: ${payload.workspace_path || "-"} (${payload.workspace_exists ? "ok" : "faltando"})`,
  ];

  if (payload.raw_status) {
    lines.push("", "Saida do comando status:", payload.raw_status);
  }

  nanobotStatusOutput.textContent = lines.join("\n");
}

async function loadNanobotStatus() {
  if (!nanobotStatusFeedback) {
    return;
  }

  nanobotStatusFeedback.textContent = "Checando status do NanoBot...";
  try {
    const payload = await fetchJson("/nanobot/status", { method: "GET" });
    nanobotStatusFeedback.textContent = payload.message || "Status carregado.";
    renderNanobotStatus(payload);
  } catch (error) {
    nanobotStatusFeedback.textContent = `Falha ao consultar status: ${error.message}`;
    if (nanobotStatusOutput) {
      nanobotStatusOutput.textContent = "-";
    }
  }
}

async function runNanobotOnboard() {
  if (!nanobotStatusFeedback) {
    return;
  }

  nanobotStatusFeedback.textContent = "Executando onboard do NanoBot...";
  try {
    const payload = await fetchJson("/nanobot/onboard", { method: "POST" });
    nanobotStatusFeedback.textContent = payload.message || "Onboard executado.";
    await loadNanobotStatus();
  } catch (error) {
    nanobotStatusFeedback.textContent = `Falha no onboard: ${error.message}`;
  }
}

if (discoverModelsDir) {
  discoverModelsDir.addEventListener("blur", async () => {
    try {
      const payload = await fetchJson("/config", { method: "GET" });
      payload.models_dir = discoverModelsDir.value;

      await fetchJson("/config", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(payload)
      });

      if (settingModelsDir) {
        settingModelsDir.value = discoverModelsDir.value;
      }

      if (discoverModelsFeedback) {
        discoverModelsFeedback.style.opacity = "1";
        setTimeout(() => {
          discoverModelsFeedback.style.opacity = "0";
        }, 2000);
      }
    } catch (error) {
      console.error("Erro ao salvar diretorio de modelos", error);
    }
  });
}

frameworkRadios.forEach((radio) => {
  radio.addEventListener("change", (event) => {
    const nextFramework = event.target.value === "nanobot" ? "nanobot" : "openclaw";
    applyAgentFramework(nextFramework, { syncRadio: false, refreshPanel: true });
  });
});

installOpenclawBtn.addEventListener("click", async () => {
  if (openclawInstallInFlight) {
    return;
  }

  openclawInstallInFlight = true;
  setOpenclawInstallButtonState({ inFlight: true });
  if (checkOpenclawStatusBtn) {
    checkOpenclawStatusBtn.disabled = true;
  }

  let installFailed = false;
  try {
    installOpenclawFeedback.textContent = "Baixando repositorio e dependencias...";
    installOpenclawFeedback.classList.remove("hidden");

    const payload = await fetchJson("/openclaw/install", { method: "POST" });
    installOpenclawFeedback.textContent = payload.message || "OpenClaw instalado com sucesso!";
  } catch (error) {
    installFailed = true;
    installOpenclawFeedback.textContent = `Erro na instalacao: ${error.message}`;
  } finally {
    openclawInstallInFlight = false;
    const status = await loadOpenclawInstallStatus({ showLoading: false, syncInstallFeedback: false });

    if (installFailed && !(status && status.cli_exists)) {
      installOpenclawBtn.disabled = false;
      installOpenclawBtn.textContent = "Tentar Novamente";
      return;
    }

    setOpenclawInstallButtonState({ installed: Boolean(status?.cli_exists), inFlight: false });
  }
});

installNanobotBtn.addEventListener("click", async () => {
  try {
    const btn = installNanobotBtn;
    btn.disabled = true;
    btn.textContent = "Instalando Nanobot...";
    installNanobotFeedback.textContent = "Executando clone/pull e instalacao pip (isso pode demorar).";
    installNanobotFeedback.classList.remove("hidden");

    const payload = await fetchJson("/nanobot/install", { method: "POST" });
    installNanobotFeedback.textContent = payload.message || "Nanobot instalado com sucesso!";
    btn.textContent = "Instalado";
    await loadNanobotStatus();
  } catch (error) {
    installNanobotFeedback.textContent = `Erro na instalacao: ${error.message}`;
    installNanobotBtn.disabled = false;
    installNanobotBtn.textContent = "Tentar Novamente";
  }
});

if (checkOpenclawStatusBtn) {
  checkOpenclawStatusBtn.addEventListener("click", () => {
    void loadOpenclawInstallStatus();
  });
}

if (checkNanobotStatusBtn) {
  checkNanobotStatusBtn.addEventListener("click", () => {
    void loadNanobotStatus();
  });
}

if (initNanobotBtn) {
  initNanobotBtn.addEventListener("click", () => {
    void runNanobotOnboard();
  });
}

if (refreshOpenclawEnvBtn) {
  refreshOpenclawEnvBtn.addEventListener("click", () => {
    void loadOpenclawEnvironment();
  });
}

if (saveOpenclawEnvBtn) {
  saveOpenclawEnvBtn.addEventListener("click", () => {
    void saveOpenclawEnvironment();
  });
}

if (revealOpenclawEnvToggle) {
  revealOpenclawEnvToggle.addEventListener("change", () => {
    setOpenclawEnvironmentInputVisibility();
  });
}

function setAgentStatus(text) {
  if (agentChatStatus) {
    agentChatStatus.textContent = text;
  }
}

function clearElement(el) {
  if (el) {
    el.innerHTML = "";
  }
}

function appendAgentMessage(role, text) {
  if (!agentChatLog) {
    return;
  }
  const card = document.createElement("article");
  card.className = `agent-msg ${role}`;
  card.textContent = text;
  agentChatLog.appendChild(card);
  agentChatLog.scrollTop = agentChatLog.scrollHeight;
}

async function loadAgentSessions() {
  try {
    const sessions = await fetchJson("/agent/sessions");
    agentSessions = Array.isArray(sessions) ? sessions : [];
    agentSessions.sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime());

    renderAgentSessionSelect();
  } catch (err) {
    console.error("Failed to load agent sessions", err);
  }
}

function renderAgentSessionSelect() {
  if (!agentSessionSelect) return;
  agentSessionSelect.innerHTML = "";

  if (agentSessions.length === 0) {
    const opt = document.createElement("option");
    opt.value = "";
    opt.textContent = "Sem sessoes salvas";
    agentSessionSelect.appendChild(opt);
    agentSessionSelect.disabled = true;
    if (agentRenameSessionBtn) agentRenameSessionBtn.disabled = true;
    if (agentExportSessionBtn) agentExportSessionBtn.disabled = true;
    if (agentDeleteSessionBtn) agentDeleteSessionBtn.disabled = true;
    if (agentChatTitle) agentChatTitle.textContent = "Nova Sessao";
    return;
  }

  agentSessionSelect.disabled = false;
  agentSessions.forEach(s => {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = `${s.name || s.id} (${s.message_count} msgs)`;
    agentSessionSelect.appendChild(opt);
  });

  if (activeAgentSessionId && agentSessions.some(s => s.id === activeAgentSessionId)) {
    agentSessionSelect.value = activeAgentSessionId;
  } else {
    activeAgentSessionId = agentSessions[0].id;
    agentSessionSelect.value = activeAgentSessionId;
  }

  if (agentRenameSessionBtn) agentRenameSessionBtn.disabled = false;
  if (agentExportSessionBtn) agentExportSessionBtn.disabled = false;
  if (agentDeleteSessionBtn) agentDeleteSessionBtn.disabled = false;

  const activeSessionMeta = agentSessions.find(s => s.id === activeAgentSessionId);
  if (activeSessionMeta && agentChatTitle) {
    agentChatTitle.textContent = activeSessionMeta.name || activeSessionMeta.id;
  }
}

async function fetchAgentSessionMessages(sessionId) {
  try {
    if (agentChatLog) clearElement(agentChatLog);
    if (!sessionId) return;
    const messages = await fetchJson(`/agent/sessions/${sessionId}`);
    if (Array.isArray(messages)) {
      messages.forEach(msg => {
        const role = msg.role;
        let content = msg.content || "";
        if (msg.tool_name) {
          content = `[Tool: ${msg.tool_name}]\n${content}`;
        }
        appendAgentMessage(role, content);
      });
    }
  } catch (err) {
    console.error("Failed to fetch messages for session", err);
  }
}

async function handleAgentSessionSelectChange() {
  if (!agentSessionSelect || !agentSessionSelect.value) return;
  const newSessionId = agentSessionSelect.value;
  if (newSessionId !== activeAgentSessionId) {
    activeAgentSessionId = newSessionId;
    renderAgentSessionSelect();
    await fetchAgentSessionMessages(newSessionId);
  }
}

async function handleAgentNewSession() {
  try {
    const body = { name: "Nova conversa" };
    const meta = await fetchJson("/agent/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body)
    });
    activeAgentSessionId = meta.id;
    await loadAgentSessions();
    if (agentChatLog) clearElement(agentChatLog);
  } catch (err) {
    console.error("Failed to create session", err);
  }
}

async function handleAgentRenameSession() {
  if (!activeAgentSessionId) return;
  const activeSessionMeta = agentSessions.find(s => s.id === activeAgentSessionId);
  const currentName = activeSessionMeta ? activeSessionMeta.name : "";
  const newName = await showTextPrompt({
    title: "Renomear sessao",
    message: "Defina um novo nome para essa sessao do agent.",
    defaultValue: currentName,
    confirmLabel: "Salvar",
  });

  if (newName === null || newName.trim() === "") return;

  try {
    await fetchJson(`/agent/sessions/${activeAgentSessionId}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name: newName.trim() })
    });
    await loadAgentSessions();
  } catch (err) {
    console.error("Failed to rename session", err);
  }
}

async function handleAgentDeleteSession() {
  if (!activeAgentSessionId) return;
  const activeSessionMeta = agentSessions.find(s => s.id === activeAgentSessionId);
  const currentName = activeSessionMeta ? activeSessionMeta.name : "";
  const confirmed = await showConfirmDialog({
    title: "Apagar sessao",
    message: `Tem certeza que deseja apagar a sessao "${currentName}"?`,
    confirmLabel: "Apagar",
    danger: true,
  });
  if (!confirmed) return;

  try {
    await fetchJson(`/agent/sessions/${activeAgentSessionId}`, {
      method: "DELETE"
    });
    activeAgentSessionId = null;
    await loadAgentSessions();
    if (activeAgentSessionId) {
      await fetchAgentSessionMessages(activeAgentSessionId);
    } else {
      if (agentChatLog) clearElement(agentChatLog);
    }
  } catch (err) {
    console.error("Failed to delete session", err);
  }
}

function handleAgentExportSession() {
  if (!activeAgentSessionId || !daemonBaseUrl) return;
  window.open(`${daemonBaseUrl}/agent/sessions/${activeAgentSessionId}/export`, "_blank");
}

function renderAgentToggleList(container, items, checkedSet) {
  if (!container) {
    return;
  }
  container.innerHTML = "";
  if (!Array.isArray(items) || !items.length) {
    const empty = document.createElement("li");
    empty.className = "meta-note";
    empty.textContent = "Sem itens.";
    container.appendChild(empty);
    return;
  }

  items.forEach((item) => {
    const li = document.createElement("li");
    li.className = "agent-toggle-item";

    const meta = document.createElement("div");
    meta.className = "agent-toggle-meta";

    const title = document.createElement("p");
    title.className = "agent-toggle-title";
    title.textContent = item.name || "-";

    const desc = document.createElement("p");
    desc.className = "agent-toggle-desc";
    desc.textContent = item.description || item.policy || "";

    meta.appendChild(title);
    meta.appendChild(desc);

    const checkbox = document.createElement("input");
    checkbox.type = "checkbox";
    checkbox.checked = checkedSet.has((item.name || "").toLowerCase());
    checkbox.dataset.itemName = item.name || "";

    li.appendChild(meta);
    li.appendChild(checkbox);
    container.appendChild(li);
  });
}

function readCheckedNames(container) {
  if (!container) {
    return [];
  }
  return Array.from(container.querySelectorAll('input[type="checkbox"]'))
    .filter((el) => el.checked)
    .map((el) => (el.dataset.itemName || "").trim())
    .filter(Boolean);
}

function syncAgentModelOptions() {
  if (!agentProviderSelect || !agentModelSelect) {
    return;
  }
  const provider = agentProviderSelect.value;
  const models = agentModelsByProvider[provider] || [];
  const previous = agentModelSelect.value;

  agentModelSelect.innerHTML = "";
  if (!models.length) {
    const option = document.createElement("option");
    option.value = "";
    option.textContent = "Sem modelos detectados";
    agentModelSelect.appendChild(option);
    return;
  }

  models.forEach((model) => {
    const option = document.createElement("option");
    option.value = model;
    option.textContent = model;
    agentModelSelect.appendChild(option);
  });

  if (previous && models.includes(previous)) {
    agentModelSelect.value = previous;
  }
}

async function loadAgentProviders() {
  const providers = await fetchJson("/agent/providers", { method: "GET" });
  agentProvidersCatalog = Array.isArray(providers) ? providers : [];
  agentModelsByProvider = {};

  if (agentProviderSelect) {
    agentProviderSelect.innerHTML = "";
  }
  if (agentFallbackProviderSelect) {
    agentFallbackProviderSelect.innerHTML = "";
  }

  agentProvidersCatalog.forEach((provider) => {
    agentModelsByProvider[provider.id] = Array.isArray(provider.models) ? provider.models : [];
    if (agentProviderSelect) {
      const opt = document.createElement("option");
      opt.value = provider.id;
      opt.textContent = provider.name;
      agentProviderSelect.appendChild(opt);
    }
    if (agentFallbackProviderSelect) {
      const opt = document.createElement("option");
      opt.value = provider.id;
      opt.textContent = provider.name;
      agentFallbackProviderSelect.appendChild(opt);
    }
  });
}

function applyAgentConfigToForm(config) {
  if (!config) {
    return;
  }
  agentConfigCache = config;

  if (agentProviderSelect) {
    agentProviderSelect.value = config.provider || "ollama";
  }
  if (agentFallbackProviderSelect) {
    agentFallbackProviderSelect.value = config.fallback_provider || "mlx";
  }
  syncAgentModelOptions();
  if (agentModelSelect) {
    const known = agentModelsByProvider[agentProviderSelect?.value] || [];
    if (known.includes(config.model_id)) {
      agentModelSelect.value = config.model_id;
    } else if (config.model_id) {
      const option = document.createElement("option");
      option.value = config.model_id;
      option.textContent = `${config.model_id} (custom)`;
      agentModelSelect.appendChild(option);
      agentModelSelect.value = config.model_id;
    }
  }

  if (agentApiKeyInput) agentApiKeyInput.value = config.api_key || "";
  if (agentBaseUrlInput) agentBaseUrlInput.value = config.base_url || "";
  if (agentStreamingToggle) agentStreamingToggle.checked = Boolean(config.streaming);
  if (agentFallbackToggle) agentFallbackToggle.checked = Boolean(config.fallback_enabled);
  if (agentFallbackModelInput) agentFallbackModelInput.value = config.fallback_model_id || "";
  if (agentExecutionModeSelect) agentExecutionModeSelect.value = config.execution_mode || "full";
  if (agentApprovalModeSelect) agentApprovalModeSelect.value = config.approval_mode || "ask";
  if (agentMaxPromptInput) agentMaxPromptInput.value = config.max_prompt_tokens ?? 2200;
  if (agentMaxHistoryInput) agentMaxHistoryInput.value = config.max_history_messages ?? 14;
  if (agentMaxToolsInput) agentMaxToolsInput.value = config.max_tools_in_prompt ?? 6;
  if (agentAggressiveToolsToggle) {
    agentAggressiveToolsToggle.checked = Boolean(config.aggressive_tool_filtering);
  }
  if (agentToolFallbackToggle) {
    agentToolFallbackToggle.checked = Boolean(config.enable_tool_call_fallback);
  }
  if (agentNodeManagerSelect) {
    agentNodeManagerSelect.value = config.node_package_manager || "npm";
  }
  if (agentEgressInput) {
    agentEgressInput.value = (config.security?.egress_allow_domains || []).join(",");
  }
  if (agentSensitivePathsInput) {
    agentSensitivePathsInput.value = (config.security?.sensitive_paths || []).join(",");
  }
}

function collectAgentConfigFromForm() {
  const base = agentConfigCache || {};
  const parseList = (value) =>
    String(value || "")
      .split(",")
      .map((v) => v.trim())
      .filter(Boolean);

  return {
    ...base,
    provider: agentProviderSelect?.value || "ollama",
    model_id: agentModelSelect?.value || "",
    api_key: agentApiKeyInput?.value || "",
    base_url: agentBaseUrlInput?.value || "",
    custom_headers: base.custom_headers || {},
    execution_mode: agentExecutionModeSelect?.value || "full",
    approval_mode: agentApprovalModeSelect?.value || "ask",
    streaming: Boolean(agentStreamingToggle?.checked),
    fallback_enabled: Boolean(agentFallbackToggle?.checked),
    fallback_provider: agentFallbackProviderSelect?.value || "mlx",
    fallback_model_id: agentFallbackModelInput?.value || "",
    max_prompt_tokens: agentMaxPromptInput?.value ? Number(agentMaxPromptInput.value) : null,
    max_history_messages: agentMaxHistoryInput?.value ? Number(agentMaxHistoryInput.value) : null,
    max_tools_in_prompt: agentMaxToolsInput?.value ? Number(agentMaxToolsInput.value) : null,
    temperature: base.temperature ?? 0.1,
    aggressive_tool_filtering: Boolean(agentAggressiveToolsToggle?.checked),
    enable_tool_call_fallback: Boolean(agentToolFallbackToggle?.checked),
    enabled_skills: agentSkillsController
      ? agentSkillsController.getSkills().filter((item) => item.enabled).map((item) => item.name)
      : [],
    node_package_manager: agentNodeManagerSelect?.value || base.node_package_manager || "npm",
    skill_overrides: base.skill_overrides || {},
    enabled_tools: readCheckedNames(agentToolsList),
    workspace_root: base.workspace_root || null,
    security: {
      ...(base.security || {}),
      tool_allowlist: base.security?.tool_allowlist || [],
      tool_denylist: base.security?.tool_denylist || [],
      exec_safe_bins: base.security?.exec_safe_bins || [],
      exec_deny_patterns: base.security?.exec_deny_patterns || [],
      egress_allow_domains: parseList(agentEgressInput?.value),
      sensitive_paths: parseList(agentSensitivePathsInput?.value),
    },
  };
}

async function loadAgentConfig() {
  const config = await fetchJson("/agent/config", { method: "GET" });
  applyAgentConfigToForm(config);
  if (agentControlPlaneController) {
    agentControlPlaneController.syncPolicyEditors(config);
  }
  return config;
}

async function saveAgentConfig() {
  const payload = collectAgentConfigFromForm();
  const saved = await fetchJson("/agent/config", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  applyAgentConfigToForm(saved);
  return saved;
}

async function loadAgentSkills() {
  if (!agentSkillsController) {
    return;
  }
  await agentSkillsController.loadSkills();
}

async function loadAgentTools() {
  const tools = await fetchJson("/agent/tools", { method: "GET" });
  agentToolsCache = Array.isArray(tools) ? tools : [];
  const checked = enabledSetFromConfig(agentConfigCache?.enabled_tools);
  renderAgentToggleList(agentToolsList, agentToolsCache, checked);
}

const agentChannelsController = createAgentChannelsController({
  elements: {
    agentChannelsRefreshBtn,
    agentChannelSelect,
    agentChannelAccountIdInput,
    agentChannelCredentialsLabel,
    agentChannelCredentialsInput,
    agentChannelCredentialHint,
    agentChannelMetadataLabel,
    agentChannelMetadataInput,
    agentChannelRoutingDefaultsLabel,
    agentChannelRoutingDefaultsInput,
    agentChannelAdapterConfigLabel,
    agentChannelAdapterConfigInput,
    agentChannelOnboardingTitle,
    agentChannelOnboardingSummary,
    agentChannelOnboardingSteps,
    agentChannelEnabledToggle,
    agentChannelSetDefaultToggle,
    agentChannelSaveBtn,
    agentChannelClearBtn,
    agentChannelFormFeedback,
    agentChannelSessionTitle,
    agentChannelSessionStatus,
    agentChannelSessionMeta,
    agentChannelSessionCapabilities,
    agentChannelLoginBtn,
    agentChannelLogoutBtn,
    agentChannelShowQrBtn,
    agentChannelQrPanel,
    agentChannelQrCanvas,
    agentChannelQrText,
    agentSendChannelSelect,
    agentSendAccountSelect,
    agentSendTargetInput,
    agentSendMessageInput,
    agentSendTestBtn,
    agentProbeChannelBtn,
    agentResolveTargetBtn,
    agentChannelActionFeedback,
    agentChannelList,
    agentChannelLogsRefreshBtn,
    agentChannelLogsChannelSelect,
    agentChannelLogsAccountSelect,
    agentChannelLogsList,
  },
  fetchJson,
  promptText: showTextPrompt,
  confirmAction: showConfirmDialog,
  showChannelLoginDialog,
  renderQrCode: renderQrCodeToCanvas,
});

agentSkillsController = createAgentSkillsController({
  elements: {
    agentSkillsList,
    agentSkillsSummary,
    agentNodeManagerSelect,
  },
  fetchJson,
  promptText: showTextPrompt,
  onStatus: (message) => {
    if (agentMeta) {
      agentMeta.textContent = message;
    }
  },
});

agentControlPlaneController = createAgentControlPlaneController({
  elements: {
    agentPluginsRefreshBtn,
    agentPluginsList,
    agentPluginDetailTitle,
    agentPluginDetailMeta,
    agentPluginConfigEnabled,
    agentPluginConfigForm,
    agentPluginSaveBtn,
    agentPluginResetBtn,
    agentPluginFeedback,
    agentToolProfileSelect,
    agentToolProfileApplyBtn,
    agentPolicyScopeSelect,
    agentPolicyAllowInput,
    agentPolicyDenyInput,
    agentPolicySaveBtn,
    agentPolicyResetBtn,
    agentPolicyFeedback,
    agentToolCatalogSummary,
    agentEffectivePolicyList,
    agentMemoryLocalToggle,
    agentMemoryBackendSelect,
    agentMemoryCompressionSelect,
    agentMemorySaveBtn,
    agentMemoryFeedback,
    agentBudgetRefreshBtn,
    agentBudgetTelemetry,
    agentMaxPromptInput,
    agentMaxPromptValue,
    agentMaxHistoryInput,
    agentMaxHistoryValue,
    agentMaxToolsInput,
    agentMaxToolsValue,
    agentRuntimeRefreshBtn,
    agentRuntimeSummary,
    agentRuntimeFrameworkMeta,
    agentRuntimeDiagnosticsList,
    agentRuntimeLogMode,
    agentRuntimeLogSource,
    agentRuntimeLogAccount,
    agentRuntimeLogList,
  },
  fetchJson,
  onStatus: (message) => {
    if (agentMeta) {
      agentMeta.textContent = message;
    }
  },
  onToolPolicyChanged: async () => {
    await loadAgentConfig();
    await loadAgentTools();
    if (agentControlPlaneController) {
      await agentControlPlaneController.loadToolPolicies(agentConfigCache);
    }
  },
  loadAgentConfig,
});

async function loadAgentChannelLogs() {
  await agentChannelsController.loadLogs();
}

async function loadAgentChannels() {
  await agentChannelsController.loadChannels();
}

function enabledSetFromConfig(values) {
  return new Set(
    Array.isArray(values)
      ? values.map((v) => String(v).toLowerCase())
      : []
  );
}

async function loadAgentAudit() {
  const payload = await fetchJson("/agent/audit?limit=12", { method: "GET" });
  const entries = Array.isArray(payload?.entries) ? payload.entries : [];
  if (!agentAuditList) {
    return;
  }

  clearElement(agentAuditList);
  if (!entries.length) {
    const li = document.createElement("li");
    li.className = "meta-note";
    li.textContent = "Sem eventos.";
    agentAuditList.appendChild(li);
    return;
  }

  entries.forEach((entry) => {
    const li = document.createElement("li");
    li.className = "agent-audit-item";
    const ts = entry.timestamp ? new Date(entry.timestamp).toLocaleString() : "-";
    li.textContent = `${ts} • ${entry.event_type || "-"} • ${entry.tool_name || "session"}${entry.duration_ms ? ` • ${entry.duration_ms}ms` : ""
      }`;
    agentAuditList.appendChild(li);
  });
}

// -------------------------------------------------------------
// Agent Observability Console (Dynamic feed)
// -------------------------------------------------------------

function getRelativeTimeString(dateString) {
  if (!dateString) return "-";
  const date = new Date(dateString);
  const diffMs = Date.now() - date.getTime();
  const diffSec = Math.floor(diffMs / 1000);

  if (diffSec < 5) return "agora mesmo";
  if (diffSec < 60) return `há ${diffSec}s`;
  if (diffSec < 3600) return `há ${Math.floor(diffSec / 60)}m`;
  if (diffSec < 86400) return `há ${Math.floor(diffSec / 3600)}h`;
  return `há ${Math.floor(diffSec / 86400)}d`;
}

async function loadObservabilityFeed() {
  if (!agentObservabilityList) return;

  const filters = {
    limit: "100",
  };

  // Override session filter if follow toggle is on
  if (auditFollowToggle && auditFollowToggle.checked) {
    if (activeAgentSessionId) {
      auditFilterSession.value = activeAgentSessionId;
      auditActiveSessionFollowCache = activeAgentSessionId;
    }
  }

  if (auditFilterSession && auditFilterSession.value) {
    filters.session_id = auditFilterSession.value;
  }
  if (auditFilterEvent && auditFilterEvent.value) {
    filters.event_type = auditFilterEvent.value;
  }
  if (auditFilterStatus && auditFilterStatus.value) {
    filters.status = auditFilterStatus.value;
  }
  if (auditFilterTool && auditFilterTool.value.trim()) {
    filters.tool_name = auditFilterTool.value.trim();
  }

  const queryParams = new URLSearchParams(filters).toString();

  if (queryParams === auditConsoleLastParams && auditObservabilityEntriesCache) {
    // Light poll just to check if new stuff arrived ? 
    // Usually we would fetch to check, but since we re-fetch everything anyway, 
    // let's do a fast equality check. Wait, best is to just fetch and compare stringified.
  }

  let payload;
  try {
    payload = await fetchJson(`/agent/audit?${queryParams}`, { method: "GET" });
  } catch (err) {
    console.error("Failed to load audit feed:", err);
    return;
  }

  auditConsoleLastParams = queryParams;

  const entries = Array.isArray(payload?.entries) ? payload.entries : [];

  const fingerprint = entries.map(e => e.id).join("|");
  if (fingerprint === auditObservabilityEntriesCache) {
    return; // No changes to the feed list
  }
  auditObservabilityEntriesCache = fingerprint;

  clearElement(agentObservabilityList);

  if (!entries.length) {
    const li = document.createElement("li");
    li.style.padding = "16px";
    li.style.textAlign = "center";
    li.style.color = "var(--muted)";
    li.textContent = "Nenhum evento corresponde aos filtros.";
    agentObservabilityList.appendChild(li);
    return;
  }

  entries.forEach((entry) => {
    const li = document.createElement("li");
    li.className = "agent-audit-item";
    li.style.cursor = "pointer";
    li.style.transition = "background-color 0.2s";

    // Status color
    let statusColor = "var(--text)";
    if (entry.status === "error") statusColor = "var(--danger)";
    if (entry.status === "denied") statusColor = "var(--warn)";
    if (entry.status === "success" && (entry.event_type === "ToolExecuted" || entry.event_type === "ResponseFinal" || entry.event_type === "ApprovalDecision")) {
      statusColor = "var(--accent)";
    }

    const timeRel = getRelativeTimeString(entry.timestamp);
    const eventType = entry.event_type || "-";

    // Determine main label: tool name, provider, or something representative
    let mainLabel = entry.tool_name || "";
    if (entry.event_type === "ProviderCall" && entry.provider) {
      mainLabel = `${entry.provider} / ${entry.model || "-"}`;
    }
    if (!mainLabel && entry.reason) {
      mainLabel = entry.reason.length > 50 ? entry.reason.substring(0, 50) + "..." : entry.reason;
    }

    li.innerHTML = `
      <div style="display: flex; justify-content: space-between; align-items: baseline; width: 100%;">
        <div>
          <span style="font-weight: 600; font-size: 0.9rem; color: ${statusColor};">${eventType}</span>
          ${mainLabel ? `<span style="margin-left: 8px; font-size: 0.85rem; color: var(--muted);">${mainLabel}</span>` : ""}
        </div>
        <div style="font-size: 0.8rem; color: var(--muted); text-align: right;">
          ${entry.duration_ms ? `<span>${entry.duration_ms}ms</span> • ` : ""}
          <span>${timeRel}</span>
        </div>
      </div>
    `;

    // Hover effect
    li.onmouseenter = () => li.style.backgroundColor = "rgba(255,255,255,0.05)";
    li.onmouseleave = () => li.style.backgroundColor = "transparent";

    // Click to view details
    li.addEventListener("click", () => openAuditDetail(entry.id));

    agentObservabilityList.appendChild(li);
  });
}

let auditObservabilityEntriesCache = "";

async function openAuditDetail(eventId) {
  if (!auditDetailPanel || !auditDetailContent) return;

  auditDetailContent.innerHTML = "Carregando...";
  auditDetailPanel.style.display = "flex";

  try {
    const entry = await fetchJson(`/agent/audit/${eventId}`, { method: "GET" });
    const formattedJson = JSON.stringify(entry, null, 2);
    auditDetailContent.innerHTML = `<pre style="margin: 0; font-family: monospace; font-size: 0.8rem; overflow-x: auto;">${formattedJson}</pre>`;
  } catch (err) {
    auditDetailContent.innerHTML = `<span style="color: var(--danger);">Falha ao carregar os detalhes do evento: ${err.message}</span>`;
  }
}

if (auditDetailCloseBtn) {
  auditDetailCloseBtn.addEventListener("click", () => {
    if (auditDetailPanel) auditDetailPanel.style.display = "none";
  });
}

function startAuditPolling() {
  if (auditPollTimer !== null) return;
  auditPollTimer = setInterval(() => {
    void loadObservabilityFeed();
  }, 1500);
}

function stopAuditPolling() {
  if (auditPollTimer !== null) {
    clearInterval(auditPollTimer);
    auditPollTimer = null;
  }
}

async function syncAuditSessionFilterDropdown() {
  if (!auditFilterSession) return;

  const currentVal = auditFilterSession.value;
  auditFilterSession.innerHTML = `<option value="">Todas Sessoes</option>`;

  agentSessions.forEach(s => {
    const opt = document.createElement("option");
    opt.value = s.id;
    opt.textContent = `${s.name || s.id}`;
    auditFilterSession.appendChild(opt);
  });

  if (currentVal && agentSessions.some(s => s.id === currentVal)) {
    auditFilterSession.value = currentVal;
  }
}

if (auditExportBtn) {
  auditExportBtn.addEventListener("click", () => {
    if (!daemonBaseUrl) return;

    const filters = new URLSearchParams();
    if (auditFilterSession && auditFilterSession.value) filters.append("session_id", auditFilterSession.value);
    if (auditFilterEvent && auditFilterEvent.value) filters.append("event_type", auditFilterEvent.value);
    if (auditFilterStatus && auditFilterStatus.value) filters.append("status", auditFilterStatus.value);
    if (auditFilterTool && auditFilterTool.value.trim()) filters.append("tool_name", auditFilterTool.value.trim());
    filters.append("limit", "5000"); // Request a higher limit for exports

    window.open(`${daemonBaseUrl}/agent/audit/export?${filters.toString()}`, "_blank");
  });
}

if (auditFilterSession) auditFilterSession.addEventListener("change", () => {
  if (auditFollowToggle && auditFollowToggle.checked && auditFilterSession.value !== activeAgentSessionId) {
    auditFollowToggle.checked = false; // Disable follow mode if user manually changes session via filter
  }
  loadObservabilityFeed();
});
if (auditFilterEvent) auditFilterEvent.addEventListener("change", () => loadObservabilityFeed());
if (auditFilterStatus) auditFilterStatus.addEventListener("change", () => loadObservabilityFeed());
if (auditFilterTool) auditFilterTool.addEventListener("input", () => {
  // debounce slightly
  clearTimeout(auditFilterTool.timer);
  auditFilterTool.timer = setTimeout(() => loadObservabilityFeed(), 300);
});
if (auditFollowToggle) auditFollowToggle.addEventListener("change", () => loadObservabilityFeed());

async function onAgentTabSelected() {
  if (!panelAgent) {
    return;
  }
  setStatus("carregando agent", "running");
  try {
    await loadAgentProviders();
    await loadAgentConfig();
    await loadAgentSkills();
    await loadAgentTools();
    await loadAgentChannels();
    if (agentControlPlaneController) {
      await agentControlPlaneController.loadPlugins();
      await agentControlPlaneController.loadToolPolicies(agentConfigCache);
      await agentControlPlaneController.loadRuntimeHealth();
    }
    await loadAgentAudit();
    await loadAgentSessions();
    syncAuditSessionFilterDropdown(); // ensure console gets the latest sessions list
    if (activeAgentSessionId) {
      await fetchAgentSessionMessages(activeAgentSessionId);
    }

    if (auditFollowToggle && auditFollowToggle.checked) {
      loadObservabilityFeed(); // initial eager fetch inside tab load
    }

    if (agentMeta) {
      agentMeta.textContent = "Configuracao carregada.";
    }
    setStatus("pronto");
  } catch (error) {
    if (agentMeta) {
      agentMeta.textContent = `Falha ao carregar Agent: ${error.message}`;
    }
    setStatus("erro agent", "error");
  }
}

async function sendAgentMessage() {
  const text = String(agentMessageInput?.value || "").trim();
  if (!text) {
    return;
  }

  agentMessageInput.value = "";
  appendAgentMessage("user", text);
  setAgentStatus("executando...");

  try {
    const payload = collectAgentConfigFromForm();
    const response = await fetchJson("/agent/run", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        session_id: activeAgentSessionId || null,
        message: text,
        provider: payload.provider,
        model_id: payload.model_id,
        api_key: payload.api_key,
        base_url: payload.base_url,
        custom_headers: payload.custom_headers || {},
        streaming: payload.streaming,
        fallback_enabled: payload.fallback_enabled,
        fallback_provider: payload.fallback_provider,
        fallback_model_id: payload.fallback_model_id,
        execution_mode: payload.execution_mode,
        approval_mode: payload.approval_mode,
        max_prompt_tokens: payload.max_prompt_tokens,
        max_history_messages: payload.max_history_messages,
        max_tools_in_prompt: payload.max_tools_in_prompt,
        aggressive_tool_filtering: payload.aggressive_tool_filtering,
        enable_tool_call_fallback: payload.enable_tool_call_fallback,
        enabled_skills: payload.enabled_skills,
        enabled_tools: payload.enabled_tools,
      }),
    });

    appendAgentMessage("assistant", response.final_response || response.content || "(sem resposta)");
    setAgentStatus(`ok • ${response.provider || "-"} • ${response.model_id || "-"}`);
    await loadAgentAudit();

    if (response.session_id && response.session_id !== activeAgentSessionId) {
      activeAgentSessionId = response.session_id;
      await loadAgentSessions();
      syncAuditSessionFilterDropdown();
    } else {
      await loadAgentSessions();
      syncAuditSessionFilterDropdown();
    }
  } catch (error) {
    appendAgentMessage("assistant", `Erro: ${error.message}`);
    setAgentStatus("falha");
  }
}

async function saveAgentChannelAccount() {
  const channel = agentChannelSelect?.value;
  const accountId = String(agentChannelAccountIdInput?.value || "").trim();
  if (!channel || !accountId) {
    throw new Error("Selecione um canal e informe o account_id.");
  }

  const payload = {
    channel,
    account_id: accountId,
    enabled: Boolean(agentChannelEnabledToggle?.checked),
    credentials: parseCredentialsInput(agentChannelCredentialsInput?.value),
    metadata: parseOptionalJsonInput(agentChannelMetadataInput?.value, {}),
    routing_defaults: parseOptionalJsonInput(agentChannelRoutingDefaultsInput?.value, {}),
    set_as_default: Boolean(agentChannelSetDefaultToggle?.checked),
    adapter_config: null,
  };

  await fetchJson("/agent/channels/upsert-account", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (agentChannelFormFeedback) {
    agentChannelFormFeedback.textContent = `Conta ${channel}:${accountId} salva.`;
  }
  await loadAgentChannels();
}

async function executeChannelOperation(path, payload) {
  const response = await fetchJson(path, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  await loadAgentChannels();
  return response;
}

async function renameAgentChannelAccount(channelId, accountId) {
  const channel = findChannelView(channelId);
  const account = channel?.accounts?.find((entry) => entry.account_id === accountId);
  if (!channel || !account) {
    throw new Error("Conta nao encontrada.");
  }
  const nextAccountId = window.prompt("Novo account_id", accountId);
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
    if (agentChannelActionFeedback) {
      agentChannelActionFeedback.textContent = `${response.channel}:${response.account_id} • ${response.status}`;
    }
    return;
  }
  if (action === "logout") {
    const response = await executeChannelOperation("/agent/channels/logout", {
      channel: channelId,
      account_id: accountId,
    });
    if (agentChannelActionFeedback) {
      agentChannelActionFeedback.textContent = `${response.channel}:${response.account_id} • ${response.status}`;
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
    if (!window.confirm(`Remover ${channelId}:${accountId}?`)) {
      return;
    }
    await executeChannelOperation("/agent/channels/remove-account", {
      channel: channelId,
      account_id: accountId,
    });
  }
}

async function sendAgentChannelTestMessage() {
  const channel = agentSendChannelSelect?.value;
  const target = String(agentSendTargetInput?.value || "").trim();
  const message = String(agentSendMessageInput?.value || "").trim();
  if (!channel || !target || !message) {
    throw new Error("Canal, target e mensagem sao obrigatorios.");
  }
  const payload = await executeChannelOperation("/agent/message/send", {
    channel,
    account_id: agentSendAccountSelect?.value || null,
    target,
    message,
  });
  if (agentChannelActionFeedback) {
    agentChannelActionFeedback.textContent = `Mensagem enviada via ${payload.channel}:${payload.account_id} (${payload.message_id})`;
  }
}

async function probeAgentChannel() {
  const channel = agentSendChannelSelect?.value;
  if (!channel) {
    throw new Error("Selecione um canal.");
  }
  const payload = await executeChannelOperation("/agent/channels/probe", {
    channel,
    account_id: agentSendAccountSelect?.value || null,
    all_accounts: !agentSendAccountSelect?.value,
  });
  const summaries = (Array.isArray(payload) ? payload : []).map((entry) => `${entry.account_id}:${entry.status}`);
  if (agentChannelActionFeedback) {
    agentChannelActionFeedback.textContent = summaries.join(" • ") || "Probe concluido.";
  }
}

async function resolveAgentChannelTarget() {
  const channel = agentSendChannelSelect?.value;
  const target = String(agentSendTargetInput?.value || "").trim();
  if (!channel || !target) {
    throw new Error("Selecione canal e target.");
  }
  const payload = await executeChannelOperation("/agent/channels/resolve", {
    channel,
    account_id: agentSendAccountSelect?.value || null,
    target,
  });
  if (agentChannelActionFeedback) {
    agentChannelActionFeedback.textContent = `Target resolvido por ${payload.account_id}: ${payload.resolved_target}`;
  }
}

if (agentProviderSelect) {
  agentProviderSelect.addEventListener("change", () => {
    syncAgentModelOptions();
  });
}

if (agentRefreshBtn) {
  agentRefreshBtn.addEventListener("click", () => {
    void onAgentTabSelected();
  });
}

if (agentSaveConfigBtn) {
  agentSaveConfigBtn.addEventListener("click", async () => {
    try {
      const old = agentSaveConfigBtn.textContent;
      agentSaveConfigBtn.disabled = true;
      agentSaveConfigBtn.textContent = "Salvando...";
      await saveAgentConfig();
      if (agentControlPlaneController) {
        await agentControlPlaneController.loadToolPolicies(agentConfigCache);
        await agentControlPlaneController.loadBudgetTelemetry();
      }
      agentSaveConfigBtn.textContent = "Salvo!";
      setTimeout(() => {
        agentSaveConfigBtn.textContent = old;
        agentSaveConfigBtn.disabled = false;
      }, 1200);
    } catch (error) {
      if (agentMeta) {
        agentMeta.textContent = `Falha ao salvar: ${error.message}`;
      }
      agentSaveConfigBtn.disabled = false;
      agentSaveConfigBtn.textContent = "Salvar configuracoes";
    }
  });
}

if (agentReloadSkillsBtn) {
  agentReloadSkillsBtn.addEventListener("click", async () => {
    try {
      await fetchJson("/agent/skills/reload", { method: "POST" });
      await loadAgentSkills();
    } catch (error) {
      if (agentMeta) {
        agentMeta.textContent = `Falha no reload de skills: ${error.message}`;
      }
    }
  });
}

if (agentCheckSkillsBtn) {
  agentCheckSkillsBtn.addEventListener("click", async () => {
    try {
      await loadAgentSkills();
    } catch (error) {
      if (agentMeta) {
        agentMeta.textContent = `Falha no check de skills: ${error.message}`;
      }
    }
  });
}

if (agentInstallSkillsBtn) {
  agentInstallSkillsBtn.addEventListener("click", async () => {
    try {
      if (agentSkillsController) {
        await agentSkillsController.installMissingSkills();
      }
    } catch (error) {
      if (agentMeta) {
        agentMeta.textContent = `Falha na instalacao de skills: ${error.message}`;
      }
    }
  });
}

if (agentConfigureSkillsBtn) {
  agentConfigureSkillsBtn.addEventListener("click", async () => {
    try {
      if (agentSkillsController) {
        await agentSkillsController.configurePendingSkill();
      }
    } catch (error) {
      if (agentMeta) {
        agentMeta.textContent = `Falha na configuracao da skill: ${error.message}`;
      }
    }
  });
}

if (agentChatForm) {
  agentChatForm.addEventListener("submit", async (event) => {
    event.preventDefault();
    await sendAgentMessage();
  });
}

if (agentMessageInput) {
  agentMessageInput.addEventListener("input", () => autoResizeTextarea(agentMessageInput));
}

if (agentSessionSelect) {
  agentSessionSelect.addEventListener("change", () => {
    handleAgentSessionSelectChange();
    if (auditFollowToggle && auditFollowToggle.checked && auditFilterSession) {
      auditFilterSession.value = agentSessionSelect.value;
      loadObservabilityFeed();
    }
  });
}
if (agentNewSessionBtn) {
  agentNewSessionBtn.addEventListener("click", handleAgentNewSession);
}
if (agentRenameSessionBtn) {
  agentRenameSessionBtn.addEventListener("click", handleAgentRenameSession);
}
if (agentExportSessionBtn) {
  agentExportSessionBtn.addEventListener("click", handleAgentExportSession);
}
if (agentDeleteSessionBtn) {
  agentDeleteSessionBtn.addEventListener("click", handleAgentDeleteSession);
}

async function bootstrap() {
  let daemonReady = false;
  try {
    const storedWebsearch = localStorage.getItem(STORAGE_CHAT_WEBSEARCH_ENABLED);
    setWebsearchToggleState(storedWebsearch === "1", { persist: false });
    const storedAirllm = localStorage.getItem(STORAGE_CHAT_AIRLLM_ENABLED);
    if (storedAirllm == null) {
      setChatAirllmToggleState(true, { persist: false });
    } else {
      setChatAirllmToggleState(storedAirllm === "1", { persist: false });
    }
    setChatModelMenuOpen(false);

    loadThreads();
    ensureActiveThread();

    renderOpenClawViews();
    setOpenClawRuntimeButtons("");
    setAiSceneBusyState();
    renderAiSceneSteps(null);
    setAiSceneStatus("Pronto para montar uma cena.");

    renderThreadList();
    rebuildChatFromThread();
    setEditMode(null);

    setStatus("conectando daemon", "running");
    daemonReady = await waitForDaemonReady();
    if (daemonReady) {
      await loadModels();
      await loadCatalogSources();
      await searchCatalogModels();
      await loadConfig();
      await loadDownloads();
      setStatus("pronto");
    } else {
      setStatus("daemon offline", "error");
    }

    if (!restoreOpenClawObservabilityFromStorage()) {
      clearChipList(openclawSkills, "Nenhuma skill reportada");
      clearChipList(openclawTools, "Nenhuma tool reportada");
    }

    if (downloadsTimer) {
      window.clearInterval(downloadsTimer);
    }

    try {
      window.particleSystem = new ParticleSystem("bg-canvas");
    } catch (e) {
      console.error("Failed to initialize Particle System:", e);
      window.particleSystem = createParticleSystemFallback();
      setAiSceneStatus("Modo fallback ativo: sem WebGL, mas com timeline de cena.");
    }

    downloadsTimer = window.setInterval(() => {
      if (daemonReady) {
        void loadDownloads();
      }
    }, 2000);

    switchTab(activeTab);
    switchDiscoverSubtab(activeDiscoverSubtab);
  } catch (error) {
    setStatus("erro inicial", "error");
    addSystemMessage(`Falha na inicializacao: ${error.message}`);
  } finally {
    setTimeout(hideSplash, 300);
  }
}

// ---------- Native-app feel: block browser context menu ----------
document.addEventListener("contextmenu", (e) => {
  // Allow context menu only on input, textarea, and contenteditable elements
  const target = e.target;
  if (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    (target instanceof HTMLElement && target.isContentEditable)
  ) {
    return;
  }
  e.preventDefault();
});

void bootstrap();
