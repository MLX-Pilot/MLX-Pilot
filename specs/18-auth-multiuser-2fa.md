# 18 — Autenticação / Multiusuário / 2FA (Opcional)

> **Tipo:** nova feature (opcional) · **Esforço:** M · **Depende de:** —
> **Escopo TCC:** **contraindicado** para o app desktop single-user padrão; spec
> documenta a decisão e descreve o caminho opcional para deploy em rede.

## 1. Objetivo

Adicionar login, múltiplos usuários com privilégios, tokens de API e webhooks,
incluindo 2FA (TOTP) — **apenas** para cenários de exposição em rede/LAN. Em
desktop local single-user (o caso do MLX-Pilot), isso adiciona fricção sem ganho
de segurança em `127.0.0.1`. O objetivo principal desta spec é **registrar a
decisão arquitetural** de manter o app single-user por padrão e oferecer um
caminho opt-in para quem quiser expor o daemon.

## 2. Contexto Técnico

- **Backend:** Rust; módulo `crates/daemon/src/auth.rs`. Hash de senha `argon2`/
  `bcrypt`; TOTP via `totp-rs`; sessões via cookie assinado/JWT; tokens de API e
  webhooks com segredos no cofre.
- **Middleware:** camada Axum opcional, **desligada por padrão** (flag
  `APP_AUTH_ENABLED=false`), com bypass para loopback em dev.
- **Frontend:** telas de login/2FA e gestão de usuários/tokens — só aparecem com auth ligado.

### Referência no Odysseus (exemplo para consulta)

- `core/auth.py`, `routes/auth_routes.py` — login, sessões, privilégios, signup, 2FA.
- `routes/api_token_routes.py` — tokens de API por integração.
- `routes/webhook_routes.py`, `src/webhook_manager.py` — webhooks.
- README/SECURITY/THREAT_MODEL do Odysseus — `AUTH_ENABLED`, `LOCALHOST_BYPASS`,
  `SECURE_COOKIES` e o racional de "tratar como console de admin".

## 3. Regras de Negócio e Restrições

- **PODE:** habilitar auth via configuração (opt-in) com usuários, privilégios,
  tokens, webhooks e 2FA.
- **NÃO PODE:** ser obrigatório nem ligado por padrão — desktop single-user em
  `127.0.0.1` permanece sem login.
- **NÃO PODE:** guardar segredos/tokens fora do cofre.
- **NÃO PODE:** expor rotas admin (MCP mgmt, vault, wipe, settings) sem gate quando
  auth estiver ligado.
- **Restrição:** se exposto fora de loopback, exigir `AUTH_ENABLED=true` e cookies
  seguros atrás de HTTPS/proxy (documentar como o Odysseus orienta).

## 4. Critérios de Aceite

- [ ] Com `APP_AUTH_ENABLED=false` (padrão), comportamento atual é idêntico (sem login).
- [ ] Com auth ligado: criar usuário admin, login, sessão, logout; 2FA TOTP
      opcional por usuário.
- [ ] Tokens de API por integração (criar/revogar); rotas admin protegidas.
- [ ] Webhooks configuráveis com segredos no cofre.
- [ ] Bypass de loopback só em dev; cookies seguros quando atrás de HTTPS.
- [ ] Decisão de escopo (single-user por padrão) documentada no TCC.

## 5. Plano de Implementação

1. **Decisão documentada:** registrar no TCC que o padrão é single-user/local; auth é opt-in.
2. **Modelo de usuário** + hash de senha; tabela `users` (opcional, só com auth on).
3. **Middleware de auth** Axum (flag), com bypass loopback em dev.
4. **Sessões + 2FA TOTP**; telas de login/2FA no front (condicionais).
5. **Tokens de API + webhooks** com segredos no cofre; gate de rotas admin.
6. **Hardening de deploy:** docs de HTTPS/proxy, `SECURE_COOKIES`, exposição segura.
