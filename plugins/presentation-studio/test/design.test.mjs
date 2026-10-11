import test from "node:test";
import assert from "node:assert/strict";
import JSZip from "jszip";
import {
  composeSlide,
  validateSlideSpec,
  LAYOUTS,
  STYLES,
} from "../src/design.js";
import { palettes, previewHtml } from "../src/document.js";
import { buildPptx } from "../src/export.js";

const normalize = (value) => value.replace(/\s+/gu, " ").trim();
const spec = (layout, patch = {}) => ({
  layout,
  eyebrow: "ESTRATÉGIA DE PRODUTO",
  title: "Uma ideia clara transforma a conversa",
  body: "Comece pelo que importa para o público.",
  items: ["cards", "steps"].includes(layout)
    ? [
        { title: "Escutar", body: "Entenda o contexto." },
        { title: "Criar", body: "Dê forma à ideia." },
        { title: "Aprender", body: "Teste com pessoas." },
      ]
    : [],
  takeaway: "O próximo passo precisa ser simples.",
  notes: "Apresente o contexto antes de mostrar a solução.",
  asset_id: null,
  ...(layout === "chart"
    ? {
        chart: {
          labels: ["Equipe A", "Equipe B"],
          values: [14, 21],
          source: "Fonte: levantamento informado no briefing.",
        },
      }
    : {}),
  ...patch,
});
const intersects = (a, b) =>
  a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;

test("all treatments have safe native geometry, legible type, and disjoint text boxes", () => {
  for (const style of STYLES)
    for (const layout of LAYOUTS) {
      const slide = composeSlide(spec(layout), { style });
      assert.ok(slide.elements.length < 100);
      const ids = new Set(),
        text = [];
      for (const element of slide.elements) {
        assert.match(element.id, /^[a-zA-Z0-9_-]{1,80}$/);
        assert.ok(!ids.has(element.id));
        ids.add(element.id);
        assert.ok(
          [element.x, element.y, element.w, element.h].every(Number.isFinite),
        );
        assert.ok(
          element.x >= 64 && element.y >= 64,
          `${style}/${layout}/${element.id}: top/left margin`,
        );
        assert.ok(
          element.x + element.w <= 1216 && element.y + element.h <= 656,
          `${style}/${layout}/${element.id}: bottom/right margin`,
        );
        assert.ok(element.w >= 1 && element.h >= 1);
        for (const key of Object.keys(element.style))
          assert.ok(
            [
              "fontSize",
              "color",
              "fill",
              "bold",
              "italic",
              "align",
              "radius",
              "opacity",
            ].includes(key),
          );
        for (const color of [element.style.color, element.style.fill].filter(
          Boolean,
        ))
          assert.match(color, /^#[0-9a-f]{6}$/i);
        if (element.kind === "text") {
          assert.ok(element.style.fontSize >= 18);
          assert.ok(
            element.text.split("\n").length * element.style.fontSize * 1.18 <=
              element.h,
          );
          for (const other of text)
            assert.ok(
              !intersects(element, other),
              `${layout}: ${element.id} overlaps ${other.id}`,
            );
          text.push(element);
        }
      }
      const heading = slide.elements.find((element) =>
        element.id.includes("-title-"),
      );
      assert.ok(heading.style.fontSize >= 48 && heading.style.fontSize <= 68);
      assert.ok(heading.text.split("\n").length <= 2);
    }
});

test("content is preserved without placeholders or silent truncation", () => {
  for (const layout of LAYOUTS) {
    const input = spec(layout);
    const slide = composeSlide(input);
    const texts = slide.elements
      .filter((element) => element.kind === "text")
      .map((element) => normalize(element.text));
    for (const field of [
      input.eyebrow,
      input.title,
      input.body,
      input.takeaway,
      ...input.items.flatMap((item) => [item.title, item.body]),
    ])
      assert.ok(texts.includes(field), `${layout} must preserve ${field}`);
    assert.equal(slide.notes, input.notes);
    assert.ok(!JSON.stringify(slide).includes("Escreva sua ideia"));
    assert.ok(!JSON.stringify(slide).includes("Dados de exemplo"));
  }
  assert.throws(
    () => validateSlideSpec(spec("hero", { title: "a".repeat(91) })),
    /title.*90/,
  );
  assert.throws(
    () => composeSlide(spec("hero", { title: "W".repeat(90) })),
    /title.*não cabe.*não foi cortado/,
  );
  assert.throws(
    () =>
      composeSlide(
        spec("cards", {
          items: Array.from({ length: 3 }, () => ({
            title: "Um título",
            body: "Uma descrição extensa que não cabe. ".repeat(4),
          })),
        }),
      ),
    /items\[0\].body.*não cabe/,
  );
});

test("images retain real references, descriptions, and contain rendering", () => {
  const asset = {
    id: "image-one",
    name: "Marca",
    description: "Símbolo azul enviado pelo usuário",
    data_url: "data:image/png;base64,AA==",
  };
  for (const layout of ["hero", "split", "closing"]) {
    const slide = composeSlide(spec(layout, { asset_id: asset.id }), {
      assets: [asset],
    });
    const image = slide.elements.find((element) => element.kind === "image");
    assert.equal(image.asset_id, asset.id);
    assert.equal(image.text, asset.description);
    const html = previewHtml(slide, {
      theme: palettes.orbital,
      assets: [asset],
      css: "",
    });
    assert.ok(html.includes("object-fit:contain"));
    assert.ok(html.includes(`alt="${asset.description}"`));
  }
  assert.throws(
    () => validateSlideSpec(spec("hero", { asset_id: "invented" })),
    /id real/,
  );
  assert.throws(
    () => validateSlideSpec(spec("cards", { asset_id: asset.id }), [asset]),
    /imagens usam/,
  );
  assert.throws(
    () =>
      validateSlideSpec(
        spec("split", {
          asset_id: asset.id,
          items: [{ title: "T", body: "B" }],
        }),
        [asset],
      ),
    /imagem com body/,
  );
});

test("charts require explicit sources and retain supplied numerical data exactly", () => {
  const values = [0, 1.25, 720];
  const chart = {
    labels: ["A", "B", "C"],
    values,
    source: "Pesquisa interna, dados fornecidos.",
  };
  const slide = composeSlide(spec("chart", { chart }));
  assert.deepEqual(
    slide.elements.find((element) => element.kind === "chart").data,
    { labels: chart.labels, values },
  );
  assert.ok(slide.elements.some((element) => element.text === chart.source));
  assert.throws(
    () => validateSlideSpec(spec("chart", { chart: { ...chart, source: "" } })),
    /chart.source/,
  );
  assert.throws(
    () =>
      validateSlideSpec(
        spec("chart", { chart: { ...chart, values: [1, -2, 3] } }),
      ),
    /não negativos/,
  );
  assert.throws(
    () =>
      validateSlideSpec(
        spec("chart", { chart: { ...chart, values: [1, Infinity, 3] } }),
      ),
    /finitos/,
  );
  assert.throws(
    () =>
      validateSlideSpec(spec("chart", { chart: { ...chart, values: [1] } })),
    /mesmo número/,
  );
  assert.throws(
    () => validateSlideSpec(spec("hero", { chart })),
    /requerem layout/,
  );
});

test("reduced motion is static and progressive motion remains finite and bounded", () => {
  for (const motion of ["none", "subtle", "expressive"]) {
    const slide = composeSlide(spec("steps"), { motion });
    assert.ok(
      slide.elements.every((element) =>
        ["none", "fade", "rise", "slide", "zoom"].includes(element.animation),
      ),
    );
    assert.ok(slide.elements.every((element) => element.delay_ms <= 600));
    if (motion === "none") {
      assert.equal(slide.transition, "none");
      assert.ok(
        slide.elements.every(
          (element) => element.animation === "none" && element.delay_ms === 0,
        ),
      );
    } else
      assert.ok(slide.elements.some((element) => element.animation !== "none"));
  }
});

test("composition is deterministic, respects themes, and leaves inputs unchanged", () => {
  const input = spec("hero");
  const theme = { ...palettes.paper },
    options = { theme, index: 2, total: 5, id: "s".repeat(80) };
  const before = JSON.stringify({ input, options });
  const first = composeSlide(input, options);
  assert.deepEqual(first, composeSlide(input, options));
  assert.equal(JSON.stringify({ input, options }), before);
  assert.equal(first.background, theme.background);
  assert.ok(
    first.elements.some((element) => element.style.color === theme.foreground),
  );
  assert.ok(
    first.elements.some((element) => element.style.fill === theme.accent),
  );
  assert.ok(first.elements.every((element) => element.id.length <= 80));
  assert.notEqual(
    first.elements[0].id,
    composeSlide(input, { ...options, id: "s".repeat(79) + "t" }).elements[0]
      .id,
  );
  assert.throws(
    () => composeSlide(input, { index: 2, total: 2 }),
    /index\/total/,
  );
  assert.throws(
    () => composeSlide(input, { theme: { ...theme, accent: "red" } }),
    /#RRGGBB/,
  );
});

test("layouts have distinct geometry and conflicting content receives actionable errors", () => {
  const signatures = LAYOUTS.map((layout) =>
    composeSlide(spec(layout))
      .elements.map(({ kind, x, y, w, h }) => `${kind}:${x},${y},${w},${h}`)
      .join("|"),
  );
  assert.equal(new Set(signatures).size, LAYOUTS.length);
  assert.throws(
    () => validateSlideSpec(spec("hero", { items: [{ title: "A" }] })),
    /não aceita items/,
  );
  assert.throws(
    () => validateSlideSpec(spec("cards", { items: [{ title: "A" }] })),
    /2 ou 3/,
  );
  assert.throws(
    () =>
      validateSlideSpec(spec("split", { body: "", items: [], asset_id: null })),
    /forneça body/,
  );
  assert.throws(
    () => validateSlideSpec(spec("hero", { body: { text: "Wrong" } })),
    /body.*texto/,
  );
});

test("split fits the real model's two comparison paragraphs without wasting heading space", () => {
  const input = spec("split", {
    title: "Superfícies impermeáveis e áreas verdes",
    body: "O desenho urbano influencia como o calor é absorvido durante o dia e liberado à noite. A combinação de sombra e superfícies permeáveis cria espaços mais confortáveis para as pessoas.",
    items: [
      {
        title: "Asfalto",
        body: "Retém calor diurno, elevando temperaturas superficiais significativamente.",
      },
      {
        title: "Vegetação",
        body: "Absorve radiação solar e resfria o ar através da evapotranspiração.",
      },
    ],
    takeaway: "Mais sombra e permeabilidade para a cidade",
  });
  for (const theme of [palettes.orbital, palettes.paper]) {
    const slide = composeSlide(input, { theme });
    const text = slide.elements.filter((element) => element.kind === "text");
    for (const item of input.items) {
      const paragraph = text.find(
        (element) => normalize(element.text) === item.body,
      );
      assert.ok(paragraph);
      assert.equal(paragraph.text.split("\n").length, 2);
      assert.ok(paragraph.style.fontSize >= 24);
    }
    for (let i = 0; i < text.length; i++)
      for (let j = i + 1; j < text.length; j++)
        assert.ok(
          !intersects(text[i], text[j]),
          `${text[i].id} overlaps ${text[j].id}`,
        );
  }
});

test("two-line takeaways and chart sources include the full line-height budget", () => {
  const takeaway =
    "Uma mensagem essencial para orientar decisões e transformar oportunidades em ações claras e efetivas";
  for (const layout of ["split", "cards", "steps", "closing"]) {
    const slide = composeSlide(spec(layout, { takeaway }), {
      theme: palettes.paper,
    });
    const element = slide.elements.find((entry) =>
      entry.id.includes("-takeaway-"),
    );
    assert.equal(normalize(element.text), takeaway);
    assert.equal(element.text.split("\n").length, 2);
    assert.ok(
      element.text.split("\n").length * element.style.fontSize * 1.18 + 4 <=
        element.h,
    );
  }
  const source =
    "Fonte: levantamento fornecido pelo usuário, com informações coletadas pelas equipes participantes e resultados consolidados para orientar o planejamento urbano.";
  const slide = composeSlide(
    spec("chart", { chart: { labels: ["A", "B"], values: [1, 2], source } }),
  );
  const element = slide.elements.find((entry) => entry.id.includes("-source-"));
  assert.equal(normalize(element.text), source);
  assert.equal(element.text.split("\n").length, 2);
  assert.ok(element.y + element.h < 620);
});

test("main titles balance two lines without orphaning the final word or changing copy", () => {
  const title = "Transformar concreto em refúgio verde";
  for (const theme of [palettes.orbital, palettes.paper]) {
    const slide = composeSlide(spec("hero", { title }), { theme });
    const heading = slide.elements.find((element) =>
      element.id.includes("-title-"),
    );
    const lines = heading.text.split("\n");
    assert.equal(lines.length, 2);
    assert.ok(lines.every((line) => line.split(" ").length >= 2));
    assert.deepEqual(heading.text.split(/\s+/u), title.split(" "));
    assert.equal(heading.style.fontSize, 68);
    assert.equal(normalize(heading.text), title);
  }
  const slide = composeSlide(spec("hero", { title: "Uma ideia clara" }));
  const heading = slide.elements.find((element) =>
    element.id.includes("-title-"),
  );
  assert.equal(heading.text, "Uma ideia clara");
});

test("steps use one ordinal marker and preserve quantities and unrelated numbers", () => {
  const titles = [
    "1. Mapear o espaço",
    "2) Planejar a sombra",
    "3.Cuidar da praça",
  ];
  const input = spec("steps", {
    body: "",
    takeaway: "",
    items: titles.map((title) => ({ title, body: "Defina uma ação viável." })),
  });
  const slide = composeSlide(input);
  for (let i = 0; i < titles.length; i++) {
    assert.equal(
      slide.elements.find((element) =>
        element.id.includes(`-step-number-${i}-`),
      ).text,
      String(i + 1).padStart(2, "0"),
    );
    assert.equal(
      normalize(
        slide.elements.find((element) =>
          element.id.includes(`-item-${i}-title-`),
        ).text,
      ),
      ["Mapear o espaço", "Planejar a sombra", "Cuidar da praça"][i],
    );
    assert.equal(input.items[i].title, titles[i]);
  }
  const numbered = [
    "1.5 metros de passagem",
    "2025. Revisar o plano",
    "2) Uma etapa anterior",
  ];
  const preserved = composeSlide(
    spec("steps", {
      body: "",
      takeaway: "",
      items: numbered.map((title) => ({ title, body: "" })),
    }),
  );
  for (let i = 0; i < numbered.length; i++)
    assert.equal(
      normalize(
        preserved.elements.find((element) =>
          element.id.includes(`-item-${i}-title-`),
        ).text,
      ),
      numbered[i],
    );
  const cards = composeSlide(
    spec("cards", {
      body: "",
      takeaway: "",
      items: titles.map((title) => ({ title, body: "" })),
    }),
  );
  assert.equal(
    normalize(
      cards.elements.find((element) => element.id.includes("-item-0-title-"))
        .text,
    ),
    titles[0],
  );
});

test("professional layouts export native editable text, shapes, and real chart series", async () => {
  const doc = {
    schema_version: 1,
    id: "composed",
    revision: 1,
    title: "Direção de arte",
    brief: "",
    theme: palettes.orbital,
    assets: [],
    css: "",
    script: "",
    slides: ["hero", "cards", "chart"].map((layout, index) =>
      composeSlide(spec(layout), { id: `slide-${index}`, index, total: 3 }),
    ),
  };
  const pptx = await buildPptx(doc);
  const zip = await JSZip.loadAsync(
    await pptx.write({ outputType: "nodebuffer" }),
  );
  const xml = await zip.file("ppt/slides/slide1.xml").async("string");
  assert.ok(xml.includes("<a:t>"));
  assert.ok(xml.includes("Uma ideia clara"));
  assert.ok(xml.includes("<p:sp>"));
  assert.ok(!xml.includes("<p:pic>"));
  const chart = await zip.file("ppt/charts/chart1.xml").async("string");
  assert.ok(chart.includes("<c:v>14</c:v>"));
  assert.ok(chart.includes("<c:v>21</c:v>"));
});
