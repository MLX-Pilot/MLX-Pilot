import assert from "node:assert/strict";
import { test, before } from "node:test";
import { createRequire } from "node:module";
import { auditDocument } from "../../presentation-studio/src/quality.js";
import { previewHtml } from "../../presentation-studio/src/document.js";

const { build } = createRequire(import.meta.resolve("@mlx-pilot/plugin-sdk"))(
  "esbuild",
);
let createReadingBrief, projectTitle, plugin;
before(async () => {
  await build({
    entryPoints: ["src/brief.ts"],
    bundle: true,
    platform: "node",
    format: "esm",
    outfile: "dist/test-brief.mjs",
  });
  ({ createReadingBrief, projectTitle } = await import(
    "../dist/test-brief.mjs"
  ));
  ({ default: plugin } = await import("../dist/test-definition.mjs"));
});

const book = (index, overrides = {}) => ({
  title: `Obra de referência ${index}`,
  author: `Autoria ${index}`,
  year: 2000 + index,
  source: `https://openlibrary.org/works/OL${index}W`,
  ...overrides,
});
const document = (query, books) => ({
  schema_version: 1,
  id: "test-reading",
  revision: 2,
  assets: [],
  css: "",
  script: "",
  ...createReadingBrief(query, books),
});

test("five results produce a composed notebook with every reference and real count", () => {
  const books = Array.from({ length: 5 }, (_, i) => book(i + 1));
  const original = JSON.stringify(books);
  const doc = document("jardins urbanos", books);
  assert.equal(doc.slides.length, 5);
  assert.equal(
    doc.slides[0].elements.find((e) => e.id === "cover-count").text,
    "05",
  );
  assert.equal(
    doc.slides[0].elements.find((e) => e.id === "cover-query").text,
    "jardins urbanos",
  );
  assert.ok(doc.slides.every((slide) => slide.elements.length > 10));
  for (const record of books) {
    assert.ok(doc.brief.includes(record.title));
    assert.ok(doc.brief.includes(record.author));
    assert.ok(doc.brief.includes(record.source));
    assert.ok(
      doc.slides
        .slice(1, -1)
        .some((slide) => slide.notes.includes(record.source)),
    );
  }
  assert.deepEqual(auditDocument(doc), []);
  assert.equal(JSON.stringify(books), original);
});

test("long CJK and supplementary Unicode metadata remain complete and use a wide card", () => {
  const title = "都市🌱".repeat(53) + "園";
  const author = "研究者".repeat(53) + "氏";
  assert.equal([...title].length, 160);
  const doc = document("都市🌱".repeat(53) + "園", [
    book(1, { title, author }),
  ]);
  const rendered = doc.slides[1].elements;
  const heading = rendered.find((e) => e.id.endsWith("book-1-title"));
  const byline = rendered.find((e) => e.id.endsWith("book-1-author"));
  assert.equal(heading.text.replaceAll("\n", ""), title);
  assert.equal(byline.text.replaceAll("\n", ""), author);
  assert.ok(heading.w > 1000);
  assert.ok(doc.brief.includes(title));
  assert.ok(doc.slides[1].notes.includes(author));
  assert.deepEqual(auditDocument(doc), []);
});

test("long Latin titles keep their words and stay inside the slide", () => {
  const title =
    "A multidisciplinary investigation of sustainable urban landscapes and community gardens: a practical handbook for researchers, educators and local organizations";
  const doc = document("Sustainable urban landscapes and community gardens", [
    book(1, { title }),
    book(2, { title }),
  ]);
  const headings = doc.slides
    .flatMap((slide) => slide.elements)
    .filter((e) => e.id.endsWith("-title") && e.id.includes("book-"));
  assert.equal(headings.length, 2);
  assert.ok(headings.every((e) => e.text.replace(/\s+/gu, " ") === title));
  assert.equal(
    auditDocument(doc).filter((d) => d.severity === "error").length,
    0,
  );
});

test("missing year remains unknown and full URLs live in notes instead of large text boxes", () => {
  const doc = document("design", [
    book(1, { year: null, source: "Open Library" }),
  ]);
  const visible = doc.slides[1].elements.map((e) => e.text).join("\n");
  assert.match(visible, /Primeira publicação: não informada/);
  assert.match(doc.slides[1].notes, /Fonte: Open Library/);
  assert.equal(doc.slides.length, 3);
  assert.deepEqual(auditDocument(doc), []);
});

test("metadata stays plain text even when it contains markup-like text", () => {
  const title = "<script>alert('catalog')</script> & typography";
  const doc = document("<img src=x>", [book(1, { title })]);
  const html = previewHtml(doc.slides[1], doc);
  assert.ok(doc.slides[1].notes.includes(title));
  assert.ok(html.includes("&lt;script&gt;"));
  assert.ok(!html.includes("<script>"));
  assert.ok(!previewHtml(doc.slides[0], doc).includes("<img src=x>"));
});

test("Unicode project titles obey the host UTF-8 byte bound without broken code points", () => {
  const title = projectTitle("🌱".repeat(160));
  assert.ok(Buffer.byteLength(title, "utf8") <= 300);
  assert.ok(title.endsWith("…"));
  assert.ok(
    !/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(
      title,
    ),
  );
  assert.equal(projectTitle("  design  "), "Leituras: design");
});

test("invalid inputs fail during composition before a project would be created", () => {
  assert.throws(
    () => createReadingBrief("design", []),
    /uma a cinco referências/,
  );
  assert.throws(() => createReadingBrief("", [book(1)]), /tema/);
  assert.throws(
    () =>
      createReadingBrief(
        "design",
        Array.from({ length: 6 }, (_, i) => book(i)),
      ),
    /uma a cinco referências/,
  );
});

test("mutating tool creates once and applies the composed notebook with the returned revision", () => {
  const saved = { Host: globalThis.Host, Memory: globalThis.Memory };
  const allocated = new Map();
  let pointer = 0n;
  const allocate = (value) => {
    const offset = ++pointer;
    const handle = {
      offset,
      readString: () => value,
      free: () => allocated.delete(offset),
    };
    allocated.set(offset, handle);
    return handle;
  };
  const calls = [];
  globalThis.Memory = {
    fromString: allocate,
    find: (offset) => allocated.get(offset),
  };
  globalThis.Host = {
    getFunctions: () => ({
      mlx_host_call(offset) {
        const call = JSON.parse(allocated.get(offset).readString());
        calls.push(call);
        let result;
        if (call.operation === "http.request")
          result = {
            status: 200,
            body: {
              docs: [
                {
                  title: "Pesquisa e leitura",
                  author_name: ["Uma autora"],
                  key: "/works/OL1W",
                  first_publish_year: 2020,
                },
              ],
            },
          };
        else if (call.operation === "documents.create")
          result = {
            id: "created-reading",
            revision: 7,
            slides: [{ id: "default-slide" }],
          };
        else if (call.operation === "documents.edit") {
          assert.equal(call.input.id, "created-reading");
          assert.equal(call.input.base_revision, 7);
          const operations = call.input.operations;
          assert.deepEqual(
            operations.filter((op) => op.type === "remove_slide"),
            [{ type: "remove_slide", slide_id: "default-slide" }],
          );
          assert.ok(
            operations.some(
              (op) => op.type === "set_theme" && op.theme.accent === "#00d4ff",
            ),
          );
          assert.ok(
            operations.some(
              (op) =>
                op.type === "set_brief" &&
                op.text.includes("https://openlibrary.org/works/OL1W"),
            ),
          );
          const slides = operations
            .filter((op) => op.type === "add_slide")
            .map((op) => op.slide);
          assert.equal(slides.length, 3);
          assert.ok(
            slides.some((slide) => slide.notes.includes("Pesquisa e leitura")),
          );
          result = {
            id: "created-reading",
            revision: 8,
            slides: slides.length,
            diagnostics: [],
          };
        } else throw new Error(`Unexpected operation ${call.operation}`);
        return allocate(JSON.stringify({ result })).offset;
      },
    }),
  };
  try {
    const result = plugin.tools
      .find((tool) => tool.name === "create_reading_brief")
      .execute({ query: "design" });
    assert.deepEqual(
      calls.map((call) => call.operation),
      ["http.request", "documents.create", "documents.edit"],
    );
    assert.deepEqual(Object.keys(result).sort(), [
      "books",
      "id",
      "revision",
      "title",
    ]);
    assert.equal(result.revision, 8);
    assert.equal(result.books[0].source, "https://openlibrary.org/works/OL1W");
    assert.equal(allocated.size, 0);
  } finally {
    if (saved.Host === undefined) delete globalThis.Host;
    else globalThis.Host = saved.Host;
    if (saved.Memory === undefined) delete globalThis.Memory;
    else globalThis.Memory = saved.Memory;
  }
});
