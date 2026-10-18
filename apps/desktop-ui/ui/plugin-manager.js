/* Plugin host: opaque frontends use their own tools and the permission-checked host broker. */
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
    openedBridgeToken = null,
    configured = null,
    busy = false;
  root.innerHTML = `<div class="pm-shell"><header class="pm-header"><div><span class="pm-kicker">SUAS FERRAMENTAS, NO SEU COMPUTADOR</span><h1>Plugins</h1><p>Instale ferramentas de IA com interface própria e controle suas permissões.</p></div><label class="pm-primary">＋ Adicionar plugin<input id="pm-file" type="file" accept=".mlxplugin.json,.json" hidden></label></header><div id="pm-status" role="status" aria-live="polite"></div><div id="pm-list" class="pm-grid"></div><section id="pm-review" class="pm-review" hidden></section><section id="pm-configuration" class="pm-configuration" aria-labelledby="pm-configuration-title" hidden></section><section id="pm-view" class="pm-view" hidden><header><h2 id="pm-view-title"></h2><button id="pm-close">Fechar interface</button></header><div id="pm-frame"></div></section><footer>Plugins executam em WASM. Interfaces HTML/CSS/JS ficam isoladas. O MLX-Pilot controla o acesso autorizado a ferramentas, serviços e documentos.</footer></div>`;
  const $ = (id) => document.getElementById("pm-" + id);
  const status = (message, error = false) => {
    $("status").textContent = message;
    $("status").classList.toggle("error", error);
  };
  function close() {
    opened = null;
    openedBridgeToken = null;
    $("frame").replaceChildren();
    $("view").hidden = true;
  }
  $("close").onclick = close;
  function closeConfiguration() {
    configured = null;
    $("configuration").replaceChildren();
    $("configuration").hidden = true;
  }
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
          (p) => {
            const authorized = p.manifest.permissions.every((permission) => p.grants.includes(permission));
            return `<article class="pm-card" data-plugin-card="${esc(p.manifest.id)}"><div class="pm-card-top"><span class="pm-icon">${p.bundled ? "▧" : "◇"}</span><span class="pm-badge ${p.enabled && authorized ? "on" : ""}">${!p.enabled ? "Desativado" : authorized ? "Ativo" : "Permissões revogadas"}</span></div><h2>${esc(p.manifest.name)}</h2><p>v${esc(p.manifest.version)} · ${p.bundled ? "Incluído no MLX-Pilot" : "Instalado localmente"}</p><div class="pm-permissions">${p.manifest.permissions.map((x) => `<span class="${p.grants.includes(x) ? "" : "revoked"}" title="${p.grants.includes(x) ? "Permitido" : "Revogado"}">${esc(x)}</span>`).join("")}</div><p class="pm-capabilities">${p.manifest.tools.length} ferramenta(s) · ${p.has_frontend ? "Interface visual disponível" : "Ferramentas para o chat"}</p><div class="pm-actions"><button data-open="${esc(p.manifest.id)}" ${!p.enabled || !p.has_frontend || !authorized ? "disabled" : ""}>Abrir</button>${!p.bundled ? `<button data-configure="${esc(p.manifest.id)}" aria-label="Configurar ${esc(p.manifest.name)}">Configurar</button>` : ""}<button data-toggle="${esc(p.manifest.id)}">${p.enabled ? "Desativar" : "Ativar"}</button><button class="pm-danger" data-remove="${esc(p.manifest.id)}">Remover</button></div></article>`;
          },
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
    root.querySelectorAll("[data-configure]").forEach(
      (button) =>
        (button.onclick = () =>
          action(() => configure(button.dataset.configure))),
    );
    if ($("restore"))
      $("restore").onclick = () =>
        action(async () => {
          await api("/bundled/presentation", { method: "POST" });
          await refresh();
          status("Presentation Studio reinstalado.");
        });
    syncStudio();
    if (configured && !packages.some((p) => p.manifest.id === configured))
      closeConfiguration();
  }
  async function action(fn) {
    if (busy) return;
    busy = true;
    root.setAttribute("aria-busy", "true");
    const controls = [...root.querySelectorAll("button,input,select,textarea")]
      .map((node) => ({ node, disabled: node.disabled }));
    controls.forEach(({ node }) => { node.disabled = true; });
    try {
      await fn();
    } catch (e) {
      status(e.message, true);
    } finally {
      busy = false;
      root.removeAttribute("aria-busy");
      controls.forEach(({ node, disabled }) => {
        if (node.isConnected) node.disabled = disabled;
      });
    }
  }

  async function configure(id) {
    const plugin = packages.find((p) => p.manifest.id === id);
    if (!plugin || plugin.bundled) return;
    status(`Carregando configuração de ${plugin.manifest.name}…`);
    const configuration = await api(`/${encodeURIComponent(id)}/configuration`);
    configured = id;
    renderConfiguration(plugin, configuration);
    status(`Configuração de ${plugin.manifest.name} carregada.`);
    $("configuration").scrollIntoView({ behavior: "smooth", block: "start" });
    $("configuration-title").focus();
  }

  function renderConfiguration(plugin, configuration) {
    const grants = new Set(configuration.grants || []);
    const services = plugin.manifest.services || [];
    const selectedDocuments = new Set(configuration.documents?.ids || []);
    const documents = [...(configuration.available_documents || [])];
    for (const id of selectedDocuments)
      if (!documents.some((entry) => entry.id === id))
        documents.push({ id, title: "Documento indisponível", unavailable: true });
    const grantLabels = {
      "compute": "Executar ferramentas de processamento",
      "network:request": "Fazer requisições HTTP autorizadas",
      "credentials:use": "Usar credenciais autorizadas",
      "documents:read": "Ler documentos autorizados",
      "documents:write": "Criar ou editar documentos autorizados",
    };
    $("configuration").innerHTML = `<header class="pm-configuration-header"><div><span class="pm-kicker">ACESSOS DO PLUGIN</span><h2 id="pm-configuration-title" tabindex="-1">Configurar ${esc(plugin.manifest.name)}</h2><p>Escolha quais recursos este plugin pode usar. Você pode mudar estas autorizações mesmo com o plugin desativado.</p></div><button type="button" id="pm-close-configuration">Fechar configuração</button></header>
      <form id="pm-configuration-form"><fieldset class="pm-config-section"><legend>Permissões</legend><p class="pm-help">Desmarque para revogar um acesso. Permissões revogadas impedem a execução do plugin até serem concedidas novamente.</p><div class="pm-check-list">${plugin.manifest.permissions.map((permission) => `<label class="pm-check"><input type="checkbox" name="grant" value="${esc(permission)}" ${grants.has(permission) ? "checked" : ""}><span>${esc(grantLabels[permission] || permission)}<small>${esc(permission)}</small></span></label>`).join("") || '<p class="pm-help">Este plugin não solicita permissões adicionais.</p>'}</div></fieldset>
      <fieldset class="pm-config-section"><legend>Serviços autorizados</legend><p class="pm-help">Cada serviço usa apenas a URL e os métodos declarados abaixo. Credenciais ficam no MLX-Pilot e não são expostas à interface do plugin.</p><div class="pm-services">${services.map((service, index) => {
        const current = configuration.services?.[service.id] || {};
        return `<section class="pm-service" data-service-id="${esc(service.id)}" aria-labelledby="pm-service-title-${index}"><div class="pm-service-header"><h3 id="pm-service-title-${index}">${esc(service.name || service.id)}</h3><label class="pm-check"><input data-service-enabled type="checkbox" ${current.enabled ? "checked" : ""}><span>Autorizar serviço</span></label></div><p class="pm-help">Métodos permitidos: ${esc((service.methods || []).join(", ") || "nenhum")}.</p><label class="pm-field" for="pm-service-url-${index}">URL autorizada<input id="pm-service-url-${index}" data-service-url type="url" required value="${esc(current.base_url ?? service.base_url ?? "")}" placeholder="https://api.exemplo.com" autocomplete="off" spellcheck="false"></label><label class="pm-check"><input data-service-private type="checkbox" ${current.allow_private_network ? "checked" : ""}><span>Permitir acesso à rede local<small>Autoriza este serviço a acessar localhost e endereços privados da sua rede.</small></span></label>${service.credential ? `<div class="pm-credential"><p class="pm-credential-status">${current.credential_set ? "Credencial cadastrada. O valor não é exibido." : "Nenhuma credencial cadastrada."}</p><label class="pm-field" for="pm-service-secret-${index}">${current.credential_set ? "Substituir credencial" : "Cadastrar credencial"}<input id="pm-service-secret-${index}" data-service-secret type="password" autocomplete="new-password" spellcheck="false" autocapitalize="off" aria-describedby="pm-secret-help-${index}" placeholder="${current.credential_set ? "Deixe vazio para manter a credencial" : "Informe o token ou a chave de API"}"></label><p id="pm-secret-help-${index}" class="pm-help">A credencial será usada somente nas chamadas autorizadas deste serviço.</p><label class="pm-check"><input data-service-clear type="checkbox"><span>Remover credencial cadastrada</span></label></div>` : '<p class="pm-help">Este serviço não solicita credencial.</p>'}</section>`;
      }).join("") || '<p class="pm-help">Este plugin não declara serviços externos.</p>'}</div></fieldset>
      <fieldset class="pm-config-section"><legend>Documentos do Presentation Studio</legend><p class="pm-help">Escolha os projetos que o plugin pode acessar, conforme as permissões concedidas acima.</p><div class="pm-document-list">${documents.map((doc) => `<label class="pm-check"><input type="checkbox" name="document" value="${esc(doc.id)}" ${selectedDocuments.has(doc.id) ? "checked" : ""}><span>${esc(doc.title)}<small>${doc.unavailable ? esc(doc.id) : `${esc(doc.slides ?? "—")} slide(s) · revisão ${esc(doc.revision ?? "—")}`}</small></span></label>`).join("") || '<p class="pm-help">Nenhuma apresentação disponível neste workspace.</p>'}</div><label class="pm-check pm-create-documents"><input id="pm-allow-create" type="checkbox" ${configuration.documents?.allow_create ? "checked" : ""}><span>Permitir criar novas apresentações</span></label></fieldset>
      <div class="pm-actions pm-config-actions"><button type="submit" class="pm-primary" id="pm-save-configuration">Salvar configuração</button><button type="button" id="pm-cancel-configuration">Cancelar</button></div><p id="pm-configuration-status" role="status" aria-live="polite"></p></form>`;
    $("configuration").hidden = false;
    for (const id of ["close-configuration", "cancel-configuration"])
      $(id).onclick = () => { if (!busy) closeConfiguration(); };
    $("configuration").querySelectorAll("[data-service-clear]").forEach((checkbox) => {
      checkbox.onchange = () => {
        const secret = checkbox.closest(".pm-service").querySelector("[data-service-secret]");
        if (checkbox.checked) secret.value = "";
        secret.disabled = checkbox.checked;
      };
    });
    $("configuration-form").onsubmit = (event) => {
      event.preventDefault();
      if (busy) return;
      action(() => saveConfiguration(plugin));
    };
  }

  async function saveConfiguration(plugin) {
    if (configured !== plugin.manifest.id) return;
    const form = $("configuration-form");
    const credentials = [];
    const services = Object.create(null);
    for (const section of form.querySelectorAll("[data-service-id]")) {
      const baseUrl = section.querySelector("[data-service-url]").value.trim();
      let parsed;
      try { parsed = new URL(baseUrl); } catch { throw new Error("Informe uma URL HTTP ou HTTPS válida para cada serviço."); }
      if (!["http:", "https:"].includes(parsed.protocol) || parsed.username || parsed.password || parsed.search || parsed.hash)
        throw new Error("Use uma URL HTTP ou HTTPS sem credenciais, parâmetros ou fragmentos. Informe tokens no campo de credencial.");
      const service = {
        enabled: section.querySelector("[data-service-enabled]").checked,
        base_url: baseUrl,
        allow_private_network: section.querySelector("[data-service-private]").checked,
      };
      if (parsed.protocol === "http:" && !service.allow_private_network)
        throw new Error("URLs HTTP exigem a autorização de acesso à rede local e um endereço local. Para serviços externos, use HTTPS.");
      const secret = section.querySelector("[data-service-secret]");
      const clear = section.querySelector("[data-service-clear]");
      if (clear?.checked) service.clear_credential = true;
      else if (secret?.value) {
        service.credential = secret.value;
        credentials.push(secret.value);
      }
      services[section.dataset.serviceId] = service;
    }
    const body = {
      grants: [...form.querySelectorAll('input[name="grant"]:checked')].map((node) => node.value),
      services,
      documents: {
        ids: [...form.querySelectorAll('input[name="document"]:checked')].map((node) => node.value),
        allow_create: $("allow-create").checked,
      },
    };
    $("configuration-status").textContent = "Salvando configuração…";
    try {
      const result = await api(`/${encodeURIComponent(plugin.manifest.id)}/configuration`, {
        method: "PUT", body: JSON.stringify(body),
      });
      const reopen = opened === plugin.manifest.id;
      if (reopen) close();
      renderConfiguration(plugin, result);
      $("configuration-status").textContent = "Configuração salva. As novas autorizações já estão em vigor.";
      status(`Configuração de ${plugin.manifest.name} salva.`);
      const refreshed = await refresh().then(() => true).catch(() => {
        status("Configuração salva. Reabra a central de plugins para atualizar a lista.");
        return false;
      });
      const current = packages.find((entry) => entry.manifest.id === plugin.manifest.id);
      if (reopen && refreshed && current?.enabled && current.has_frontend && current.manifest.permissions.every((permission) => current.grants.includes(permission)))
        await open(plugin.manifest.id).catch(() => {
          status("Configuração salva. Use Abrir para carregar novamente a interface do plugin.");
        });
    } catch (error) {
      let message = error.message || "Não foi possível salvar a configuração.";
      for (const credential of credentials) message = message.split(credential).join("[credencial protegida]");
      $("configuration-status").textContent = message;
      throw new Error(message);
    } finally {
      // Drop secret values after a write attempt; a retry requires entering them again.
      form.querySelectorAll("[data-service-secret]").forEach((node) => { node.value = ""; });
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
    const bridgeToken = crypto.randomUUID();
    openedBridgeToken = bridgeToken;
    const frame = document.createElement("iframe");
    frame.title = packages.find((p) => p.manifest.id === id).manifest.name;
    frame.setAttribute("sandbox", "allow-scripts");
    frame.referrerPolicy = "no-referrer";
    const payload = JSON.stringify(ui).replace(/</g, "\\u003c");
    // CSP is host-controlled and precedes all package markup. Never add allow-same-origin.
    frame.srcdoc = `<!doctype html><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none';"><script>window.__MLX_PLUGIN_BRIDGE_TOKEN__=${JSON.stringify(bridgeToken)};const ui=${payload};document.addEventListener('DOMContentLoaded',()=>{document.body.innerHTML=ui.html;const style=document.createElement('style');style.textContent=ui.css;document.head.append(style);const script=document.createElement('script');script.textContent=ui.javascript;document.body.append(script);});<\/script><body></body>`;
    let loaded = false;
    frame.addEventListener("load", () => {
      if (!loaded) { loaded = true; return; }
      if (opened === id && openedBridgeToken === bridgeToken && frame.isConnected) {
        close();
        status("A interface do plugin mudou de página e foi desconectada. Clique em Abrir para carregá-la novamente.", true);
      }
    });
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
    const { requestId, tool, operation, input } = event.data;
    if (
      typeof requestId !== "string" ||
      !requestId.length ||
      requestId.length > 100
    )
      return;
    if (event.data.bridgeToken !== openedBridgeToken) {
      if (event.data.bridgeToken === undefined)
        frame.contentWindow.postMessage({
          channel: "mlx-plugin-v1", requestId,
          error: "Recompile e reinstale este plugin com o SDK atualizado para conectar a interface.",
        }, "*");
      return;
    }
    const id = opened;
    const bridgeToken = openedBridgeToken;
    const reply = (value) => {
      if (opened === id && openedBridgeToken === bridgeToken && frame.isConnected)
        frame.contentWindow.postMessage(
          { channel: "mlx-plugin-v1", bridgeToken, requestId, ...value },
          "*",
        );
    };
    try {
      const operations = ["http.request", "credentials.status", "documents.list", "documents.get", "documents.create", "documents.edit"];
      const hostRequest = operation !== undefined;
      if (hostRequest ? !operations.includes(operation) || tool !== undefined : typeof tool !== "string" || !/^[a-z0-9_]{1,30}$/.test(tool))
        throw new Error("Operação do plugin não autorizada.");
      const payload = input ?? {};
      if (JSON.stringify(payload).length > 1024 * 1024)
        throw new Error("Entrada muito grande.");
      const result = await api(hostRequest ? `/${encodeURIComponent(id)}/host` : `/${encodeURIComponent(id)}/tools/${tool}`, {
        method: "POST",
        body: JSON.stringify(hostRequest ? { operation, input: payload } : payload),
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
        (pkg.manifest.services != null && !Array.isArray(pkg.manifest.services)) ||
        typeof pkg.wasm_base64 !== "string"
      )
        throw new Error("Pacote inválido.");
      const services = pkg.manifest.services || [];
      if (services.length > 20 || services.some((service) =>
        !service || typeof service.base_url !== "string" || !Array.isArray(service.methods) || !service.methods.every((method) => typeof method === "string")))
        throw new Error("O pacote contém uma declaração de serviços inválida.");
      const serviceReview = services.length
        ? `<section class="pm-install-services" aria-label="Serviços solicitados"><h3>Serviços solicitados</h3><p class="pm-help">Depois de instalar, abra Configurar para autorizar cada serviço, escolher documentos e cadastrar credenciais. Atualizações que mudam permissões ou serviços exigem revisar essas autorizações.</p><div class="pm-services">${services.map((service) => `<article class="pm-service"><h3>${esc(service.name || service.id)}</h3><dl class="pm-service-details"><dt>Destino declarado</dt><dd>${esc(service.base_url)}</dd><dt>Métodos</dt><dd>${esc(service.methods.join(", "))}</dd>${service.credential ? `<dt>Destino da credencial</dt><dd>${esc(service.base_url)}<br>Cabeçalho <code>${esc(service.credential.header)}</code>${service.credential.prefix ? ` · prefixo <code>${esc(service.credential.prefix)}</code>` : ""}<br>Credencial: ${esc(service.credential.id)}</dd>` : '<dt>Credencial</dt><dd>Não solicitada</dd>'}</dl></article>`).join("")}</div></section>`
        : "";
      pending = pkg;
      $("review").innerHTML =
        `<h2>Adicionar ${esc(pkg.manifest.name)}?</h2><p>Revise as permissões antes de instalar. ${pkg.manifest.frontend ? "Este pacote inclui uma interface HTML/CSS/JS isolada." : "Este pacote oferece ferramentas para o chat."}</p>${pkg.manifest.permissions.map((p) => `<label><input type="checkbox" value="${esc(p)}"> ${esc(p)}</label>`).join("")}${serviceReview}<div class="pm-actions"><button id="pm-install" class="pm-primary">Instalar plugin</button><button id="pm-cancel">Cancelar</button></div>`;
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
