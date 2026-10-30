import assert from "node:assert/strict";
import { appendFile, cp, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

const presentation = fileURLToPath(
  new URL("../../docs/apresentacao-plugins/", import.meta.url),
);

async function copyPresentation(t) {
  const temporary = await mkdtemp(path.join(tmpdir(), "mlx-presentation-"));
  t.after(() => rm(temporary, { recursive: true, force: true }));
  const directory = path.join(temporary, "presentation");
  await cp(presentation, directory, { recursive: true });
  return directory;
}

function verify(directory) {
  const result = spawnSync(process.execPath, [path.join(directory, "verify.mjs")], {
    cwd: directory,
    encoding: "utf8",
    timeout: 30_000,
  });
  assert.ifError(result.error);
  assert.equal(result.signal, null, result.stderr);
  return result;
}

test("the checked-in presentation passes without installing dependencies", () => {
  const result = verify(presentation);
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /Apresentação verificada: 12 slides, 3 MP4/);
});

test("a missing video fails verification", async (t) => {
  const directory = await copyPresentation(t);
  await rm(path.join(directory, "assets", "01-instalar-configurar.mp4"));

  const result = verify(directory);
  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stderr, /arquivo indisponível.*01-instalar-configurar\.mp4/);
});

test("a remote executable script fails verification", async (t) => {
  const directory = await copyPresentation(t);
  await appendFile(
    path.join(directory, "index.html"),
    '\n<script defer src="https://example.invalid/remote.js"></script>\n',
  );

  const result = verify(directory);
  assert.equal(result.status, 1, result.stdout);
  assert.match(result.stderr, /<script>: use um caminho relativo local/);
});

test("an invalid subtitle interval fails verification", async (t) => {
  const directory = await copyPresentation(t);
  await writeFile(
    path.join(directory, "assets", "01-instalar-configurar.vtt"),
    "WEBVTT\n\n00:00:05.000 --> 00:00:02.000\nIntervalo invertido\n",
  );

  const result = verify(directory);
  assert.equal(result.status, 1, result.stdout);
  assert.match(
    result.stderr,
    /01-instalar-configurar\.vtt: tempos de legenda inválidos/,
  );
});
