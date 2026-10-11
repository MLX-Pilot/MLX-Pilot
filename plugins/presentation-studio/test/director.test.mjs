import test from "node:test";
import assert from "node:assert/strict";
import {
  directPresentation,
  validateQuantitativeClaims,
} from "../src/director.js";
import { palettes } from "../src/document.js";

const copy = (value) => JSON.parse(JSON.stringify(value));

function document() {
  return {
    schema_version: 1,
    id: "existing-project",
    revision: 7,
    title: "Projeto existente",
    brief: "Explique o uso responsável de IA para uma equipe de design.",
    theme: copy(palettes.orbital),
    css: ".text { letter-spacing: 1px; }",
    script: "console.log('custom animation');",
    assets: [
      {
        id: "brand-logo",
        name: "Marca",
        description: "Logotipo aprovado da equipe",
        data_url: "data:image/png;base64,PRIVATE_IMAGE_BYTES",
      },
    ],
    slides: [
      {
        id: "existing-slide",
        notes: "Notas que devem continuar salvas.",
        transition: "none",
        background: null,
        elements: [
          {
            id: "existing-heading",
            kind: "text",
            x: 80,
            y: 170,
            w: 1100,
            h: 180,
            text: "Conteúdo aprovado",
            style: { fontSize: 56 },
            animation: "none",
            delay_ms: 0,
            data: null,
            asset_id: null,
          },
        ],
      },
    ],
  };
}

function plan(overrides = {}) {
  return {
    title: "IA apoia o trabalho criativo",
    audience: "Equipe de design",
    goal: "Escolher um experimento responsável com IA",
    assumptions: ["A equipe está começando a experimentar IA."],
    style: "editorial",
    motion: "subtle",
    slides: [
      {
        layout: "hero",
        focus: "Use IA para explorar alternativas com critério.",
      },
      { layout: "closing", focus: "Escolha uma tarefa e revise o resultado." },
    ],
    ...overrides,
  };
}

function slide(
  layout = "hero",
  title = "Amplie possibilidades com critério",
  overrides = {},
) {
  return {
    layout,
    eyebrow: "Criação responsável",
    title,
    body: "Use IA para explorar alternativas. A equipe avalia a pertinência de cada proposta.",
    items: [],
    takeaway: "O julgamento continua com as pessoas.",
    notes:
      "Compare alternativas e revise possíveis erros antes de compartilhar.",
    asset_id: null,
    ...overrides,
  };
}

const review = (revisions = []) => ({
  summary: "História revisada e coerente com o pedido.",
  revisions,
});
const closing = () => slide("closing", "Comece com uma tarefa pequena");

function scriptedGenerator(responses) {
  const calls = [];
  return {
    calls,
    generate: async (input) => {
      const index = calls.length;
      calls.push({
        messages: copy(input.messages),
        options: copy(input.options),
        signal: input.signal,
      });
      assert.ok(index < responses.length, `Unexpected model call ${index + 1}`);
      const response =
        typeof responses[index] === "function"
          ? await responses[index](input)
          : responses[index];
      return typeof response === "string" ? response : JSON.stringify(response);
    },
  };
}

function slideHasText(value, text) {
  const normalize = (input) => input.replace(/\s+/g, " ").trim();
  return value.elements.some(
    (element) =>
      element.kind === "text" && normalize(element.text) === normalize(text),
  );
}

test("the director plans, composes, reviews and inspects a detached candidate without mutating the saved project", async () => {
  const original = document();
  const before = copy(original);
  const model = scriptedGenerator([plan(), slide(), closing(), review()]);
  const progress = [];
  const inspections = [];
  const result = await directPresentation({
    document: original,
    request: "Crie dois slides sobre IA no design.",
    generate: model.generate,
    inspect: async (candidate) => {
      inspections.push(copy(candidate));
      assert.deepEqual(original, before);
      return [
        {
          slide_id: candidate.slides[0].id,
          severity: "warning",
          message: "Verifique a descrição da imagem.",
        },
      ];
    },
    onProgress: (message) => progress.push(message),
  });

  assert.equal(model.calls.length, 4);
  assert.equal(inspections.length, 1);
  assert.equal(result.document.slides.length, 2);
  assert.equal(result.summary, review().summary);
  assert.equal(result.diagnostics[0].severity, "warning");
  assert.deepEqual(original, before);
  assert.notEqual(result.document, original);
  assert.notEqual(result.document.assets, original.assets);
  assert.notEqual(result.document.theme, original.theme);
  assert.match(
    result.document.slides[0].notes,
    /A equipe está começando a experimentar IA/,
  );
  assert.deepEqual(
    progress
      .filter((message) => /^\d\/4/.test(message))
      .map((message) => message.slice(0, 3)),
    ["1/4", "2/4", "2/4", "3/4", "4/4"],
  );

  const planningInput = JSON.parse(model.calls[0].messages[1].content);
  assert.equal(planningInput.context.selected_slide, undefined);
  assert.equal(
    JSON.stringify(model.calls).includes("PRIVATE_IMAGE_BYTES"),
    false,
  );
  assert.deepEqual(
    JSON.parse(model.calls[2].messages[1].content).previous_titles,
    [slide().title],
  );
  result.document.assets[0].description = "Outra descrição";
  result.document.theme.accent = "#ffffff";
  assert.deepEqual(original, before);
});

test("append preserves existing slide IDs, assets, palette and code even when the model proposes a theme", async () => {
  const original = document();
  const before = copy(original);
  const model = scriptedGenerator([
    plan({ theme: copy(palettes.paper) }),
    slide(),
    closing(),
    review(),
  ]);
  const result = await directPresentation({
    document: original,
    request: "Adicione dois slides.",
    mode: "append",
    generate: model.generate,
    inspect: async () => [],
  });

  assert.deepEqual(original, before);
  assert.equal(result.document.title, before.title);
  assert.equal(result.document.revision, before.revision);
  assert.deepEqual(result.document.slides[0], before.slides[0]);
  assert.equal(result.document.slides.length, 3);
  assert.equal(
    new Set(result.document.slides.map((value) => value.id)).size,
    3,
  );
  assert.deepEqual(result.document.assets, before.assets);
  assert.deepEqual(result.document.theme, before.theme);
  assert.equal(result.document.css, before.css);
  assert.equal(result.document.script, before.script);
  assert.equal(
    JSON.parse(model.calls[1].messages[1].content).theme.accent,
    before.theme.accent,
  );
});

test("create replaces old slides and code, preserving the palette unless a new palette is supplied", async (t) => {
  for (const changeTheme of [false, true]) {
    await t.test(
      changeTheme ? "explicit palette change" : "existing palette",
      async () => {
        const original = document();
        const before = copy(original);
        const theme = copy(palettes.paper);
        const model = scriptedGenerator([
          plan(changeTheme ? { theme } : {}),
          slide(),
          closing(),
          review(),
        ]);
        const result = await directPresentation({
          document: original,
          request: changeTheme
            ? "Recrie em dois slides com uma paleta clara de papel e terracota."
            : "Recrie em dois slides.",
          generate: model.generate,
          inspect: async () => [],
        });

        assert.equal(result.document.title, plan().title);
        assert.equal(
          result.document.slides.some((value) => value.id === "existing-slide"),
          false,
        );
        assert.equal(result.document.css, "");
        assert.equal(result.document.script, "");
        assert.deepEqual(result.document.assets, before.assets);
        assert.deepEqual(
          result.document.theme,
          changeTheme ? theme : before.theme,
        );
        assert.deepEqual(original, before);
      },
    );
  }
});

test("a malformed plan is repaired once before any slide is generated", async () => {
  const model = scriptedGenerator([
    "{not valid JSON",
    plan(),
    slide(),
    closing(),
    review(),
  ]);
  let inspections = 0;
  const result = await directPresentation({
    document: document(),
    request: "Crie dois slides.",
    generate: model.generate,
    inspect: async () => {
      inspections++;
      return [];
    },
  });

  assert.equal(result.document.slides.length, 2);
  assert.equal(model.calls.length, 5);
  assert.equal(inspections, 1);
  assert.equal(model.calls[1].messages.length, 3);
  assert.match(model.calls[1].messages[2].content, /resposta foi rejeitada/i);
  assert.deepEqual(
    model.calls[1].messages.slice(0, 2),
    model.calls[0].messages,
  );
});

test("a second invalid plan ends the run with the original project intact", async () => {
  const original = document();
  const before = copy(original);
  const model = scriptedGenerator([plan({ slides: [] }), plan({ slides: [] })]);
  let inspections = 0;
  await assert.rejects(
    directPresentation({
      document: original,
      request: "Crie dois slides.",
      generate: model.generate,
      inspect: async () => {
        inspections++;
        return [];
      },
    }),
    /Planeje/,
  );

  assert.equal(model.calls.length, 2);
  assert.equal(inspections, 0);
  assert.deepEqual(original, before);
});

test("cancellation during generation stops subsequent requests and leaves saved content intact", async () => {
  const original = document();
  const before = copy(original);
  const controller = new AbortController();
  const model = scriptedGenerator([
    plan(),
    ({ signal }) => {
      assert.equal(signal, controller.signal);
      controller.abort();
      return slide();
    },
  ]);
  let inspections = 0;
  await assert.rejects(
    directPresentation({
      document: original,
      request: "Crie dois slides.",
      generate: model.generate,
      inspect: async () => {
        inspections++;
        return [];
      },
      signal: controller.signal,
    }),
    { name: "AbortError" },
  );

  assert.equal(model.calls.length, 2);
  assert.equal(inspections, 0);
  assert.deepEqual(original, before);
});

test("cancellation after rendering does not return a partially inspected candidate", async () => {
  const original = document();
  const before = copy(original);
  const controller = new AbortController();
  const model = scriptedGenerator([plan(), slide(), closing(), review()]);
  await assert.rejects(
    directPresentation({
      document: original,
      request: "Crie dois slides.",
      generate: model.generate,
      inspect: async (_candidate, { signal }) => {
        assert.equal(signal, controller.signal);
        controller.abort();
        return [];
      },
      signal: controller.signal,
    }),
    { name: "AbortError" },
  );

  assert.equal(model.calls.length, 4);
  assert.deepEqual(original, before);
});

test("invented chart values are rejected even when those numbers appear only in slide geometry", async (t) => {
  for (const values of [
    [943, 817],
    [80, 170],
  ]) {
    await t.test(values.join(", "), async () => {
      const original = document();
      const before = copy(original);
      const chart = slide("chart", "Compare os resultados", {
        chart: {
          labels: ["Antes", "Depois"],
          values,
          source: "Briefing fornecido",
        },
      });
      const model = scriptedGenerator([
        plan({
          slides: [{ layout: "chart", focus: "Compare os resultados." }],
        }),
        chart,
        chart,
      ]);
      let inspections = 0;
      await assert.rejects(
        directPresentation({
          document: original,
          request:
            "Explique o papel da IA no design, sem dados numéricos disponíveis.",
          generate: model.generate,
          inspect: async () => {
            inspections++;
            return [];
          },
        }),
        /valores ausentes|dados reais|dados fornecidos/,
      );

      assert.equal(model.calls.length, 3);
      assert.equal(inspections, 0);
      assert.deepEqual(original, before);
    });
  }
});

test("a chart can use numeric values actually supplied in the request", async () => {
  const chart = slide("chart", "A equipe escolheu prototipar", {
    chart: {
      labels: ["Pesquisa", "Protótipo"],
      values: [18, 32],
      source: "Votação da equipe fornecida pelo usuário",
    },
  });
  const model = scriptedGenerator([
    plan({
      slides: [{ layout: "chart", focus: "Mostre o resultado da votação." }],
    }),
    chart,
    review(),
  ]);
  const result = await directPresentation({
    document: document(),
    request:
      "Mostre a votação da equipe: Pesquisa 18; Protótipo 32. Fonte: votação da equipe.",
    generate: model.generate,
    inspect: async () => [],
  });

  const element = result.document.slides[0].elements.find(
    (value) => value.kind === "chart",
  );
  assert.deepEqual(element.data.values, [18, 32]);
  assert.deepEqual(element.data.labels, ["Pesquisa", "Protótipo"]);
});

test("rendering errors trigger a targeted slide repair and a second full inspection", async () => {
  const original = document();
  const before = copy(original);
  const repaired = slide("hero", "Explore com critério", {
    body: "Compare alternativas. Revise antes de usar.",
  });
  const model = scriptedGenerator([
    plan(),
    slide(),
    closing(),
    review(),
    repaired,
  ]);
  const inspections = [];
  const result = await directPresentation({
    document: original,
    request: "Crie dois slides.",
    generate: model.generate,
    inspect: async (candidate) => {
      inspections.push(copy(candidate));
      return inspections.length === 1
        ? [
            {
              slide_id: candidate.slides[0].id,
              severity: "error",
              message: "O título ultrapassa a caixa de texto.",
            },
          ]
        : [];
    },
  });

  assert.equal(inspections.length, 2);
  assert.equal(model.calls.length, 5);
  assert.deepEqual(JSON.parse(model.calls[4].messages[1].content).repair, [
    "O título ultrapassa a caixa de texto.",
  ]);
  assert.deepEqual(
    inspections[0].slides.map((value) => value.id),
    inspections[1].slides.map((value) => value.id),
  );
  assert.deepEqual(inspections[0].slides[1], inspections[1].slides[1]);
  assert.ok(slideHasText(result.document.slides[0], repaired.title));
  assert.deepEqual(result.diagnostics, []);
  assert.deepEqual(original, before);
});

test("persistent rendering errors fail instead of returning a candidate with clipped content", async () => {
  const original = document();
  const before = copy(original);
  const model = scriptedGenerator([
    plan(),
    slide(),
    closing(),
    review(),
    slide("hero", "Título menor"),
  ]);
  let inspections = 0;
  await assert.rejects(
    directPresentation({
      document: original,
      request: "Crie dois slides.",
      generate: model.generate,
      inspect: async (candidate) => {
        inspections++;
        return [
          {
            slide_id: candidate.slides[0].id,
            severity: "error",
            message: "Texto ainda cortado.",
          },
        ];
      },
    }),
    /Texto ainda cortado.*projeto salvo foi preservado/s,
  );

  assert.equal(inspections, 2);
  assert.equal(model.calls.length, 5);
  assert.deepEqual(original, before);
});

test("editorial revisions apply by index without reordering slides or duplicating their IDs", async () => {
  const first = slide("hero", "Escolhas melhores começam com alternativas");
  const last = slide("closing", "Combine um experimento com revisão humana");
  const model = scriptedGenerator([
    plan(),
    slide(),
    closing(),
    review([
      { index: 1, slide: last },
      { index: 0, slide: first },
    ]),
  ]);
  let inspected;
  const result = await directPresentation({
    document: document(),
    request: "Crie dois slides.",
    generate: model.generate,
    inspect: async (candidate) => {
      inspected = copy(candidate);
      return [];
    },
  });

  assert.equal(model.calls.length, 4);
  assert.equal(result.document.slides.length, 2);
  assert.ok(slideHasText(result.document.slides[0], first.title));
  assert.ok(slideHasText(result.document.slides[1], last.title));
  assert.deepEqual(
    result.document.slides.map((value) => value.id),
    inspected.slides.map((value) => value.id),
  );
  assert.equal(
    new Set(result.document.slides.map((value) => value.id)).size,
    2,
  );
});

test("supplied copy, speaker notes, chart data and table facts reach every generation stage", async () => {
  const original = document();
  original.slides[0].notes =
    "A oficina ocorreu em Recife. A conclusão está pendente de revisão.";
  original.slides[0].elements[0].text =
    "O projeto usa materiais reaproveitados.";
  original.slides[0].elements.push(
    {
      ...copy(original.slides[0].elements[0]),
      id: "source-chart",
      kind: "chart",
      text: "Resultados informados pela equipe",
      data: { labels: ["Oficinas", "Protótipos"], values: [17, 23] },
    },
    {
      ...copy(original.slides[0].elements[0]),
      id: "source-table",
      kind: "table",
      text: "Responsáveis",
      data: {
        rows: [
          ["Pessoa", "Responsabilidade"],
          ["Lia", "Revisão editorial"],
        ],
      },
    },
  );
  const before = copy(original);
  const model = scriptedGenerator([
    plan(),
    slide(),
    closing(),
    review(),
    slide("hero", "Crie e revise"),
  ]);
  let inspections = 0;
  await directPresentation({
    document: original,
    request: "Recrie dois slides usando o conteúdo e as notas já fornecidos.",
    generate: model.generate,
    inspect: async (candidate) =>
      ++inspections === 1
        ? [
            {
              slide_id: candidate.slides[0].id,
              severity: "error",
              message: "Encurte o título.",
            },
          ]
        : [],
  });

  assert.equal(model.calls.length, 5);
  for (const [index, call] of model.calls.entries()) {
    const input = JSON.parse(call.messages[1].content);
    const source = input.context?.source_content ?? input.source_content;
    assert.ok(
      Array.isArray(source),
      `Generation stage ${index + 1} must receive original source material`,
    );
    assert.equal(source[0].notes, original.slides[0].notes);
    assert.ok(
      source[0].content.some(
        (item) => item.text === original.slides[0].elements[0].text,
      ),
    );
    assert.ok(
      source[0].content.some(
        (item) =>
          JSON.stringify(item.data) ===
          JSON.stringify(original.slides[0].elements[1].data),
      ),
    );
    assert.ok(
      source[0].content.some(
        (item) =>
          JSON.stringify(item.data) ===
          JSON.stringify(original.slides[0].elements[2].data),
      ),
    );
    assert.equal(JSON.stringify(source).includes("PRIVATE_IMAGE_BYTES"), false);
    assert.equal(
      source[0].content.some(
        (item) => "x" in item || "y" in item || "style" in item,
      ),
      false,
    );
  }
  assert.deepEqual(original, before);
});

test("oversized input is refused before contacting the model and without changing the project", async (t) => {
  const cases = [
    {
      name: "request longer than the local input limit",
      setup: (original) => ({ original, request: "x".repeat(6001) }),
    },
    {
      name: "source notes exceeding the source budget",
      setup: (original) => {
        original.slides[0].notes = "Texto ".repeat(2100);
        return { original, request: "Recrie dois slides." };
      },
    },
    {
      name: "UTF-8 source bytes exceeding the source budget",
      setup: (original) => {
        original.slides[0].notes = "🟣".repeat(2600);
        return { original, request: "Recrie dois slides." };
      },
    },
    {
      name: "UTF-8 prompt bytes exceeding the total call budget",
      setup: (original) => {
        original.title = "🟣".repeat(5000);
        return { original, request: "Recrie dois slides." };
      },
    },
  ];
  for (const { name, setup } of cases) {
    await t.test(name, async () => {
      const { original, request } = setup(document());
      const before = copy(original);
      let modelCalls = 0;
      let inspections = 0;
      await assert.rejects(
        directPresentation({
          document: original,
          request,
          generate: async () => {
            modelCalls++;
            return JSON.stringify(plan());
          },
          inspect: async () => {
            inspections++;
            return [];
          },
        }),
        /contexto|material|caracteres|limite|orçamento/i,
      );

      assert.equal(modelCalls, 0);
      assert.equal(inspections, 0);
      assert.deepEqual(original, before);
    });
  }
});

test("an editorial revision that omits notes preserves the slide's already composed speaker notes", async () => {
  const originalClosing = closing();
  originalClosing.notes =
    "Reserve tempo para uma rodada de revisão e registre dúvidas antes de publicar.";
  const revisedClosing = slide("closing", "Revise antes de compartilhar");
  delete revisedClosing.notes;
  const model = scriptedGenerator([
    plan(),
    slide(),
    originalClosing,
    review([{ index: 1, slide: revisedClosing }]),
  ]);
  const result = await directPresentation({
    document: document(),
    request: "Crie dois slides.",
    generate: model.generate,
    inspect: async () => [],
  });

  assert.ok(slideHasText(result.document.slides[1], revisedClosing.title));
  assert.equal(result.document.slides[1].notes, originalClosing.notes);
});

test("a twelve-slide deck is reviewed in bounded batches with absolute indices and complete notes preserved", async () => {
  const specs = Array.from({ length: 12 }, (_, index) =>
    slide(
      index === 11 ? "closing" : index % 2 ? "statement" : "hero",
      `Etapa ${String.fromCharCode(65 + index)} do projeto`,
      {
        notes: "A equipe revisa as alternativas antes de publicar. ".repeat(35),
      },
    ),
  );
  const fullPlan = plan({
    slides: specs.map((spec) => ({ layout: spec.layout, focus: spec.title })),
  });
  const revised = { ...specs[11], title: "Compartilhe a decisão revisada" };
  delete revised.notes;
  const model = scriptedGenerator([
    fullPlan,
    ...specs,
    review(),
    review(),
    review(),
    review([{ index: 11, slide: revised }]),
  ]);
  const original = document();
  const before = copy(original);
  const result = await directPresentation({
    document: original,
    request: "Crie doze slides com notas completas para o apresentador.",
    generate: model.generate,
    inspect: async () => [],
  });

  assert.equal(model.calls.length, 17);
  assert.equal(result.document.slides.length, 12);
  const batches = model.calls
    .slice(13)
    .map((call) => JSON.parse(call.messages[1].content));
  assert.deepEqual(
    batches.map((batch) => batch.index_offset),
    [0, 3, 6, 9],
  );
  assert.deepEqual(
    batches.map((batch) => batch.slides.length),
    [3, 3, 3, 3],
  );
  assert.ok(batches.every((batch) => batch.outline.length === 12));
  assert.ok(
    model.calls.every(
      (call) =>
        Buffer.byteLength(
          call.messages.map((message) => message.content).join("\n"),
          "utf8",
        ) <= 18000,
    ),
  );
  for (let index = 1; index < specs.length; index++) {
    assert.equal(
      result.document.slides[index].notes,
      specs[index].notes.trim(),
    );
  }
  assert.ok(slideHasText(result.document.slides[11], revised.title));
  assert.deepEqual(original, before);
});

test("unsupported temperatures, percentages and deadlines are rejected in slide copy and speaker notes", () => {
  for (const field of ["title", "body", "takeaway", "notes", "eyebrow"]) {
    for (const claim of ["Reduz até5°C", "Ganho20%", "Entrega em90dias"]) {
      assert.throws(
        () =>
          validateQuantitativeClaims(
            slide("hero", "Explore alternativas", { [field]: claim }),
            "Use design para melhorar o conforto urbano.",
          ),
        /quantitativa não fornecida/i,
        `${field} must not contain the unsupported claim: ${claim}`,
      );
    }
  }
  for (const field of ["title", "body"]) {
    assert.throws(
      () =>
        validateQuantitativeClaims(
          slide("cards", "Explore alternativas", {
            items: [
              {
                title: "Primeiro passo",
                body: "Compare ideias",
                [field]: "Melhora de 20%",
              },
            ],
          }),
          "Compare opções de projeto.",
        ),
      /quantitativa não fornecida/i,
      `items.${field} must not bypass quantitative grounding`,
    );
  }
});

test("quantitative grounding handles spacing, letter case and underscore emphasis", () => {
  for (const claim of ["Reduz 5 ° c", "GANHO DE _20%_", "PRAZO DE _90 DIAS_"]) {
    assert.throws(
      () =>
        validateQuantitativeClaims(
          slide("hero", "Explore alternativas", { notes: claim }),
          "Dados quantitativos não foram fornecidos.",
        ),
      /quantitativa não fornecida/i,
      `Formatting must not hide the unsupported claim: ${claim}`,
    );
  }
  for (const [claim, source] of [
    ["Reduz até 5 ° C", "Estudo fornecido: redução de 5°C."],
    ["GANHO DE _20%_", "O usuário informou um ganho de 20 %."],
    ["PRAZO DE 90 DIAS", "O prazo aprovado é de _90 dias_."],
    ["O ganho foi de 12,5%", "Dado fornecido: 12.5 %."],
  ]) {
    assert.doesNotThrow(
      () =>
        validateQuantitativeClaims(
          slide("hero", "Explore alternativas", { notes: claim }),
          source,
        ),
      `The same supplied quantity should remain valid: ${claim}`,
    );
  }
});

test("a quantity is not grounded by appearing as a substring of a different source quantity", () => {
  for (const [claim, source] of [
    ["Reduz até 5°C", "Temperatura informada: 15°C."],
    ["Ganho de 20%", "O dado enviado foi 120%."],
    ["Prazo de 90 dias", "O prazo informado foi 190 dias."],
  ]) {
    assert.throws(
      () =>
        validateQuantitativeClaims(
          slide("hero", "Explore alternativas", { body: claim }),
          source,
        ),
      /quantitativa não fornecida/i,
      `${source} does not substantiate ${claim}`,
    );
  }
});

test("an invented temperature introduced by visual repair fails before any candidate is returned", async () => {
  const original = document();
  const before = copy(original);
  const invented = slide("hero", "Mais conforto com design", {
    notes: "O projeto reduz até 5°C na região.",
  });
  const model = scriptedGenerator([
    plan(),
    slide(),
    closing(),
    review(),
    invented,
    invented,
  ]);
  let inspections = 0;
  await assert.rejects(
    directPresentation({
      document: original,
      request: "Crie dois slides sobre design responsável.",
      generate: model.generate,
      inspect: async (candidate) => {
        inspections++;
        return [
          {
            slide_id: candidate.slides[0].id,
            severity: "error",
            message: "Encurte o título.",
          },
        ];
      },
    }),
    /quantitativa não fornecida/i,
  );

  assert.equal(model.calls.length, 6);
  assert.equal(inspections, 1);
  assert.deepEqual(original, before);
});

test("slide counts and years in a request cannot be repurposed as chart values", async () => {
  const original = document();
  const before = copy(original);
  const chart = slide("chart", "Compare antes e depois", {
    chart: {
      labels: ["Antes", "Depois"],
      values: [3, 2026],
      source: "Pedido do usuário",
    },
  });
  const model = scriptedGenerator([
    plan({ slides: [{ layout: "chart", focus: "Compare antes e depois." }] }),
    chart,
    chart,
  ]);
  let inspections = 0;
  await assert.rejects(
    directPresentation({
      document: original,
      request: "Crie 3 slides sobre 2026.",
      generate: model.generate,
      inspect: async () => {
        inspections++;
        return [];
      },
    }),
    /vínculo com os rótulos|pares explícitos|valores ausentes/i,
  );

  assert.equal(model.calls.length, 3);
  assert.equal(inspections, 0);
  assert.deepEqual(original, before);
});

test("quantities supplied across line breaks in original notes remain available as evidence", async () => {
  const original = document();
  original.slides[0].notes =
    "Prazo aprovado: 90\nDIAS. Ganho medido: 20\n%. Redução informada: 5\n°C.";
  const before = copy(original);
  const supplied = slide("hero", "Use os resultados fornecidos", {
    body: "O ganho medido foi de 20%, com redução informada de 5°C.",
    notes: "O prazo aprovado é de 90 dias.",
  });
  const model = scriptedGenerator([plan(), supplied, closing(), review()]);
  const result = await directPresentation({
    document: original,
    request: "Crie dois slides usando somente os dados das notas existentes.",
    generate: model.generate,
    inspect: async () => [],
  });

  assert.equal(model.calls.length, 4);
  assert.ok(slideHasText(result.document.slides[0], supplied.body));
  assert.match(result.document.slides[0].notes, /prazo aprovado é de 90 dias/);
  assert.deepEqual(original, before);
});

test("table cells remain evidence for quantitative copy and chart label/value pairs", async () => {
  const original = document();
  original.slides[0].elements.push({
    ...copy(original.slides[0].elements[0]),
    id: "supplied-table",
    kind: "table",
    text: "Resultados fornecidos",
    data: {
      rows: [
        ["Indicador", "Resultado"],
        ["Participação", "20 pessoas"],
        ["Receita", "R$ 500"],
        ["Pesquisa", "18"],
        ["Protótipo", "32"],
      ],
    },
  });
  const before = copy(original);
  const supplied = slide("hero", "A oficina recebeu 20 pessoas", {
    notes: "A receita registrada foi de R$ 500.",
  });
  const chart = slide("chart", "Resultados da votação", {
    chart: {
      labels: ["Pesquisa", "Protótipo"],
      values: [18, 32],
      source: "Tabela fornecida pelo usuário",
    },
  });
  const model = scriptedGenerator([
    plan({
      slides: [
        { layout: "hero", focus: "Apresente a participação registrada." },
        { layout: "chart", focus: "Mostre o resultado da votação fornecida." },
      ],
    }),
    supplied,
    chart,
    review(),
  ]);
  const result = await directPresentation({
    document: original,
    request: "Crie dois slides com os resultados da tabela existente.",
    generate: model.generate,
    inspect: async () => [],
  });
  assert.equal(model.calls.length, 4);
  assert.ok(slideHasText(result.document.slides[0], supplied.title));
  assert.match(result.document.slides[0].notes, /R\$ 500/);
  assert.deepEqual(
    result.document.slides[1].elements.find((e) => e.kind === "chart").data
      .values,
    [18, 32],
  );
  assert.deepEqual(original, before);
});

test("currency prefixes and currency units require supplied monetary evidence", () => {
  for (const claim of [
    "R$ 500 de receita",
    "$500 de receita",
    "€500 de receita",
    "£500 de receita",
    "USD 500 de receita",
    "500 euros de receita",
    "500 dólares de receita",
    "500 GBP de receita",
    "R$ 1.500,50 de receita",
  ]) {
    const spec = slide("hero", "Resultados", { notes: claim });
    assert.throws(
      () =>
        validateQuantitativeClaims(spec, "Dados financeiros não fornecidos."),
      /quantitativa não fornecida/i,
      `Unsupported money must be rejected: ${claim}`,
    );
    assert.doesNotThrow(() =>
      validateQuantitativeClaims(spec, `Valor informado: ${claim}.`),
    );
  }
  assert.throws(
    () =>
      validateQuantitativeClaims(
        slide("hero", "Resultados", { notes: "Receita de R$ 1.500,50." }),
        "Receita informada de R$ 1.500,00.",
      ),
    /quantitativa não fornecida/i,
    "Different cents must not match a shared currency prefix",
  );
});

test("append updates generated page totals while preserving manual footer text", async () => {
  const original = document();
  const auto = {
    ...original.slides[0].elements[0],
    id: "directed-original-page-9",
    text: "01 / 01",
  };
  const manual = { ...auto, id: "manual-page", text: "01 / 01" };
  original.slides[0].elements.push(auto, manual);
  const model = scriptedGenerator([plan(), slide(), closing(), review()]);
  const result = await directPresentation({
    document: original,
    request: "Acrescente dois slides.",
    mode: "append",
    generate: model.generate,
    inspect: async () => [],
  });
  assert.equal(
    result.document.slides[0].elements.find((e) => e.id === auto.id).text,
    "01 / 03",
  );
  assert.equal(
    result.document.slides[0].elements.find((e) => e.id === manual.id).text,
    "01 / 01",
  );
  assert.equal(auto.text, "01 / 01");
});

test("append rejects existing visual defects before spending model calls", async () => {
  const original = document(),
    before = copy(original);
  let called = false;
  await assert.rejects(
    directPresentation({
      document: original,
      request: "Acrescente dois slides.",
      mode: "append",
      generate: async () => {
        called = true;
      },
      inspect: async () => [{ code: "TEXT_OVERFLOW", severity: "error" }],
    }),
    /slides existentes/,
  );
  assert.equal(called, false);
  assert.deepEqual(original, before);
});

test("editorial review can patch copy without regenerating geometry, assets or notes", async () => {
  const first = slide("hero", "Amplie possibilidades", {
    asset_id: "brand-logo",
  });
  const model = scriptedGenerator([
    plan(),
    first,
    closing(),
    review([
      { index: 0, changes: { title: "Explore alternativas com critério" } },
    ]),
  ]);
  const result = await directPresentation({
    document: document(),
    request: "Uma apresentação sobre IA.",
    generate: model.generate,
    inspect: async () => [],
  });
  assert.ok(
    result.document.slides[0].elements.some((e) =>
      e.text.includes("Explore alternativas"),
    ),
  );
  assert.ok(
    result.document.slides[0].elements.some((e) => e.asset_id === "brand-logo"),
  );
  assert.ok(result.document.slides[0].notes.startsWith(first.notes));
});
