# 17 — Contatos (Agenda)

> **Tipo:** nova feature · **Esforço:** M · **Depende de:** — (valor real só com
> E-mail 14 / Calendário 15) · **Escopo TCC:** baixa prioridade; adiar até 14/15.

## 1. Objetivo

Uma agenda de contatos local (nome, e-mails, telefones, notas), ciente do agente,
com import/export (vCard/CSV) e, opcionalmente, CardDAV no futuro. Resolve a falta
de um cadastro de pessoas que E-mail e Calendário possam consumir (autocompletar
destinatários, convidados).

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/contacts.rs`; tabela `contacts`
  (SQLite). Import/export vCard via crate `vcard`/parsing manual; CSV via `csv`.
- **Integração:** expor busca de contatos para E-mail (14) e Calendário (15);
  o agente pode consultar via tool.
- **Frontend:** aba/painel `Contatos` (padrão `wave1.js`): lista, busca,
  criar/editar, import/export.

### Referência no Odysseus (exemplo para consulta)

- `routes/contacts_routes.py` — CRUD de contatos, import/export, integração.

## 3. Regras de Negócio e Restrições

- **PODE:** CRUD; busca; import/export vCard/CSV; fornecer autocomplete a outras features.
- **NÃO PODE:** exigir CardDAV/servidor externo — 100% local.
- **NÃO PODE:** duplicar contatos no import — dedup por e-mail/nome.
- **Restrição:** dados locais, privados; nada sai sem ação do usuário.

## 4. Critérios de Aceite

- [ ] Tabela `contacts` + CRUD (`/api/contacts*`).
- [ ] Busca por nome/e-mail; criar/editar/excluir na UI.
- [ ] Import vCard/CSV com dedup; export vCard/CSV.
- [ ] Autocomplete de contato disponível para E-mail/Calendário (quando existirem).

## 5. Plano de Implementação

1. **Tabela `contacts`** + CRUD em `state_store.rs`.
2. **Endpoints** de CRUD e busca.
3. **Import/export** vCard/CSV com dedup.
4. **UI:** painel de contatos (lista/busca/editar/import-export).
5. **Integração:** endpoint de busca reutilizável por E-mail/Calendário.
