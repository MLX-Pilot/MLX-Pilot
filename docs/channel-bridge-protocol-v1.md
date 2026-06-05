# Channel Protocol v1

Este documento formaliza o contrato `v1` usado pelo runtime de canais.

## Negotiation

Requisição:

- header opcional, porém recomendado: `x-channel-protocol-version: v1`

Resposta:

- sucesso: campo `protocol_version: "v1"` no body
- erro de compatibilidade: `error_code: "protocol_version_mismatch"`

## Envelope de erro

Todos os transports retornam erros canônicos no formato:

```json
{
  "error": "human-readable message",
  "error_code": "invalid_request",
  "protocol_version": "v1"
}
```

Valores válidos:

- `invalid_request`
- `auth_error`
- `permission_error`
- `rate_limited`
- `network_error`
- `invalid_target`
- `provider_error`
- `protocol_version_mismatch`

## `bridge_http_v1`

Usado por:

- `signal`
- `imessage`
- `bluebubbles`
- `nostr`
- `nextcloud-talk`
- `line`
- `zalo`
- `zalouser`
- `tlon`

Schema resumido:

```json
{
  "family": "bridge_http_v1",
  "protocol_version": "v1",
  "request_headers": {
    "x-channel-protocol-version": "v1"
  },
  "operations": {
    "login": {
      "request": {
        "type": "object",
        "required": ["channel", "account_id"]
      },
      "response": {
        "type": "object",
        "required": ["protocol_version", "status", "message"]
      }
    },
    "logout": {
      "response": {
        "type": "object",
        "required": ["protocol_version", "status", "message"]
      }
    },
    "probe": {
      "response": {
        "type": "object",
        "required": ["protocol_version", "status", "message"]
      }
    },
    "resolve": {
      "request": {
        "type": "object",
        "required": ["channel", "account_id", "target"]
      },
      "response": {
        "type": "object",
        "required": ["protocol_version", "resolved_target"]
      }
    },
    "send": {
      "request": {
        "type": "object",
        "required": ["channel", "account_id", "target", "message"]
      },
      "response": {
        "type": "object",
        "required": ["protocol_version", "message_id"]
      }
    }
  }
}
```

Exemplo `send`:

```json
{
  "channel": "signal",
  "account_id": "ops",
  "target": "@oncall",
  "message": "bridge smoke ok"
}
```

## `webhook_http_v1`

Usado por:

- `googlechat`
- `feishu`
- `msteams`
- `mattermost`
- `synology-chat`

Credencial validada em runtime:

```json
{
  "webhook_url": "https://provider.example/webhook/..."
}
```

Operações suportadas:

- `login`
- `logout`
- `probe`
- `resolve`
- `send`
- `status`

Semântica:

- `probe`: faz request real ao webhook configurado e valida resposta HTTP
- `resolve`: devolve target informado quando o provedor não expõe lookup
- `send`: texto puro obrigatório

## `irc_tcp_v1`

Usado por:

- `irc`

Credencial validada em runtime:

```json
{
  "server": "irc.example.net",
  "port": 6667,
  "nick": "mlx-pilot",
  "username": "mlx-pilot",
  "password": "optional"
}
```

Operações:

- `login`: abre socket e inicializa sessão IRC da conta
- `logout`: envia `QUIT`
- `probe`: valida handshake/socket
- `resolve`: normaliza `#channel` ou `nick`
- `send`: envia `PRIVMSG`
- `status`: lê health persistido da conta

## `matrix_http_v1`

Usado por:

- `matrix`

Credencial validada em runtime:

```json
{
  "homeserver": "https://matrix.example",
  "token": "syt_xxx"
}
```

Operações:

- `login`
- `logout`
- `probe`
- `resolve`
- `send`
- `status`

## Compat mapping por canal bridge

| Canal | Capabilities | Normalização de target |
| --- | --- | --- |
| signal | `probe`, `resolve`, `send` | `@user` ou target retornado pelo bridge |
| imessage | `probe`, `resolve`, `send` | handle/room ID retornado pelo bridge |
| bluebubbles | `probe`, `resolve`, `send`, `media` | chat GUID canônico |
| nostr | `probe`, `resolve`, `send` | npub/nprofile/event target |
| nextcloud-talk | `probe`, `resolve`, `send` | room token |
| line | `probe`, `resolve`, `send` | user/group ID |
| zalo | `probe`, `resolve`, `send` | user/thread ID |
| zalouser | `probe`, `resolve`, `send` | user/thread ID |
| tlon | `probe`, `resolve`, `send` | bridge-defined ID |

## Rejeição de payload inválido

Exemplos de validação em runtime:

- `bridge_http_v1` sem `base_url` -> `invalid_request`
- `webhook_http_v1` sem `webhook_url` -> `invalid_request`
- `matrix_http_v1` sem `homeserver` ou `token` -> `invalid_request`
- `irc_tcp_v1` sem `server` ou `nick` -> `invalid_request`

Essas validações são executadas em `upsert-account` antes da persistência.
