use axum::http::HeaderMap;
use serde_json::{json, Value};
use std::collections::BTreeMap;

pub const CHANNEL_PROTOCOL_VERSION: &str = "v1";
pub const CHANNEL_PROTOCOL_HEADER: &str = "x-channel-protocol-version";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelTransportFamily {
    BridgeHttpV1,
    WebhookHttpV1,
    IrcTcpV1,
    MatrixHttpV1,
    TokenBotV1,
    NativeRuntimeV1,
}

impl ChannelTransportFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::BridgeHttpV1 => "bridge_http_v1",
            Self::WebhookHttpV1 => "webhook_http_v1",
            Self::IrcTcpV1 => "irc_tcp_v1",
            Self::MatrixHttpV1 => "matrix_http_v1",
            Self::TokenBotV1 => "token_bot_v1",
            Self::NativeRuntimeV1 => "native_runtime_v1",
        }
    }

    pub fn schema(self) -> Value {
        match self {
            Self::BridgeHttpV1 => bridge_http_v1_schema(),
            Self::WebhookHttpV1 => webhook_http_v1_schema(),
            Self::IrcTcpV1 => irc_tcp_v1_schema(),
            Self::MatrixHttpV1 => matrix_http_v1_schema(),
            Self::TokenBotV1 => token_bot_v1_schema(),
            Self::NativeRuntimeV1 => native_runtime_v1_schema(),
        }
    }
}

pub fn ensure_supported_request_version(headers: &HeaderMap) -> Result<(), String> {
    let Some(value) = headers.get(CHANNEL_PROTOCOL_HEADER) else {
        return Ok(());
    };
    let version = value.to_str().map_err(|_| {
        channel_protocol_error(
            "invalid_request",
            "protocol version header is not valid ASCII",
        )
    })?;
    if version.eq_ignore_ascii_case(CHANNEL_PROTOCOL_VERSION) {
        return Ok(());
    }
    Err(channel_protocol_error(
        "protocol_version_mismatch",
        &format!(
            "unsupported channel protocol version '{version}', expected '{}'",
            CHANNEL_PROTOCOL_VERSION
        ),
    ))
}

pub fn family_for_channel(channel: &str) -> ChannelTransportFamily {
    match channel.trim().to_ascii_lowercase().as_str() {
        "whatsapp" => ChannelTransportFamily::NativeRuntimeV1,
        "telegram" | "discord" | "slack" => ChannelTransportFamily::TokenBotV1,
        "matrix" => ChannelTransportFamily::MatrixHttpV1,
        "irc" => ChannelTransportFamily::IrcTcpV1,
        "googlechat" | "feishu" | "msteams" | "mattermost" | "synology-chat" => {
            ChannelTransportFamily::WebhookHttpV1
        }
        _ => ChannelTransportFamily::BridgeHttpV1,
    }
}

pub fn validate_account_payload(
    channel: &str,
    account_id: &str,
    credentials: Option<&Value>,
    routing_defaults: &BTreeMap<String, String>,
) -> Result<(), String> {
    if channel.trim().is_empty() {
        return Err(channel_protocol_error(
            "invalid_request",
            "channel is required",
        ));
    }
    if account_id.trim().is_empty() {
        return Err(channel_protocol_error(
            "invalid_request",
            "account_id is required",
        ));
    }

    match family_for_channel(channel) {
        ChannelTransportFamily::WebhookHttpV1 => {
            let creds = credentials.ok_or_else(|| {
                channel_protocol_error("invalid_request", "webhook channels require credentials")
            })?;
            let webhook = creds
                .get("webhook_url")
                .and_then(Value::as_str)
                .or_else(|| creds.get("url").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    channel_protocol_error("invalid_request", "missing webhook_url in credentials")
                })?;
            if !webhook.starts_with("http://") && !webhook.starts_with("https://") {
                return Err(channel_protocol_error(
                    "invalid_request",
                    "webhook_url must be an absolute http(s) url",
                ));
            }
        }
        ChannelTransportFamily::BridgeHttpV1 => {
            let creds = credentials.ok_or_else(|| {
                channel_protocol_error("invalid_request", "bridge channels require credentials")
            })?;
            let bridge_url = creds
                .get("base_url")
                .and_then(Value::as_str)
                .or_else(|| creds.get("bridge_url").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    channel_protocol_error(
                        "invalid_request",
                        "missing base_url/bridge_url in credentials",
                    )
                })?;
            if !bridge_url.starts_with("http://") && !bridge_url.starts_with("https://") {
                return Err(channel_protocol_error(
                    "invalid_request",
                    "bridge_url/base_url must be an absolute http(s) url",
                ));
            }
        }
        ChannelTransportFamily::MatrixHttpV1 => {
            let creds = credentials.ok_or_else(|| {
                channel_protocol_error("invalid_request", "matrix channels require credentials")
            })?;
            if creds
                .get("homeserver")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(channel_protocol_error(
                    "invalid_request",
                    "matrix credentials require homeserver",
                ));
            }
            if creds
                .get("token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(channel_protocol_error(
                    "invalid_request",
                    "matrix credentials require token",
                ));
            }
        }
        ChannelTransportFamily::IrcTcpV1 => {
            let creds = credentials.ok_or_else(|| {
                channel_protocol_error("invalid_request", "irc channels require credentials")
            })?;
            for required in ["server", "nick"] {
                if creds
                    .get(required)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .is_none()
                {
                    return Err(channel_protocol_error(
                        "invalid_request",
                        &format!("irc credentials require {required}"),
                    ));
                }
            }
        }
        ChannelTransportFamily::TokenBotV1 => {
            let creds = credentials.ok_or_else(|| {
                channel_protocol_error("invalid_request", "bot channels require credentials")
            })?;
            if creds
                .get("token")
                .and_then(Value::as_str)
                .or_else(|| creds.get("bot_token").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .is_none()
            {
                return Err(channel_protocol_error(
                    "invalid_request",
                    "bot channels require token/bot_token",
                ));
            }
        }
        ChannelTransportFamily::NativeRuntimeV1 => {}
    }

    if let Some(preferred_target) = routing_defaults.get("target") {
        if preferred_target.trim().is_empty() {
            return Err(channel_protocol_error(
                "invalid_request",
                "routing_defaults.target cannot be empty when present",
            ));
        }
    }

    Ok(())
}

pub fn channel_protocol_error(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

pub fn bridge_channel_capabilities(channel: &str) -> Vec<&'static str> {
    match channel {
        "signal" => vec!["resolve", "send", "probe"],
        "imessage" => vec!["resolve", "send", "probe"],
        "bluebubbles" => vec!["resolve", "send", "probe", "media"],
        "nostr" => vec!["resolve", "send", "probe"],
        "nextcloud-talk" => vec!["resolve", "send", "probe"],
        "line" => vec!["resolve", "send", "probe"],
        "zalo" | "zalouser" => vec!["resolve", "send", "probe"],
        "tlon" => vec!["resolve", "send", "probe"],
        _ => vec!["resolve", "send", "probe"],
    }
}

fn bridge_http_v1_schema() -> Value {
    json!({
        "family": "bridge_http_v1",
        "protocol_version": CHANNEL_PROTOCOL_VERSION,
        "request_headers": {
            CHANNEL_PROTOCOL_HEADER: CHANNEL_PROTOCOL_VERSION
        },
        "operations": {
            "login": {
                "request": {
                    "type": "object",
                    "required": ["channel", "account_id"],
                    "properties": {
                        "channel": {"type":"string"},
                        "account_id": {"type":"string"},
                        "metadata": {"type":"object"}
                    }
                },
                "response": {
                    "type":"object",
                    "required":["status","message"],
                    "properties": {
                        "protocol_version":{"const": CHANNEL_PROTOCOL_VERSION},
                        "status":{"type":"string"},
                        "message":{"type":"string"}
                    }
                }
            },
            "logout": {"response_ref": "login.response"},
            "probe": {"response_ref": "login.response"},
            "resolve": {
                "request": {
                    "type":"object",
                    "required":["channel","account_id","target"]
                },
                "response": {
                    "type":"object",
                    "required":["resolved_target"],
                    "properties": {
                        "protocol_version":{"const": CHANNEL_PROTOCOL_VERSION},
                        "resolved_target":{"type":"string"}
                    }
                }
            },
            "send": {
                "request": {
                    "type":"object",
                    "required":["channel","account_id","target","message"]
                },
                "response": {
                    "type":"object",
                    "required":["message_id"],
                    "properties": {
                        "protocol_version":{"const": CHANNEL_PROTOCOL_VERSION},
                        "message_id":{"type":"string"}
                    }
                }
            }
        },
        "error_codes":["invalid_request","auth_error","permission_error","rate_limited","network_error","invalid_target","provider_error","protocol_version_mismatch"]
    })
}

fn webhook_http_v1_schema() -> Value {
    json!({
        "family": "webhook_http_v1",
        "protocol_version": CHANNEL_PROTOCOL_VERSION,
        "credentials": {
            "type":"object",
            "required":["webhook_url"]
        },
        "operations": ["login","logout","probe","resolve","send","status"],
        "error_codes":["invalid_request","auth_error","permission_error","rate_limited","network_error","invalid_target","provider_error","protocol_version_mismatch"]
    })
}

fn irc_tcp_v1_schema() -> Value {
    json!({
        "family": "irc_tcp_v1",
        "protocol_version": CHANNEL_PROTOCOL_VERSION,
        "credentials": {
            "type":"object",
            "required":["server","nick"],
            "properties": {
                "server":{"type":"string"},
                "port":{"type":"integer"},
                "nick":{"type":"string"},
                "username":{"type":"string"},
                "password":{"type":"string"}
            }
        },
        "operations":["login","logout","probe","resolve","send","status"],
        "error_codes":["invalid_request","auth_error","permission_error","rate_limited","network_error","invalid_target","provider_error","protocol_version_mismatch"]
    })
}

fn matrix_http_v1_schema() -> Value {
    json!({
        "family":"matrix_http_v1",
        "protocol_version": CHANNEL_PROTOCOL_VERSION,
        "credentials":{
            "type":"object",
            "required":["homeserver","token"]
        },
        "operations":["login","logout","probe","resolve","send","status"],
        "error_codes":["invalid_request","auth_error","permission_error","rate_limited","network_error","invalid_target","provider_error","protocol_version_mismatch"]
    })
}

fn token_bot_v1_schema() -> Value {
    json!({
        "family":"token_bot_v1",
        "protocol_version": CHANNEL_PROTOCOL_VERSION,
        "credentials":{
            "type":"object",
            "required":["token"]
        },
        "operations":["login","logout","probe","resolve","send","status"],
        "error_codes":["invalid_request","auth_error","permission_error","rate_limited","network_error","invalid_target","provider_error","protocol_version_mismatch"]
    })
}

fn native_runtime_v1_schema() -> Value {
    json!({
        "family":"native_runtime_v1",
        "protocol_version": CHANNEL_PROTOCOL_VERSION,
        "operations":["login","logout","probe","resolve","send","status"],
        "error_codes":["invalid_request","auth_error","permission_error","rate_limited","network_error","invalid_target","provider_error","protocol_version_mismatch"]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn accepts_missing_or_v1_protocol_header() {
        let headers = HeaderMap::new();
        assert!(ensure_supported_request_version(&headers).is_ok());

        let mut headers = HeaderMap::new();
        headers.insert(
            CHANNEL_PROTOCOL_HEADER,
            HeaderValue::from_static(CHANNEL_PROTOCOL_VERSION),
        );
        assert!(ensure_supported_request_version(&headers).is_ok());
    }

    #[test]
    fn rejects_protocol_mismatch() {
        let mut headers = HeaderMap::new();
        headers.insert(CHANNEL_PROTOCOL_HEADER, HeaderValue::from_static("v2"));
        let error = ensure_supported_request_version(&headers).expect_err("must reject");
        assert!(error.contains("protocol_version_mismatch"));
    }

    #[test]
    fn validates_credentials_by_family() {
        let ok = validate_account_payload(
            "matrix",
            "bot",
            Some(&json!({"homeserver":"https://matrix.example","token":"secret"})),
            &BTreeMap::new(),
        );
        assert!(ok.is_ok());

        let err = validate_account_payload(
            "irc",
            "acct",
            Some(&json!({"server":"irc.example"})),
            &BTreeMap::new(),
        )
        .expect_err("missing nick");
        assert!(err.contains("invalid_request"));
    }
}
