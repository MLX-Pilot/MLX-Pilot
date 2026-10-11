#!/usr/bin/env node
import { readFile, writeFile, mkdir, access } from "node:fs/promises";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { build } from "esbuild";
const [command, arg] = process.argv.slice(2),
  cwd = process.cwd(),
  sdk = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
async function main() {
  if (command === "init") {
    if (!arg || !/^[a-z0-9_-]+$/.test(arg))
      throw new Error("Usage: mlx-plugin init my_plugin");
    const dir = path.resolve(cwd, arg);
    try {
      await access(dir);
      throw new Error("Destination already exists");
    } catch (e) {
      if (e.code !== "ENOENT") throw e;
    }
    await mkdir(path.join(dir, "src"), { recursive: true });
    await writeFile(
      path.join(dir, "package.json"),
      JSON.stringify(
        {
          name: arg,
          private: true,
          type: "module",
          scripts: { build: "mlx-plugin build", pack: "mlx-plugin pack" },
          dependencies: { "@mlx-pilot/plugin-sdk": `file:${sdk}` },
        },
        null,
        2,
      ),
    );
    await writeFile(
      path.join(dir, "src/index.ts"),
      `import {definePlugin,defineTool} from '@mlx-pilot/plugin-sdk';\nexport default definePlugin({id:${JSON.stringify(arg.replaceAll("-", "_"))},name:${JSON.stringify(arg)},version:'1.0.0',frontend:{html:'src/index.html',script:'src/frontend.ts',style:'src/frontend.css'},tools:[defineTool({name:'summarize_numbers',description:'Calculate the count and sum of supplied numbers.',parameters:{type:'object',properties:{values:{type:'array',items:{type:'number'},maxItems:1000}},required:['values'],additionalProperties:false},execute({values}:{values:number[]}){return {count:values.length,sum:values.reduce((a,b)=>a+b,0)};}})]});\n`,
    );
    await writeFile(
      path.join(dir, "src/index.html"),
      `<main><span>MLX-PILOT PLUGIN</span><h1>Somar números</h1><p>Uma interface própria conectada ao seu WASM.</p><label>Números separados por vírgula<input id="values" value="10, 20, 30"></label><button id="calculate">Calcular</button><pre id="result" role="status"></pre></main>`,
    );
    await writeFile(
      path.join(dir, "src/frontend.css"),
      `body{margin:0;background:#101827;color:#edf2f9;font:16px system-ui}main{max-width:650px;margin:48px auto;padding:30px}span{font-size:11px;letter-spacing:2px;color:#baf66d}input,button{display:block;padding:12px;margin:16px 0;border-radius:8px;border:1px solid #45536a}input{width:90%;background:#192539;color:white}button{background:#baf66d;color:#10200c;cursor:pointer}pre{white-space:pre-wrap}`,
    );
    await writeFile(
      path.join(dir, "src/frontend.ts"),
      `import {createFrontendClient} from '@mlx-pilot/plugin-sdk';
const host = createFrontendClient();
document.getElementById('calculate')!.onclick = async () => {
  const result = document.getElementById('result')!;
  try {
    const values = (document.getElementById('values') as HTMLInputElement).value.split(',').map(v=>Number(v.trim()));
    if (values.some(v=>!Number.isFinite(v))) throw new Error('Use apenas números.');
    result.textContent = JSON.stringify(await host.invoke('summarize_numbers', {values}), null, 2);
  } catch(e) { result.textContent = String(e); }
};`,
    );
    console.log(
      `Created ${dir}. Run npm install, npm run build, npm run pack. Build requires extism-js on PATH.`,
    );
    return;
  }
  if (command === "build") {
    await mkdir("dist", { recursive: true });
    await build({
      entryPoints: ["src/index.ts"],
      bundle: true,
      format: "esm",
      platform: "node",
      outfile: "dist/definition.mjs",
    });
    // Build tooling executes developer-owned definitions; installation never executes source/build scripts.
    const plugin = (
      await import(pathToFileURL(path.join(cwd, "dist/definition.mjs")))
    ).default;
    const manifest = {
      api_version: 1,
      id: plugin.id,
      name: plugin.name,
      version: plugin.version,
      permissions: plugin.permissions,
      tools: plugin.tools.map(({ execute, ...t }) => ({
        ...t,
        export: t.name,
      })),
    };
    if (plugin.frontend) {
      const ui = plugin.frontend;
      const js = ui.script
        ? await build({
            entryPoints: [ui.script],
            bundle: true,
            platform: "browser",
            format: "iife",
            write: false,
            target: "es2020",
          })
        : null;
      manifest.frontend = {
        html: await readFile(ui.html, "utf8"),
        css: ui.style ? await readFile(ui.style, "utf8") : "",
        javascript: js ? js.outputFiles[0].text : "",
      };
      if (Buffer.byteLength(JSON.stringify(manifest.frontend)) > 1024 * 1024)
        throw new Error("Frontend exceeds 1 MiB");
    }
    await writeFile("dist/plugin.json", JSON.stringify(manifest, null, 2));
    const exports = plugin.tools
      .map(
        (t) =>
          `export function ${t.name}(){const input=JSON.parse(Host.inputString());const result=plugin.tools.find(t=>t.name===${JSON.stringify(t.name)}).execute(input);Host.outputString(JSON.stringify(result));}`,
      )
      .join("\n");
    await build({
      stdin: {
        contents: `import plugin from './src/index.ts';\n${exports}`,
        resolveDir: cwd,
        loader: "ts",
      },
      bundle: true,
      format: "cjs",
      target: "es2020",
      outfile: "dist/guest.js",
    });
    await writeFile(
      "dist/guest.d.ts",
      `declare module "main" {${plugin.tools.map((t) => `export function ${t.name}(): I32;`).join("\n")}}`,
    );
    const r = spawnSync(
      "extism-js",
      ["dist/guest.js", "-i", "dist/guest.d.ts", "-o", "dist/plugin.wasm"],
      { stdio: "inherit" },
    );
    if (r.error || r.status !== 0)
      throw new Error(
        "WASM build failed. Install the Extism JavaScript PDK (extism-js).",
      );
    console.log("Built dist/plugin.wasm");
    return;
  }
  if (command === "pack") {
    const dir = arg || "dist";
    const manifest = JSON.parse(
      await readFile(path.join(dir, "plugin.json"), "utf8"),
    );
    const wasm = await readFile(path.join(dir, "plugin.wasm"));
    if (wasm.subarray(0, 4).toString("hex") !== "0061736d")
      throw new Error("Invalid WASM");
    const output = `${manifest.id}-${manifest.version}.mlxplugin.json`;
    await writeFile(
      output,
      JSON.stringify({ manifest, wasm_base64: wasm.toString("base64") }),
    );
    console.log(output);
    return;
  }
  if (command === "test") {
    const fixtures = JSON.parse(await readFile(arg || "fixtures.json", "utf8"));
    await mkdir("dist", { recursive: true });
    await build({
      entryPoints: ["src/index.ts"],
      bundle: true,
      format: "esm",
      platform: "node",
      outfile: "dist/test-definition.mjs",
    });
    const plugin = (
      await import(pathToFileURL(path.join(cwd, "dist/test-definition.mjs")))
    ).default;
    for (const fixture of fixtures) {
      const tool = plugin.tools.find((t) => t.name === fixture.tool);
      if (!tool) throw new Error("Unknown fixture tool");
      const actual = tool.execute(fixture.input);
      if (JSON.stringify(actual) !== JSON.stringify(fixture.expected))
        throw new Error(`Fixture failed: ${fixture.tool}`);
    }
    console.log(`${fixtures.length} fixtures passed`);
    return;
  }
  console.log(
    "mlx-plugin init <name> | build | test [fixtures.json] | pack [directory]",
  );
}
main().catch((e) => {
  console.error(e.message);
  process.exitCode = 1;
});
