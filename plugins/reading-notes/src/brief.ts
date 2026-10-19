/** Deterministic bibliography, not a book-summary generator. Every catalog field
 * remains in the briefing and notes; display line breaks only affect layout. */
export type Book = {
  title: string;
  author: string;
  year: number | null;
  source: string;
};

type Element = {
  id: string;
  kind: string;
  x: number;
  y: number;
  w: number;
  h: number;
  text: string;
  style: Record<string, unknown>;
  animation: string;
  delay_ms: number;
};
type Slide = {
  id: string;
  background: string;
  transition: string;
  notes: string;
  elements: Element[];
};

export const readingTheme = {
  background: "#0a0e17",
  foreground: "#e8edf5",
  accent: "#00d4ff",
  muted: "#a4b3c9",
  font: "Arial",
};
const panel = "#141c2b";
const violet = "#a391ff";

/** Keep document titles below the host's 300-byte limit without broken UTF-8. */
export function projectTitle(query: string) {
  let result = "Leituras: ";
  let bytes = result.length;
  for (const char of query.trim()) {
    const point = char.codePointAt(0)!;
    const length = point < 128 ? 1 : point < 2048 ? 2 : point < 65536 ? 3 : 4;
    if (bytes + length > 290) return `${result}…`;
    result += char;
    bytes += length;
  }
  return result;
}

export function sourceNotes(books: Book[]) {
  return books
    .map(
      (book, index) =>
        `${index + 1}. ${book.title}\nAutoria: ${book.author}\nPrimeira publicação: ${book.year ?? "não informada"}\nFonte: ${book.source}`,
    )
    .join("\n\n");
}

type Word = { value: string; units: number };
const glyphCache = new Map<string, number>();
const wordCache = new Map<string, Word[]>();

function glyphWidth(char: string): number {
  const cached = glyphCache.get(char);
  if (cached !== undefined) return cached;
  const point = char.codePointAt(0)!;
  let width = 0.6;
  // QuickJS executes inside the WASM fuel budget. Avoid Unicode-property regexes
  // in this hot path; classify each distinct glyph once, with ASCII first.
  if (point < 128) {
    if (point <= 32) width = 0.3;
    else if ("ilIjtfr.,:;'!|".includes(char)) width = 0.36;
    else if ("MW@%&".includes(char)) width = 1.02;
    else if (char === "m" || char === "w") width = 0.9;
    else if (point >= 65 && point <= 90) width = 0.8;
  } else if (
    point === 0x200d ||
    point === 0xfe0f ||
    (point >= 0x0300 && point <= 0x036f) ||
    (point >= 0x1ab0 && point <= 0x1aff) ||
    (point >= 0x1dc0 && point <= 0x1dff) ||
    (point >= 0x20d0 && point <= 0x20ff) ||
    (point >= 0xfe20 && point <= 0xfe2f)
  )
    width = 0;
  else if (point > 0x2e7f) width = 1.08;
  else if (char === "—") width = 1.02;
  else if (char.toUpperCase() === char && char.toLowerCase() !== char)
    width = 0.8;
  glyphCache.set(char, width);
  return width;
}

function wordsOf(value: string) {
  let words = wordCache.get(value);
  if (!words) {
    words = value
      .trim()
      .split(/\s+/u)
      .map((value) => {
        let units = 0;
        for (const char of value) units += glyphWidth(char);
        return { value, units };
      });
    wordCache.set(value, words);
  }
  return words;
}

// Conservative metrics plus explicit breaks keep HTML and editable PPTX aligned.
// Long unspaced titles (including CJK) wrap without dropping any code points.
function wrap(value: string, width: number, size: number, bold: boolean) {
  const limit = width / (size * (bold ? 1.04 : 1));
  const lines: string[] = [];
  let line = "";
  let lineUnits = 0;
  for (const word of wordsOf(value)) {
    const nextUnits = lineUnits + (line ? 0.3 : 0) + word.units;
    if (nextUnits <= limit) {
      line = line ? `${line} ${word.value}` : word.value;
      lineUnits = nextUnits;
      continue;
    }
    if (line) {
      lines.push(line);
      line = "";
      lineUnits = 0;
    }
    if (word.units <= limit) {
      line = word.value;
      lineUnits = word.units;
      continue;
    }
    for (const char of word.value) {
      const units = glyphWidth(char);
      if (line && lineUnits + units > limit) {
        lines.push(line);
        line = "";
        lineUnits = 0;
      }
      line += char;
      lineUnits += units;
    }
  }
  if (line) lines.push(line);
  return lines;
}

function text(
  id: string,
  value: string,
  x: number,
  y: number,
  w: number,
  h: number,
  size = 24,
  color = readingTheme.foreground,
  bold = false,
  minSize = size,
): Element {
  for (let fontSize = size; fontSize >= minSize; fontSize -= 2) {
    const lines = wrap(value, w - 4, fontSize, bold);
    if (lines.length * fontSize * 1.18 + 3 <= h)
      return {
        id,
        kind: "text",
        x,
        y,
        w,
        h,
        text: lines.join("\n"),
        style: { fontSize, color, bold },
        animation: "rise",
        delay_ms: 80,
      };
  }
  throw new Error(
    `O texto de ${id} excede o espaço disponível. Reduza o tema da pesquisa.`,
  );
}

function shape(
  id: string,
  x: number,
  y: number,
  w: number,
  h: number,
  fill: string,
  radius = 0,
): Element {
  return {
    id,
    kind: "shape",
    x,
    y,
    w,
    h,
    text: "",
    style: { fill, radius },
    animation: "none",
    delay_ms: 0,
  };
}

function frame(
  id: string,
  page: number,
  total: number,
  label: string,
  notes: string,
): Slide {
  return {
    id,
    background: readingTheme.background,
    transition: "fade",
    notes,
    elements: [
      shape(`${id}-rule`, 72, 48, 48, 4, readingTheme.accent),
      text(`${id}-eyebrow`, label, 136, 34, 1000, 32, 18, readingTheme.accent),
      shape(`${id}-footer-rule`, 72, 644, 1136, 1, "#344157"),
      text(
        `${id}-footer`,
        "READING NOTES  /  OPEN LIBRARY",
        72,
        665,
        940,
        28,
        18,
        readingTheme.muted,
      ),
      text(
        `${id}-page`,
        `${String(page).padStart(2, "0")} / ${String(total).padStart(2, "0")}`,
        1108,
        665,
        100,
        28,
        18,
        readingTheme.muted,
      ),
    ],
  };
}

function compactBook(book: Book) {
  return (
    wrap(book.title, 496, 22, true).length * 22 * 1.18 + 3 <= 174 &&
    wrap(book.author, 496, 18, false).length * 18 * 1.18 + 3 <= 82
  );
}

export function createReadingBrief(query: string, books: Book[]) {
  // Scope caches to one synchronous composition, including long-lived runtimes.
  glyphCache.clear();
  wordCache.clear();
  query = query.trim();
  if (!query || !books.length || books.length > 5)
    throw new Error("Use um tema e de uma a cinco referências.");
  const notes = sourceNotes(books);
  const brief = `Prepare uma apresentação sobre leituras relacionadas a ${query}.\n\nOs itens abaixo são metadados de catálogo fornecidos pela Open Library, na ordem retornada pela pesquisa. Não invente o conteúdo, conclusões ou citações dos livros. O ano é o da primeira publicação registrada, não necessariamente o da edição consultada. A consulta não é uma recomendação ou avaliação das obras.\n\n${notes}`;
  const groups: Book[][] = [];
  for (const book of books) {
    const last = groups[groups.length - 1];
    if (last?.length === 1 && compactBook(last[0]) && compactBook(book))
      last.push(book);
    else groups.push([book]);
  }
  const total = groups.length + 2;
  const cover = frame(
    "reading-cover",
    1,
    total,
    "CADERNO DE PESQUISA",
    `${brief}\n\nConsulta: ${query}`,
  );
  cover.elements.push(
    shape("cover-orbit", 1008, 136, 200, 250, panel, 24),
    text(
      "cover-query",
      query,
      72,
      136,
      872,
      248,
      58,
      readingTheme.foreground,
      true,
      28,
    ),
    text(
      "cover-count",
      String(books.length).padStart(2, "0"),
      1040,
      152,
      148,
      152,
      112,
      readingTheme.accent,
      true,
    ),
    text(
      "cover-count-label",
      books.length === 1 ? "obra localizada" : "obras localizadas",
      1040,
      304,
      142,
      64,
      22,
      readingTheme.muted,
    ),
    text(
      "cover-purpose",
      "Referências para orientar sua próxima leitura.",
      72,
      420,
      790,
      100,
      32,
      readingTheme.foreground,
      false,
    ),
    text(
      "cover-source-label",
      "CATÁLOGO CONSULTADO",
      916,
      440,
      292,
      30,
      18,
      violet,
    ),
    text(
      "cover-source",
      "Open Library",
      916,
      482,
      292,
      52,
      36,
      readingTheme.foreground,
      true,
    ),
    text(
      "cover-scope",
      "Metadados bibliográficos. Ideias, resumos e citações exigem consulta às obras.",
      72,
      562,
      1136,
      54,
      22,
      readingTheme.muted,
    ),
  );
  const slides = [cover];
  let offset = 0;
  for (const [groupIndex, group] of groups.entries()) {
    const id = `reading-sources-${groupIndex + 1}`;
    const slide = frame(
      id,
      slides.length + 1,
      total,
      "REFERÊNCIAS BIBLIOGRÁFICAS",
      sourceNotes(group),
    );
    slide.elements.push(
      text(
        `${id}-title`,
        "Um repertório para explorar",
        72,
        92,
        1136,
        62,
        44,
        readingTheme.foreground,
        true,
      ),
      text(
        `${id}-range`,
        `Registros ${offset + 1}${group.length > 1 ? `–${offset + group.length}` : ""} de ${books.length} · ordem retornada pelo catálogo`,
        72,
        159,
        1136,
        36,
        20,
        readingTheme.muted,
      ),
    );
    for (const [index, book] of group.entries()) {
      const single = group.length === 1;
      const x = single ? 72 : 72 + index * 584;
      const width = single ? 1136 : 552;
      const inner = width - 48;
      const prefix = `${id}-book-${index + 1}`;
      const source = book.source.startsWith("https://openlibrary.org/works/")
        ? `openlibrary.org · ${book.source.split("/").pop()}`
        : "Open Library · registro sem identificador";
      slide.elements.push(
        shape(`${prefix}-panel`, x, 216, width, 402, panel, 18),
        shape(
          `${prefix}-accent`,
          x,
          240,
          4,
          48,
          index ? violet : readingTheme.accent,
        ),
        text(
          `${prefix}-number`,
          `REFERÊNCIA ${String(offset + index + 1).padStart(2, "0")}`,
          x + 24,
          238,
          inner,
          28,
          18,
          index ? violet : readingTheme.accent,
        ),
        text(
          `${prefix}-title`,
          book.title,
          x + 24,
          282,
          inner,
          174,
          single ? 40 : 34,
          readingTheme.foreground,
          true,
          single ? 26 : 22,
        ),
        text(
          `${prefix}-author`,
          book.author,
          x + 24,
          466,
          inner,
          82,
          22,
          readingTheme.muted,
          false,
          18,
        ),
        text(
          `${prefix}-year`,
          `Primeira publicação: ${book.year ?? "não informada"}`,
          x + 24,
          555,
          inner,
          28,
          18,
          readingTheme.muted,
        ),
        text(
          `${prefix}-source`,
          source,
          x + 24,
          586,
          inner,
          26,
          18,
          readingTheme.accent,
        ),
      );
    }
    offset += group.length;
    slides.push(slide);
  }
  const closing = frame(
    "reading-next",
    total,
    total,
    "PRÓXIMAS LEITURAS",
    "Roteiro de trabalho sugerido. Não descreve nem avalia o conteúdo das obras. Consulte as referências completas nas notas das páginas anteriores.",
  );
  closing.elements.push(
    text(
      "next-title",
      "Da referência à leitura crítica",
      72,
      102,
      1136,
      76,
      48,
      readingTheme.foreground,
      true,
    ),
    text(
      "next-intro",
      "Transforme a pesquisa em anotações próprias, com contexto e fontes.",
      72,
      192,
      1136,
      74,
      26,
      readingTheme.muted,
    ),
  );
  const steps = [
    [
      "Escolha uma obra",
      "Abra o registro no catálogo e confirme a edição disponível para leitura.",
    ],
    [
      "Registre evidências",
      "Ao ler, anote ideias, páginas e citações com o contexto original.",
    ],
    [
      "Construa sua síntese",
      "Compare o que você leu e acrescente suas conclusões às notas.",
    ],
  ];
  for (const [index, [title, body]] of steps.entries()) {
    const x = 72 + index * 388;
    closing.elements.push(
      shape(`next-${index}-panel`, x, 296, 360, 310, panel, 18),
      text(
        `next-${index}-number`,
        `0${index + 1}`,
        x + 24,
        320,
        312,
        46,
        32,
        index === 1 ? violet : readingTheme.accent,
        true,
      ),
      text(
        `next-${index}-title`,
        title,
        x + 24,
        382,
        312,
        76,
        28,
        readingTheme.foreground,
        true,
      ),
      text(
        `next-${index}-body`,
        body,
        x + 24,
        474,
        312,
        112,
        22,
        readingTheme.muted,
      ),
    );
  }
  slides.push(closing);
  return {
    title: projectTitle(query),
    brief,
    theme: { ...readingTheme },
    slides,
  };
}
