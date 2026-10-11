/* Plugin host: UI assets execute in an opaque iframe and can invoke only their own WASM tools. */
(() => {
  const root = document.getElementById("plugin-manager");
  if (!root) return;
  const esc = (s) =>
    String(s).replace(
      /[&<>"']/g,
      (c) =>
        ({
          "&": "&amp;",
          "<": "&lt;",
          ">": "&gt;",
          '"': "&quot;",
          "'": "&#39;",
        })[c],
    );
  const base = () =>
    window.__MLX_PILOT_DAEMON_URL__ ||
    localStorage.getItem("mlxPilotDaemonUrl") ||
    "http://127.0.0.1:11435";
  async function api(path, options = {}) {
    const r = await fetch(base() + "/agent/wasm-plugins" + path, {
      ...options,
      headers: { "Content-Type": "application/json" },
    });
    const v = await r.json();
    if (!r.ok) throw new Error(v.error || `HTTP ${r.status}`);
    return v;
  }
  let packages = [],
    pending = null,
    opened = null,
    busy = false;
  root.innerHTML = `<div class="pm-shell"><header class="pm-header"><div><span class="pm-kicker">SUAS FERRAMENTAS, NO SEU COMPUTADOR</span><h1>Plugins</h1><p>Instale ferramentas de IA com interface própria e controle suas permissões.</p></div><label class="pm-primary">＋ Adicionar plugin<input id="pm-file" type="file" accept=".mlxplugin.json,.json" hidden></label></header><div id="pm-status" role="status" aria-live="polite"></div><div id="pm-list" class="pm-grid"></div><section id="pm-review" class="pm-review" hidden></section><section id="pm-view" class="pm-view" hidden><header><h2 id="pm-view-title"></h2><button id="pm-close">Fechar interface</button></header><div id="pm-frame"></div></section><footer>Plugins executam em WASM. Interfaces HTML/CSS/JS ficam isoladas e acessam apenas as ferramentas autorizadas do próprio plugin.</footer></div>`;
  const $ = (id) => document.getElementById("pm-" + id);
  const status = (message, error = false) => {
    $("status").textContent = message;
    $("status").classList.toggle("error", error);
  };
  function close() {
    opened = null;
    $("frame").replaceChildren();
    $("view").hidden = true;
  }
  $("close").onclick = close;
  function syncStudio() {
    const p = packages.find((p) => p.manifest.id === "presentation");
    const enabled =
      !!p?.enabled && p.manifest.permissions.every((x) => p.grants.includes(x));
    const panel = document.getElementById("presentation-studio");
    panel.inert = !enabled;
    let notice = document.getElementById("presentation-unavailable");
    if (!notice) {
      notice = document.createElement("div");
      notice.id = "presentation-unavailable";
      notice.className = "pm-unavailable";
      panel.before(notice);
    }
    notice.hidden = enabled;
    notice.replaceChildren();
    if (!enabled) {
      const text = document.createElement("p");
      text.textContent = p
        ? "Presentation Studio está desativado."
        : "Presentation Studio foi removido. Seus projetos foram preservados.";
      const button = document.createElement("button");
      button.textContent = "Gerenciar plugins";
      button.onclick = () =>
        document.querySelector('[data-panel="plugins"]').click();
      notice.append(text, button);
    }
    panel.hidden = !enabled;
    window.dispatchEvent(
      new CustomEvent("mlx:plugins-changed", { detail: { plugins: packages } }),
    );
    if (opened && !packages.some((p) => p.manifest.id === opened && p.enabled))
      close();
  }
  async function refresh() {
    packages = await api("");
    $("list").innerHTML =
      packages
        .map(
          (p) =>
            `<article class="pm-card" data-plugin-card="${p.manifest.id}"><div class="pm-card-top"><span class="pm-icon">${p.bundled ? "▧" : "◇"}</span><span class="pm-badge ${p.enabled ? "on" : ""}">${p.enabled ? "Ativo" : "Desativado"}</span></div><h2>${esc(p.manifest.name)}</h2><p>v${esc(p.manifest.version)} · ${p.bundled ? "Incluído no MLX-Pilot" : "Instalado localmente"}</p><div class="pm-permissions">${p.manifest.permissions.map((x) => `<span>${esc(x)}</span>`).join("")}</div><p class="pm-capabilities">${p.manifest.tools.length} ferramenta(s) · ${p.has_frontend ? "Interface visual disponível" : "Ferramentas para o chat"}</p><div class="pm-actions"><button data-open="${p.manifest.id}" ${!p.enabled || !p.has_frontend ? "disabled" : ""}>Abrir</button><button data-toggle="${p.manifest.id}">${p.enabled ? "Desativar" : "Ativar"}</button><button class="pm-danger" data-remove="${p.manifest.id}">Remover</button></div></article>`,
        )
        .join("") +
      (packages.some((p) => p.bundled)
        ? ""
        : `<article class="pm-card"><span class="pm-icon">▧</span><h2>Presentation Studio</h2><p>Crie apresentações com chat, código e mouse. Exporte PDF, PPTX e HTML.</p><button id="pm-restore">Reinstalar Presentation Studio</button></article>`);
    root.querySelectorAll("[data-toggle]").forEach(
      (b) =>
        (b.onclick = () =>
          action(async () => {
            const p = packages.find((p) => p.manifest.id === b.dataset.toggle);
            await api("/" + p.manifest.id, {
              method: "PATCH",
              body: JSON.stringify({ enabled: !p.enabled }),
            });
            await refresh();
            status(
              `${p.manifest.name} ${p.enabled ? "desativado" : "ativado"}.`,
            );
          })),
    );
    root.querySelectorAll("[data-remove]").forEach(
      (b) =>
        (b.onclick = () =>
          action(async () => {
            const p = packages.find((p) => p.manifest.id === b.dataset.remove);
            if (
              !window.confirm(
                `Remover ${p.manifest.name}? Os documentos salvos serão preservados.`,
              )
            )
              return;
            await api("/" + p.manifest.id, { method: "DELETE" });
            await refresh();
            status(`${p.manifest.name} removido.`);
          })),
    );
    root
      .querySelectorAll("[data-open]")
      .forEach((b) => (b.onclick = () => action(() => open(b.dataset.open))));
    if ($("restore"))
      $("restore").onclick = () =>
        action(async () => {
          await api("/bundled/presentation", { method: "POST" });
          await refresh();
          status("Presentation Studio reinstalado.");
        });
    syncStudio();
  }
  async function action(fn) {
    if (busy) return;
    busy = true;
    root.setAttribute("aria-busy", "true");
    try {
      await fn();
    } catch (e) {
      status(e.message, true);
    } finally {
      busy = false;
      root.removeAttribute("aria-busy");
    }
  }
  async function open(id) {
    if (id === "presentation") {
      document.querySelector('[data-panel="presentations"]').click();
      return;
    }
    const ui = await api("/" + id + "/frontend");
    close();
    opened = id;
    const frame = document.createElement("iframe");
    frame.title = packages.find((p) => p.manifest.id === id).manifest.name;
    frame.setAttribute("sandbox", "allow-scripts");
    frame.referrerPolicy = "no-referrer";
    const payload = JSON.stringify(ui).replace(/</g, "\\u003c");
    // CSP is host-controlled and precedes all package markup. Never add allow-same-origin.
    frame.srcdoc = `<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none';"><script>const ui=${payload};document.addEventListener('DOMContentLoaded',()=>{document.body.innerHTML=ui.html;const style=document.createElement('style');style.textContent=ui.css;document.head.append(style);const script=document.createElement('script');script.textContent=ui.javascript;document.body.append(script);});<\/script><body></body>`;
    $("view-title").textContent = frame.title;
    $("frame").append(frame);
    $("view").hidden = false;
    $("view").scrollIntoView({ behavior: "smooth", block: "start" });
  }
  window.addEventListener("message", async (event) => {
    const frame = $("frame").querySelector("iframe");
    if (
      !opened ||
      event.source !== frame?.contentWindow ||
      event.data?.channel !== "mlx-plugin-v1"
    )
      return;
    const { requestId, tool, input } = event.data;
    if (
      typeof requestId !== "string" ||
      requestId.length > 100 ||
      typeof tool !== "string" ||
      !/^[a-z0-9_]{1,30}$/.test(tool)
    )
      return;
    const id = opened;
    const reply = (value) => {
      if (opened === id && frame.isConnected)
        frame.contentWindow.postMessage(
          { channel: "mlx-plugin-v1", requestId, ...value },
          "*",
        );
    };
    try {
      if (JSON.stringify(input).length > 1024 * 1024)
        throw new Error("Entrada muito grande.");
      const result = await api(`/${id}/tools/${tool}`, {
        method: "POST",
        body: JSON.stringify(input),
      });
      reply({ result });
    } catch (e) {
      reply({ error: e.message });
    }
  });
  $("file").onchange = () =>
    action(async () => {
      pending = null;
      $("review").hidden = true;
      const file = $("file").files[0];
      $("file").value = "";
      if (!file) return;
      if (file.size > 13 * 1024 * 1024)
        throw new Error("Limite de 13 MB por pacote.");
      const pkg = JSON.parse(await file.text());
      if (
        !pkg.manifest ||
        !Array.isArray(pkg.manifest.permissions) ||
        typeof pkg.wasm_base64 !== "string"
      )
        throw new Error("Pacote inválido.");
      pending = pkg;
      $("review").innerHTML =
        `<h2>Adicionar ${esc(pkg.manifest.name)}?</h2><p>Revise as permissões antes de instalar. ${pkg.manifest.frontend ? "Este pacote inclui uma interface HTML/CSS/JS isolada." : "Este pacote oferece ferramentas para o chat."}</p>${pkg.manifest.permissions.map((p) => `<label><input type="checkbox" value="${esc(p)}"> ${esc(p)}</label>`).join("")}<div class="pm-actions"><button id="pm-install" class="pm-primary">Instalar plugin</button><button id="pm-cancel">Cancelar</button></div>`;
      $("review").hidden = false;
      $("review").scrollIntoView({ behavior: "smooth" });
      $("cancel").onclick = () => {
        pending = null;
        $("review").hidden = true;
      };
      $("install").onclick = () =>
        action(async () => {
          if (!pending) return;
          const grants = [...$("review").querySelectorAll("input:checked")].map(
            (x) => x.value,
          );
          if (grants.length !== pending.manifest.permissions.length)
            throw new Error(
              "Conceda as permissões solicitadas para ativar este plugin.",
            );
          status("Validando e instalando plugin…");
          await api("", {
            method: "POST",
            body: JSON.stringify({ ...pending, grants }),
          });
          pending = null;
          $("review").hidden = true;
          await refresh();
          status("Plugin instalado e ativado.");
        });
    });
  document
    .querySelector('[data-panel="plugins"]')
    .addEventListener("click", () => action(refresh));
  window.PluginManagerUI = { refresh };
  refresh().catch((e) => status(e.message, true));
})();
