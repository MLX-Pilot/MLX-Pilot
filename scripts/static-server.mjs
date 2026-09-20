// Servidor estatico para abrir a UI do desktop no navegador sem o Tauri.
// Uso: node scripts/static-server.mjs <diretorio> <porta>
//
// So serve arquivos. A UI fala com o daemon real em 127.0.0.1:11435; para
// trabalhar sem o daemon, use scripts/mock-daemon.mjs, que serve a UI e
// responde a API na mesma porta.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";

const ROOT = path.resolve(process.argv[2] || ".");
const PORT = Number(process.argv[3] || 5599);

const MIME = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "application/javascript; charset=utf-8",
  ".mjs": "application/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
};

const server = http.createServer((req, res) => {
  const pathname = decodeURIComponent(new URL(req.url, "http://localhost").pathname);
  const relative = pathname === "/" ? "index.html" : pathname.slice(1);

  // Barra travessia de diretorio: o caminho resolvido tem de ficar sob ROOT.
  const target = path.resolve(ROOT, relative);
  if (!target.startsWith(ROOT)) {
    res.writeHead(403);
    res.end("Forbidden");
    return;
  }

  fs.readFile(target, (error, content) => {
    if (error) {
      res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("Not found");
      return;
    }
    res.writeHead(200, {
      "Content-Type": MIME[path.extname(target).toLowerCase()] || "application/octet-stream",
      "Cache-Control": "no-store",
    });
    res.end(content);
  });
});

server.listen(PORT, "127.0.0.1", () => {
  console.log(`UI estatica em http://127.0.0.1:${PORT}`);
  console.log(`Servindo: ${ROOT}`);
});
