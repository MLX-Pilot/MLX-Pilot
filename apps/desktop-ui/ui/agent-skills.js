export function createAgentSkillsController({
  elements,
  fetchJson,
  promptText,
  onStatus,
}) {
  let skillsCache = [];
  let summaryCache = null;

  function setStatus(message) {
    if (typeof onStatus === "function") {
      onStatus(message);
    }
  }

  function renderSummary(summary) {
    if (!elements.agentSkillsSummary) {
      return;
    }
    if (!summary) {
      elements.agentSkillsSummary.textContent = "Check das skills pendente.";
      return;
    }
    elements.agentSkillsSummary.textContent =
      `${summary.eligible}/${summary.total} elegiveis, ` +
      `${summary.active} ativas, ` +
      `${summary.missing_dependencies} com dependencias faltando, ` +
      `${summary.missing_configuration} aguardando config.`;
  }

  async function loadSkills() {
    const payload = await fetchJson("/agent/skills/check", { method: "GET" });
    skillsCache = Array.isArray(payload?.skills) ? payload.skills : [];
    summaryCache = payload?.summary || null;
    renderSkills();
    renderSummary(summaryCache);
    return payload;
  }

  async function toggleSkill(skillName, enabled) {
    await fetchJson(enabled ? "/agent/skills/enable" : "/agent/skills/disable", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ skill: skillName }),
    });
    await loadSkills();
  }

  async function configureSkill(skill) {
    if (!skill?.name) {
      return;
    }

    const missingEnv = Array.isArray(skill.missing)
      ? skill.missing
          .filter((item) => String(item).startsWith("env:"))
          .map((item) => String(item).slice("env:".length))
      : [];
    const missingConfig = Array.isArray(skill.missing)
      ? skill.missing
          .filter((item) => String(item).startsWith("config:"))
          .map((item) => String(item).slice("config:".length))
      : [];

    const envPayload = {};
    const firstEnv = skill.primary_env || missingEnv[0] || "";
    if (firstEnv) {
      const envValue = await promptText({
        title: `${skill.name}: ${firstEnv}`,
        message: "Informe o valor do env. Deixe vazio para manter como esta.",
        defaultValue: "",
        confirmLabel: "Salvar env",
      });
      if (envValue !== null && envValue !== "") {
        envPayload[firstEnv] = envValue;
      }
    }

    const configPayload = {};
    for (const key of missingConfig) {
      const value = await promptText({
        title: `${skill.name}: ${key}`,
        message: "Informe o valor da configuracao.",
        defaultValue: "",
        confirmLabel: "Salvar config",
      });
      if (value === null) {
        continue;
      }
      configPayload[key] = value;
    }

    if (!Object.keys(envPayload).length && !Object.keys(configPayload).length) {
      return;
    }

    await fetchJson("/agent/skills/config", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        skill: skill.name,
        enabled: true,
        env: envPayload,
        config: configPayload,
      }),
    });

    await loadSkills();
  }

  async function installSkills(skillNames) {
    const names = Array.isArray(skillNames) ? skillNames.filter(Boolean) : [];
    if (!names.length) {
      return null;
    }

    const payload = await fetchJson("/agent/skills/install", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        skills: names,
        node_manager: elements.agentNodeManagerSelect?.value || "npm",
      }),
    });

    const results = Array.isArray(payload?.results) ? payload.results : [];
    const lines = results.flatMap((result) => {
      const installs = Array.isArray(result.installs) ? result.installs : [];
      if (!installs.length) {
        return [`${result.skill}: sem instalador relevante`];
      }
      return installs.map((install) => {
        const status = install.ok ? "ok" : "falhou";
        const extra = Array.isArray(install.warnings) && install.warnings.length
          ? ` [${install.warnings.join(", ")}]`
          : "";
        return `${result.skill}: ${install.label} -> ${status}${extra}`;
      });
    });

    if (lines.length) {
      setStatus(lines.join(" | "));
    }

    await loadSkills();
    return payload;
  }

  function renderSkills() {
    const container = elements.agentSkillsList;
    if (!container) {
      return;
    }

    container.innerHTML = "";
    if (!skillsCache.length) {
      const empty = document.createElement("li");
      empty.className = "meta-note";
      empty.textContent = "Sem skills detectadas.";
      container.appendChild(empty);
      return;
    }

    skillsCache.forEach((item) => {
      const li = document.createElement("li");
      li.className = "agent-toggle-item";
      if (item.active) {
        li.classList.add("skill-active");
      }
      if (!item.eligible) {
        li.classList.add("skill-ineligible");
      }

      const meta = document.createElement("div");
      meta.className = "agent-toggle-meta";

      const title = document.createElement("p");
      title.className = "agent-toggle-title";
      title.textContent = item.name || "-";

      const desc = document.createElement("p");
      desc.className = "agent-toggle-desc";
      desc.textContent = item.description || "";

      const detail = document.createElement("p");
      detail.className = "agent-toggle-desc";
      detail.textContent =
        Array.isArray(item.missing) && item.missing.length
          ? `Faltando: ${item.missing.join(", ")}`
          : `Source: ${item.source || "workspace"} | Integrity: ${item.integrity || "unknown"}`;

      const badges = document.createElement("div");
      badges.className = "agent-skill-badges";
      [
        item.active ? "ativa" : item.enabled ? "habilitada" : "desabilitada",
        item.eligible ? "eligivel" : "faltando requisitos",
        item.primary_env ? `env ${item.primary_env}` : "",
        Array.isArray(item.capabilities) && item.capabilities.length
          ? item.capabilities.join(" / ")
          : "",
      ]
        .filter(Boolean)
        .forEach((label) => {
          const badge = document.createElement("span");
          badge.className = "agent-skill-tag";
          badge.textContent = label;
          badges.appendChild(badge);
        });

      meta.appendChild(title);
      meta.appendChild(desc);
      meta.appendChild(detail);
      meta.appendChild(badges);

      const actions = document.createElement("div");
      actions.className = "agent-skill-actions";

      const checkbox = document.createElement("input");
      checkbox.type = "checkbox";
      checkbox.checked = Boolean(item.enabled);
      checkbox.dataset.itemName = item.name || "";
      checkbox.addEventListener("change", () => {
        void toggleSkill(item.name, checkbox.checked);
      });

      const configureBtn = document.createElement("button");
      configureBtn.type = "button";
      configureBtn.className = "ghost-btn";
      configureBtn.textContent = "Config";
      configureBtn.addEventListener("click", () => {
        void configureSkill(item);
      });

      const installBtn = document.createElement("button");
      installBtn.type = "button";
      installBtn.className = "ghost-btn";
      installBtn.textContent = "Install";
      installBtn.disabled = !Array.isArray(item.install_options) || !item.install_options.length;
      installBtn.addEventListener("click", () => {
        void installSkills([item.name]);
      });

      actions.appendChild(checkbox);
      actions.appendChild(configureBtn);
      actions.appendChild(installBtn);

      li.appendChild(meta);
      li.appendChild(actions);
      container.appendChild(li);
    });
  }

  function getSkills() {
    return skillsCache.slice();
  }

  async function installMissingSkills() {
    const missingSkills = skillsCache
      .filter((item) => !item.eligible && Array.isArray(item.install_options) && item.install_options.length)
      .map((item) => item.name);
    if (!missingSkills.length) {
      setStatus("Nenhuma skill com dependencias instalaveis faltando.");
      return null;
    }
    return installSkills(missingSkills);
  }

  async function configurePendingSkill() {
    const pending = skillsCache.find((item) =>
      Array.isArray(item.missing) &&
      item.missing.some(
        (value) =>
          String(value).startsWith("env:") || String(value).startsWith("config:"),
      ),
    );
    if (!pending) {
      setStatus("Nenhuma skill aguardando env/config.");
      return null;
    }
    return configureSkill(pending);
  }

  return {
    loadSkills,
    getSkills,
    installSkills,
    installMissingSkills,
    configurePendingSkill,
    renderSummary,
  };
}
