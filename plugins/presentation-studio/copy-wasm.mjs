import { copyFile } from "node:fs/promises";
await copyFile(
  new URL(
    "./guest/target/wasm32-unknown-unknown/release/presentation_studio_plugin.wasm",
    import.meta.url,
  ),
  new URL("./plugin.wasm", import.meta.url),
);
