# 19 — App Híbrido: Modelos Cloud + Locais no Seletor Unificado

> **Tipo:** nova feature (UX + integração; reaproveita providers remotos existentes)
> **Esforço:** M · **Depende de:** cofre de segredos (existente),
> `http_llm_provider` (existente), perfis de provider (`/agent/provider-profiles`).
> **Foco principal:** Agent (e também Chat).

## 1. Objetivo

Transformar o MLX-Pilot num **app híbrido**: além dos modelos **locais**
instalados (MLX/llama.cpp/Ollama), o usuário pode usar **modelos em nuvem**
(DeepSeek, OpenAI, Anthropic, Groq, OpenRouter, custom) de forma transparente.

Fluxo-alvo: o usuário salva a **API key do DeepSeek** no cofre (Settings →
Secrets). A partir daí, ao passar o mouse/clicar no **seletor de modelos**
(`#model-picker`), aparece uma seção agrupada por provider — `Local` com os
modelos instalados e `DeepSeek` com os modelos cloud (ex.: *DeepSeek V4 Flash*,
*DeepSeek V4 Pro*), exatamente como na referência visual. Selecionar um modelo
cloud roteia a inferência para o provider remoto usando a chave do cofre. Vale
para o **Chat** e, principalmente, para o **Agent**.

Resolve o fato de hoje os providers remotos existirem no backend, mas **não**
estarem expostos de forma fluida e agrupada no seletor de modelos — o usuário não
consegue "ligar a nuvem" e escolher um modelo cloud em dois cliques.

## 2. Contexto Técnico

- **Providers remotos (já existem):** crate `http_llm_provider`
  (`HttpLlmProvider`, `HttpApiKind`, `HttpLlmProviderConfig`) — OpenAI-compatible
  e Anthropic. DeepSeek é OpenAI-compatible (`base_url` `https://api.deepseek.com`).
- **Perfis de provider (já existem):** em `crates/daemon/src/agent_api.rs` há
  `provider_profiles` com `requires_api_key` e `default_base_url`; endpoints
  `/agent/providers` e `/agent/provider-profiles`. Esta feature **estende** isso
  com um **catálogo de modelos por provider** e a noção de "provider ativo" (tem chave).
- **Segredos:** chaves no cofre (`crates/daemon/src/secrets_vault.rs`), referenciadas
  por `vault://<provider>.api_key` (padrão já usado por `resolve_agent_api_key`).
- **Roteamento:** local continua via `chat_with_routing`/`route_model_request`;
  cloud via `HttpLlmProvider` com `RuntimeProviderConfig { base_url, api_key, headers }`.
  Introduzir um **id de modelo qualificado por provider** (ex.: `deepseek:deepseek-v4-pro`)
  para o roteador saber destino sem ambiguidade.
- **Catálogo cloud:** lista curada por provider (estática, versionada) + descoberta
  dinâmica opcional via `GET {base_url}/models` (OpenAI-compatible) quando o provider
  expõe. Cache com TTL.
- **Frontend:** estender `renderModelPicker()` e o menu `#model-menu` no `app.js`
  para renderizar **grupos** (Local + cada provider cloud ativo), com badge
  `local`/`cloud`. O seletor do Agent reusa a mesma fonte de dados.
- **Airgap:** respeitar `provider_allowed_in_airgap`/`is_local_base_url` — em modo
  airgapped, ocultar/bloquear modelos cloud.

### Referência no Odysseus (exemplo para consulta)

- `routes/model_routes.py`, `src/model_discovery.py`, `src/endpoint_resolver.py` —
  descoberta de modelos e resolução de endpoint por provider (local + API).
- `static/js/providers.js`, `static/js/modelPicker.js`, `static/js/models.js` —
  configuração de providers e o **seletor agrupado** de modelos (modelo direto da UX
  pedida).
- `routes/copilot_routes.py`, `routes/chatgpt_subscription_routes.py`,
  `routes/device_flow.py` — exemplos de providers cloud com auth (referência avançada).
- README do Odysseus, seção *Chat*: "chat with any local model or API; adding them
  is super simple" — a intenção de produto que esta spec materializa.

## 3. Regras de Negócio e Restrições

- **PODE:** mostrar um grupo de provider cloud no seletor **somente** quando há chave
  válida salva no cofre para ele.
- **PODE:** misturar local e cloud na mesma lista, agrupados, com badge de origem.
- **PODE:** usar modelo cloud no Chat e no Agent; o Agent mantém suas políticas,
  aprovação, auditoria e budget normalmente, independentemente da origem do modelo.
- **PODE:** descobrir modelos dinamicamente via API do provider quando disponível;
  senão, usar o catálogo curado.
- **NÃO PODE:** guardar a API key fora do cofre nem exibi-la na UI (só status
  "configurado/✔").
- **NÃO PODE:** enviar dados a provider cloud sem que o usuário tenha **escolhido**
  um modelo cloud (opt-in explícito por seleção); o padrão permanece local.
- **NÃO PODE:** expor modelos cloud quando em **modo airgapped/owner-only** —
  respeitar os gates existentes.
- **NÃO PODE:** quebrar o roteamento local atual — ids locais continuam funcionando;
  ids cloud usam o prefixo de provider.
- **NÃO PODE:** travar a UI se a descoberta dinâmica falhar — cair para o catálogo
  curado e marcar o provider como `degraded`.
- **Privacidade/custo:** exibir aviso claro de que modelos cloud enviam dados ao
  provider e podem ter custo; deixar o badge `cloud` sempre visível.

## 4. Critérios de Aceite

- [ ] Salvar a API key do DeepSeek no cofre faz surgir, no `#model-menu`, um grupo
      **DeepSeek** com seus modelos (curados e/ou descobertos), além do grupo **Local**.
- [ ] Remover a chave faz o grupo desaparecer; sem chave, nenhum modelo cloud aparece.
- [ ] Selecionar um modelo cloud e enviar no **Chat** roteia via `HttpLlmProvider`
      com a chave do cofre e retorna resposta.
- [ ] Selecionar um modelo cloud para o **Agent** executa um run usando o provider
      remoto, mantendo policy/approval/audit/budget.
- [ ] Cada item do seletor mostra badge `local` ou `cloud`; a key nunca é exibida.
- [ ] Em modo airgapped, modelos cloud não aparecem/são bloqueados.
- [ ] Descoberta dinâmica indisponível → usa catálogo curado, provider marcado
      `degraded`, sem travar a UI.
- [ ] `cargo build` verde; roteamento de modelos locais inalterado.

## 5. Plano de Implementação

1. **Modelo de provider ativo:** função no backend que, para cada provider de perfil
   (`provider_profiles`), verifica se há chave no cofre (`vault://<provider>.api_key`)
   e o marca como "ativo".
2. **Catálogo de modelos cloud:** estrutura curada por provider (id, label, family,
   context, flags) versionada no código; util de id qualificado `provider:model`.
3. **Descoberta dinâmica (opcional):** para OpenAI-compatible, `GET {base_url}/models`
   com a chave; cache + TTL; fallback para o catálogo curado.
4. **Endpoint unificado:** `GET /models/all` (ou estender `/models`) retornando
   grupos `[{provider, kind: local|cloud, status, models:[...]}]`.
5. **Roteamento cloud:** estender `route_model_request`/`chat_with_routing` para
   reconhecer ids `provider:model`, montar `RuntimeProviderConfig` (base_url +
   chave do cofre + headers) e chamar `HttpLlmProvider`; respeitar gates de airgap.
6. **Agent:** garantir que `/agent/config`/run aceitem o id qualificado e que o run
   use o provider remoto preservando policy/approval/audit/budget.
7. **UI do seletor:** estender `renderModelPicker()` para render agrupado (Local +
   providers cloud ativos), com badges e aviso de privacidade/custo; reusar no Agent.
8. **Settings:** ao salvar/remover chave de um provider, refrescar o seletor
   (re-fetch de `/models/all`).
9. **Salvaguardas:** ocultar cloud em airgapped; nunca exibir a chave; status
   `degraded` quando a descoberta falha.
