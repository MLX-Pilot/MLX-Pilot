const WIDTH = 1280,
  HEIGHT = 720;

function rgb(value) {
  if (/^#[\da-f]{6}$/i.test(value || ""))
    return [1, 3, 5].map((offset) =>
      parseInt(value.slice(offset, offset + 2), 16),
    );
  const match = String(value || "").match(
    /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+))?\s*\)$/i,
  );
  if (match && (match[4] === undefined || Number(match[4]) === 1))
    return match.slice(1, 4).map(Number);
  return null;
}
function blend(foreground, background, opacity = 1) {
  return foreground.map((c, i) => c * opacity + background[i] * (1 - opacity));
}
function luminance(color) {
  const linear = color.map((c) => {
    const channel = c / 255;
    return channel <= 0.04045
      ? channel / 12.92
      : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722;
}
function contrast(a, b) {
  const first = luminance(a),
    second = luminance(b);
  return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
}
function intersection(a, b) {
  return {
    w: Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x),
    h: Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y),
  };
}
function covers(a, b) {
  return (
    a.x <= b.x && a.y <= b.y && a.x + a.w >= b.x + b.w && a.y + a.h >= b.y + b.h
  );
}
function backgroundBehind(doc, slide, elementIndex) {
  let background = rgb(slide.background || doc.theme.background);
  const target = slide.elements[elementIndex];
  for (const previous of slide.elements.slice(0, elementIndex)) {
    const overlap = intersection(previous, target);
    if (overlap.w <= 0 || overlap.h <= 0 || previous.style?.opacity === 0)
      continue;
    const fill = rgb(previous.style?.fill);
    if (fill && covers(previous, target)) {
      const opacity = previous.style?.opacity ?? 1;
      background =
        opacity === 1 ? fill : background && blend(fill, background, opacity);
    } else if (fill || ["image", "chart", "table"].includes(previous.kind)) {
      // A picture, chart or partially covered text box has no single background.
      background = null;
    }
  }
  return background;
}

/** Heuristics and measurable defects, never a claim of complete design approval. */
export function auditSlide(doc, slide, frameDocument) {
  const diagnostics = [];
  const report = (code, element, message, severity = "error") =>
    diagnostics.push({
      code,
      slide_id: slide.id,
      element_id: element?.id || "",
      message,
      severity,
    });
  const slideNode = frameDocument
    ? [...frameDocument.querySelectorAll("[data-slide-id]")].find(
        (node) => node.dataset.slideId === slide.id,
      )
    : null;
  const nodes = new Map(
    [...(slideNode?.querySelectorAll("[data-id]") || [])].map((node) => [
      node.dataset.id,
      node,
    ]),
  );
  const rectangles = new Map();
  const customCss = Boolean(doc.css?.trim());
  const texts = slide.elements.filter(
    (element) => element.kind === "text" && element.text?.trim(),
  );
  const wordCount = texts.reduce(
    (count, element) => count + element.text.trim().split(/\s+/).length,
    0,
  );
  if (wordCount > 70)
    report(
      "DENSE_SLIDE",
      null,
      `Slide com ${wordCount} palavras; mova detalhes às notas ou divida o conteúdo.`,
      "warning",
    );

  for (const [index, element] of slide.elements.entries()) {
    const node = nodes.get(element.id);
    const computed = node && frameDocument.defaultView?.getComputedStyle(node);
    let box = element;
    if (node && slideNode) {
      const rect = node.getBoundingClientRect(),
        slideRect = slideNode.getBoundingClientRect();
      // jsdom and detached elements report zero rectangles; structural coordinates remain useful.
      if (rect.width > 0 && rect.height > 0 && slideRect.width > 0)
        box = {
          x: rect.left - slideRect.left,
          y: rect.top - slideRect.top,
          w: rect.width,
          h: rect.height,
        };
    }
    rectangles.set(element.id, box);
    if (
      ![box.x, box.y, box.w, box.h].every(Number.isFinite) ||
      box.w <= 0 ||
      box.h <= 0 ||
      box.x < -0.5 ||
      box.y < -0.5 ||
      box.x + box.w > WIDTH + 0.5 ||
      box.y + box.h > HEIGHT + 0.5
    )
      report(
        "OUT_OF_BOUNDS",
        element,
        "Elemento fora do slide; ajuste posição e tamanho.",
      );
    if (
      node &&
      (node.scrollHeight > node.clientHeight + 2 ||
        node.scrollWidth > node.clientWidth + 2)
    )
      report(
        "TEXT_OVERFLOW",
        element,
        "Conteúdo cortado; aumente a área ou reduza o texto.",
      );
    if (element.kind !== "text" || !element.text?.trim()) continue;

    const fontSize =
      Number.parseFloat(computed?.fontSize) || element.style?.fontSize || 32;
    let opacity = computed?.opacity
      ? Number(computed.opacity)
      : (element.style?.opacity ?? 1);
    let hidden =
      computed?.display === "none" || computed?.visibility === "hidden";
    for (
      let parent = node?.parentElement;
      parent;
      parent = parent.parentElement
    ) {
      const parentStyle = frameDocument.defaultView?.getComputedStyle(parent);
      if (parentStyle?.opacity) opacity *= Number(parentStyle.opacity);
      hidden ||=
        parentStyle?.display === "none" || parentStyle?.visibility === "hidden";
    }
    if (opacity <= 0.01 || hidden) {
      report(
        "INVISIBLE_TEXT",
        element,
        "Texto invisível; reveja opacidade e visibilidade.",
      );
      continue;
    }
    if (fontSize < 18)
      report(
        "SMALL_TEXT",
        element,
        `Texto com ${fontSize}px; use pelo menos 18px para leitura confortável.`,
        "warning",
      );
    if (element.text.length > 350)
      report(
        "DENSE_TEXT",
        element,
        "Bloco de texto extenso; reduza a redação ou use notas do apresentador.",
        "warning",
      );
    if (customCss) {
      report(
        "CONTRAST_UNVERIFIED",
        element,
        "CSS personalizado pode alterar fundos e composição; confira o contraste na prévia renderizada.",
        "warning",
      );
      continue;
    }
    let background = backgroundBehind(doc, slide, index);
    const foreground = rgb(element.style?.color || doc.theme.foreground);
    const fill = rgb(element.style?.fill);
    if (!background || !foreground) {
      report(
        "CONTRAST_UNVERIFIED",
        element,
        "Fundo variável sob o texto; confira o contraste sobre toda a área.",
        "warning",
      );
      continue;
    }
    const surface = fill || background;
    const visibleBackground = blend(surface, background, opacity);
    const visibleText = blend(foreground, background, opacity);
    const bold = element.style?.bold || Number(computed?.fontWeight) >= 700;
    const threshold = fontSize >= 24 || (bold && fontSize >= 18.667) ? 3 : 4.5;
    const ratio = contrast(visibleText, visibleBackground);
    if (ratio + 0.001 < threshold)
      report(
        "LOW_CONTRAST",
        element,
        `Contraste ${ratio.toFixed(2)}:1; mínimo ${threshold}:1. Ajuste texto ou fundo.`,
      );
  }
  for (let index = 0; index < texts.length; index++) {
    for (const other of texts.slice(index + 1)) {
      const first = texts[index];
      const overlap = intersection(
        rectangles.get(first.id),
        rectangles.get(other.id),
      );
      if (overlap.w > 1 && overlap.h > 1)
        report(
          "TEXT_OVERLAP",
          other,
          `Caixa de texto sobreposta a ${first.id}; separe os blocos.`,
        );
    }
  }
  return diagnostics;
}

/** Structural checks only; inspectPresentation adds browser layout measurements. */
export function auditDocument(doc) {
  return doc.slides.flatMap((slide) => auditSlide(doc, slide));
}
