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

Login e probe verificam a identidade do bot com a API do provedor. O envio só
retorna sucesso após receber a confirmação do serviço.

| Provedor | Identidade | Envio | Confirmação |
| --- | --- | --- | --- |
| Telegram | `getMe` | `sendMessage` com ID do chat ou @username | `result.message_id` |
| Discord | `GET /users/@me` | `POST /channels/{id}/messages` com ID numérico do canal | `id` |
| Slack | `auth.test` | `chat.postMessage` com ID do canal | `ts` |

O logout encerra a sessão local da conta. O provedor administra a revogação do
token. [Telegram Bot API](https://core.telegram.org/bots/api) ·
[Discord Messages](https://docs.discord.com/developers/resources/message#create-message) ·
[Slack chat.postMessage](https://docs.slack.dev/reference/methods/chat.postMessage/).

### `native_runtime_v1`

Usada por `whatsapp`.

O helper WhatsApp usa Baileys. Sessão QR, diretório de autenticação e reconexão
são isolados por conta. O primeiro uso prepara as dependências do helper e
requer Node.js 22+, npm e acesso ao registro. Para ativação em produção, o
operador lê o QR com o aplicativo do telefone.

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

## Reproduzir a validação de transportes

```sh
cargo build --locked -p mlx-ollama-daemon
node scripts/release-gate/channels-real-validation.mjs
node scripts/release-gate/whatsapp-helper-smoke.mjs
```

O primeiro comando Node testa o catálogo de 20 canais. O daemon usa o cofre
e o estado de uma conta temporária. Os receptores de teste conferem a mensagem
que chegou por HTTP ou TCP em cada um dos 19 adaptadores de transporte.
O caso de WhatsApp exercita o ciclo da sessão local em modo simulado.
O segundo comando testa o helper distribuído separadamente. O uso de contas
externas e o pareamento físico exigem credenciais e destinatários de teste.
