const STORAGE_DAEMON_URL = "mlxPilotDaemonUrl";
const STORAGE_CHAT_THREADS = "mlxPilotChatThreadsV2";

const STREAM_CHARS_PER_TICK = 22;
const STREAM_TICK_MS = 20;
const CHAT_SCROLL_THRESHOLD_PX = 72;
const OPENCLAW_LOG_POLL_MS = 1500;
const OPENCLAW_LOG_MAX_CHARS = 120000;

const appShell = document.getElementById("app-shell");
const statusPill = document.getElementById("status-pill");

const daemonInput = document.getElementById("daemon-url");
const saveUrlBtn = document.getElementById("save-url");

const tabButtons = Array.from(document.querySelectorAll(".tab-btn[data-tab]"));
const panelChat = document.getElementById("panel-chat");
const panelDiscover = document.getElementById("panel-discover");
const panelOpenClaw = document.getElementById("panel-openclaw");

const chatModelSwitcher = document.getElementById("chat-model-switcher");
const chatModelSelect = document.getElementById("chat-model-select");
const refreshModelsBtn = document.getElementById("refresh-models");

const chatHistoryMeta = document.getElementById("chat-history-meta");
const chatHistoryList = document.getElementById("chat-history-list");
const newChatThreadBtn = document.getElementById("new-chat-thread");

const selectedThreadLabel = document.getElementById("selected-thread-label");
const selectedModelLabel = document.getElementById("selected-model-label");
const chatForm = document.getElementById("chat-form");
const chatLog = document.getElementById("chat-log");
const messageInput = document.getElementById("message-input");
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
const remoteCardTemplate = document.getElementById("remote-card-template");
const refreshDownloadsBtn = document.getElementById("refresh-downloads");
const downloadList = document.getElementById("download-list");
const downloadItemTemplate = document.getElementById("download-item-template");

const openclawStatusText = document.getElementById("openclaw-status-text");
const refreshOpenclawStatusBtn = document.getElementById("refresh-openclaw-status");

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
const openclawCloudModelSelect = document.getElementById("openclaw-cloud-model-select");
const openclawLocalModelSelect = document.getElementById("openclaw-local-model-select");
const refreshOpenclawModelsBtn = document.getElementById("refresh-openclaw-models");
const applyOpenclawModelBtn = document.getElementById("apply-openclaw-model");
const openclawModelCurrent = document.getElementById("openclaw-model-current");
const openclawConfigFeedback = document.getElementById("openclaw-config-feedback");

let daemonBaseUrl = localStorage.getItem(STORAGE_DAEMON_URL) || "http://127.0.0.1:11435";
let selectedModelId = null;
let localModels = [];

let chatThreads = [];
let activeThreadId = null;

let activeTab = "chat";
let isGenerating = false;
let activeStreamController = null;
let streamFallbackNotified = false;
let pendingEditMessageIndex = null;

let lastDownloadsFingerprint = "";
let downloadsTimer = null;

let openclawStatusLoaded = false;
let openclawChatInFlight = false;
let openclawLogsTimer = null;
let openclawActiveLogStream = "gateway";
let openclawLogCursor = 0;
let openclawSelectedViews = new Set(["chat"]);
let openclawMultiView = false;
let openclawModelsCatalog = {
  cloud_models: [],
  local_models: [],
  current: null,
};

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
      detail = body.error;
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
              .map((item) => ({ role: item.role, content: item.content }))
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

function getModelLabelById(modelId) {
  const model = localModels.find((entry) => entry.id === modelId);
  return model ? model.name : "modelo n/d";
}

function renderThreadList() {
  chatHistoryList.innerHTML = "";

  const sorted = sortThreadsForView();
  chatHistoryMeta.textContent = `${sorted.length} conversa(s)`;

  sorted.forEach((thread) => {
    const li = document.createElement("li");
    li.className = "history-item";

    const button = document.createElement("button");
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
      setEditMode(null);
      activeThreadId = thread.id;
      syncModelWithActiveThread();
      renderThreadList();
      rebuildChatFromThread();
    });

    li.appendChild(button);
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

  renderSelectedThreadMeta();
}

function renderSelectedThreadMeta() {
  const active = getActiveThread();
  if (!active) {
    selectedThreadLabel.textContent = "Nova conversa";
    selectedModelLabel.textContent = "Nenhum modelo selecionado";
    return;
  }

  selectedThreadLabel.textContent = active.title || "Nova conversa";
  const model = localModels.find((entry) => entry.id === selectedModelId);
  selectedModelLabel.textContent = model ? `${model.name} (${model.provider})` : "Modelo nao selecionado";
}

function addSystemMessage(text) {
  const node = messageTemplate.content.firstElementChild.cloneNode(true);
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
  node.querySelector(".message-role").textContent = role;
  node.querySelector(".message-content").textContent = content;

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

function rebuildChatFromThread() {
  chatLog.innerHTML = "";
  const active = getActiveThread();

  if (!active) {
    setEditMode(null);
    renderSelectedThreadMeta();
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

    addMessageCard(message.role, message.content, { forceScroll: false });
  });

  scrollChatToBottom(true);
  renderSelectedThreadMeta();
}

function appendMessageToActiveThread(role, content) {
  const active = getActiveThread();
  if (!active) {
    return;
  }

  active.messages.push({ role, content });
  active.updatedAt = Date.now();

  if (role === "user" && (active.title === "Nova conversa" || !active.title?.trim())) {
    active.title = deriveThreadTitle(content);
  }

  persistThreads();
  renderThreadList();
  renderSelectedThreadMeta();
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

function createAssistantStreamCard() {
  const node = assistantStreamTemplate.content.firstElementChild.cloneNode(true);
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
    thinkingQueue: "",
    answerQueue: "",
    flushTimer: null,
    latestMetrics: null,
  };

  chatLog.appendChild(node);
  scrollChatToBottom(true);
  return ui;
}

function setAssistantState(ui, status) {
  const labels = {
    waiting: "aguardando modelo",
    thinking: "thinking",
    answering: "respondendo",
    completed: "finalizado",
    cancelled: "interrompido",
    error: "erro",
  };

  ui.stateLabel.textContent = labels[status] || status;

  if (status === "waiting") {
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
    ui.answerText.textContent += chunk;
    ui.finalAnswer = ui.answerText.textContent;
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

function renderAssistantMetrics(ui, event) {
  if (!event) {
    return;
  }

  const lines = [];
  const hasRawMetrics = typeof event.raw_metrics === "string" && event.raw_metrics.trim().length > 0;

  if (!hasRawMetrics && event.prompt_tokens != null) {
    lines.push(`Prompt: ${event.prompt_tokens} tokens`);
  }
  if (!hasRawMetrics && event.completion_tokens != null) {
    lines.push(`Generation: ${event.completion_tokens} tokens`);
  }
  if (event.total_tokens != null) {
    lines.push(`Total: ${event.total_tokens} tokens`);
  }
  if (!hasRawMetrics && event.prompt_tps != null) {
    lines.push(`Prompt rate: ${Number(event.prompt_tps).toFixed(3)} tokens/sec`);
  }
  if (!hasRawMetrics && event.generation_tps != null) {
    lines.push(`Generation rate: ${Number(event.generation_tps).toFixed(3)} tokens/sec`);
  }
  if (!hasRawMetrics && event.peak_memory_gb != null) {
    lines.push(`Peak memory: ${Number(event.peak_memory_gb).toFixed(3)} GB`);
  }
  if (event.latency_ms != null) {
    lines.push(`Latency: ${event.latency_ms} ms`);
  }

  if (hasRawMetrics) {
    lines.unshift(event.raw_metrics.trim());
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

function buildChatPayload(messages) {
  return {
    model_id: selectedModelId,
    messages,
    options: {
      temperature: 0.2,
      max_tokens: 512,
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

  if (split.thinking) {
    setAssistantState(ui, "thinking");
    appendThinking(ui, split.thinking);
  }

  if (split.answer) {
    setAssistantState(ui, "answering");
    appendAnswer(ui, split.answer);
  }

  await waitForAssistantFlush(ui);
  setAssistantState(ui, "completed");
  renderAssistantMetrics(ui, {
    prompt_tokens: body?.usage?.prompt_tokens,
    completion_tokens: body?.usage?.completion_tokens,
    total_tokens: body?.usage?.total_tokens,
    latency_ms: body?.latency_ms,
    raw_metrics: extractRawMetrics(body?.raw_output),
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
        setAssistantState(ui, event.status || "waiting");
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

      if (event.event === "done") {
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
    await consumeChatStream(buildChatPayload(thread.messages), assistantUi, activeStreamController.signal);

    const finalAnswer = assistantUi.finalAnswer.trim();
    if (finalAnswer) {
      appendMessageToActiveThread("assistant", finalAnswer);
    }

    setStatus("resposta concluida");
  } catch (error) {
    if (isAbortError(error)) {
      setAssistantState(assistantUi, "cancelled");
      assistantUi.metricsSection.classList.remove("hidden");
      assistantUi.metricsText.textContent = "Geracao interrompida pelo usuario.";
      const partialAnswer = assistantUi.finalAnswer.trim();
      if (partialAnswer) {
        appendMessageToActiveThread("assistant", partialAnswer);
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

    setStatus("pronto");
  } catch (error) {
    setStatus("erro modelos", "error");
    addSystemMessage(`Falha ao carregar modelos: ${error.message}`);
  }
}

async function loadCatalogSources() {
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
    node.querySelector(".remote-summary").textContent =
      model.summary || "Sem descricao detalhada no catalogo.";
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

    const link = node.querySelector(".remote-link");
    link.href = model.model_url;

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

function updateOpenClawObservability(response) {
  const provider = response.provider || "provider n/d";
  const model = response.model || "model n/d";
  openclawProviderModel.textContent = `${provider} • ${model}`;
  renderOpenClawUsage(response.usage);
  renderChipList(openclawSkills, response.skills, "Nenhuma skill reportada");
  renderChipList(openclawTools, response.tools, "Nenhuma tool reportada");
}

function addOpenClawChatMessage(role, content, meta = "") {
  const node = document.createElement("article");
  node.className = "message-card";

  const roleNode = document.createElement("header");
  roleNode.className = "message-role";
  roleNode.textContent = role;

  const contentNode = document.createElement("div");
  contentNode.className = "message-content";
  contentNode.textContent = content;

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

function setOpenClawSendingState(nextState) {
  openclawChatInFlight = nextState;
  openclawSendBtn.disabled = nextState;
  openclawMessageInput.disabled = nextState;
}

async function loadOpenClawStatus() {
  try {
    const status = await fetchJson("/openclaw/status");
    openclawStatusLoaded = true;

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
    const chunk = await fetchJson(`/openclaw/logs?${params.toString()}`);

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
  setStatus("consultando openclaw", "running");

  try {
    const response = await fetchJson("/openclaw/chat", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ message }),
    });

    const reply = response.reply?.trim() || "(sem resposta textual)";
    const meta = [
      response.status || "",
      response.summary || "",
      response.duration_ms != null ? `${response.duration_ms} ms` : "",
      response.run_id ? `run ${response.run_id}` : "",
    ]
      .filter(Boolean)
      .join(" • ");

    addOpenClawChatMessage("assistant", reply, meta);
    updateOpenClawObservability(response);
    setStatus("openclaw respondeu");
  } catch (error) {
    addOpenClawChatMessage("system", `Erro no OpenClaw: ${error.message}`);
    setStatus("erro openclaw", "error");
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
  const source = openclawModelSource.value || "cloud";
  openclawCloudPicker.classList.toggle("hidden", source !== "cloud");
  openclawLocalPicker.classList.toggle("hidden", source !== "local");
}

async function loadOpenClawModelCatalog() {
  try {
    const payload = await fetchJson("/openclaw/models");
    openclawModelsCatalog = {
      cloud_models: Array.isArray(payload.cloud_models) ? payload.cloud_models : [],
      local_models: Array.isArray(payload.local_models) ? payload.local_models : [],
      current: payload.current || null,
    };

    renderOpenClawModelSelectors();
    openclawConfigFeedback.textContent = "Modelos carregados.";
  } catch (error) {
    openclawConfigFeedback.textContent = `Erro ao carregar modelos: ${error.message}`;
  }
}

async function applyOpenClawModelSelection() {
  const source = openclawModelSource.value || "cloud";
  openclawConfigFeedback.textContent = "Aplicando modelo...";
  setStatus("aplicando modelo openclaw", "running");

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
    const current = await fetchJson("/openclaw/model", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });

    openclawModelsCatalog.current = current;
    renderOpenClawModelSelectors();
    openclawConfigFeedback.textContent = `Modelo aplicado: ${current.label}`;
    setStatus("modelo openclaw atualizado");
  } catch (error) {
    openclawConfigFeedback.textContent = `Falha ao aplicar modelo: ${error.message}`;
    setStatus("erro modelo openclaw", "error");
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

  if (!openclawStatusLoaded) {
    void loadOpenClawStatus();
  }

  void loadOpenClawModelCatalog();
}

function switchTab(nextTab) {
  activeTab = nextTab;

  if (nextTab !== "chat") {
    setEditMode(null);
  }

  tabButtons.forEach((button) => {
    const active = button.dataset.tab === nextTab;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", active ? "true" : "false");
  });

  panelChat.classList.toggle("active", nextTab === "chat");
  panelDiscover.classList.toggle("active", nextTab === "discover");
  panelOpenClaw.classList.toggle("active", nextTab === "openclaw");

  appShell.classList.toggle("chat-mode", nextTab === "chat");
  chatModelSwitcher.classList.toggle("hidden", nextTab !== "chat");

  if (nextTab === "discover") {
    void searchCatalogModels();
    void loadDownloads();
  }

  if (nextTab === "openclaw") {
    onOpenClawTabSelected();
  } else {
    stopOpenClawLogPolling();
  }
}

saveUrlBtn.addEventListener("click", () => {
  daemonBaseUrl = daemonInput.value.trim().replace(/\/$/, "");
  localStorage.setItem(STORAGE_DAEMON_URL, daemonBaseUrl);
  openclawStatusLoaded = false;
  resetOpenClawLogState();
  setStatus("url salva");
  void bootstrap();
});

newChatThreadBtn.addEventListener("click", () => {
  if (isGenerating) {
    addSystemMessage("Pare a geracao atual antes de iniciar nova conversa.");
    return;
  }

  setEditMode(null);
  const thread = createThread({ modelId: selectedModelId });
  chatThreads.unshift(thread);
  activeThreadId = thread.id;
  persistThreads();
  renderThreadList();
  rebuildChatFromThread();
  messageInput.focus();
});

refreshModelsBtn.addEventListener("click", () => {
  void loadModels();
});

chatModelSelect.addEventListener("change", () => {
  const modelId = chatModelSelect.value;
  if (!modelId) {
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

tabButtons.forEach((button) => {
  button.addEventListener("click", () => {
    switchTab(button.dataset.tab);
  });
});

refreshOpenclawStatusBtn.addEventListener("click", () => {
  void loadOpenClawStatus();
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

async function bootstrap() {
  try {
    loadThreads();
    ensureActiveThread();

    renderOpenClawViews();
    clearChipList(openclawSkills, "Nenhuma skill reportada");
    clearChipList(openclawTools, "Nenhuma tool reportada");

    renderThreadList();
    rebuildChatFromThread();
    setEditMode(null);

    await loadModels();
    await loadCatalogSources();
    await searchCatalogModels();
    await loadDownloads();

    if (downloadsTimer) {
      window.clearInterval(downloadsTimer);
    }

    downloadsTimer = window.setInterval(() => {
      void loadDownloads();
    }, 2000);

    switchTab(activeTab);
    setStatus("pronto");
  } catch (error) {
    setStatus("erro inicial", "error");
    addSystemMessage(`Falha na inicializacao: ${error.message}`);
  }
}

void bootstrap();
