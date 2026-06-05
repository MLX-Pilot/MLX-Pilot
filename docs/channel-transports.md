# Channel Transports

O runtime de canais do MLX-Pilot opera sempre em modo multi-conta:

- `channels.<channel>.accounts.<account_id>`
- `channels.<channel>.default_account_id`
- segredos no vault local via `credentials_ref`
- sessão isolada por conta em `channel-sessions/<channel>/<account_id>/`

Todas as respostas de canais expõem `protocol_family` e `protocol_version`. A negociação de versão aceita `x-channel-protocol-version: v1` e rejeita versões incompatíveis com `error_code=protocol_version_mismatch`.

## Matriz

| Canal | Família | Versão |
| --- | --- | --- |
| whatsapp | `native_runtime_v1` | `v1` |
| telegram | `token_bot_v1` | `v1` |
| discord | `token_bot_v1` | `v1` |
| slack | `token_bot_v1` | `v1` |
| irc | `irc_tcp_v1` | `v1` |
| matrix | `matrix_http_v1` | `v1` |
| googlechat | `webhook_http_v1` | `v1` |
| feishu | `webhook_http_v1` | `v1` |
| msteams | `webhook_http_v1` | `v1` |
| mattermost | `webhook_http_v1` | `v1` |
| synology-chat | `webhook_http_v1` | `v1` |
| signal | `bridge_http_v1` | `v1` |
| imessage | `bridge_http_v1` | `v1` |
| bluebubbles | `bridge_http_v1` | `v1` |
| nostr | `bridge_http_v1` | `v1` |
| nextcloud-talk | `bridge_http_v1` | `v1` |
| line | `bridge_http_v1` | `v1` |
| zalo | `bridge_http_v1` | `v1` |
| zalouser | `bridge_http_v1` | `v1` |
| tlon | `bridge_http_v1` | `v1` |

## Famílias

### `bridge_http_v1`

Usada por `signal`, `imessage`, `bluebubbles`, `nostr`, `nextcloud-talk`, `line`, `zalo`, `zalouser`, `tlon`.

Credencial mínima:

```json
{
  "base_url": "https://bridge.example",
  "token": "optional-bearer-token"
}
```

Capacidades por canal:

- `signal`, `imessage`, `nostr`, `nextcloud-talk`, `line`, `zalo`, `zalouser`, `tlon`: `probe`, `resolve`, `send`
- `bluebubbles`: `probe`, `resolve`, `send`, `media`

Normalização de target:

- chats diretos: `@handle` ou equivalente do bridge
- canais/salas: `#room` ou ID canônico retornado por `resolve`
- o adapter persiste sempre o target resolvido por conta; não compartilha resolução entre contas

### `webhook_http_v1`

Usada por `googlechat`, `feishu`, `msteams`, `mattermost`, `synology-chat`.

Credencial mínima:

```json
{
  "webhook_url": "https://provider.example/webhook/..."
}
```

`probe` valida webhook e permissões mínimas; `send` suporta texto; `resolve` devolve o target informado quando o provedor não oferece resolução remota.

### `irc_tcp_v1`

Usada por `irc`.

Credencial mínima:

```json
{
  "server": "irc.example.net",
  "port": 6667,
  "nick": "mlx-pilot",
  "username": "mlx-pilot",
  "password": "optional"
}
```

O adapter abre socket por conta, executa `NICK/USER`, usa `JOIN` quando o target é canal e publica via `PRIVMSG`.

### `matrix_http_v1`

Usada por `matrix`.

Credencial mínima:

```json
{
  "homeserver": "https://matrix.example",
  "token": "syt_xxx"
}
```

`resolve` usa lookup de room/alias quando aplicável; `send` usa Matrix Client-Server API `/_matrix/client/v3/...`.

### `token_bot_v1`

Usada por `telegram`, `discord`, `slack`.

Credencial mínima:

```json
{
  "token": "provider-token"
}
```

### `native_runtime_v1`

Usada por `whatsapp`.

Sessão QR, auth-dir e reconnect são isolados por conta.

## Erros canônicos

Todos os adapters convertem falhas para:

- `invalid_request`
- `auth_error`
- `permission_error`
- `rate_limited`
- `network_error`
- `invalid_target`
- `provider_error`
- `protocol_version_mismatch`

Não existe retorno sem `error_code`. Logs e auditoria incluem sempre `channel`, `account_id`, `action`, `result`, `error_code`.

## Exemplos

Probe com negociação explícita:

```bash
curl -X POST http://127.0.0.1:11435/agent/channels/probe \
  -H 'content-type: application/json' \
  -H 'x-channel-protocol-version: v1' \
  -d '{"channel":"matrix","account_id":"ops"}'
```

Send com conta explícita:

```bash
curl -X POST http://127.0.0.1:11435/agent/message/send \
  -H 'content-type: application/json' \
  -H 'x-channel-protocol-version: v1' \
  -d '{"channel":"slack","account_id":"workspace-a","target":"#alerts","message":"smoke ok"}'
```

Versão incompatível:

```json
{
  "error": "unsupported channel protocol version 'v2', expected 'v1'",
  "error_code": "protocol_version_mismatch",
  "protocol_version": "v1"
}
```

O contrato completo por família está em [channel-bridge-protocol-v1.md](/Users/kaike/mlx-ollama-pilot/docs/channel-bridge-protocol-v1.md).
