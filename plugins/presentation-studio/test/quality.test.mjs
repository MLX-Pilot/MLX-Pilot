import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { auditDocument, auditSlide } from "../src/quality.js";
import {
  buildPptx,
  exportPdf,
  inspectPresentation,
  renderFrame,
} from "../src/export.js";

const text = (id, overrides = {}) => ({
  id,
  kind: "text",
  x: 64,
  y: 64,
  w: 600,
  h: 100,
  text: "Uma ideia clara",
  style: { fontSize: 32 },
  ...overrides,
});
function deck(elements = [text("title")]) {
  return {
    id: "quality",
    revision: 1,
    title: "Qualidade",
    css: "",
    assets: [],
    theme: {
      background: "#ffffff",
      foreground: "#111111",
      accent: "#005ea8",
      muted: "#555555",
      font: "Arial",
    },
    slides: [{ id: "slide-1", elements, transition: "fade", notes: "" }],
  };
}
const codes = (doc) => auditDocument(doc).map((item) => item.code);

test("checks contrast with text size, bold weight and effective opacity", () => {
  assert.ok(!codes(deck()).includes("LOW_CONTRAST"));
  assert.ok(
    codes(
      deck([text("body", { style: { fontSize: 20, color: "#888888" } })]),
    ).includes("LOW_CONTRAST"),
  );
  assert.ok(
    !codes(
      deck([text("title", { style: { fontSize: 32, color: "#888888" } })]),
    ).includes("LOW_CONTRAST"),
  );
  assert.ok(
    !codes(
      deck([
        text("bold", { style: { fontSize: 20, color: "#888888", bold: true } }),
      ]),
    ).includes("LOW_CONTRAST"),
  );
  assert.ok(
    codes(deck([text("faint", { style: { opacity: 0.1 } })])).includes(
      "LOW_CONTRAST",
    ),
  );
  assert.ok(
    codes(deck([text("hidden", { style: { opacity: 0 } })])).includes(
      "INVISIBLE_TEXT",
    ),
  );
});

test("contrast accounts for covering shape backgrounds and transparent fills", () => {
  const backdrop = {
    id: "panel",
    kind: "shape",
    x: 32,
    y: 32,
    w: 700,
    h: 250,
    style: { fill: "#111111" },
  };
  assert.ok(
    !codes(
      deck([backdrop, text("light", { style: { color: "#ffffff" } })]),
    ).includes("LOW_CONTRAST"),
  );
  assert.ok(codes(deck([backdrop, text("dark")])).includes("LOW_CONTRAST"));
  backdrop.style.opacity = 0.1;
  assert.ok(
    codes(
      deck([backdrop, text("light", { style: { color: "#ffffff" } })]),
    ).includes("LOW_CONTRAST"),
  );
  const fullFill = text("label", {
    style: { color: "#ffffff", fill: "#111111" },
  });
  assert.ok(!codes(deck([fullFill])).includes("LOW_CONTRAST"));
});

test("variable image backgrounds and custom CSS are explicitly unverified", () => {
  const doc = deck([
    { id: "photo", kind: "image", x: 0, y: 0, w: 1280, h: 720 },
    text("title"),
  ]);
  assert.ok(codes(doc).includes("CONTRAST_UNVERIFIED"));
  assert.ok(!codes(doc).includes("LOW_CONTRAST"));
  const styled = deck();
  styled.css = ".slide {background:linear-gradient(black,white)}";
  const results = auditDocument(styled);
  assert.equal(
    results.find((item) => item.code === "CONTRAST_UNVERIFIED").severity,
    "warning",
  );
});

test("overlap checks only text boxes and keeps decoration layering valid", () => {
  const decoration = {
    id: "panel",
    kind: "shape",
    x: 40,
    y: 40,
    w: 700,
    h: 200,
    style: { fill: "#eeeeee" },
  };
  assert.ok(!codes(deck([decoration, text("title")])).includes("TEXT_OVERLAP"));
  const diagnostics = auditDocument(
    deck([text("title"), text("body", { y: 100 })]),
  );
  const overlap = diagnostics.find((item) => item.code === "TEXT_OVERLAP");
  assert.equal(overlap.element_id, "body");
  assert.equal(overlap.severity, "error");
  assert.match(overlap.message, /title/);
  assert.ok(
    !codes(deck([text("title"), text("body", { y: 164 })])).includes(
      "TEXT_OVERLAP",
    ),
  );
});

test("bounds are errors; dense or small readable content produces warnings", () => {
  const doc = deck([
    text("outside", { x: 1200 }),
    text("dense", {
      y: 200,
      style: { fontSize: 16 },
      text: "palavra ".repeat(80),
    }),
  ]);
  const diagnostics = auditDocument(doc);
  assert.equal(
    diagnostics.find((item) => item.code === "OUT_OF_BOUNDS").severity,
    "error",
  );
  for (const code of ["SMALL_TEXT", "DENSE_TEXT", "DENSE_SLIDE"])
    assert.equal(
      diagnostics.find((item) => item.code === code).severity,
      "warning",
    );
});

test("DOM measurements detect clipping and actual geometry", () => {
  const doc = deck();
  const frame = new JSDOM(
    '<section data-slide-id="slide-1"><div data-id="title">Texto</div></section>',
  ).window.document;
  const node = frame.querySelector("div");
  Object.defineProperties(node, {
    scrollHeight: { value: 140 },
    clientHeight: { value: 100 },
  });
  node.getBoundingClientRect = () => ({
    left: 1200,
    top: 64,
    width: 600,
    height: 100,
  });
  frame.querySelector("section").getBoundingClientRect = () => ({
    left: 0,
    top: 0,
    width: 1280,
    height: 720,
  });
  const result = auditSlide(doc, doc.slides[0], frame);
  assert.ok(result.some((item) => item.code === "TEXT_OVERFLOW"));
  assert.ok(result.some((item) => item.code === "OUT_OF_BOUNDS"));
});

test("hidden ancestors and CSS opacity cannot silently hide exported text", () => {
  const doc = deck();
  doc.css = ".slide {opacity:0}";
  const frame = new JSDOM(
    '<section data-slide-id="slide-1" style="opacity:0"><div data-id="title">Texto</div></section>',
  ).window.document;
  assert.ok(
    auditSlide(doc, doc.slides[0], frame).some(
      (item) => item.code === "INVISIBLE_TEXT",
    ),
  );
});

function browserStub(prepare = () => {}) {
  const original = globalThis.document;
  const browser = new JSDOM("<!doctype html><body></body>");
  const document = browser.window.document;
  const append = document.body.append.bind(document.body);
  const frames = [];
  document.body.append = (frame) => {
    append(frame);
    const rendered = new JSDOM(frame.srcdoc).window.document;
    Object.defineProperty(rendered, "fonts", {
      value: { ready: Promise.resolve() },
    });
    Object.defineProperty(frame, "contentDocument", { value: rendered });
    frames.push(frame);
    prepare(frame, frames.length);
    queueMicrotask(() => frame.onload?.());
  };
  globalThis.document = document;
  return {
    document,
    frames,
    restore() {
      globalThis.document = original;
      browser.window.close();
    },
  };
}

test("inspection renders every slide sequentially and cleans all temporary frames", async () => {
  const doc = deck();
  doc.slides.push({ ...doc.slides[0], id: "slide-2" });
  const browser = browserStub((frame, count) => {
    if (count === 2) {
      const node = frame.contentDocument.querySelector("[data-id]");
      Object.defineProperties(node, {
        scrollHeight: { value: 140 },
        clientHeight: { value: 100 },
      });
    }
  });
  const progress = [];
  try {
    const result = await inspectPresentation(doc, {
      onProgress: (value) => progress.push(value),
    });
    assert.equal(browser.frames.length, 2);
    assert.deepEqual(progress, [0.5, 1]);
    assert.equal(
      result.find((item) => item.code === "TEXT_OVERFLOW").slide_id,
      "slide-2",
    );
    assert.equal(browser.document.querySelectorAll("iframe").length, 0);
  } finally {
    browser.restore();
  }
});

test("static frames freeze custom animations without changing the document", async () => {
  const doc = deck();
  doc.css =
    ".text {animation: entrance 3s infinite !important; transition: all 1s}";
  const browser = browserStub();
  try {
    const frame = await renderFrame(doc, doc.slides[0]);
    const node = frame.contentDocument.querySelector("[data-id]");
    assert.equal(node.style.animation, "none");
    assert.equal(node.style.getPropertyPriority("animation"), "important");
    assert.equal(node.style.transition, "none");
    assert.match(doc.css, /entrance/);
    frame.remove();
  } finally {
    browser.restore();
  }
});

test("cancelled inspections clean frames and do not continue to the next slide", async () => {
  const doc = deck();
  doc.slides.push({ ...doc.slides[0], id: "slide-2" });
  const controller = new AbortController();
  const browser = browserStub(() => queueMicrotask(() => controller.abort()));
  try {
    await assert.rejects(
      inspectPresentation(doc, { signal: controller.signal }),
      { name: "AbortError" },
    );
    assert.equal(browser.document.querySelectorAll("iframe").length, 0);
    assert.equal(browser.frames.length, 1);
  } finally {
    browser.restore();
  }
});

test("editable PPTX rejects structural quality errors without a browser", async () => {
  await assert.rejects(buildPptx(deck([text("bad", { x: 1270 })])), (error) => {
    assert.equal(error.name, "PresentationQualityError");
    assert.ok(error.diagnostics.some((item) => item.code === "OUT_OF_BOUNDS"));
    return true;
  });
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(
    inspectPresentation(deck(), { signal: controller.signal }),
    { name: "AbortError" },
  );
});

test("editable PPTX audits its actual structured colors when CSS differs", async () => {
  const doc = deck([text("light", { style: { color: "#ffffff" } })]);
  doc.css = ".slide {background:black!important}";
  await assert.rejects(buildPptx(doc), (error) => {
    assert.ok(error.diagnostics.some((item) => item.code === "LOW_CONTRAST"));
    return true;
  });
});

test("PDF preflight prevents clipping before any image export and cleans frames", async () => {
  const browser = browserStub((frame) => {
    const node = frame.contentDocument.querySelector("[data-id]");
    Object.defineProperties(node, {
      scrollHeight: { value: 140 },
      clientHeight: { value: 100 },
    });
  });
  try {
    await assert.rejects(exportPdf(deck()), (error) => {
      assert.equal(error.name, "PresentationQualityError");
      assert.ok(
        error.diagnostics.some((item) => item.code === "TEXT_OVERFLOW"),
      );
      return true;
    });
    assert.equal(browser.document.querySelectorAll("iframe").length, 0);
  } finally {
    browser.restore();
  }
});
