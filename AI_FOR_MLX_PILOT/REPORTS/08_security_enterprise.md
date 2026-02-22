# 08 Security Enterprise Report

## Objetivo
Implementar modo **Enterprise/Paranoid Security** no Agent Rust com foco em:
- capabilities declarativas por skill
- integridade de skills com hash/pin
- cofre local de secrets criptografado
- controle rigoroso de egress
- modo airgapped
- modo owner-only

## Entrega Implementada

### 1) Capabilities declarativas por skill
Foi adotado modelo declarativo com capacidades efetivas:
- `fs_read`
- `fs_write`
- `network`
- `exec`
- `secrets_access`

Compatibilidade com metadados legados foi preservada.

Arquivos-chave:
- `/Users/kaike/mlx-ollama-pilot/crates/agent-skills/src/types.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/agent-skills/src/frontmatter.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/agent-core/src/policy.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/agent-core/src/agent_loop.rs`

### 2) Integridade de skills (SHA256 + pin + aviso de mudança)
Implementado:
- cálculo SHA256 no carregamento de `SKILL.md`
- pin opcional por skill (`security.skill_sha256_pins`)
- snapshot local de integridade para detectar mudança entre loads
- aviso (`changed`) quando hash muda
- bloqueio (`blocked`) em mismatch com pin

Arquivos-chave:
- `/Users/kaike/mlx-ollama-pilot/crates/agent-skills/src/loader.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/agent-core/src/policy.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/daemon/src/agent_api.rs`

### 3) Secrets vault local criptografado
Criado `SecretsVault` com criptografia local (CHACHA20-POLY1305 via `ring`):
- chave local persistida (`agent_secrets.key`)
- secrets cifrados em arquivo JSON (`agent_secrets.v1.json`)
- integração com `/agent/config` para persistir API key no vault
- `settings.json` salva referência (`api_key_ref`) em vez da chave em claro

Arquivos-chave:
- `/Users/kaike/mlx-ollama-pilot/crates/daemon/src/secrets_vault.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/daemon/src/agent_api.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/daemon/src/config.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/daemon/Cargo.toml`

### 4) Network egress control
Implementado no `PolicyEngine`:
- allowlist de domínios (`network_allow_domains`)
- bloqueio de egress por IP direto (`block_direct_ip_egress`)
- validação de host com suporte a regras exatas/subdomínio/wildcard simples

Arquivo-chave:
- `/Users/kaike/mlx-ollama-pilot/crates/agent-core/src/policy.rs`

### 5) Modo airgapped
Implementado:
- bloqueio de ferramentas de rede no policy
- bloqueio de qualquer provider remoto no `/agent/run`
- somente providers locais (ou custom localhost) são permitidos

Arquivo-chave:
- `/Users/kaike/mlx-ollama-pilot/crates/daemon/src/agent_api.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/agent-core/src/policy.rs`

### 6) Modo owner-only
Implementado:
- validação de workspace no `/agent/run` para restringir execução ao diretório do projeto
- reforço no policy para bloquear paths fora do workspace configurado

Arquivo-chave:
- `/Users/kaike/mlx-ollama-pilot/crates/daemon/src/agent_api.rs`
- `/Users/kaike/mlx-ollama-pilot/crates/agent-core/src/policy.rs`

## Ajustes de API/Config
Novos campos de segurança em `AgentSecurityConfig`:
- `security_mode` (`standard|enterprise|paranoid`)
- `require_capabilities`
- `airgapped`
- `owner_only`
- `block_direct_ip_egress`
- `skill_sha256_pins`
- `use_secrets_vault`

No `AgentUiConfig`:
- `api_key_ref` (ponteiro para segredo no vault)

## Testes Obrigatórios
Novos testes foram adicionados e/ou expandidos para:
- enforcement de capabilities
- egress/IP/airgap/owner-only
- pin de hash + detecção de mudança
- vault (roundtrip e não persistência em claro)
- hash SHA256 no carregamento de skills

Execuções de validação:
- `cargo fmt` ✅
- `cargo test` ✅
- `cargo build` ✅

## Resultado
Modo Enterprise/Paranoid foi integrado sem alterar OpenClaw/NanoBot e com validação automatizada passando em todo o workspace.
