# 07 — Gerenciamento de Servidores MCP

> **Tipo:** nova feature · **Esforço:** M · **Depende de:** cofre de segredos
> (existente) · **Habilita:** ferramentas externas no Agent.

## 1. Objetivo

Permitir registrar, conectar e gerenciar **servidores MCP (Model Context
Protocol)** — locais (stdio) e remotos (SSE/HTTP) — injetando dinamicamente as
ferramentas deles no `ToolRegistry` do agente. Resolve a limitação de o agente só
ter ferramentas internas, abrindo o ecossistema MCP (browser, filesystem, APIs).

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/mcp_servers.rs` (+ possivelmente em
  `crates/agent-core`); tabela `mcp_servers` (SQLite).
- **Cliente MCP:** crate `mcp`/`rmcp` (verificar o que já consta no `Cargo.lock`);
  transportes **stdio** (processo filho), **SSE** e **HTTP streamable**.
- **Integração com o agente:** as tools descobertas viram entradas dinâmicas no
  `ToolRegistry` (`crates/agent-core/src/registry.rs`), respeitando o
  `PolicyEngine`/`ApprovalService` já existentes.
- **OAuth:** fluxo de autorização para servidores que exigem; tokens guardados no
  cofre (`secrets_vault.rs`).
- **Frontend:** painel "Servidores MCP" na área do Agent (lista + status + add +
  toggles por tool + colar token OAuth).

### Referência no Odysseus (exemplo para consulta)

- `routes/mcp_routes.py`, `src/mcp_manager.py` — registro/ciclo de vida e descoberta de tools.
- `src/mcp_oauth.py` — fluxo OAuth de servidores MCP.
- `src/builtin_mcp.py`, `mcp_servers/` — servidores MCP embutidos (browser, memória,
  imagem) como exemplos de registro automático no startup.

## 3. Regras de Negócio e Restrições

- **PODE:** adicionar/remover/editar servidores; conectar/desconectar; listar
  tools expostas; habilitar/desabilitar tool por tool.
- **PODE:** auto-registrar servidores embutidos no startup **somente se** o pacote
  já estiver em cache local (não bloquear boot baixando nada).
- **NÃO PODE:** auto-executar tools MCP sem passar pelas políticas/aprovação do agente.
- **NÃO PODE:** guardar tokens/segredos em texto puro — sempre no cofre.
- **NÃO PODE:** travar o daemon se um servidor MCP estiver offline — conexão
  resiliente, com status `degraded`/`error` visível.
- **Segurança:** tratar descrições/resultados de tools MCP como dados não confiáveis
  (risco de prompt-injection) — manter as salvaguardas do agente.

## 4. Critérios de Aceite

- [ ] Tabela `mcp_servers` + CRUD; `GET/POST/DELETE /agent/mcp/servers*`.
- [ ] Conectar a um servidor stdio de exemplo lista suas tools; elas ficam
      disponíveis ao agente (com policy aplicada).
- [ ] Habilitar/desabilitar uma tool específica reflete no `ToolRegistry`.
- [ ] Fluxo OAuth: iniciar, colar/capturar token, persistir no cofre, reconectar.
- [ ] Servidor offline aparece como `error`/`degraded` sem derrubar o daemon.
- [ ] `cargo build` verde.

## 5. Plano de Implementação

1. **Tabela `mcp_servers`** (id, nome, transporte, comando/url, env, status, enabled)
   + CRUD.
2. **Camada de cliente MCP:** wrapper sobre o crate MCP para stdio/SSE/HTTP;
   `connect()`/`list_tools()`/`call_tool()`.
3. **Injeção dinâmica** das tools no `ToolRegistry`, com namespacing
   (`mcp:<server>:<tool>`) e respeito à policy/approval.
4. **OAuth:** endpoints de início/callback; persistência de token no cofre.
5. **Resiliência:** reconexão, status por servidor, timeouts.
6. **UI:** painel de servidores (status/add/edit/remove), toggles de tools, fluxo OAuth.
