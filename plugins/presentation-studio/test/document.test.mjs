import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import {
  template,
  palettes,
  sourceHtml,
  parseSource,
  safeCss,
  extractJson,
  aiContext,
  previewHtml,
} from "../src/document.js";
import { buildPptx, exportHtml } from "../src/export.js";
import JSZip from "jszip";
const Parser = new JSDOM("").window.DOMParser;
function document() {
  return {
    schema_version: 1,
    id: "test",
    revision: 1,
    title: "Relatório",
    brief: "Paleta verde e entradas suaves",
    theme: palettes.orbital,
    assets: [],
    slides: ["title", "content", "comparison", "chart", "table", "closing"].map(
      (x, i) => template(x, i + 1),
    ),
    css: "",
    script: "",
  };
}
test("HTML code round trip preserves content, geometry, data and animation", () => {
  const doc = document();
  doc.slides[1].elements[0].text = "A < B & C";
  doc.slides[1].elements[0].animation = "rise";
  const parsed = parseSource(sourceHtml(doc), doc, Parser);
  assert.deepEqual(parsed.slides, doc.slides);
});
test("source rejects active markup and remote CSS", () => {
  const doc = document();
  assert.throws(() => parseSource("<script>alert(1)</script>", doc, Parser));
  assert.throws(() => safeCss("body{background:url(https://example.org)}"));
  assert.throws(() => safeCss("</style><script>"));
  assert.throws(() => safeCss('@import "x"'));
  assert.equal(
    safeCss(".text {letter-spacing:1px}"),
    ".text {letter-spacing:1px}",
  );
});
test("preview escapes text and image metadata", () => {
  const doc = document();
  doc.slides[0].elements[0].text = "<script>alert(1)</script>";
  const html = previewHtml(doc.slides[0], doc);
  assert.ok(html.includes("&lt;script&gt;"));
  assert.ok(!html.includes("<script>"));
});
test("AI context omits binary data and limits itself to selected slide", () => {
  const doc = document();
  doc.assets = [
    {
      id: "a",
      name: "ref",
      data_url: "SECRET_IMAGE_BYTES",
      description: "Montanhas ao pôr do sol",
    },
  ];
  const context = aiContext(doc, 1);
  assert.ok(!JSON.stringify(context).includes("SECRET_IMAGE_BYTES"));
  assert.equal(context.selected_slide.id, doc.slides[1].id);
  assert.equal(context.outline.length, 6);
});
test("local model JSON handles fences and rejects unbounded operations", () => {
  assert.equal(
    extractJson(
      '```json\n{"operations":[{"type":"set_title","text":"X"}]}\n```',
    ).operations.length,
    1,
  );
  assert.throws(() => extractJson('{"operations":[]}'));
});
test("PPTX contains native editable text, a chart and a table", async () => {
  const doc = document(),
    pptx = await buildPptx(doc),
    bytes = await pptx.write({ outputType: "nodebuffer" }),
    zip = await JSZip.loadAsync(bytes);
  const slides = Object.keys(zip.files).filter((n) =>
    /^ppt\/slides\/slide\d+\.xml$/.test(n),
  );
  assert.equal(slides.length, 6);
  assert.ok(
    (await zip.file("ppt/slides/slide1.xml").async("string")).includes(
      "Uma ideia que merece",
    ),
  );
  assert.ok(
    Object.keys(zip.files).some((n) => /^ppt\/charts\/chart\d+\.xml$/.test(n)),
  );
  assert.ok(
    (await zip.file("ppt/slides/slide5.xml").async("string")).includes(
      "<a:tbl>",
    ),
  );
});
test("HTML export is offline and contains a working player script", async () => {
  const text = await exportHtml(document()).text();
  assert.ok(text.includes("</script>"));
  assert.ok(text.includes("addEventListener('keydown'"));
  assert.ok(!text.includes('src="http'));
});
