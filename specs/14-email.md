# 14 — E-mail (IMAP/SMTP + Triagem por IA)

> **Tipo:** nova feature (pesada) · **Esforço:** XL · **Depende de:** Infra Jobs (01,
> poller), cofre de segredos · **Escopo TCC:** opcional — avaliar se vale.

## 1. Objetivo

Caixa de entrada IMAP/SMTP nativa com **triagem por IA**: resumo automático,
auto-tag, marcação de urgência, rascunhos de resposta e detecção de spam.
Demonstra o agente atuando sobre dados reais do usuário. **Nota de escopo:** é a
feature mais custosa em manutenção (quirks de IMAP/MIME); considerar adiar/descopar
no TCC, justificando.

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/email.rs`. IMAP via crate `imap` +
  `native-tls`/`rustls`; SMTP via `lettre`; parsing MIME via `mail-parser`.
- **Contas:** múltiplas; credenciais **somente** no cofre (`secrets_vault.rs`).
- **Poller:** job recorrente (infra 01) busca novos e-mails, aplica triagem IA
  (`chat_with_routing`) e cacheia em SQLite.
- **Triagem:** resumo, tags, score de urgência, rascunho de resposta, spam — tudo
  como sugestões editáveis (nunca enviar/apagar sem ação do usuário).
- **Frontend:** aba `E-mail` (padrão `wave1.js`): lista, leitura, compor/responder,
  filtros por conta/tag; UI responsiva.

### Referência no Odysseus (exemplo para consulta)

- `routes/email_routes.py`, `routes/email_helpers.py`, `routes/email_pollers.py` —
  fluxo IMAP/SMTP, helpers e polling.
- `src/email_thread_parser.py` — parsing de threads/MIME.
- `mcp_servers/email_server.py` — servidor MCP de e-mail (triagem como tool).
- `static/js/emailInbox.js`, `static/js/emailLibrary.js` — UI da caixa de entrada.
- `docs/email-outlook.md` — limitação OAuth (Outlook/365) documentada.

## 3. Regras de Negócio e Restrições

- **PODE:** ler, listar, buscar, compor, responder, mover/arquivar; gerar resumos/
  tags/urgência/rascunhos; marcar spam (como sugestão).
- **NÃO PODE:** **enviar** e-mail ou **apagar** mensagens sem confirmação explícita
  do usuário (a IA só propõe).
- **NÃO PODE:** guardar senhas/tokens fora do cofre.
- **NÃO PODE:** bloquear a UI durante fetch — tudo via poller/jobs + cache.
- **Limitação conhecida:** Outlook/365 normalmente exigem OAuth (senha simples
  falha) — documentar como o Odysseus faz.
- **Privacidade:** conteúdo de e-mail só vai ao LLM local por padrão; provider
  remoto exige opt-in.

## 4. Critérios de Aceite

- [ ] Adicionar conta IMAP/SMTP (credenciais no cofre); testar conexão.
- [ ] Listar/abrir/buscar e-mails; cache em SQLite; UI responsiva.
- [ ] Poller atualiza a caixa em background e gera resumo/tags/urgência.
- [ ] Compor e responder funcionam; envio só após confirmação.
- [ ] Rascunho de resposta gerado por IA é editável antes de enviar.
- [ ] Falha de conexão/credencial é reportada sem derrubar o daemon.

## 5. Plano de Implementação

1. **Contas + cofre:** modelo de conta; armazenamento de credenciais cifradas; teste de conexão.
2. **IMAP read:** conectar, listar pastas, fetch headers/corpo (paginado), cache SQLite.
3. **SMTP send:** compor/responder com `lettre`; confirmação obrigatória.
4. **Poller (infra 01):** buscar novos, parsear MIME, gerar triagem IA, persistir.
5. **Triagem IA:** resumo/tags/urgência/rascunho/spam via `chat_with_routing` (sugestões).
6. **UI:** aba `E-mail` (lista/leitura/compor/filtros) responsiva.
7. **Docs:** limitação OAuth (Outlook/365) e caminho futuro.
