# Plugin security guest

`broker.wat` is a deliberately small Extism guest used only by the release gate.
It forwards its JSON input to the real `mlx_host_call` import. The `quota` export
repeats the same request four times within one invocation; `spin` loops until the
runtime's fuel limit stops execution. No external service or model is involved.

Rebuild the JSON byte fixture with the locked workspace dependency:

```sh
cargo run --locked --quiet -p mlx-plugin-runtime --example plugin_security_fixture -- OUTPUT_JSON
```

The gate runs this generator in its artifact directory and records the compiled
WASM SHA-256. It installs two separately named copies through the daemon API.
Do not ship these adversarial tools as application plugins.
