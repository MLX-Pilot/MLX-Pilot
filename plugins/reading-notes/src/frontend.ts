import { createFrontendClient } from "@mlx-pilot/plugin-sdk";
const host = createFrontendClient();
const form = document.querySelector<HTMLFormElement>("#search-form")!;
const query = document.querySelector<HTMLInputElement>("#query")!;
const status = document.querySelector<HTMLElement>("#status")!;
const results = document.querySelector<HTMLElement>("#results")!;
const buttons = [...document.querySelectorAll<HTMLButtonElement>("button")];
type Book = {
  title: string;
  author: string;
  year: number | null;
  source: string;
};

function showBooks(books: Book[]) {
  results.replaceChildren(
    ...books.map((book) => {
      const article = document.createElement("article");
      const title = document.createElement("h2");
      title.textContent = book.title;
      const author = document.createElement("p");
      author.textContent = `${book.author}${book.year === null ? "" : ` · ${book.year}`}`;
      const source = document.createElement("p");
      source.className = "source";
      source.textContent = book.source;
      article.append(title, author, source);
      return article;
    }),
  );
}

async function run(action: () => Promise<void>) {
  buttons.forEach((button) => {
    button.disabled = true;
  });
  status.textContent = "Consultando…";
  try {
    await action();
  } catch (error) {
    status.textContent = error instanceof Error ? error.message : String(error);
  } finally {
    buttons.forEach((button) => {
      button.disabled = false;
    });
  }
}
form.onsubmit = (event) => {
  event.preventDefault();
  void run(async () => {
    const response = await host.invoke<{ books: Book[] }>("search_books", {
      query: query.value.trim(),
    });
    showBooks(response.books);
    status.textContent = `${response.books.length} livros encontrados.`;
  });
};
document.querySelector<HTMLButtonElement>("#create")!.onclick = () => {
  if (!form.reportValidity()) return;
  void run(async () => {
    const response = await host.invoke<{ title: string; books: Book[] }>(
      "create_reading_brief",
      { query: query.value.trim() },
    );
    showBooks(response.books);
    status.textContent = `Briefing criado: ${response.title}. Abra o projeto no Presentation Studio para compor sua apresentação.`;
  });
};
document.querySelector<HTMLButtonElement>("#projects")!.onclick = () => {
  void run(async () => {
    const projects = await host.documents.list();
    results.replaceChildren(
      ...projects.map((project) => {
        const article = document.createElement("article");
        const title = document.createElement("h2");
        title.textContent = project.title;
        const detail = document.createElement("p");
        detail.textContent = `${project.slides} slides · revisão ${project.revision}`;
        article.append(title, detail);
        return article;
      }),
    );
    status.textContent = `${projects.length} projetos acessíveis a este plugin.`;
  });
};
window.addEventListener("pagehide", () => host.dispose(), { once: true });
