import {
  clone,
  uid,
  element,
  template,
  palettes,
  previewHtml,
  sourceHtml,
  parseSource,
  safeCss,
  validateLayout,
  extractJson,
  aiContext,
  escapeHtml,
} from "./document.js";
import { exportPdf, buildPptx, exportHtml, download } from "./export.js";
import "./studio.css";

const root = document.getElementById("presentation-studio");
const state = {
  doc: null,
  index: 0,
  selected: null,
  undo: [],
  redo: [],
  dirty: false,
  busy: false,
  saveTimer: null,
  savePromise: null,
  chatController: null,
  codeMode: "html",
  play: false,
  loadSequence: 0,
};
const $ = (id) => root.querySelector(`#ps-${id}`);
const url = () =>
  window.__MLX_PILOT_DAEMON_URL__ ||
  localStorage.getItem("mlxPilotDaemonUrl") ||
  "http://127.0.0.1:11435";
async function api(path, options = {}) {
  const r = await fetch(url() + path, {
    ...options,
    headers: { "Content-Type": "application/json", ...options.headers },
  });
  const v = await r.json();
  if (!r.ok) {
    const e = new Error(v.error || `HTTP ${r.status}`);
    e.status = r.status;
    throw e;
  }
  return v;
}
function status(text, error = false) {
  $("status").textContent = text;
  $("status").classList.toggle("error", error);
}
function report(e) {
  status(e.message || String(e), true);
}
function slide() {
  return state.doc?.slides[state.index];
}
function selected() {
  return slide()?.elements.find((e) => e.id === state.selected);
}
function remember() {
  state.undo.push(clone(state.doc));
  if (state.undo.length > 20) state.undo.shift();
  state.redo = [];
}
function change(fn) {
  if (!state.doc || state.busy) return;
  remember();
  fn(state.doc);
  state.dirty = true;
  render();
  scheduleSave();
}
function scheduleSave() {
  clearTimeout(state.saveTimer);
  state.saveTimer = setTimeout(() => save().catch(report), 900);
  status("Alterações locais · salvando…");
}
async function save() {
  clearTimeout(state.saveTimer);
  if (state.savePromise) {
    await state.savePromise;
    return state.dirty ? save() : state.doc;
  }
  if (!state.doc || !state.dirty) return state.doc;
  const snapshot = clone(state.doc),
    sequence = state.loadSequence;
  state.dirty = false;
  state.savePromise = api(`/presentations/${snapshot.id}`, {
    method: "PUT",
    body: JSON.stringify({
      base_revision: snapshot.revision,
      document: snapshot,
    }),
  });
  let saved;
  try {
    saved = await state.savePromise;
    if (sequence === state.loadSequence) {
      state.doc.revision = saved.revision;
      status(`Salvo · revisão ${saved.revision}`);
      refreshProjects().catch(report);
    }
  } catch (e) {
    state.dirty = true;
    if (e.status === 409) {
      clearTimeout(state.saveTimer);
      status(
        "Conflito: baixe o projeto local ou recarregue a versão salva antes de continuar.",
        true,
      );
    }
    throw e;
  } finally {
    state.savePromise = null;
  }
  return state.dirty && sequence === state.loadSequence ? save() : saved;
}
async function refreshProjects() {
  const docs = await api("/presentations");
  $("projects").innerHTML =
    '<option value="">Abrir apresentação…</option>' +
    docs
      .map(
        (d) =>
          `<option value="${d.id}">${escapeHtml(d.title)} · ${d.slides} slides</option>`,
      )
      .join("");
  if (state.doc) $("projects").value = state.doc.id;
}
async function load(id) {
  if (state.busy) return;
  if (state.dirty) await save();
  const sequence = ++state.loadSequence;
  const doc = await api(`/presentations/${id}`);
  if (sequence !== state.loadSequence) return;
  state.doc = doc;
  state.index = 0;
  state.selected = null;
  state.undo = [];
  state.redo = [];
  state.dirty = false;
  render();
  status(`Salvo · revisão ${doc.revision}`);
  await refreshProjects();
}
async function create() {
  if (state.busy) return;
  if (state.dirty) await save();
  const doc = await api("/presentations", {
    method: "POST",
    body: JSON.stringify({ title: "Minha apresentação" }),
  });
  state.loadSequence++;
  state.doc = doc;
  state.index = 0;
  state.selected = null;
  state.undo = [];
  state.redo = [];
  render();
  await refreshProjects();
  status("Pronta para criar. Escolha um layout ou descreva sua ideia.");
}
function source() {
  if (!state.doc) return "";
  if (state.codeMode === "html") return sourceHtml(state.doc);
  if (state.codeMode === "css") return state.doc.css;
  if (state.codeMode === "js")
    return (
      state.doc.script ||
      '// Edite o documento pela variável deck.\n// Exemplo: deck.slides[0].elements[0].text = "Novo título";\n'
    );
  return JSON.stringify(state.doc, null, 2);
}
function render() {
  if (!state.doc) return;
  state.index = Math.min(state.index, state.doc.slides.length - 1);
  $("title").value = state.doc.title;
  $("brief").value = state.doc.brief;
  $("notes").value = slide().notes || "";
  $("slide-count").textContent =
    `${state.index + 1} / ${state.doc.slides.length}`;
  $("slides").innerHTML = state.doc.slides
    .map(
      (s, i) =>
        `<button class="ps-slide-thumb ${i === state.index ? "active" : ""}" data-slide="${i}"><span>${String(i + 1).padStart(2, "0")}</span><strong>${escapeHtml(s.elements.find((e) => e.kind === "text")?.text.slice(0, 85) || "Slide")}</strong><small>${s.elements.length} elementos</small></button>`,
    )
    .join("");
  $("slides")
    .querySelectorAll("[data-slide]")
    .forEach(
      (b) =>
        (b.onclick = () => {
          state.index = +b.dataset.slide;
          state.selected = null;
          render();
        }),
    );
  $("assets").innerHTML = state.doc.assets
    .map(
      (a) =>
        `<div class="ps-asset"><img src="${escapeHtml(a.data_url)}" alt="${escapeHtml(a.name)}"><div><strong>${escapeHtml(a.name)}</strong><input aria-label="Descrição de ${escapeHtml(a.name)}" data-description="${a.id}" value="${escapeHtml(a.description)}" placeholder="Descreva a imagem para a IA"><div class="ps-asset-actions"><button data-insert="${a.id}">Inserir</button><button data-palette="${a.id}">Paleta</button><button data-remove="${a.id}" aria-label="Excluir ${escapeHtml(a.name)}">×</button></div></div></div>`,
    )
    .join("");
  $("assets")
    .querySelectorAll("[data-insert]")
    .forEach(
      (b) =>
        (b.onclick = () =>
          change((d) => {
            const e = element("image", {
              asset_id: b.dataset.insert,
              x: 660,
              y: 235,
              w: 520,
              h: 360,
              style: {},
            });
            slide().elements.push(e);
            state.selected = e.id;
          })),
    );
  $("assets")
    .querySelectorAll("[data-palette]")
    .forEach(
      (b) =>
        (b.onclick = () =>
          paletteFromImage(
            state.doc.assets.find((a) => a.id === b.dataset.palette),
          ).catch(report)),
    );
  $("assets")
    .querySelectorAll("[data-description]")
    .forEach(
      (input) =>
        (input.onchange = () =>
          change((d) => {
            d.assets.find(
              (a) => a.id === input.dataset.description,
            ).description = input.value;
          })),
    );
  $("assets")
    .querySelectorAll("[data-remove]")
    .forEach(
      (b) =>
        (b.onclick = () =>
          change((d) => {
            d.assets = d.assets.filter((a) => a.id !== b.dataset.remove);
            d.slides.forEach(
              (s) =>
                (s.elements = s.elements.filter(
                  (e) => e.asset_id !== b.dataset.remove,
                )),
            );
          })),
    );
  for (const k of ["background", "foreground", "accent", "muted"])
    $(k).value = state.doc.theme[k];
  $("font").value = state.doc.theme.font;
  $("theme").value =
    Object.keys(palettes).find((key) =>
      Object.keys(palettes[key]).every(
        (k) => palettes[key][k] === state.doc.theme[k],
      ),
    ) || "custom";
  $("code").value = source();
  $("undo").disabled = !state.undo.length;
  $("redo").disabled = !state.redo.length;
  $("transition").value = slide().transition || "none";
  renderInspector();
  renderCanvas();
}
function renderInspector() {
  const e = selected();
  $("inspector-empty").hidden = !!e;
  $("inspector").hidden = !e;
  if (!e) return;
  $("selection").textContent = `${e.kind} · ${e.id}`;
  for (const key of ["x", "y", "w", "h"]) $(key).value = e[key];
  $("text").value = e.text || "";
  $("text").closest("label").hidden = e.kind !== "text";
  $("font-size").value = e.style?.fontSize || 32;
  $("text-color").value = e.style?.color || state.doc.theme.foreground;
  $("fill").value = e.style?.fill || state.doc.theme.accent;
  $("animation").value = e.animation || "none";
  $("delay").value = e.delay_ms || 0;
  $("chart-data").value = JSON.stringify(e.data || {}, null, 2);
  $("chart-data").closest("label").hidden = !["chart", "table"].includes(
    e.kind,
  );
}
function renderCanvas() {
  const frame = $("canvas");
  frame.onload = () => {
    const fd = frame.contentDocument;
    fd.querySelectorAll("[data-id]").forEach((node) => {
      node.style.cursor = state.play ? "default" : "move";
      if (node.dataset.id === state.selected && !state.play) {
        node.style.outline = "3px solid #6e8dff";
        node.style.outlineOffset = "-3px";
      }
      node.addEventListener("pointerdown", (event) => startDrag(event, node));
      node.addEventListener("dblclick", () => {
        $("text").focus();
        $("text").select();
      });
    });
    if (selected() && !state.play) {
      const e = selected(),
        handle = fd.createElement("div");
      handle.className = "resize-handle";
      handle.style.cssText = `position:absolute;left:${e.x + e.w - 10}px;top:${e.y + e.h - 10}px;width:20px;height:20px;background:#baf66d;border:2px solid #101b2d;border-radius:50%;cursor:nwse-resize;z-index:1000`;
      fd.querySelector(".slide").append(handle);
      handle.addEventListener("pointerdown", (event) =>
        startDrag(event, handle, true),
      );
    }
    $("diagnostics").textContent =
      validateLayout(state.doc, fd)
        .map((d) => `${d.slide_id} / ${d.element_id}: ${d.message}`)
        .join("\n") || "Layout sem cortes detectados no slide atual.";
    scaleCanvas();
  };
  frame.srcdoc = previewHtml(slide(), state.doc, state.play);
}
function scaleCanvas() {
  const viewport = $("viewport");
  const scale = Math.min(
    (viewport.clientWidth - 32) / 1280,
    (viewport.clientHeight - 32) / 720,
  );
  $("canvas-wrap").style.width = `${1280 * scale}px`;
  $("canvas-wrap").style.height = `${720 * scale}px`;
  $("canvas").style.transform = `scale(${scale})`;
}
function startDrag(event, node, resize = false) {
  if (state.play || state.busy || event.button !== 0) return;
  event.preventDefault();
  node.setPointerCapture?.(event.pointerId);
  const id = resize ? state.selected : node.dataset.id;
  const e = slide().elements.find((e) => e.id === id);
  if (!e) return;
  state.selected = id;
  renderInspector();
  const initial = { x: e.x, y: e.y, w: e.w, h: e.h },
    start = { x: event.clientX, y: event.clientY };
  let moved = false;
  const fd = $("canvas").contentDocument;
  const target = fd.querySelector(`[data-id="${id}"]`);
  // Pointer coordinates within the iframe are already in document pixels.
  const move = (ev) => {
    if (!moved) {
      remember();
      moved = true;
    }
    const dx = ev.clientX - start.x,
      dy = ev.clientY - start.y;
    if (resize) {
      e.w = Math.max(
        30,
        Math.min(1280 - e.x, Math.round((initial.w + dx) / 4) * 4),
      );
      e.h = Math.max(
        24,
        Math.min(720 - e.y, Math.round((initial.h + dy) / 4) * 4),
      );
    } else {
      e.x = Math.max(
        0,
        Math.min(1280 - e.w, Math.round((initial.x + dx) / 4) * 4),
      );
      e.y = Math.max(
        0,
        Math.min(720 - e.h, Math.round((initial.y + dy) / 4) * 4),
      );
    }
    Object.assign(target.style, {
      left: e.x + "px",
      top: e.y + "px",
      width: e.w + "px",
      height: e.h + "px",
    });
    renderInspector();
  };
  const up = () => {
    fd.removeEventListener("pointermove", move);
    fd.removeEventListener("pointerup", up);
    fd.removeEventListener("pointercancel", up);
    if (moved) {
      state.dirty = true;
      scheduleSave();
    }
    renderCanvas();
    $("code").value = source();
  };
  fd.addEventListener("pointermove", move);
  fd.addEventListener("pointerup", up);
  fd.addEventListener("pointercancel", up);
}
async function addImages(files) {
  for (const file of files) {
    if (
      !["image/png", "image/jpeg", "image/webp"].includes(file.type) ||
      file.size > 5 * 1024 * 1024
    )
      throw new Error("Use PNG, JPEG ou WebP com até 5 MB.");
    const data = await new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result);
      reader.onerror = reject;
      reader.readAsDataURL(file);
    });
    const img = new Image();
    img.src = data;
    await img.decode();
    if (img.naturalWidth * img.naturalHeight > 40_000_000)
      throw new Error("Imagem muito grande; limite de 40 megapixels.");
    change((d) =>
      d.assets.push({
        id: uid("asset"),
        name: file.name,
        data_url: data,
        description: file.name.replace(/\.[^.]+$/, ""),
      }),
    );
  }
  await save();
}
async function paletteFromImage(asset) {
  const img = new Image();
  img.src = asset.data_url;
  await img.decode();
  const canvas = document.createElement("canvas");
  canvas.width = 80;
  canvas.height = 80;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(img, 0, 0, 80, 80);
  const pixels = ctx.getImageData(0, 0, 80, 80).data,
    counts = new Map();
  for (let i = 0; i < pixels.length; i += 4) {
    if (pixels[i + 3] < 128) continue;
    const rgb = [0, 1, 2].map((k) =>
      Math.min(255, Math.round(pixels[i + k] / 32) * 32),
    );
    const hex = "#" + rgb.map((v) => v.toString(16).padStart(2, "0")).join("");
    counts.set(hex, (counts.get(hex) || 0) + 1);
  }
  const colors = [...counts].sort((a, b) => b[1] - a[1]).map(([c]) => c);
  if (!colors.length) throw new Error("Imagem transparente sem cores.");
  const accent =
    colors.find((c) => {
      const rgb = c.match(/[a-f\d]{2}/gi).map((v) => parseInt(v, 16));
      return Math.max(...rgb) - Math.min(...rgb) > 45;
    }) || colors[0];
  change((d) => {
    d.theme.accent = accent;
  });
  status(
    `Cor de destaque extraída: ${accent}. Ajuste as demais cores no tema.`,
  );
}
async function importPalette(file) {
  const text = await file.text(),
    colors = [...new Set(text.match(/#[0-9a-fA-F]{6}\b/g) || [])];
  if (colors.length < 2)
    throw new Error("A paleta precisa de pelo menos duas cores #RRGGBB.");
  change((d) => {
    ["background", "foreground", "accent", "muted"].forEach((k, i) => {
      if (colors[i]) d.theme[k] = colors[i];
    });
  });
}
async function applyCode() {
  const text = $("code").value;
  if (state.codeMode === "js") {
    await runScript(text);
    return;
  }
  let next = clone(state.doc);
  if (state.codeMode === "html") next = parseSource(text, next);
  if (state.codeMode === "css") next.css = safeCss(text);
  if (state.codeMode === "json") {
    next = JSON.parse(text);
    if (next.id !== state.doc.id)
      throw new Error("O id do projeto não pode mudar.");
  }
  // Validate server-side before replacing the editor; invalid source leaves the last good document intact.
  await save();
  next.revision = state.doc.revision;
  const saved = await api(`/presentations/${state.doc.id}`, {
    method: "PUT",
    body: JSON.stringify({ base_revision: state.doc.revision, document: next }),
  });
  remember();
  state.doc = saved;
  state.dirty = false;
  render();
  status(`Código aplicado · revisão ${saved.revision}`);
}
async function runScript(code) {
  if (code.length > 64000) throw new Error("JavaScript muito grande.");
  await save();
  const base = state.doc.revision,
    id = state.doc.id,
    token = uid("run");
  const frame = document.createElement("iframe");
  frame.setAttribute("sandbox", "allow-scripts");
  frame.hidden = true;
  const payload = JSON.stringify({ deck: state.doc, code, token }).replace(
    /</g,
    "\\u003c",
  );
  const script = `const p=${payload};const source='onmessage = ({data}) => { try { let deck = data.deck; '+p.code+'; postMessage({deck}); } catch(e) { postMessage({error:String(e.message||e)}); } };';const worker=new Worker(URL.createObjectURL(new Blob([source],{type:'text/javascript'})));worker.onmessage=e=>{parent.postMessage({...e.data,token:p.token},'*');worker.terminate();};worker.onerror=e=>parent.postMessage({error:e.message,token:p.token},'*');worker.postMessage({deck:p.deck});`;
  frame.srcdoc = `<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; worker-src blob:; connect-src 'none';"><script>${script}<\/script>`;
  const result = await new Promise((resolve, reject) => {
    const timeout = setTimeout(
      () => finish(new Error("JavaScript excedeu 2 segundos.")),
      2000,
    );
    function receive(e) {
      if (e.source !== frame.contentWindow || e.data?.token !== token) return;
      finish(e.data.error ? new Error(e.data.error) : null, e.data.deck);
    }
    function finish(err, deck) {
      clearTimeout(timeout);
      window.removeEventListener("message", receive);
      frame.remove();
      err ? reject(err) : resolve(deck);
    }
    window.addEventListener("message", receive);
    document.body.append(frame);
  });
  if (result.id !== id || state.doc.id !== id || state.doc.revision !== base)
    throw new Error("O projeto mudou durante a execução. Tente novamente.");
  result.script = code;
  const saved = await api(`/presentations/${id}`, {
    method: "PUT",
    body: JSON.stringify({ base_revision: base, document: result }),
  });
  remember();
  state.doc = saved;
  render();
  status(`JavaScript aplicado · revisão ${saved.revision}`);
}
function chatMessage(text, who = "assistant") {
  const p = document.createElement("p");
  p.className = `ps-message ${who}`;
  p.textContent = text;
  $("messages").append(p);
  $("messages").scrollTop = $("messages").scrollHeight;
}
async function sendChat() {
  const prompt = $("prompt").value.trim(),
    model = $("model").value;
  if (!prompt || !state.doc) return;
  if (!model) throw new Error("Selecione um modelo local disponível.");
  await save();
  const projectId = state.doc.id,
    base = state.doc.revision;
  state.busy = true;
  $("send").disabled = true;
  $("cancel").hidden = false;
  state.chatController = new AbortController();
  chatMessage(prompt, "user");
  $("prompt").value = "";
  try {
    const { instructions } = await api("/presentations/instructions");
    const context = aiContext(state.doc, state.index);
    let response;
    const messages = [
      { role: "system", content: instructions },
      { role: "user", content: JSON.stringify({ request: prompt, context }) },
    ];
    for (let attempt = 0; attempt < 2; attempt++) {
      status("Modelo local preparando alterações…");
      response = await api("/chat", {
        method: "POST",
        signal: state.chatController.signal,
        body: JSON.stringify({
          model_id: model,
          messages,
          options: {
            temperature: 0.25,
            max_tokens: 4096,
            context_window: 8192,
            thinking: false,
          },
        }),
      });
      try {
        const parsed = extractJson(response.message?.content || "");
        if (
          state.doc.id !== projectId ||
          state.doc.revision !== base ||
          state.dirty
        )
          throw new Error("O documento mudou. Reenvie o pedido após salvar.");
        const saved = await api(`/presentations/${projectId}/edit`, {
          method: "POST",
          signal: state.chatController.signal,
          body: JSON.stringify({
            base_revision: base,
            operations: parsed.operations,
          }),
        });
        remember();
        state.doc = saved;
        render();
        chatMessage(parsed.message || "Alterações aplicadas.");
        status(`Salvo · revisão ${saved.revision}`);
        return;
      } catch (e) {
        if (
          e.status === 409 ||
          attempt === 1 ||
          state.chatController.signal.aborted
        )
          throw e;
        messages.push(
          { role: "assistant", content: response.message?.content || "" },
          {
            role: "user",
            content: `Corrija a resposta anterior. Erro: ${e.message}. Retorne apenas o JSON das operações válidas.`,
          },
        );
      }
    }
  } catch (e) {
    if (e.name === "AbortError") {
      chatMessage(
        "Pedido cancelado. Recarregue se uma alteração já tiver sido salva.",
      );
    } else {
      chatMessage(e.message);
      throw e;
    }
  } finally {
    state.busy = false;
    $("send").disabled = false;
    $("cancel").hidden = true;
    state.chatController = null;
  }
}
async function models() {
  try {
    const groups = await api("/models/all");
    $("model").innerHTML = '<option value="">Selecione o modelo local</option>';
    for (const group of groups) {
      if (group.kind === "cloud") continue;
      for (const model of group.models || []) {
        const opt = document.createElement("option");
        opt.value = model.id;
        opt.textContent = model.label || model.id;
        $("model").append(opt);
      }
    }
    const preferred = [...$("model").options].find((o) =>
      o.value.includes("qwen3.5:9b"),
    );
    if (preferred) $("model").value = preferred.value;
    else if ($("model").options.length > 1) $("model").selectedIndex = 1;
  } catch (e) {
    status(
      "Daemon indisponível. Inicie o MLX-Pilot para abrir projetos.",
      true,
    );
  }
}
async function exportFile(format) {
  if (format !== "json") await save();
  const doc = clone(state.doc);
  state.busy = true;
  $("export").disabled = true;
  try {
    const progress = (n) =>
      status(`Exportando ${format.toUpperCase()} · ${Math.round(n * 100)}%`);
    let blob,
      ext = format;
    const name =
      doc.title.replace(/[^\p{L}\p{N} _-]/gu, "").slice(0, 80) ||
      "apresentacao";
    if (format === "pdf") blob = await exportPdf(doc, progress);
    if (format === "pptx") {
      const pptx = await buildPptx(doc, {
        raster: $("raster").checked,
        onProgress: progress,
      });
      blob = await pptx.write({ outputType: "blob" });
    }
    if (format === "html") blob = exportHtml(doc);
    if (format === "json")
      blob = new Blob([JSON.stringify(doc, null, 2)], {
        type: "application/json",
      });
    download(blob, `${name}.${ext}`);
    status(`${format.toUpperCase()} exportado · revisão ${doc.revision}`);
  } finally {
    state.busy = false;
    $("export").disabled = false;
  }
}
function undo(redo = false) {
  const from = redo ? state.redo : state.undo,
    to = redo ? state.undo : state.redo;
  if (!from.length || state.busy) return;
  const revision = state.doc.revision;
  to.push(clone(state.doc));
  state.doc = from.pop();
  state.doc.revision = revision;
  state.dirty = true;
  state.selected = null;
  render();
  scheduleSave();
}
function button(id, fn) {
  $(id).addEventListener("click", () => {
    if (state.busy && id !== "cancel") return;
    Promise.resolve().then(fn).catch(report);
  });
}
async function present() {
  await save();
  const dialog = document.createElement("dialog");
  dialog.className = "ps-player";
  const frame = document.createElement("iframe");
  frame.title = state.doc.title;
  frame.onload = () => frame.focus();
  frame.setAttribute("sandbox", "allow-scripts");
  frame.setAttribute("allow", "fullscreen");
  frame.srcdoc = await exportHtml(state.doc).text();
  const close = document.createElement("button");
  close.textContent = "✕ Fechar";
  close.onclick = () => dialog.close();
  dialog.append(frame, close);
  document.body.append(dialog);
  dialog.addEventListener("close", () => dialog.remove(), { once: true });
  dialog.showModal();
  frame.focus();
}

if (root) {
  root.innerHTML = `<div class="ps-shell">
 <header class="ps-header"><div class="ps-brand"><span class="ps-brand-icon">▧</span><div><strong>Presentation Studio</strong><small>Ideias, em movimento.</small></div></div><input id="ps-title" aria-label="Título da apresentação" placeholder="Minha apresentação"><select id="ps-projects" aria-label="Abrir apresentação"></select><button id="ps-new" class="ps-primary">+ Nova</button><button id="ps-reload" title="Recarregar versão salva">↻</button></header>
 <div class="ps-toolbar"><button id="ps-undo" title="Desfazer">↶</button><button id="ps-redo" title="Refazer">↷</button><span class="ps-divider"></span><select id="ps-layout" aria-label="Layout do novo slide"><option value="title">Capa</option><option value="content">Conteúdo</option><option value="comparison">Comparação</option><option value="image">Imagem</option><option value="chart">Gráfico</option><option value="table">Tabela</option><option value="closing">Encerramento</option></select><button id="ps-add-slide">+ Slide</button><button id="ps-add-text">Texto</button><button id="ps-add-shape">Forma</button><label class="ps-upload">＋ Imagens<input id="ps-upload" type="file" accept="image/png,image/jpeg,image/webp" multiple hidden></label><span class="ps-spacer"></span><button id="ps-play">▶ Apresentar</button><select id="ps-format" aria-label="Formato de exportação"><option value="pdf">PDF</option><option value="pptx">PPTX editável</option><option value="html">HTML animado</option><option value="json">Projeto JSON</option></select><button id="ps-export" class="ps-primary">Exportar ↓</button></div>
 <div class="ps-body"><aside class="ps-slides"><div class="ps-eyebrow">SLIDES <span id="ps-slide-count"></span></div><div id="ps-slides"></div><div class="ps-slide-actions"><button id="ps-move-up" title="Mover slide para cima">↑</button><button id="ps-move-down" title="Mover slide para baixo">↓</button><button id="ps-duplicate" title="Duplicar slide">Duplicar</button><button id="ps-delete-slide" title="Excluir slide">×</button></div><label class="ps-upload ps-import">Importar projeto<input id="ps-import-project" type="file" accept="application/json,.json" hidden></label></aside>
 <main class="ps-center"><div id="ps-viewport" class="ps-viewport"><div id="ps-canvas-wrap"><iframe id="ps-canvas" sandbox="allow-same-origin" title="Slide editável"></iframe></div><div class="ps-canvas-hint">Clique para selecionar · arraste para mover · canto verde para redimensionar</div></div><div class="ps-note-row"><label>Notas do apresentador<textarea id="ps-notes" rows="2" placeholder="Contexto, fontes e o que você quer dizer…"></textarea></label></div>
 <details class="ps-code"><summary>Código e validação <span>HTML · CSS · JavaScript · JSON</span></summary><div class="ps-code-bar"><select id="ps-code-mode" aria-label="Linguagem"><option value="html">HTML</option><option value="css">CSS</option><option value="js">JavaScript</option><option value="json">Documento JSON</option></select><button id="ps-apply-code">Aplicar código</button><button id="ps-validate">Validar</button><button id="ps-history">Histórico</button></div><textarea id="ps-code" spellcheck="false" aria-label="Código da apresentação"></textarea><pre id="ps-diagnostics"></pre></details></main>
 <aside class="ps-side"><div class="ps-side-tabs"><button data-side="design" class="active">Design</button><button data-side="chat">Chat</button><button data-side="assets">Imagens</button><button data-side="plugins">Plugins</button></div>
 <div id="ps-side-design" class="ps-side-content"><div class="ps-eyebrow">DIREÇÃO VISUAL</div><label>Briefing e animações<textarea id="ps-brief" rows="3" placeholder="Público, referências, tom, cores e movimentos desejados…"></textarea></label><label>Tema<select id="ps-theme"><option value="custom" disabled>Personalizado</option><option value="orbital">Orbital</option><option value="paper">Editorial</option><option value="ocean">Oceano</option><option value="plum">Ameixa</option></select></label><div class="ps-colors">${["background", "foreground", "accent", "muted"].map((k, i) => `<label>${["Fundo", "Texto", "Destaque", "Apoio"][i]}<input id="ps-${k}" type="color"></label>`).join("")}</div><label>Fonte<select id="ps-font">${["Arial", "Georgia", "Verdana", "Helvetica", "Courier New"].map((f) => `<option>${f}</option>`).join("")}</select></label><label class="ps-upload">Importar paleta<input id="ps-palette-file" type="file" accept=".json,.txt,.css" hidden></label><label>Transição do slide<select id="ps-transition"><option value="none">Nenhuma</option><option value="fade">Fade</option><option value="slide">Deslizar</option></select></label><p class="ps-help">Animações no HTML. PDF e PPTX são estáticos.</p><label class="ps-checkbox"><input id="ps-raster" type="checkbox">PPTX como imagens (preserva CSS customizado)</label>
 <hr><div id="ps-inspector-empty" class="ps-help">Selecione um elemento no slide para editar.</div><div id="ps-inspector" hidden><div id="ps-selection" class="ps-eyebrow"></div><label>Texto<textarea id="ps-text" rows="4"></textarea></label><div class="ps-geometry">${["x", "y", "w", "h"].map((k) => `<label>${{ x: "X", y: "Y", w: "Largura", h: "Altura" }[k]}<input id="ps-${k}" type="number" min="0" max="1280"></label>`).join("")}</div><div class="ps-geometry"><label>Tamanho<input id="ps-font-size" type="number" min="10" max="160"></label><label>Cor<input id="ps-text-color" type="color"></label><label>Preenchimento<input id="ps-fill" type="color"></label></div><div class="ps-inline"><button id="ps-bold"><b>B</b></button><button id="ps-align-left">←</button><button id="ps-align-center">↔</button><button id="ps-align-right">→</button><button id="ps-front">Frente</button><button id="ps-delete-element">Excluir</button></div><label>Animação<select id="ps-animation">${["none", "fade", "rise", "slide", "zoom"].map((a) => `<option>${a}</option>`).join("")}</select></label><label>Atraso (ms)<input id="ps-delay" type="number" min="0" max="10000" step="100"></label><label>Dados (JSON)<textarea id="ps-chart-data" rows="5"></textarea></label></div></div>
 <div id="ps-side-chat" class="ps-side-content" hidden><div class="ps-eyebrow">SEU COPILOTO LOCAL</div><select id="ps-model" aria-label="Modelo local"><option>Carregando modelos…</option></select><button id="ps-refresh-models">Atualizar modelos</button><p class="ps-help">A IA recebe o slide atual, briefing, paleta e descrições das imagens. Descreva o que importa nas referências.</p><div id="ps-messages"><p class="ps-message">Conte sua ideia ou selecione um slide e peça uma mudança. Ex.: “Crie mais 3 slides sobre este tema, use a paleta atual e entradas suaves.”</p></div><textarea id="ps-prompt" rows="4" placeholder="O que vamos criar ou melhorar?" aria-label="Pedido para a IA"></textarea><div class="ps-inline"><button id="ps-send" class="ps-primary">Enviar ↑</button><button id="ps-cancel" hidden>Cancelar</button></div></div>
 <div id="ps-side-assets" class="ps-side-content" hidden><div class="ps-eyebrow">REFERÊNCIAS E IMAGENS</div><p class="ps-help">Arraste imagens para o editor ou use “＋ Imagens”. Adicione uma descrição para orientar modelos sem visão. Extraia uma cor de destaque com “Paleta”.</p><div id="ps-assets"></div></div>
 <div id="ps-side-plugins" class="ps-side-content" hidden><div class="ps-eyebrow">PLUGINS WASM</div><p class="ps-help">Pacotes locais, com permissões explícitas e execução limitada.</p><div id="ps-plugins"></div><label class="ps-upload">Instalar pacote .mlxplugin.json<input id="ps-install-plugin" type="file" accept=".json" hidden></label><pre id="ps-plugin-review"></pre><button id="ps-confirm-plugin" hidden>Conceder permissões e instalar</button></div>
 </aside></div><footer class="ps-footer"><span id="ps-status" role="status">Conectando ao daemon…</span><span>1280 × 720 · 16:9 · local</span></footer></div>`;
  button("new", create);
  button("reload", async () => {
    if (!state.doc) return;
    if (
      state.dirty &&
      !window.confirm(
        "Descartar alterações locais e recarregar? Exporte JSON primeiro para guardar uma cópia.",
      )
    )
      return;
    clearTimeout(state.saveTimer);
    state.dirty = false;
    await load(state.doc.id);
  });
  $("projects").onchange = () => {
    if ($("projects").value) load($("projects").value).catch(report);
  };
  // Keep text drafts in the document before another control or async render updates the UI.
  function draft(id, apply) {
    let grouped = false;
    $(id).oninput = () => {
      if (!state.doc || state.busy) return;
      if (!grouped) {
        remember();
        grouped = true;
      }
      apply($(id).value);
      state.dirty = true;
      scheduleSave();
    };
    $(id).onblur = () => {
      grouped = false;
    };
  }
  draft("title", (value) => {
    state.doc.title = value;
  });
  draft("brief", (value) => {
    state.doc.brief = value;
  });
  draft("notes", (value) => {
    slide().notes = value;
  });
  button("add-slide", () =>
    change((d) => {
      d.slides.push(template($("layout").value, d.slides.length + 1));
      state.index = d.slides.length - 1;
      state.selected = null;
    }),
  );
  button("add-text", () =>
    change(() => {
      const e = element();
      slide().elements.push(e);
      state.selected = e.id;
    }),
  );
  button("add-shape", () =>
    change((d) => {
      const e = element("shape", {
        w: 300,
        h: 180,
        style: { fill: d.theme.accent, radius: 12 },
      });
      slide().elements.push(e);
      state.selected = e.id;
    }),
  );
  button("delete-slide", () => {
    if (state.doc.slides.length < 2)
      throw new Error("Mantenha pelo menos um slide.");
    change((d) => {
      d.slides.splice(state.index, 1);
      state.selected = null;
    });
  });
  button("duplicate", () =>
    change((d) => {
      const copy = clone(slide());
      copy.id = uid("slide");
      copy.elements.forEach((e) => (e.id = uid(e.kind)));
      d.slides.splice(state.index + 1, 0, copy);
      state.index++;
      state.selected = null;
    }),
  );
  for (const [id, delta] of [
    ["move-up", -1],
    ["move-down", 1],
  ])
    button(id, () => {
      const to = state.index + delta;
      if (to < 0 || to >= state.doc.slides.length) return;
      change((d) => {
        const s = d.slides.splice(state.index, 1)[0];
        d.slides.splice(to, 0, s);
        state.index = to;
      });
    });
  button("undo", () => undo());
  button("redo", () => undo(true));
  for (const k of ["background", "foreground", "accent", "muted"])
    $(k).onchange = () => change((d) => (d.theme[k] = $(k).value));
  $("font").onchange = () => change((d) => (d.theme.font = $("font").value));
  $("theme").onchange = () =>
    change((d) => (d.theme = clone(palettes[$("theme").value])));
  $("transition").onchange = () =>
    change(() => (slide().transition = $("transition").value));
  for (const k of ["x", "y", "w", "h"])
    $(k).onchange = () => {
      const n = Number($(k).value);
      if (Number.isFinite(n))
        change(() => {
          if (selected()) selected()[k] = n;
        });
    };
  $("text").onchange = () =>
    change(() => {
      if (selected()) selected().text = $("text").value;
    });
  for (const [id, key] of [
    ["font-size", "fontSize"],
    ["text-color", "color"],
    ["fill", "fill"],
  ])
    $(id).onchange = () =>
      change(() => {
        const e = selected();
        if (e)
          e.style = {
            ...e.style,
            [key]: id === "font-size" ? Number($(id).value) : $(id).value,
          };
      });
  $("animation").onchange = () =>
    change(() => {
      if (selected()) selected().animation = $("animation").value;
    });
  $("delay").onchange = () =>
    change(() => {
      if (selected()) selected().delay_ms = Number($("delay").value);
    });
  $("chart-data").onchange = () => {
    try {
      const data = JSON.parse($("chart-data").value);
      change(() => {
        if (selected()) selected().data = data;
      });
    } catch (e) {
      report(e);
    }
  };
  button("bold", () =>
    change(() => {
      const e = selected();
      if (e) e.style = { ...e.style, bold: !e.style?.bold };
    }),
  );
  for (const a of ["left", "center", "right"])
    button(`align-${a}`, () =>
      change(() => {
        const e = selected();
        if (e) e.style = { ...e.style, align: a };
      }),
    );
  button("front", () =>
    change(() => {
      const e = selected();
      if (e) {
        slide().elements = slide().elements.filter((x) => x !== e);
        slide().elements.push(e);
      }
    }),
  );
  button("delete-element", () =>
    change(() => {
      slide().elements = slide().elements.filter(
        (e) => e.id !== state.selected,
      );
      state.selected = null;
    }),
  );
  $("upload").onchange = (e) =>
    addImages([...e.target.files])
      .catch(report)
      .finally(() => (e.target.value = ""));
  root.addEventListener("dragover", (e) => {
    e.preventDefault();
  });
  root.addEventListener("drop", (e) => {
    e.preventDefault();
    if (e.dataTransfer.files.length)
      addImages([...e.dataTransfer.files]).catch(report);
  });
  $("palette-file").onchange = (e) => {
    if (e.target.files[0]) importPalette(e.target.files[0]).catch(report);
    e.target.value = "";
  };
  $("import-project").onchange = async (e) => {
    if (state.busy) {
      e.target.value = "";
      return;
    }
    try {
      const file = e.target.files[0];
      if (!file) return;
      if (file.size > 24 * 1024 * 1024)
        throw new Error("Projeto maior que 24 MB.");
      const doc = JSON.parse(await file.text());
      await create();
      doc.id = state.doc.id;
      doc.revision = state.doc.revision;
      const saved = await api(`/presentations/${doc.id}`, {
        method: "PUT",
        body: JSON.stringify({ base_revision: doc.revision, document: doc }),
      });
      state.doc = saved;
      render();
      await refreshProjects();
    } catch (err) {
      report(err);
    } finally {
      e.target.value = "";
    }
  };
  $("code-mode").onchange = () => {
    state.codeMode = $("code-mode").value;
    $("code").value = source();
  };
  button("apply-code", async () => {
    state.busy = true;
    try {
      await applyCode();
    } finally {
      state.busy = false;
    }
  });
  button("send", sendChat);
  button("cancel", () => state.chatController?.abort());
  button("refresh-models", models);
  button("validate", async () => {
    await save();
    const r = await api(`/presentations/${state.doc.id}/validate`);
    const issues = [
      ...r.diagnostics,
      ...validateLayout(state.doc, $("canvas").contentDocument),
    ];
    $("diagnostics").textContent =
      issues
        .map((x) => `${x.code} · ${x.slide_id}/${x.element_id}: ${x.message}`)
        .join("\n") || "Documento válido.";
    status(`${issues.length} observações de layout`);
  });
  button("history", async () => {
    await save();
    const h = await api(`/presentations/${state.doc.id}/history`);
    const answer = window.prompt(
      `Revisão atual: ${h.current}. Revisões disponíveis: ${h.revisions.join(", ") || "nenhuma"}. Informe a revisão para restaurar:`,
    );
    if (!answer) return;
    const revision = Number(answer);
    if (!h.revisions.includes(revision))
      throw new Error("Revisão indisponível.");
    const saved = await api(`/presentations/${state.doc.id}/restore`, {
      method: "POST",
      body: JSON.stringify({ revision, base_revision: state.doc.revision }),
    });
    remember();
    state.doc = saved;
    render();
  });
  button("export", () => exportFile($("format").value));
  button("play", present);
  root.querySelectorAll("[data-side]").forEach(
    (b) =>
      (b.onclick = () => {
        root
          .querySelectorAll("[data-side]")
          .forEach((x) => x.classList.toggle("active", x === b));
        root
          .querySelectorAll(".ps-side-content")
          .forEach((p) => (p.hidden = p.id !== `ps-side-${b.dataset.side}`));
        if (b.dataset.side === "plugins") loadPlugins().catch(report);
      }),
  );
  async function loadPlugins() {
    $("plugins").innerHTML =
      '<p class="ps-help">Instale, abra, ative, desative e remova plugins na central de plugins.</p><button id="ps-open-manager">Abrir central de plugins</button>';
    $("open-manager").onclick = () =>
      document.querySelector('[data-panel="plugins"]').click();
  }
  $("install-plugin").closest("label").hidden = true;
  window.addEventListener("mlx:plugins-changed", ({ detail }) => {
    const enabled = detail.plugins.some(
      (p) => p.manifest.id === "presentation" && p.enabled,
    );
    if (!enabled) {
      clearTimeout(state.saveTimer);
      state.chatController?.abort();
    } else if (!state.doc)
      Promise.all([refreshProjects(), models()])
        .then(async () => {
          const docs = await api("/presentations");
          if (docs.length) await load(docs[0].id);
          else await create();
        })
        .catch(report);
  });
  new ResizeObserver(scaleCanvas).observe($("viewport"));
  window.addEventListener("beforeunload", (e) => {
    if (state.dirty) {
      e.preventDefault();
      e.returnValue = "";
    }
  });
  document.addEventListener("keydown", (e) => {
    if (!root.closest(".panel")?.classList.contains("active")) return;
    if ((e.ctrlKey || e.metaKey) && e.key === "s") {
      e.preventDefault();
      save().catch(report);
    }
    if (state.play && ["ArrowLeft", "ArrowRight"].includes(e.key)) {
      state.index = Math.max(
        0,
        Math.min(
          state.doc.slides.length - 1,
          state.index + (e.key === "ArrowRight" ? 1 : -1),
        ),
      );
      render();
    }
  });
  window.PresentationStudio = {
    getDocument: () => clone(state.doc),
    save,
    load,
    refreshProjects,
  };
  Promise.all([refreshProjects(), models()])
    .then(async () => {
      const docs = await api("/presentations");
      if (docs.length) await load(docs[0].id);
      else await create();
    })
    .catch(report);
}
