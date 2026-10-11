export const WIDTH = 1280,
  HEIGHT = 720;
export const clone = (value) => JSON.parse(JSON.stringify(value));
export const uid = (prefix) => `${prefix}-${crypto.randomUUID().slice(0, 8)}`;
export const escapeHtml = (value) =>
  String(value ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
export const palettes = {
  orbital: {
    background: "#101b2d",
    foreground: "#f6f3eb",
    accent: "#baf66d",
    muted: "#a5b1c2",
    font: "Arial",
  },
  paper: {
    background: "#f7f4ed",
    foreground: "#263138",
    accent: "#d65636",
    muted: "#647078",
    font: "Georgia",
  },
  ocean: {
    background: "#eef7fb",
    foreground: "#112f45",
    accent: "#167caa",
    muted: "#577383",
    font: "Arial",
  },
  plum: {
    background: "#211529",
    foreground: "#f8edf5",
    accent: "#ffbca6",
    muted: "#c6abc5",
    font: "Georgia",
  },
};
export function element(kind = "text", overrides = {}) {
  return {
    id: uid(kind),
    kind,
    x: 80,
    y: 170,
    w: 1100,
    h: 180,
    text: kind === "text" ? "Escreva sua ideia" : "",
    style: { fontSize: 32 },
    animation: "none",
    delay_ms: 0,
    data: null,
    asset_id: null,
    ...overrides,
  };
}
export function template(layout = "title", index = 1) {
  const title = element("text", {
    y: 80,
    h: 120,
    text:
      layout === "title" ? "Uma ideia que merece ser vista" : "Título do slide",
    style: { fontSize: 56, bold: true },
  });
  let elements = [title];
  if (layout === "title") {
    title.y = 230;
    title.h = 180;
    title.style.fontSize = 68;
    elements.push(
      element("text", {
        y: 445,
        h: 90,
        text: "Subtítulo · contexto · autoria",
        style: { fontSize: 28 },
      }),
    );
  }
  if (layout === "content")
    elements.push(
      element("text", {
        y: 245,
        w: 1060,
        h: 310,
        text: "Uma mensagem principal.\n\nContexto, evidência e próximo passo.",
        style: { fontSize: 32 },
      }),
    );
  if (layout === "comparison")
    elements.push(
      element("text", {
        y: 230,
        w: 500,
        h: 360,
        text: "Antes\n\nDescreva o cenário atual.",
        style: { fontSize: 32 },
      }),
      element("text", {
        x: 690,
        y: 230,
        w: 510,
        h: 360,
        text: "Depois\n\nMostre a mudança desejada.",
        style: { fontSize: 32 },
      }),
    );
  if (layout === "chart")
    elements.push(
      element("chart", {
        y: 245,
        h: 370,
        w: 1070,
        data: { labels: ["A", "B", "C"], values: [25, 60, 85] },
        text: "Dados de exemplo — substitua pelos seus",
        style: { fontSize: 26 },
      }),
    );
  if (layout === "table")
    elements.push(
      element("table", {
        y: 240,
        h: 350,
        data: {
          rows: [
            ["Categoria", "Resultado", "Observação"],
            ["Item A", "—", "Adicione dados reais"],
            ["Item B", "—", "Adicione dados reais"],
          ],
        },
        style: { fontSize: 27 },
      }),
    );
  if (layout === "image")
    elements.push(
      element("text", {
        y: 260,
        w: 480,
        h: 280,
        text: "Insira uma imagem e conte sua história.",
        style: { fontSize: 34 },
      }),
    );
  if (layout === "closing") {
    title.y = 210;
    title.text = "Próximos passos";
    elements.push(
      element("text", {
        y: 365,
        h: 180,
        text: "O que acontece agora?\nQuem participa e qual é o prazo?",
        style: { fontSize: 32 },
      }),
    );
  }
  elements.push(
    element("text", {
      x: 1150,
      y: 656,
      w: 65,
      h: 35,
      text: String(index).padStart(2, "0"),
      style: { fontSize: 18, align: "right" },
      id: uid("page"),
    }),
  );
  return {
    id: uid("slide"),
    notes: "",
    transition: "fade",
    background: null,
    elements,
  };
}
export function safeCss(css = "") {
  if (
    css.length > 64000 ||
    /@import|url\s*\(|expression\s*\(|<|>|@font-face|\\/i.test(css)
  )
    throw new Error(
      "CSS permite estilos locais. Imports, URLs e escapes não são permitidos.",
    );
  return css;
}
export function styles(e, theme) {
  const s = e.style || {};
  return `position:absolute;left:${e.x}px;top:${e.y}px;width:${e.w}px;height:${e.h}px;box-sizing:border-box;overflow:hidden;font-size:${s.fontSize || 32}px;font-family:${theme.font};color:${s.color || theme.foreground};background:${s.fill || "transparent"};font-weight:${s.bold ? "700" : "400"};font-style:${s.italic ? "italic" : "normal"};text-align:${s.align || "left"};border-radius:${s.radius || 0}px;opacity:${s.opacity ?? 1};white-space:pre-wrap;line-height:1.18;margin:0;`;
}
function content(e, doc) {
  if (e.kind === "image") {
    const a = doc.assets.find((a) => a.id === e.asset_id);
    return a
      ? `<img alt="${escapeHtml(a.description || a.name)}" src="${escapeHtml(a.data_url)}" style="display:block;width:100%;height:100%;object-fit:contain">`
      : "";
  }
  if (e.kind === "chart") {
    const values = e.data?.values || [],
      labels = e.data?.labels || [];
    const max = Math.max(1, ...values);
    return `<div class="studio-chart">${values.map((v, i) => `<div class="studio-bar-col"><span>${escapeHtml(v)}</span><div class="studio-bar" style="height:${(v / max) * 75}%;background:${doc.theme.accent}"></div><span>${escapeHtml(labels[i])}</span></div>`).join("")}</div>`;
  }
  if (e.kind === "table")
    return `<table class="studio-table">${(e.data?.rows || []).map((row, i) => `<tr>${row.map((cell) => `<${i ? "td" : "th"}>${escapeHtml(cell)}</${i ? "td" : "th"}>`).join("")}</tr>`).join("")}</table>`;
  return escapeHtml(e.text);
}
export function slideMarkup(slide, doc, animated = false) {
  return `<section class="slide${animated ? " animate" : ""}" data-slide-id="${slide.id}" data-transition="${slide.transition || "none"}" style="background:${slide.background || doc.theme.background}">${slide.elements.map((e) => `<div class="element ${e.kind}" data-id="${e.id}" data-kind="${e.kind}" data-animation="${e.animation || "none"}" style="${styles(e, doc.theme)};animation-delay:${e.delay_ms || 0}ms">${content(e, doc)}</div>`).join("")}</section>`;
}
export const baseCss = `*{box-sizing:border-box}html,body{margin:0;padding:0}body{background:transparent}.slide{position:relative;width:1280px;height:720px;overflow:hidden}.element{overflow-wrap:break-word}.studio-chart{height:100%;display:flex;align-items:flex-end;gap:24px;padding:10px 20px}.studio-bar-col{height:100%;flex:1;display:flex;flex-direction:column;justify-content:flex-end;align-items:center;gap:12px}.studio-bar{width:100%;min-height:2px;border-radius:6px 6px 0 0}.studio-table{border-collapse:collapse;width:100%;height:100%;font-size:inherit}.studio-table td,.studio-table th{border-bottom:1px solid currentColor;padding:12px;text-align:left}.animate [data-animation=fade]{animation:studio-fade .65s both}.animate [data-animation=rise]{animation:studio-rise .7s both}.animate [data-animation=slide]{animation:studio-slide .7s both}.animate [data-animation=zoom]{animation:studio-zoom .7s both}@keyframes studio-fade{from{opacity:0}}@keyframes studio-rise{from{opacity:0;transform:translateY(28px)}}@keyframes studio-slide{from{opacity:0;transform:translateX(-35px)}}@keyframes studio-zoom{from{opacity:0;transform:scale(.9)}}@media(prefers-reduced-motion:reduce){.animate .element{animation:none!important}}`;
export function previewHtml(slide, doc, animated = false) {
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src data: blob:; style-src 'unsafe-inline'; font-src data:;"><style>${baseCss}\n${safeCss(doc.css || "")}</style></head><body>${slideMarkup(slide, doc, animated)}</body></html>`;
}
export function sourceHtml(doc) {
  return doc.slides
    .map(
      (s) =>
        `<section data-slide-id="${s.id}" data-transition="${s.transition || "none"}">\n${s.elements
          .map((e) => {
            const attrs = `data-id="${e.id}" data-kind="${e.kind}" data-animation="${e.animation || "none"}" data-delay="${e.delay_ms || 0}" data-style="${escapeHtml(JSON.stringify(e.style || {}))}" data-payload="${escapeHtml(JSON.stringify(e.data))}"${e.asset_id ? ` data-asset="${e.asset_id}"` : ""}`;
            return `  <div ${attrs} style="left:${e.x}px;top:${e.y}px;width:${e.w}px;height:${e.h}px">${escapeHtml(e.text)}</div>`;
          })
          .join("\n")}\n</section>`,
    )
    .join("\n\n");
}
export function parseSource(html, doc, Parser = DOMParser) {
  if (html.length > 512000) throw new Error("HTML muito grande.");
  const root = new Parser().parseFromString(html, "text/html");
  if (root.querySelector("script,iframe,object,embed,link,style,svg,form,base"))
    throw new Error(
      "Use HTML dos slides, CSS na aba CSS e JavaScript na aba JS.",
    );
  const sections = [...root.body.children];
  if (
    !sections.length ||
    sections.some((s) => !s.matches("section[data-slide-id]"))
  )
    throw new Error('Cada slide precisa de <section data-slide-id="...">.');
  const next = clone(doc);
  next.slides = sections.map((s) => {
    const old = doc.slides.find((x) => x.id === s.dataset.slideId);
    const elements = [...s.children].map((n) => {
      if (
        !n.matches("div[data-id][data-kind]") ||
        n.children.length ||
        [...n.attributes].some((a) => /^on/i.test(a.name))
      )
        throw new Error(
          "Elementos devem ser divs com data-id/data-kind e conteúdo de texto. Imagens usam data-asset.",
        );
      const num = (k) => {
        const raw = n.style[k];
        if (!/^\d+(\.\d+)?px$/.test(raw))
          throw new Error(`Informe ${k} em px.`);
        return Number.parseFloat(raw);
      };
      return {
        id: n.dataset.id,
        kind: n.dataset.kind,
        x: num("left"),
        y: num("top"),
        w: num("width"),
        h: num("height"),
        text: n.textContent,
        style: JSON.parse(n.dataset.style || "{}"),
        data: JSON.parse(n.dataset.payload || "null"),
        asset_id: n.dataset.asset || null,
        animation: n.dataset.animation || "none",
        delay_ms: Number(n.dataset.delay || 0),
      };
    });
    return {
      id: s.dataset.slideId,
      transition: s.dataset.transition || "none",
      notes: old?.notes || "",
      background: old?.background || null,
      elements,
    };
  });
  return next;
}
export function validateLayout(doc, frameDocument) {
  const issues = [];
  for (const s of doc.slides)
    for (const e of s.elements) {
      if (e.x + e.w > WIDTH || e.y + e.h > HEIGHT || e.x < 0 || e.y < 0)
        issues.push({
          code: "OUT_OF_BOUNDS",
          slide_id: s.id,
          element_id: e.id,
          message: "Elemento fora do slide.",
        });
      if (e.kind === "text" && e.text.length > 650)
        issues.push({
          code: "DENSE_TEXT",
          slide_id: s.id,
          element_id: e.id,
          message: "Texto extenso; divida em mais slides.",
        });
      const node = frameDocument?.querySelector(
        `[data-slide-id="${s.id}"] [data-id="${e.id}"]`,
      );
      if (
        node &&
        (node.scrollHeight > node.clientHeight + 2 ||
          node.scrollWidth > node.clientWidth + 2)
      )
        issues.push({
          code: "TEXT_OVERFLOW",
          slide_id: s.id,
          element_id: e.id,
          message: "Conteúdo cortado; aumente a área ou reduza o texto.",
        });
    }
  return issues;
}
export function extractJson(text) {
  const raw = text
    .trim()
    .replace(/^```(?:json)?\s*/i, "")
    .replace(/\s*```$/, "");
  const first = raw.indexOf("{"),
    last = raw.lastIndexOf("}");
  if (first < 0 || last < first)
    throw new Error(
      "O modelo não retornou JSON. Tente um pedido menor, sobre um slide.",
    );
  const result = JSON.parse(raw.slice(first, last + 1));
  if (
    !Array.isArray(result.operations) ||
    !result.operations.length ||
    result.operations.length > 20
  )
    throw new Error("O modelo deve retornar 1–20 operações.");
  return result;
}
export function aiContext(doc, slideIndex) {
  return {
    project_id: doc.id,
    revision: doc.revision,
    title: doc.title,
    brief: doc.brief,
    theme: doc.theme,
    selected_slide: doc.slides[slideIndex],
    outline: doc.slides.map((s) => ({
      id: s.id,
      title: s.elements.find((e) => e.kind === "text")?.text.slice(0, 150),
    })),
    assets: doc.assets.map(({ id, name, description }) => ({
      id,
      name,
      description,
    })),
  };
}
