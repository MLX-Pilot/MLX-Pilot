This directory is used by the Tauri bundle resource glob in `tauri.conf.json`.

The llama.cpp runtime files are intentionally not committed. To stage the optional
bundled Windows engine for installer builds, run:

```powershell
./scripts/fetch-llama-engine.ps1
```

During development, this placeholder keeps `cargo tauri dev` from failing while
the daemon falls back to PATH lookup or automatic llama.cpp installation.
