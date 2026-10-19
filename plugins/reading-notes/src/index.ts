import {
  createHostClient,
  definePlugin,
  defineTool,
} from "@mlx-pilot/plugin-sdk";
import { createReadingBrief, type Book } from "./brief";

const host = createHostClient();
const querySchema = {
  type: "object",
  properties: { query: { type: "string", minLength: 1, maxLength: 160 } },
  required: ["query"],
  additionalProperties: false,
};

const catalogText = (value: string, fallback: string) =>
  [...value].slice(0, 160).join("") || fallback;

function search(query: string): Book[] {
  query = query.trim();
  if (!query) throw new Error("Informe um tema para pesquisar.");
  const response = host.http.request<{ docs?: Record<string, unknown>[] }>({
    service: "open_library",
    path: `/search.json?q=${encodeURIComponent(query)}&fields=key,title,author_name,first_publish_year&limit=5`,
    method: "GET",
    response_type: "json",
  });
  if (response.status !== 200 || !Array.isArray(response.body?.docs))
    throw new Error("A Open Library não retornou uma pesquisa válida.");
  return response.body.docs.slice(0, 5).map((book) => ({
    title:
      typeof book.title === "string"
        ? catalogText(book.title, "Título não informado")
        : "Título não informado",
    author: Array.isArray(book.author_name)
      ? catalogText(
          book.author_name
            .filter((value) => typeof value === "string")
            .slice(0, 3)
            .join(", "),
          "Autoria não informada",
        )
      : "Autoria não informada",
    year:
      typeof book.first_publish_year === "number" &&
      Number.isInteger(book.first_publish_year)
        ? book.first_publish_year
        : null,
    source:
      typeof book.key === "string" && /^\/works\/OL\d+W$/.test(book.key)
        ? `https://openlibrary.org${book.key}`
        : "Open Library",
  }));
}

export default definePlugin({
  id: "reading_notes",
  name: "Reading Notes",
  version: "1.1.0",
  permissions: ["network:request", "documents:read", "documents:write"],
  services: [
    {
      id: "open_library",
      name: "Open Library",
      base_url: "https://openlibrary.org/",
      methods: ["GET"],
    },
  ],
  frontend: {
    html: "src/index.html",
    script: "src/frontend.ts",
    style: "src/frontend.css",
  },
  tools: [
    defineTool({
      name: "search_books",
      description:
        "Search up to five books in the public Open Library catalog. Returns source metadata, not invented summaries.",
      parameters: querySchema,
      execute({ query }: { query: string }) {
        return { books: search(query) };
      },
    }),
    defineTool({
      name: "create_reading_brief",
      description:
        "Create an editable, visually composed reading notebook in Presentation Studio from Open Library metadata. Includes catalog references and source notes, never inferred book summaries. Requires permission to create documents.",
      parameters: querySchema,
      mutating: true,
      execute({ query }: { query: string }) {
        const books = search(query);
        if (!books.length)
          throw new Error("Nenhum livro encontrado. Tente outro tema.");
        // Compose before creating a project: layout failure must not leave a blank notebook.
        const { title, brief, theme, slides } = createReadingBrief(
          query,
          books,
        );
        const document = host.documents.create({ title });
        const operations: Record<string, unknown>[] = [
          { type: "set_brief", text: brief },
          { type: "set_theme", theme },
          ...slides.map((slide) => ({ type: "add_slide", slide })),
          ...document.slides.map((slide) => ({
            type: "remove_slide",
            slide_id: slide.id,
          })),
        ];
        const result = host.documents.edit({
          id: document.id,
          base_revision: document.revision,
          operations,
        });
        return { id: result.id, title, revision: result.revision, books };
      },
    }),
  ],
});
