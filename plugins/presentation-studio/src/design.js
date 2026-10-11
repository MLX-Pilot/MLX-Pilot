import { palettes } from "./document.js";

export const LAYOUTS = Object.freeze([
  "hero",
  "split",
  "cards",
  "steps",
  "statement",
  "closing",
  "chart",
]);
export const STYLES = Object.freeze(["editorial", "minimal", "bold"]);
export const MOTIONS = Object.freeze(["none", "subtle", "expressive"]);
export const SLIDE_SPEC_LIMITS = Object.freeze({
  eyebrow: 40,
  title: 90,
  body: 220,
  itemTitle: 48,
  itemBody: 160,
  takeaway: 100,
  notes: 2400,
  chartLabel: 24,
  chartSource: 160,
});

const fail = (message) => {
  throw new Error(message);
};
const record = (value) =>
  value && typeof value === "object" && !Array.isArray(value);
const validId = (value) =>
  typeof value === "string" && /^[a-zA-Z0-9_-]{1,80}$/.test(value);
function text(value, name, max, required = false, paragraphs = false) {
  if (value == null && !required) return "";
  if (typeof value !== "string")
    fail(`${name}: forneça texto, não um objeto ou número.`);
  const result = paragraphs ? value.trim() : value.trim().replace(/\s+/gu, " ");
  if (required && !result) fail(`${name}: informe um texto com conteúdo.`);
  if ([...result].length > max)
    fail(
      `${name}: máximo ${max} caracteres; resuma e mova detalhes para notes. Não corte palavras.`,
    );
  return result;
}

/** Content contracts are deliberately smaller than the document schema.
 * hero/statement/closing/chart: body and takeaway, no items.
 * cards/steps: 2–3 items. split: 0–2 items, or body with an image.
 * Images belong to hero/split/closing; chart values must come from supplied data.
 * Character limits are ceilings, not a promise that every string fits every column.
 * composeSlide reports the field to repair when the actual line budget is exceeded.
 */
export function validateSlideSpec(value, assets = []) {
  if (!record(value)) fail("O slide deve ser um objeto JSON.");
  if (!LAYOUTS.includes(value.layout))
    fail(`layout: escolha ${LAYOUTS.join(", ")}.`);
  const spec = {
    layout: value.layout,
    eyebrow: text(value.eyebrow, "eyebrow", SLIDE_SPEC_LIMITS.eyebrow),
    title: text(value.title, "title", SLIDE_SPEC_LIMITS.title, true),
    body: text(value.body, "body", SLIDE_SPEC_LIMITS.body),
    items: [],
    takeaway: text(value.takeaway, "takeaway", SLIDE_SPEC_LIMITS.takeaway),
    notes: text(value.notes, "notes", SLIDE_SPEC_LIMITS.notes, false, true),
    asset_id: value.asset_id ?? null,
  };
  if (value.items != null && !Array.isArray(value.items))
    fail("items: forneça uma lista.");
  const items = value.items ?? [];
  const grouped = ["cards", "steps"].includes(spec.layout);
  if (grouped && (items.length < 2 || items.length > 3))
    fail(`${spec.layout}: use exatamente 2 ou 3 items.`);
  if (spec.layout === "split" && items.length > 2)
    fail("split: use no máximo 2 items.");
  if (!grouped && spec.layout !== "split" && items.length)
    fail(
      `${spec.layout}: não aceita items; use body/takeaway ou mude para cards/steps.`,
    );
  spec.items = items.map((item, i) => {
    if (!record(item)) fail(`items[${i}]: forneça {title, body}.`);
    return {
      title: text(
        item.title,
        `items[${i}].title`,
        SLIDE_SPEC_LIMITS.itemTitle,
        true,
      ),
      body: text(item.body, `items[${i}].body`, SLIDE_SPEC_LIMITS.itemBody),
    };
  });
  if (spec.asset_id !== null) {
    if (
      !validId(spec.asset_id) ||
      !assets.some((asset) => asset.id === spec.asset_id)
    )
      fail(
        "asset_id: selecione um id real da lista de imagens fornecidas, ou null.",
      );
    if (!["hero", "split", "closing"].includes(spec.layout))
      fail(
        `${spec.layout}: imagens usam hero/split/closing; escolha esse layout ou asset_id:null.`,
      );
    if (spec.items.length)
      fail("split: use imagem com body, ou items sem imagem.");
  }
  if (
    spec.layout === "split" &&
    !spec.body &&
    !spec.items.length &&
    !spec.asset_id
  )
    fail(
      "split: forneça body, items ou uma imagem real para construir a composição.",
    );
  if (spec.layout === "chart") {
    if (!record(value.chart))
      fail(
        "chart: forneça labels, values e source com dados reais do usuário.",
      );
    const { labels, values, source } = value.chart;
    if (
      !Array.isArray(labels) ||
      !Array.isArray(values) ||
      labels.length < 2 ||
      labels.length > 6 ||
      labels.length !== values.length
    )
      fail("chart: use 2–6 labels e o mesmo número de values.");
    if (
      !values.every(
        (number) =>
          typeof number === "number" && Number.isFinite(number) && number >= 0,
      )
    )
      fail(
        "chart.values: use somente números finitos não negativos fornecidos pelo usuário.",
      );
    spec.chart = {
      labels: labels.map((label, i) =>
        text(label, `chart.labels[${i}]`, SLIDE_SPEC_LIMITS.chartLabel, true),
      ),
      values: [...values],
      source: text(source, "chart.source", SLIDE_SPEC_LIMITS.chartSource, true),
    };
  } else if (value.chart != null)
    fail("chart: dados de gráfico requerem layout:'chart'.");
  return spec;
}

function hash(value) {
  let result = 2166136261;
  for (const char of value)
    result = Math.imul(result ^ char.codePointAt(0), 16777619);
  return (result >>> 0).toString(36);
}
function mix(first, second, weight) {
  const rgb = (hex) =>
    [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16));
  const a = rgb(first),
    b = rgb(second);
  return `#${a
    .map((channel, i) =>
      Math.round(channel * (1 - weight) + b[i] * weight)
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}
function luminance(hex) {
  return [1, 3, 5]
    .map((i) => Number.parseInt(hex.slice(i, i + 2), 16) / 255)
    .map((n) => (n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4))
    .reduce((sum, n, i) => sum + n * [0.2126, 0.7152, 0.0722][i], 0);
}
function onColor(background, foreground) {
  const contrast = (color) => {
    const a = luminance(background),
      b = luminance(color);
    return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
  };
  return [foreground, "#101820", "#ffffff"].sort(
    (a, b) => contrast(b) - contrast(a),
  )[0];
}

// Conservative font metrics keep the same composition in the browser and native PPTX.
// Explicit line breaks are stable across the supported fonts; no text is ellipsized.
function widthOf(value, size, font, bold) {
  let units = 0;
  for (const char of value) {
    if (font === "Courier New")
      units += char.codePointAt(0) > 0x2e7f ? 1.2 : 0.62;
    else if (/\s/u.test(char)) units += 0.29;
    else if (/[ilIjtfr.,:;'!|]/u.test(char)) units += 0.34;
    else if (/[MW@%&—]/u.test(char)) units += 1;
    else if (/[mw]/u.test(char)) units += 0.88;
    else if (/\p{Lu}/u.test(char)) units += 0.78;
    else if (char.codePointAt(0) > 0x2e7f) units += 1.05;
    else units += 0.58;
  }
  return (
    units *
    size *
    (font === "Georgia" ? 1.06 : font === "Verdana" ? 1.06 : 1) *
    (bold ? 1.035 : 1)
  );
}
function linesOf(value, width, size, font, bold) {
  const lines = [];
  let line = "";
  for (const word of value.split(" ")) {
    if (widthOf(word, size, font, bold) > width) return null;
    const next = line ? `${line} ${word}` : word;
    if (line && widthOf(next, size, font, bold) > width) {
      lines.push(line);
      line = word;
    } else line = next;
  }
  if (line) lines.push(line);
  return lines;
}

function balancedTitle(value, width, size, font) {
  const words = value.split(" ");
  let result = null,
    difference = Infinity;
  for (let at = 1; at < words.length; at++) {
    const first = words.slice(0, at).join(" "),
      second = words.slice(at).join(" ");
    const firstWidth = widthOf(first, size, font, true),
      secondWidth = widthOf(second, size, font, true);
    if (firstWidth > width || secondWidth > width) continue;
    const candidate = Math.abs(firstWidth - secondWidth);
    if (candidate < difference) {
      difference = candidate;
      result = `${first}\n${second}`;
    }
  }
  return result;
}

function stepHeading(value, index) {
  // The separate 01/02/03 marker already conveys this ordinal. Require either
  // whitespace or a following letter so quantities such as "1.5 metros" survive.
  return value.replace(
    new RegExp(`^${index + 1}[.)](?:\\s+(?=\\S)|(?=\\p{L}))`, "u"),
    "",
  );
}

/** Build a complete, editable slide using bounded native text/shapes/charts/images. */
export function composeSlide(
  value,
  {
    theme = palettes.orbital,
    assets = [],
    index = 0,
    total = 1,
    motion = "subtle",
    style = "editorial",
    id = `designed-${index + 1}`,
  } = {},
) {
  const spec = validateSlideSpec(value, assets);
  if (!STYLES.includes(style)) fail(`style: escolha ${STYLES.join(", ")}.`);
  if (!MOTIONS.includes(motion)) fail(`motion: escolha ${MOTIONS.join(", ")}.`);
  if (!validId(id))
    fail("id: use 1–80 letras, números, hífens ou sublinhados.");
  if (
    !Number.isInteger(index) ||
    !Number.isInteger(total) ||
    index < 0 ||
    total < 1 ||
    index >= total ||
    total > 60
  )
    fail(
      "index/total: informe a posição do slide (base zero) e total entre 1 e 60.",
    );
  const t = { ...palettes.orbital, ...theme };
  if (
    ![t.background, t.foreground, t.accent, t.muted].every(
      (color) => typeof color === "string" && /^#[0-9a-f]{6}$/i.test(color),
    )
  )
    fail("theme: as cores devem usar #RRGGBB.");
  if (
    !["Arial", "Georgia", "Verdana", "Helvetica", "Courier New"].includes(
      t.font,
    )
  )
    fail("theme.font: fonte não suportada.");
  const prefix = id.length <= 54 ? id : `${id.slice(0, 44)}-${hash(id)}`;
  const slide = {
    id,
    notes: spec.notes,
    background: t.background,
    transition:
      motion === "none"
        ? "none"
        : motion === "expressive" && ["split", "steps"].includes(spec.layout)
          ? "slide"
          : "fade",
    elements: [],
  };
  let sequence = 0;
  const surface = mix(
    t.background,
    t.foreground,
    style === "minimal" ? 0.035 : 0.065,
  );
  const line = mix(
    t.background,
    t.foreground,
    style === "minimal" ? 0.2 : 0.26,
  );
  function add(kind, name, box, content = {}) {
    const order = sequence++;
    const element = {
      id: `${prefix}-${name}-${order}`,
      kind,
      ...box,
      text: "",
      asset_id: null,
      style: {},
      animation: "none",
      delay_ms: 0,
      data: null,
      ...content,
    };
    if (motion !== "none" && kind !== "shape") {
      element.animation =
        motion === "subtle"
          ? kind === "image"
            ? "fade"
            : "rise"
          : kind === "image"
            ? "zoom"
            : name.startsWith("item")
              ? "slide"
              : "rise";
      element.delay_ms = Math.min(
        600,
        order * (motion === "expressive" ? 75 : 45),
      );
    }
    slide.elements.push(element);
    return element;
  }
  function shape(name, x, y, w, h, fill, extra = {}) {
    return add("shape", name, { x, y, w, h }, { style: { fill, ...extra } });
  }
  function copy(
    name,
    content,
    x,
    y,
    w,
    h,
    {
      size = 28,
      min = 24,
      maxLines = 20,
      bold = false,
      color = t.foreground,
      align = "left",
      field = name,
    } = {},
  ) {
    if (!content) return null;
    for (let fontSize = size; fontSize >= min; fontSize -= 2) {
      const lines = linesOf(content, w, fontSize, t.font, bold);
      if (
        lines &&
        lines.length <= maxLines &&
        lines.length * fontSize * 1.18 + 4 <= h
      )
        return add(
          "text",
          name,
          { x, y, w, h },
          { text: lines.join("\n"), style: { fontSize, bold, color, align } },
        );
    }
    fail(
      `${field}: texto não cabe em ${Math.min(maxLines, Math.floor((h - 4) / (min * 1.18)))} linhas neste layout. Resuma esse campo, mova detalhes para notes ou escolha outro layout; o texto não foi cortado.`,
    );
  }
  function title(
    y,
    { x = 64, w = 1152, size = 60, h = 148, align = "left" } = {},
  ) {
    const heading = copy("title", spec.title, x, y, w, h, {
      size,
      min: 48,
      maxLines: 2,
      bold: true,
      align,
    });
    if (heading.text.split("\n").length === 2)
      heading.text =
        balancedTitle(spec.title, w, heading.style.fontSize, t.font) ||
        heading.text;
    return heading;
  }
  function image(x, y, w, h) {
    const asset = assets.find((entry) => entry.id === spec.asset_id);
    shape("image-panel", x, y, w, h, surface, {
      radius: style === "minimal" ? 0 : 18,
    });
    add(
      "image",
      "image",
      { x: x + 16, y: y + 16, w: w - 32, h: h - 32 },
      {
        asset_id: asset.id,
        text: asset.description || asset.name || "",
      },
    );
  }
  function abstract(x, y, w, h) {
    // Abstract editorial marks convey rhythm, never pretend to represent data.
    const offset = (index % 3) * 18;
    if (style === "minimal") {
      shape("visual-line", x + 64, y + 24, 2, h - 48, t.accent);
      shape(
        "visual-square",
        x + 116,
        y + 54,
        Math.min(120, w - 140),
        120,
        surface,
      );
      shape(
        "visual-rule",
        x + 116,
        y + 195,
        Math.min(180, w - 140),
        3,
        t.accent,
      );
    } else {
      shape("visual-back", x + 26, y + 6, w - 52, h - 32, surface, {
        radius: style === "bold" ? 24 : 0,
      });
      shape(
        "visual-block",
        x + 68 + offset,
        y + 30,
        Math.min(180, w - 124),
        h - 84,
        t.accent,
        {
          opacity: style === "bold" ? 1 : 0.82,
          radius: style === "bold" ? 20 : 0,
        },
      );
      shape("visual-cut", x + 32, y + h - 86, w - 76, 34, t.background);
      shape("visual-spine", x + w - 62, y + 68, 2, h - 90, t.foreground, {
        opacity: 0.5,
      });
    }
  }
  function takeaway(
    y = 554,
    x = 64,
    w = 1152,
    h = 64,
    size = 26,
    align = "left",
  ) {
    copy("takeaway", spec.takeaway, x, y, w, h, {
      size,
      min: 24,
      maxLines: 2,
      bold: true,
      color: t.accent,
      align,
    });
  }

  // Every treatment retains the user's palette and a clear 64 px safe area.
  if (style === "bold") {
    shape("brand-block", 64, 64, spec.eyebrow ? 24 : 80, 24, t.accent);
  } else
    shape(
      "brand-rule",
      64,
      99,
      style === "minimal" ? 40 : 88,
      style === "minimal" ? 2 : 4,
      t.accent,
    );
  copy("eyebrow", spec.eyebrow, style === "bold" ? 104 : 64, 64, 1040, 28, {
    size: 20,
    min: 18,
    maxLines: 1,
    color: t.muted,
  });

  if (spec.layout === "hero") {
    title(spec.asset_id ? 124 : 144, {
      size: style === "minimal" ? 60 : 68,
      h: 168,
    });
    if (spec.asset_id) {
      copy("body", spec.body, 64, 320, 568, 192, {
        size: 30,
        min: 24,
        maxLines: 6,
      });
      image(704, 312, 512, 246);
      takeaway(536, 64, 608, 72);
    } else {
      copy("body", spec.body, 64, 346, 752, 158, {
        size: 30,
        min: 24,
        maxLines: 5,
      });
      abstract(900, 324, 316, 244);
      takeaway(538, 64, 752, 72);
    }
  } else if (spec.layout === "split") {
    title(118, { size: 58, h: 140 });
    copy("body", spec.body, 64, 296, 496, 234, {
      size: 30,
      min: 24,
      maxLines: 7,
    });
    shape("column-rule", 608, 292, 2, 240, line);
    if (spec.asset_id) image(656, 286, 560, 252);
    else if (spec.items.length) {
      const rowHeight = spec.items.length === 1 ? 252 : 132;
      spec.items.forEach((item, i) => {
        const y = 282 + i * 136;
        const heading = copy(
          `item-${i}-title`,
          item.title,
          656,
          y,
          560,
          spec.items.length === 1 ? 70 : 62,
          {
            size: 28,
            min: 24,
            maxLines: 2,
            bold: true,
            field: `items[${i}].title`,
          },
        );
        heading.h = Math.ceil(
          heading.text.split("\n").length * heading.style.fontSize * 1.18 + 4,
        );
        const bodyTop = y + heading.h + 8;
        copy(
          `item-${i}-body`,
          item.body,
          656,
          bodyTop,
          560,
          y + rowHeight - bodyTop,
          {
            size: 26,
            min: 24,
            maxLines: spec.items.length === 1 ? 5 : 2,
            color: t.muted,
            field: `items[${i}].body`,
          },
        );
      });
    } else abstract(656, 286, 560, 252);
    takeaway(552, 64, 1152, 64);
  } else if (spec.layout === "cards" || spec.layout === "steps") {
    title(112, { size: 56, h: 140 });
    copy("body", spec.body, 64, 256, 1152, 66, {
      size: 26,
      min: 24,
      maxLines: 2,
    });
    const top = spec.body ? 344 : 282;
    const bottom = spec.takeaway ? 536 : 588;
    const gap = 24,
      count = spec.items.length,
      width = (1152 - gap * (count - 1)) / count;
    spec.items.forEach((item, i) => {
      const x = 64 + i * (width + gap),
        inset = spec.layout === "cards" ? 24 : 0;
      const contentWidth = width - 2 * inset;
      let textTop = top + 24;
      if (spec.layout === "cards") {
        shape(`card-${i}`, x, top, width, bottom - top, surface, {
          radius: style === "minimal" ? 0 : 14,
        });
        shape(`card-rule-${i}`, x + 24, top + 16, 36, 3, t.accent);
      } else {
        shape(`step-rule-${i}`, x, top + 62, width, 2, line);
        copy(
          `step-number-${i}`,
          String(i + 1).padStart(2, "0"),
          x,
          top,
          width,
          56,
          { size: 42, min: 40, maxLines: 1, color: t.accent },
        );
        textTop = top + 76;
      }
      const heading = copy(
        `item-${i}-title`,
        spec.layout === "steps" ? stepHeading(item.title, i) : item.title,
        x + inset,
        textTop,
        contentWidth,
        70,
        {
          size: 28,
          min: 24,
          maxLines: 2,
          bold: true,
          field: `items[${i}].title`,
        },
      );
      const headingHeight =
        heading.text.split("\n").length * heading.style.fontSize * 1.18 + 4;
      const bodyTop = textTop + headingHeight + 14;
      copy(
        `item-${i}-body`,
        item.body,
        x + inset,
        bodyTop,
        contentWidth,
        bottom - bodyTop - (spec.layout === "cards" ? 22 : 0),
        {
          size: 26,
          min: 24,
          maxLines: 6,
          color: t.muted,
          field: `items[${i}].body`,
        },
      );
      // Tight text boxes make the editor's selection bounds match the visible hierarchy.
      heading.h = Math.ceil(headingHeight);
    });
    takeaway(554, 64, 1152, 64, 24);
  } else if (spec.layout === "statement") {
    shape(
      "statement-rule",
      592,
      136,
      96,
      style === "minimal" ? 2 : 5,
      t.accent,
    );
    title(186, { x: 108, w: 1064, size: 68, h: 172, align: "center" });
    copy("body", spec.body, 164, 390, 952, 116, {
      size: 30,
      min: 24,
      maxLines: 3,
      color: t.muted,
      align: "center",
    });
    takeaway(536, 128, 1024, 74, 28, "center");
  } else if (spec.layout === "closing") {
    if (style === "bold")
      shape("closing-panel", 64, 124, 1152, 456, surface, { radius: 22 });
    const x = style === "bold" ? 92 : 64,
      width = style === "bold" ? 1096 : 1152;
    title(156, { x, w: width, size: 64, h: 160 });
    copy("body", spec.body, x, 342, spec.asset_id ? 620 : 952, 128, {
      size: 30,
      min: 24,
      maxLines: 4,
    });
    if (spec.asset_id) image(900, 340, 288, 192);
    if (spec.takeaway) {
      const takeWidth = spec.asset_id ? 720 : width;
      shape("closing-action", x, 500, takeWidth, 78, t.accent, {
        radius: style === "minimal" ? 0 : 10,
      });
      copy("takeaway", spec.takeaway, x + 20, 510, takeWidth - 40, 64, {
        size: 28,
        min: 24,
        maxLines: 2,
        bold: true,
        color: onColor(t.accent, t.foreground),
      });
    }
  } else if (spec.layout === "chart") {
    title(112, { size: 56, h: 140 });
    copy("body", spec.body, 64, 256, 1152, 66, {
      size: 26,
      min: 24,
      maxLines: 2,
    });
    add(
      "chart",
      "chart",
      { x: 64, y: 340, w: spec.takeaway ? 820 : 1152, h: 208 },
      {
        text: spec.title,
        data: { labels: spec.chart.labels, values: spec.chart.values },
        style: { fontSize: 24, color: t.foreground },
      },
    );
    if (spec.takeaway) {
      shape("chart-divider", 920, 344, 2, 194, line);
      copy("takeaway", spec.takeaway, 954, 350, 262, 182, {
        size: 28,
        min: 24,
        maxLines: 6,
        bold: true,
        color: t.accent,
      });
    }
    copy("source", spec.chart.source, 64, 568, 1152, 48, {
      size: 18,
      min: 18,
      maxLines: 2,
      color: t.muted,
      field: "chart.source",
    });
  }

  shape("footer-rule", 64, 620, 1152, 1, line);
  copy(
    "page",
    `${String(index + 1).padStart(2, "0")} / ${String(total).padStart(2, "0")}`,
    1076,
    630,
    140,
    26,
    { size: 18, min: 18, maxLines: 1, color: t.muted, align: "right" },
  );
  return slide;
}
