import { build } from "esbuild";
import { fileURLToPath } from "node:url";
import path from "node:path";
const root = path.dirname(fileURLToPath(import.meta.url));
await build({
  entryPoints: [path.join(root, "src/studio.js")],
  bundle: true,
  format: "iife",
  target: ["es2020"],
  // Escape multiline literals, including significant whitespace in bundled dependencies.
  supported: { "template-literal": false },
  minify: true,
  sourcemap: false,
  outfile: path.resolve(
    root,
    "../../apps/desktop-ui/ui/presentation-studio.js",
  ),
  legalComments: "eof",
});
