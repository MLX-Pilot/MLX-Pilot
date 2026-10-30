import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import http from "node:http";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

function request(port, requestPath) {
  return new Promise((resolve, reject) => {
    // Keep the raw target: URL/fetch would normalize traversal before sending it.
    const req = http.request({ hostname: "127.0.0.1", port, path: requestPath }, (res) => {
      let body = "";
      res.setEncoding("utf8");
      res.on("data", (chunk) => { body += chunk; });
      res.on("end", () => resolve({ status: res.statusCode, headers: res.headers, body }));
      res.on("error", reject);
    });
    req.setTimeout(5000, () => req.destroy(new Error("Preview request timed out")));
    req.on("error", reject);
    req.end();
  });
}

async function startServer(t, script, temporary, ui) {
  const child = spawn(process.execPath, [fileURLToPath(new URL(`../${script}`, import.meta.url)), ui, "0"], {
    cwd: temporary,
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
  });
  t.after(async () => {
    if (child.pid && child.exitCode === null && child.signalCode === null) {
      const exited = once(child, "exit");
      child.kill();
      await exited;
    }
    assert.equal(path.dirname(temporary), path.resolve(tmpdir()));
    await rm(temporary, { recursive: true, force: true });
  });
  return new Promise((resolve, reject) => {
    let output = "";
    let errors = "";
    const timer = setTimeout(() => reject(new Error(`Preview startup timed out: ${errors}`)), 10000);
    child.stderr.setEncoding("utf8").on("data", (chunk) => { errors += chunk; });
    child.stdout.setEncoding("utf8").on("data", (chunk) => {
      output += chunk;
      const match = output.match(/http:\/\/127\.0\.0\.1:(\d+)/);
      if (match) {
        clearTimeout(timer);
        const port = Number(match[1]);
        if (port === 0) reject(new Error("Preview must log its actual bound port"));
        else resolve(port);
      }
    });
    child.once("error", (error) => { clearTimeout(timer); reject(error); });
    child.once("exit", (code, signal) => {
      clearTimeout(timer);
      reject(new Error(`Preview exited (${code ?? signal}): ${errors}`));
    });
  });
}

for (const script of ["static-server.mjs", "mock-daemon.mjs"]) {
  test(`${script} serves an isolated UI and survives invalid request targets`, { timeout: 30000 }, async (t) => {
    const temporary = await mkdtemp(path.join(tmpdir(), "mlx-preview-"));
    const ui = path.join(temporary, "ui");
    const privateDirectory = path.join(temporary, "ui-private");
    await mkdir(path.join(ui, "assets"), { recursive: true });
    await mkdir(privateDirectory);
    await writeFile(path.join(ui, "index.html"), "<h1>Isolated preview</h1>");
    await writeFile(path.join(ui, "space name.txt"), "encoded filename");
    await writeFile(path.join(ui, "assets", "font.woff"), "woff fixture");
    await writeFile(path.join(ui, "assets", "font.woff2"), "woff2 fixture");
    await writeFile(path.join(privateDirectory, "secret.txt"), "private sibling content");
    const port = await startServer(t, script, temporary, ui);

    await t.test("uses the requested directory and serves encoded paths and font MIME types", async () => {
      const home = await request(port, "/");
      assert.equal(home.status, 200);
      assert.equal(home.body, "<h1>Isolated preview</h1>");
      assert.match(home.headers["content-type"], /^text\/html/);
      const encoded = await request(port, "/space%20name.txt");
      assert.equal(encoded.status, 200);
      assert.equal(encoded.body, "encoded filename");
      for (const extension of ["woff", "woff2"]) {
        const font = await request(port, `/assets/font.${extension}`);
        assert.equal(font.status, 200);
        assert.equal(font.headers["content-type"], `font/${extension}`);
      }
      assert.equal((await request(port, "/missing.txt")).status, 404);
    });

    const rejectedTargets = [
      ["/..%2fui-private%2fsecret.txt", 403],
      ["/agent/..%2f..%2fui-private%2fsecret.txt", 403],
      ["/%E0%A4%A", 400],
      ["/%00", 400],
      ["http://[", 400],
    ];
    if (process.platform === "win32") {
      rejectedTargets.push(["/..%5cui-private%5csecret.txt", 403]);
    }
    for (const [target, status] of rejectedTargets) {
      await t.test(`rejects ${target} with ${status} and remains healthy`, async () => {
        const response = await request(port, target);
        assert.equal(response.status, status);
        assert.doesNotMatch(response.body, /private sibling content/);
        const health = await request(port, script === "mock-daemon.mjs" ? "/health" : "/");
        assert.equal(health.status, 200);
        if (script === "mock-daemon.mjs") assert.equal(JSON.parse(health.body).status, "ok");
        else assert.equal(health.body, "<h1>Isolated preview</h1>");
      });
    }
  });
}
