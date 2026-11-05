import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { JSDOM } from "jsdom";

const uiDir = new URL("../ui/", import.meta.url);
const indexHtml = readFileSync(new URL("index.html", uiDir), "utf8");
const wave5Source = readFileSync(new URL("wave5.js", uiDir), "utf8");

// What the daemon answers for Mac Studio (M4 Max) with 64 GB.
const SIMULATED_MAC = {
  platform: "macos",
  cpu_name: "Apple M4 Max",
  cpu_cores: 16,
  ram_gb: 64,
  available_ram_gb: 57.6,
  gpus: [{ name: "Apple M4 Max GPU", vram_gb: 48, index: 0, backend: "metal", bandwidth_gb_s: 546 }],
  gpu_count: 1,
  total_vram_gb: 48,
  primary_backend: "metal",
  is_cpu_only: false,
  gpu_groups: [],
  detected_at: "2026-11-05T00:00:00Z",
};

function loadHardwarePanel() {
  const dom = new JSDOM(indexHtml, { runScripts: "outside-only", url: "http://localhost/" });
  const { window } = dom;
  const requests = [];
  const alerts = [];
  window.fetch = async (url) => {
    requests.push(new URL(String(url)));
    return {
      ok: true,
      headers: { get: () => "application/json" },
      json: async () => ({ hardware: SIMULATED_MAC, models: [], total: 0 }),
    };
  };
  window.alert = (message) => alerts.push(message);
  window.console.log = () => {};
  window.eval(wave5Source);
  return { window, document: window.document, requests, alerts };
}

function choose(window, id, value) {
  const select = window.document.getElementById(id);
  select.value = value;
  assert.equal(select.value, value, `option ${value} missing from #${id}`);
  select.dispatchEvent(new window.Event("change"));
}

async function settle(window) {
  for (let i = 0; i < 5; i += 1) {
    await new Promise((resolve) => window.setTimeout(resolve, 0));
  }
}

const isHidden = (document, id) => document.getElementById(id).style.display === "none";

test("Mac devices are listed in the GPU / device picker, grouped by device", () => {
  const { document } = loadHardwarePanel();
  const groups = [...document.querySelectorAll("#hw-sim-gpu-preset optgroup")].map((g) => g.label);

  assert.deepEqual(groups, [
    "Mac · MacBook Air",
    "Mac · MacBook Pro",
    "Mac · Mac mini",
    "Mac · Mac Studio",
    "Mac · iMac",
    "Mac · Mac Pro",
  ]);

  const studio = document.querySelector('#hw-sim-gpu-preset optgroup[label="Mac · Mac Studio"]');
  assert.ok([...studio.children].some((o) => o.textContent === "Mac Studio (M5 Ultra)"));

  // "Personalizada" stays the last option, after the Mac groups.
  const select = document.getElementById("hw-sim-gpu-preset");
  assert.equal(select.options[select.options.length - 1].value, "custom");
});

test("picking a Mac swaps the discrete GPU fields for the chip's unified memory sizes", () => {
  const { window, document } = loadHardwarePanel();
  choose(window, "hw-sim-gpu-preset", "mac-mac-studio-m4-max");

  for (const id of ["hw-sim-gpu-count-box", "hw-sim-vram-box", "hw-sim-ram-preset-box", "hw-sim-backend-box"]) {
    assert.ok(isHidden(document, id), `${id} should be hidden for a Mac`);
  }
  assert.ok(!isHidden(document, "hw-sim-mac-memory-box"));
  assert.ok(!isHidden(document, "hw-sim-mac-note"));

  const sizes = [...document.getElementById("hw-sim-mac-memory").options].map((o) => o.value);
  assert.deepEqual(sizes, ["36", "48", "64", "128"]);

  choose(window, "hw-sim-gpu-preset", "rtx-4090");
  for (const id of ["hw-sim-gpu-count-box", "hw-sim-vram-box", "hw-sim-ram-preset-box", "hw-sim-backend-box"]) {
    assert.ok(!isHidden(document, id), `${id} should be back for a discrete GPU`);
  }
  assert.ok(isHidden(document, "hw-sim-mac-memory-box"));
  assert.equal(document.getElementById("hw-sim-backend").value, "cuda");
});

test("applying a simulated Mac asks the daemon for unified memory, not discrete VRAM", async () => {
  const { window, document, requests, alerts } = loadHardwarePanel();
  choose(window, "hw-sim-gpu-preset", "mac-mac-studio-m4-max");
  choose(window, "hw-sim-mac-memory", "64");

  document.getElementById("btn-hw-sim-apply").click();
  await settle(window);

  assert.deepEqual(alerts, []);
  const ranking = requests.find((u) => u.pathname === "/api/hwfit/models");
  assert.ok(ranking, "ranking request was not sent");
  const params = ranking.searchParams;
  assert.equal(params.get("manual_mode"), "true");
  assert.equal(params.get("manual_gpu_name"), "Apple M4 Max");
  assert.equal(params.get("manual_unified_memory_gb"), "64");
  assert.equal(params.get("ignore_detected_gpu"), "true");
  assert.equal(params.get("ignore_detected_ram"), "true");
  for (const discrete of ["manual_vram_gb", "manual_gpu_count", "manual_ram_gb", "manual_backend"]) {
    assert.equal(params.has(discrete), false, `${discrete} should not be sent for a Mac`);
  }

  assert.equal(
    document.getElementById("hw-mode-badge").textContent,
    "Modo: Simulado · Mac Studio (M4 Max) · 64 GB"
  );
  const cards = document.getElementById("hw-cards").textContent;
  assert.match(cards, /Memória unificada/);
  assert.match(cards, /GPU integrada · 48\.0 GB para a GPU/);
  assert.match(cards, /≈75% da memória unificada/);
});

test("restoring the real hardware leaves Mac mode", async () => {
  const { window, document } = loadHardwarePanel();
  choose(window, "hw-sim-gpu-preset", "mac-macbook-air-m5");

  document.getElementById("btn-hw-sim-reset").click();
  await settle(window);

  assert.equal(document.getElementById("hw-sim-gpu-preset").value, "current");
  assert.ok(isHidden(document, "hw-sim-mac-memory-box"));
  assert.ok(!isHidden(document, "hw-sim-vram-box"));
  assert.equal(document.getElementById("hw-mode-badge").textContent, "Modo: Detectado");
});
