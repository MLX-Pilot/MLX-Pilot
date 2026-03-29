const daemonInput = document.getElementById("daemon-url");
const saveUrlBtn = document.getElementById("save-url");
const refreshModelsBtn = document.getElementById("refresh-models");
const localModelCount = document.getElementById("local-model-count");
const modelsList = document.getElementById("models-list");
const selectedModelLabel = document.getElementById("selected-model-label");
const chatForm = document.getElementById("chat-form");
const chatLog = document.getElementById("chat-log");
const messageInput = document.getElementById("message-input");
const statusPill = document.getElementById("status-pill");
const messageTemplate = document.getElementById("message-template");
const assistantStreamTemplate = document.getElementById("assistant-stream-template");

const tabButtons = document.querySelectorAll(".tab-btn");
const panelChat = document.getElementById("panel-chat");
const panelDiscover = document.getElementById("panel-discover");
const panelOpenClaw = document.getElementById("panel-openclaw");

const catalogSource = document.getElementById("catalog-source");
const catalogQuery = document.getElementById("catalog-query");
const catalogSearchBtn = document.getElementById("catalog-search-btn");
const catalogMeta = document.getElementById("catalog-meta");
const remoteResults = document.getElementById("remote-results");
const remoteCardTemplate = document.getElementById("remote-card-template");

const refreshDownloadsBtn = document.getElementById("refresh-downloads");
const downloadList = document.getElementById("download-list");
const downloadItemTemplate = document.getElementById("download-item-template");
const sendMessageBtn = chatForm.querySelector('button[type="submit"]');
const stopGenerationBtn = document.getElementById("stop-generation");

const openclawStatusText = document.getElementById("openclaw-status-text");
const refreshOpenclawStatusBtn = document.getElementById("refresh-openclaw-status");
const openclawLogStreamSelect = document.getElementById("openclaw-log-stream");
const refreshOpenclawLogBtn = document.getElementById("refresh-openclaw-log");
const clearOpenclawLogBtn = document.getElementById("clear-openclaw-log");
const openclawLogMeta = document.getElementById("openclaw-log-meta");
const openclawLogViewer = document.getElementById("openclaw-log-viewer");
const openclawChatForm = document.getElementById("openclaw-chat-form");
const openclawChatLog = document.getElementById("openclaw-chat-log");
const openclawMessageInput = document.getElementById("openclaw-message-input");
const openclawSendBtn = openclawChatForm.querySelector('button[type="submit"]');
const openclawProviderModel = document.getElementById("openclaw-provider-model");
const openclawUsage = document.getElementById("openclaw-usage");
const openclawSkills = document.getElementById("openclaw-skills");
const openclawTools = document.getElementById("openclaw-tools");

const STREAM_CHARS_PER_TICK = 22;
const STREAM_TICK_MS = 20;
const CHAT_SCROLL_THRESHOLD_PX = 72;
const OPENCLAW_LOG_POLL_MS = 1500;
const OPENCLAW_LOG_MAX_CHARS = 120000;

let daemonBaseUrl = localStorage.getItem("mlxPilotDaemonUrl") || "http://127.0.0.1:11435";
let selectedModelId = null;
let messages = [];
let localModels = [];
let lastDownloadsFingerprint = "";
let downloadsTimer = null;
let activeStreamController = null;
let isGenerating = false;
let streamFallbackNotified = false;
let openclawLogsTimer = null;
let openclawLogCursor = 0;
let openclawActiveLogStream = "gateway";
let openclawChatInFlight = false;
let openclawStatusLoaded = false;

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

function isChatNearBottom() {
  const distanceFromBottom =
    chatLog.scrollHeight - chatLog.scrollTop - chatLog.clientHeight;
  return distanceFromBottom <= CHAT_SCROLL_THRESHOLD_PX;
}

function scrollChatToBottom(force = false) {
  if (force || isChatNearBottom()) {
    chatLog.scrollTop = chatLog.scrollHeight;
  }
}

function addMessage(
  role,
  content,
  { editable = false, messageIndex = null, forceScroll = true } = {},
) {
  const node = messageTemplate.content.firstElementChild.cloneNode(true);
  node.querySelector(".message-role").textContent = role;
  node.querySelector(".message-content").textContent = content;

  const actions = node.querySelector(".message-actions");
  const editBtn = node.querySelector(".edit-message-btn");
  if (editable && Number.isInteger(messageIndex)) {
    actions.classList.remove("hidden");
    editBtn.addEventListener("click", () => {
      editMessageAndRegenerate(messageIndex);
    });
  } else {
    actions.remove();
  }

  chatLog.appendChild(node);
  scrollChatToBottom(forceScroll);
}

function rebuildChatFromMessages() {
  chatLog.innerHTML = "";

  messages.forEach((message, index) => {
    if (message.role === "user") {
      addMessage("user", message.content, {
        editable: true,
        messageIndex: index,
        forceScroll: false,
      });
      return;
    }

    addMessage(message.role, message.content, { forceScroll: false });
  });

  scrollChatToBottom(true);
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

  if (status === "answering" || status === "completed" || status === "cancelled") {
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

function setGeneratingState(nextState) {
  isGenerating = nextState;
  sendMessageBtn.disabled = nextState;
  messageInput.disabled = nextState;
  stopGenerationBtn.disabled = !nextState;
}

function isAbortError(error) {
  return (
    error?.name === "AbortError" ||
    String(error?.message || "").toLowerCase().includes("aborted")
  );
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
    // ignore body parse error
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
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(date);
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

function clearOpenClawChips(container, fallbackText) {
  container.innerHTML = "";
  const item = document.createElement("li");
  item.className = "chip-empty";
  item.textContent = fallbackText;
  container.appendChild(item);
}

function renderOpenClawChips(container, values, fallbackText) {
  container.innerHTML = "";
  if (!Array.isArray(values) || values.length === 0) {
    clearOpenClawChips(container, fallbackText);
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

function updateOpenClawMeta(response) {
  const provider = response.provider || "provider n/d";
  const model = response.model || "model n/d";
  openclawProviderModel.textContent = `${provider} • ${model}`;
  renderOpenClawUsage(response.usage);
  renderOpenClawChips(openclawSkills, response.skills, "Nenhuma skill reportada");
  renderOpenClawChips(openclawTools, response.tools, "Nenhuma tool reportada");
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

async function loadOpenClawStatus() {
  try {
    const status = await fetchJson("/openclaw/status");
    openclawStatusLoaded = true;

    if (status.available) {
      openclawStatusText.textContent = `online • session ${status.session_key}`;
      if (status.health?.result?.ok) {
        openclawStatusText.textContent += " • gateway ok";
      }
    } else {
      openclawStatusText.textContent = status.error
        ? `offline • ${status.error}`
        : "offline";
    }
  } catch (error) {
    openclawStatusText.textContent = `erro status • ${error.message}`;
  }
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
    updateOpenClawMeta(response);
    setStatus("openclaw respondeu");
  } catch (error) {
    addOpenClawChatMessage("system", `Erro no OpenClaw: ${error.message}`);
    setStatus("erro openclaw", "error");
  } finally {
    setOpenClawSendingState(false);
    openclawMessageInput.focus();
  }
}

function onOpenClawTabSelected() {
  if (!openclawStatusLoaded) {
    void loadOpenClawStatus();
  }
  startOpenClawLogPolling();
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

function renderModels(models) {
  localModels = models;
  modelsList.innerHTML = "";
  localModelCount.textContent = `${models.length} disponivel(is)`;

  if (!models.length) {
    const empty = document.createElement("li");
    empty.textContent = "Nenhum modelo local encontrado.";
    modelsList.appendChild(empty);
    selectedModelId = null;
    selectedModelLabel.textContent = "Nenhum modelo selecionado";
    return;
  }

  if (!models.some((entry) => entry.id === selectedModelId)) {
    selectedModelId = models[0].id;
  }

  models.forEach((model) => {
    const li = document.createElement("li");
    li.className = "model-item";

    const button = document.createElement("button");
    button.textContent = model.name;
    button.title = model.path;
    if (model.id === selectedModelId) {
      button.classList.add("active");
    }

    button.addEventListener("click", () => {
      selectedModelId = model.id;
      selectedModelLabel.textContent = `${model.name} (${model.provider})`;
      renderModels(localModels);
    });

    li.appendChild(button);
    modelsList.appendChild(li);
  });

  const selected = models.find((entry) => entry.id === selectedModelId);
  selectedModelLabel.textContent = selected
    ? `${selected.name} (${selected.provider})`
    : "Nenhum modelo selecionado";
}

async function loadModels() {
  setStatus("carregando modelos", "running");
  try {
    const models = await fetchJson("/models");
    renderModels(models);
    setStatus("pronto");
  } catch (error) {
    setStatus("erro modelos", "error");
    addMessage("system", `Falha ao carregar modelos: ${error.message}`);
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
        addMessage(
          "system",
          `Download iniciado para ${created.model_id}. Pasta: ${created.destination}`,
        );
      } catch (error) {
        setStatus("erro download", "error");
        addMessage("system", `Falha ao iniciar download: ${error.message}`);
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
    addMessage("system", `Falha ao cancelar download: ${error.message}`);
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
    const statusLabel = {
      queued: "queued",
      running: "running",
      cancelling: "cancelling",
      completed: "completed",
      failed: "failed",
      cancelled: "cancelled",
    };
    status.textContent = statusLabel[job.status] || job.status;
    status.classList.add(job.status);

    const when = job.finished_at || job.started_at || job.created_at;
    node.querySelector(".download-time").textContent = formatEpoch(when);

    const cancelBtn = node.querySelector(".download-cancel-btn");
    if (job.can_cancel) {
      cancelBtn.disabled = job.status === "cancelling";
      cancelBtn.textContent = job.status === "cancelling" ? "Cancelando..." : "Cancelar";
      cancelBtn.addEventListener("click", () => cancelDownload(job.id));
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

    const fingerprint = jobs
      .map((job) => `${job.id}:${job.status}:${job.finished_at || ""}`)
      .join("|");

    const completedChanged = fingerprint !== lastDownloadsFingerprint;
    lastDownloadsFingerprint = fingerprint;

    const hasRunning = jobs.some((job) =>
      ["running", "queued", "cancelling"].includes(job.status),
    );

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

function switchTab(nextTab) {
  tabButtons.forEach((button) => {
    const active = button.dataset.tab === nextTab;
    button.classList.toggle("active", active);
    button.setAttribute("aria-selected", active ? "true" : "false");
  });

  panelChat.classList.toggle("active", nextTab === "chat");
  panelDiscover.classList.toggle("active", nextTab === "discover");
  panelOpenClaw.classList.toggle("active", nextTab === "openclaw");

  if (nextTab === "discover") {
    searchCatalogModels();
    loadDownloads();
  }

  if (nextTab === "openclaw") {
    onOpenClawTabSelected();
    return;
  }

  stopOpenClawLogPolling();
}

saveUrlBtn.addEventListener("click", () => {
  daemonBaseUrl = daemonInput.value.trim().replace(/\/$/, "");
  localStorage.setItem("mlxPilotDaemonUrl", daemonBaseUrl);
  openclawStatusLoaded = false;
  resetOpenClawLogState();
  setStatus("url salva");
  bootstrap();
});

refreshModelsBtn.addEventListener("click", () => {
  loadModels();
});

catalogSearchBtn.addEventListener("click", () => {
  searchCatalogModels();
});

catalogSource.addEventListener("change", () => {
  searchCatalogModels();
});

catalogQuery.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    searchCatalogModels();
  }
});

refreshDownloadsBtn.addEventListener("click", () => {
  loadDownloads(true);
});

refreshOpenclawStatusBtn.addEventListener("click", () => {
  void loadOpenClawStatus();
});

refreshOpenclawLogBtn.addEventListener("click", () => {
  void pollOpenClawLogs({ reset: true });
});

clearOpenclawLogBtn.addEventListener("click", () => {
  resetOpenClawLogState();
});

openclawLogStreamSelect.addEventListener("change", () => {
  void pollOpenClawLogs({ reset: true });
});

openclawChatForm.addEventListener("submit", async (event) => {
  event.preventDefault();
  await sendOpenClawMessage();
});

tabButtons.forEach((button) => {
  button.addEventListener("click", () => {
    switchTab(button.dataset.tab);
  });
});

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
        addMessage(
          "system",
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
      const lastEvent = JSON.parse(buffer.trim());
      if (lastEvent.event === "done") {
        doneEvent = lastEvent;
      } else if (lastEvent.event === "metrics") {
        ui.latestMetrics = lastEvent;
      } else if (lastEvent.event === "error") {
        throw createHttpError(500, lastEvent.message || "Erro na geracao");
      }
    } catch {
      // ignore trailing non-json content
    }
  }

  if (!doneEvent) {
    throw new Error("Stream encerrado sem evento final");
  }

  await waitForAssistantFlush(ui);
  setAssistantState(ui, "completed");
  renderAssistantMetrics(ui, doneEvent || ui.latestMetrics);
}

function buildChatPayload() {
  return {
    model_id: selectedModelId,
    messages,
    options: {
      temperature: 0.2,
      max_tokens: 512,
    },
  };
}

async function runAssistantGeneration() {
  const assistantUi = createAssistantStreamCard();
  setAssistantState(assistantUi, "waiting");
  setStatus("gerando resposta", "running");

  activeStreamController = new AbortController();
  setGeneratingState(true);

  try {
    await consumeChatStream(buildChatPayload(), assistantUi, activeStreamController.signal);

    const finalAnswer = assistantUi.finalAnswer.trim();
    if (finalAnswer) {
      messages.push({ role: "assistant", content: finalAnswer });
    }

    setStatus("resposta concluida");
  } catch (error) {
    if (isAbortError(error)) {
      setAssistantState(assistantUi, "cancelled");
      assistantUi.metricsSection.classList.remove("hidden");
      assistantUi.metricsText.textContent = "Geracao interrompida pelo usuario.";
      const partialAnswer = assistantUi.finalAnswer.trim();
      if (partialAnswer) {
        messages.push({ role: "assistant", content: partialAnswer });
      }
      setStatus("geracao interrompida");
      scrollChatToBottom(true);
      return;
    }

    setAssistantState(assistantUi, "error");
    assistantUi.metricsSection.classList.remove("hidden");
    assistantUi.metricsText.textContent = `Erro: ${error.message}`;
    scrollChatToBottom(true);
    addMessage("system", `Erro no chat: ${error.message}`);
    setStatus("erro chat", "error");
  } finally {
    activeStreamController = null;
    setGeneratingState(false);
    messageInput.focus();
  }
}

async function editMessageAndRegenerate(messageIndex) {
  const entry = messages[messageIndex];
  if (!entry || entry.role !== "user") {
    return;
  }

  if (isGenerating) {
    addMessage("system", "Pare a geracao atual antes de editar uma mensagem.");
    return;
  }

  const edited = window.prompt("Editar mensagem e regenerar:", entry.content);
  if (edited === null) {
    return;
  }

  const nextContent = edited.trim();
  if (!nextContent) {
    addMessage("system", "A mensagem editada nao pode ficar vazia.");
    return;
  }

  messages = [...messages.slice(0, messageIndex), { role: "user", content: nextContent }];
  rebuildChatFromMessages();

  setStatus("regenerando a partir da mensagem editada", "running");
  await runAssistantGeneration();
}

chatForm.addEventListener("submit", async (event) => {
  event.preventDefault();

  if (isGenerating) {
    return;
  }

  if (!selectedModelId) {
    addMessage("system", "Selecione um modelo antes de enviar mensagem.");
    return;
  }

  const userText = messageInput.value.trim();
  if (!userText) {
    return;
  }

  messageInput.value = "";
  messages.push({ role: "user", content: userText });
  addMessage("user", userText, {
    editable: true,
    messageIndex: messages.length - 1,
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

async function bootstrap() {
  try {
    await loadModels();
    await loadCatalogSources();
    await searchCatalogModels();
    await loadDownloads();

    if (downloadsTimer) {
      window.clearInterval(downloadsTimer);
    }

    downloadsTimer = window.setInterval(() => {
      loadDownloads();
    }, 2000);
  } catch (error) {
    setStatus("erro inicial", "error");
    addMessage("system", `Falha na inicializacao: ${error.message}`);
  }
}

clearOpenClawChips(openclawSkills, "Nenhuma skill reportada");
clearOpenClawChips(openclawTools, "Nenhuma tool reportada");

bootstrap();
