# 11 — Backup/Restore e UI do Cofre de Segredos

> **Tipo:** completar (cofre cripto já existe) · **Esforço:** M · **Depende de:** —

## 1. Objetivo

Permitir **exportar/importar** todos os dados do usuário (sessões, memória,
presets, comparações, documentos, notas/tarefas) num arquivo versionado, fazer
**wipe seletivo** por categoria, e expor uma **UI para o cofre de segredos**
(status das chaves, sem revelar valores). Resolve a falta de portabilidade/segurança
operacional dos dados locais.

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/backup.rs`. Export = serializar as
  tabelas do `state.sqlite` num JSON versionado (`{version, exported_at, tables:{...}}`);
  import = merge com dedup por id. Wipe = `DELETE` por categoria.
- **Cofre:** reutilizar `crates/daemon/src/secrets_vault.rs` (criptografia já
  existente). UI mostra **quais** segredos existem e seu status, nunca o valor.
- **Frontend:** cards em Settings — "Backup & Restauração", "Cofre de Segredos",
  e "Zona de Perigo" (wipe com confirmação dupla).

### Referência no Odysseus (exemplo para consulta)

- `routes/backup_routes.py` — export/import de dados.
- `routes/vault_routes.py`, `src/secret_storage.py` — cofre de segredos.
- `routes/admin_wipe_routes.py` — wipe seletivo por categoria.

## 3. Regras de Negócio e Restrições

- **PODE:** exportar tudo num arquivo; importar mesclando (sem apagar o que não
  está no backup, salvo modo "substituir" explícito); wipe por categoria.
- **NÃO PODE:** incluir segredos do cofre em texto puro no export (export omite
  valores de segredos ou os mantém cifrados; documentar a escolha).
- **NÃO PODE:** executar wipe sem confirmação explícita (dupla) do usuário.
- **NÃO PODE:** revelar valores de segredos na UI — só nome/existência/status.
- **Restrição:** import deve validar a versão do schema e migrar/recusar
  graciosamente formatos incompatíveis.

## 4. Critérios de Aceite

- [ ] `POST /api/backup/export` baixa um JSON com todas as tabelas relevantes.
- [ ] `POST /api/backup/import` restaura/mescla a partir do arquivo, com dedup.
- [ ] `DELETE /api/admin/wipe/{kind}` apaga só a categoria indicada, após confirmação.
- [ ] UI do cofre lista segredos existentes (nome/status), sem valores.
- [ ] Import de versão incompatível é recusado com mensagem clara.
- [ ] Round-trip export→wipe→import recupera os dados.

## 5. Plano de Implementação

1. **Export:** ler tabelas do `state.sqlite` → JSON versionado; endpoint de download.
2. **Import:** parsear, validar versão, mesclar com dedup por id (modo merge/replace).
3. **Wipe seletivo:** endpoint por categoria com confirmação no front.
4. **UI do cofre:** endpoint que lista nomes/status de segredos (sem valores).
5. **UI em Settings:** cards de Backup/Restore, Cofre e Zona de Perigo.
6. **Testes:** round-trip e recusa de versão incompatível.
