use super::{
    channel_error, connected_session_state, logged_out_session_state, require_token_value,
    AdapterContext, AdapterResponse, ChannelAdapter, ProbeResult, ResolveResult, SendResult,
};
use async_trait::async_trait;
use reqwest::{redirect::Policy, Client, Method, Url};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone, Copy)]
pub(super) enum BotProvider {
    Telegram,
    Discord,
    Slack,
}

impl BotProvider {
    pub(super) fn id(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
            Self::Slack => "slack",
        }
    }

    fn origin(self) -> &'static str {
        match self {
            Self::Telegram => "https://api.telegram.org",
            Self::Discord => "https://discord.com",
            Self::Slack => "https://slack.com",
        }
    }
}

pub(super) struct TokenBotAdapter {
    provider: BotProvider,
    fixture_origin: Option<String>,
}

impl TokenBotAdapter {
    pub(super) fn new(provider: BotProvider) -> Self {
        Self {
            provider,
            fixture_origin: std::env::var("MLX_CHANNEL_BOT_FIXTURE_URL").ok(),
        }
    }

    #[cfg(test)]
    pub(super) fn with_fixture(provider: BotProvider, origin: String) -> Self {
        Self {
            provider,
            fixture_origin: Some(origin),
        }
    }

    fn origin(&self) -> Result<Url, String> {
        let Some(origin) = self.fixture_origin.as_deref() else {
            return Ok(Url::parse(self.provider.origin()).expect("official API URL"));
        };
        let invalid = || {
            channel_error(
                "invalid_request",
                "bot fixture requires an HTTP loopback origin",
            )
        };
        let url = Url::parse(origin).map_err(|_| invalid())?;
        let loopback = match url.host() {
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        if !loopback
            || url.scheme() != "http"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        Ok(url)
    }

    fn target<'a>(&self, raw: &'a str) -> Result<&'a str, String> {
        let target = raw.trim();
        let valid = match self.provider {
            BotProvider::Discord => snowflake(target),
            BotProvider::Telegram => {
                target.parse::<i64>().is_ok_and(|id| id != 0)
                    || target.strip_prefix('@').is_some_and(|name| {
                        !name.is_empty()
                            && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
                    })
            }
            BotProvider::Slack => !target.is_empty() && !target.chars().any(char::is_whitespace),
        };
        if !valid || target.chars().any(char::is_control) {
            return Err(channel_error(
                "invalid_target",
                "invalid bot channel or conversation identifier",
            ));
        }
        Ok(target)
    }

    // Identity returns the authenticated provider user ID. Send returns only the
    // upstream message ID. Account fields never determine the request origin.
    async fn request(
        &self,
        ctx: &AdapterContext,
        send: Option<(&str, &str)>,
    ) -> Result<String, String> {
        let token = require_token_value(ctx)?;
        let mut url = self.origin()?;
        let (method, body) = {
            let mut segments = url.path_segments_mut().expect("HTTP origin");
            segments.clear();
            match (self.provider, send) {
                (BotProvider::Telegram, send) => {
                    segments.push(&format!("bot{token}"));
                    let body = if let Some((target, text)) = send {
                        segments.push("sendMessage");
                        let chat_id = target
                            .parse::<i64>()
                            .map(Value::from)
                            .unwrap_or_else(|_| json!(target));
                        json!({"chat_id":chat_id,"text":text})
                    } else {
                        segments.push("getMe");
                        json!({})
                    };
                    (Method::POST, Some(body))
                }
                (BotProvider::Discord, Some((target, content))) => {
                    segments.extend(["api", "v10", "channels", target, "messages"]);
                    (
                        Method::POST,
                        Some(json!({"content":content, "allowed_mentions":{"parse":[]}})),
                    )
                }
                (BotProvider::Discord, None) => {
                    segments.extend(["api", "v10", "users", "@me"]);
                    (Method::GET, None)
                }
                (BotProvider::Slack, Some((channel, text))) => {
                    segments.extend(["api", "chat.postMessage"]);
                    (Method::POST, Some(json!({"channel":channel, "text":text})))
                }
                (BotProvider::Slack, None) => {
                    segments.extend(["api", "auth.test"]);
                    (Method::POST, Some(json!({})))
                }
            }
        };
        let mut builder = Client::builder()
            .redirect(Policy::none())
            .timeout(Duration::from_secs(8))
            .user_agent(concat!(
                "DiscordBot (https://github.com/MLX-Pilot/MLX-Pilot, ",
                env!("CARGO_PKG_VERSION"),
                ")"
            ));
        if self.fixture_origin.is_some() {
            builder = builder.no_proxy();
        }
        let client = builder
            .build()
            .map_err(|_| channel_error("provider_error", "could not initialize bot HTTP client"))?;
        let mut request = client.request(method, url);
        request = match self.provider {
            BotProvider::Discord => {
                request.header(reqwest::header::AUTHORIZATION, format!("Bot {token}"))
            }
            BotProvider::Slack => request.bearer_auth(&token),
            BotProvider::Telegram => request,
        };
        if let Some(body) = body {
            request = request.json(&body);
        }
        // reqwest errors include the URL and Telegram puts its token in the path.
        // A remote response can also contain credentials or the submitted message.
        let mut res = request
            .send()
            .await
            .map_err(|_| channel_error("network_error", "bot API request failed"))?;
        let status = res.status();
        if !status.is_success() {
            return Err(status_error(status.as_u16(), send.is_some()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = res
            .chunk()
            .await
            .map_err(|_| channel_error("network_error", "bot API response interrupted"))?
        {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err(invalid_response());
            }
            bytes.extend_from_slice(&chunk);
        }
        let data: Value = serde_json::from_slice(&bytes).map_err(|_| invalid_response())?;
        self.response_id(&data, send.is_some())
    }

    fn response_id(&self, data: &Value, send: bool) -> Result<String, String> {
        match self.provider {
            BotProvider::Telegram => {
                match data["ok"].as_bool() {
                    Some(true) => (),
                    Some(false) => {
                        return Err(status_error(
                            data["error_code"]
                                .as_u64()
                                .and_then(|n| u16::try_from(n).ok())
                                .unwrap_or(500),
                            send,
                        ))
                    }
                    None => return Err(invalid_response()),
                }
                if !send && data["result"]["is_bot"] != true {
                    return Err(invalid_response());
                }
                data["result"][if send { "message_id" } else { "id" }]
                    .as_u64()
                    .filter(|id| *id > 0)
                    .map(|id| id.to_string())
                    .ok_or_else(invalid_response)
            }
            BotProvider::Discord => {
                if !send && data["bot"] != true {
                    return Err(invalid_response());
                }
                data["id"]
                    .as_str()
                    .filter(|id| snowflake(id))
                    .map(str::to_owned)
                    .ok_or_else(invalid_response)
            }
            BotProvider::Slack => {
                match data["ok"].as_bool() {
                    Some(true) => (),
                    Some(false) => {
                        let code = match data["error"].as_str() {
                            Some(
                                "not_authed" | "invalid_auth" | "token_revoked" | "token_expired"
                                | "account_inactive",
                            ) => "auth_error",
                            Some(
                                "missing_scope" | "not_in_channel" | "restricted_action"
                                | "no_permission" | "is_archived",
                            ) => "permission_error",
                            Some("channel_not_found" | "user_not_found") => "invalid_target",
                            Some("ratelimited" | "rate_limited") => "rate_limited",
                            Some("no_text" | "msg_too_long" | "invalid_arguments") => {
                                "invalid_request"
                            }
                            _ => "provider_error",
                        };
                        return Err(channel_error(code, "Slack rejected the bot operation"));
                    }
                    None => return Err(invalid_response()),
                }
                data[if send { "ts" } else { "user_id" }]
                    .as_str()
                    .filter(|id| {
                        if send {
                            id.split_once('.').is_some_and(|(seconds, fraction)| {
                                digits(seconds) && digits(fraction)
                            })
                        } else {
                            !id.is_empty() && id.bytes().all(|c| c.is_ascii_alphanumeric())
                        }
                    })
                    .map(str::to_owned)
                    .ok_or_else(invalid_response)
            }
        }
    }
}

fn digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|c| c.is_ascii_digit())
}
fn snowflake(value: &str) -> bool {
    digits(value) && value.parse::<u64>().is_ok_and(|id| id > 0)
}
fn invalid_response() -> String {
    channel_error("provider_error", "bot API returned an invalid response")
}
fn status_error(status: u16, send: bool) -> String {
    let code = match status {
        401 => "auth_error",
        403 => "permission_error",
        404 if send => "invalid_target",
        404 => "auth_error",
        400 => "invalid_request",
        408 | 504 => "network_error",
        429 => "rate_limited",
        _ => "provider_error",
    };
    channel_error(
        code,
        &format!("bot API rejected the request (HTTP {status})"),
    )
}

#[async_trait]
impl ChannelAdapter for TokenBotAdapter {
    fn capabilities(&self) -> Vec<String> {
        let mut capabilities = vec!["send-messages", "receive-messages", "bot-token", "probe"];
        if matches!(self.provider, BotProvider::Slack) {
            capabilities.push("workspace-app");
        }
        capabilities.into_iter().map(str::to_owned).collect()
    }

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        let id = self.request(&ctx, None).await?;
        Ok(AdapterResponse {
            status: "connected".to_owned(),
            message: format!("{} account authenticated.", self.provider.id()),
            details: json!({"provider_user_id": id}),
            session_state: Some(connected_session_state(&ctx)),
        })
    }

    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        Ok(AdapterResponse {
            status: "logged_out".to_owned(),
            message: format!("{} account disconnected.", self.provider.id()),
            details: Value::Null,
            session_state: Some(logged_out_session_state(&ctx)),
        })
    }

    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String> {
        self.request(&ctx, None).await?;
        Ok(ProbeResult {
            status: "healthy".to_owned(),
            message: format!("{} API confirmed the bot identity.", self.provider.id()),
        })
    }

    async fn resolve(&self, _ctx: AdapterContext, target: String) -> Result<ResolveResult, String> {
        Ok(ResolveResult {
            resolved_target: self.target(&target)?.to_owned(),
        })
    }

    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        message: String,
    ) -> Result<SendResult, String> {
        let target = self.target(&target)?;
        if message.trim().is_empty() {
            return Err(channel_error("invalid_request", "message cannot be empty"));
        }
        Ok(SendResult {
            message_id: self.request(&ctx, Some((target, &message))).await?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Bytes,
        http::{HeaderMap, StatusCode, Uri},
        Json, Router,
    };
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    const TOKEN: &str = "123:test-token";
    type Observation = (Method, String, Option<String>, Value);

    async fn fixture(
        status: StatusCode,
        payload: Value,
    ) -> (String, UnboundedReceiver<Observation>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = unbounded_channel();
        let app = Router::new().fallback(
            move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
                let tx = tx.clone();
                let payload = payload.clone();
                async move {
                    let auth = headers
                        .get("authorization")
                        .map(|value| value.to_str().unwrap().to_owned());
                    let data = if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&body).unwrap()
                    };
                    tx.send((method, uri.path().to_owned(), auth, data))
                        .unwrap();
                    (status, Json(payload))
                }
            },
        );
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (origin, rx)
    }

    fn context(provider: BotProvider) -> AdapterContext {
        AdapterContext {
            channel: provider.id().to_owned(),
            account_id: "offline-test".into(),
            session_root: Default::default(),
            _metadata: Default::default(),
            _routing_defaults: Default::default(),
            _adapter_config: json!({"base_url": "https://ignored.invalid"}),
            credentials: Some(json!({"bot_token": TOKEN, "base_url": "https://ignored.invalid"})),
        }
    }

    #[tokio::test]
    async fn official_bot_requests_validate_identity_and_return_upstream_ids() {
        for provider in [
            BotProvider::Telegram,
            BotProvider::Discord,
            BotProvider::Slack,
        ] {
            let (
                identity,
                send_result,
                method,
                auth,
                identity_path,
                send_path,
                target,
                expected_body,
                identity_id,
                message_id,
            ) = match provider {
                BotProvider::Telegram => (
                    json!({"ok":true,"result":{"is_bot":true,"id":123}}),
                    json!({"ok":true,"result":{"message_id":654}}),
                    Method::POST,
                    None,
                    "/bot123:test-token/getMe",
                    "/bot123:test-token/sendMessage",
                    "-9876",
                    json!({"chat_id":-9876,"text":"Olá!"}),
                    "123",
                    "654",
                ),
                BotProvider::Discord => (
                    json!({"id":"123","bot":true}),
                    json!({"id":"945678912345678901"}),
                    Method::GET,
                    Some(format!("Bot {TOKEN}")),
                    "/api/v10/users/@me",
                    "/api/v10/channels/789/messages",
                    "789",
                    json!({"content":"Olá!","allowed_mentions":{"parse":[]}}),
                    "123",
                    "945678912345678901",
                ),
                BotProvider::Slack => (
                    json!({"ok":true,"user_id":"U123"}),
                    json!({"ok":true,"ts":"987.654321"}),
                    Method::POST,
                    Some(format!("Bearer {TOKEN}")),
                    "/api/auth.test",
                    "/api/chat.postMessage",
                    "CAbC1",
                    json!({"channel":"CAbC1","text":"Olá!"}),
                    "U123",
                    "987.654321",
                ),
            };
            let ctx = context(provider);
            let (origin, mut seen) = fixture(StatusCode::OK, identity).await;
            let adapter = TokenBotAdapter::with_fixture(provider, origin);
            let login = adapter.login(ctx.clone()).await.unwrap();
            assert_eq!(login.details["provider_user_id"], identity_id);
            assert_eq!(adapter.probe(ctx.clone()).await.unwrap().status, "healthy");
            for _ in 0..2 {
                let (observed_method, path, authorization, _) = seen.recv().await.unwrap();
                assert_eq!(observed_method, method);
                assert_eq!(path, identity_path);
                assert_eq!(authorization, auth);
            }
            let (origin, mut seen) = fixture(StatusCode::OK, send_result).await;
            let adapter = TokenBotAdapter::with_fixture(provider, origin);
            let resolved = adapter
                .resolve(ctx.clone(), format!(" {target} "))
                .await
                .unwrap();
            assert_eq!(resolved.resolved_target, target);
            assert_eq!(
                adapter
                    .send_message(ctx, resolved.resolved_target, "Olá!".into())
                    .await
                    .unwrap()
                    .message_id,
                message_id
            );
            assert_eq!(
                seen.recv().await.unwrap(),
                (Method::POST, send_path.to_owned(), auth, expected_body)
            );
        }
    }

    #[tokio::test]
    async fn upstream_failures_are_canonical_and_redacted() {
        for (provider, status, payload, expected) in [
            (
                BotProvider::Telegram,
                StatusCode::UNAUTHORIZED,
                json!({"description":TOKEN}),
                "auth_error",
            ),
            (
                BotProvider::Discord,
                StatusCode::FORBIDDEN,
                json!({"message":TOKEN}),
                "permission_error",
            ),
            (
                BotProvider::Discord,
                StatusCode::TOO_MANY_REQUESTS,
                json!({"message":TOKEN}),
                "rate_limited",
            ),
            (
                BotProvider::Slack,
                StatusCode::OK,
                json!({"ok":false,"error":"invalid_auth","detail":TOKEN}),
                "auth_error",
            ),
            (
                BotProvider::Slack,
                StatusCode::OK,
                json!({"ok":false,"error":"missing_scope"}),
                "permission_error",
            ),
            (
                BotProvider::Slack,
                StatusCode::OK,
                json!({"ok":false,"error":TOKEN}),
                "provider_error",
            ),
            (
                BotProvider::Telegram,
                StatusCode::OK,
                json!({"ok":true,"result":{}}),
                "provider_error",
            ),
        ] {
            let (origin, _requests) = fixture(status, payload).await;
            let adapter = TokenBotAdapter::with_fixture(provider, origin);
            let err = adapter.probe(context(provider)).await.unwrap_err();
            assert!(err.starts_with(&format!("{expected}:")), "{err}");
            assert!(!err.contains(TOKEN) && !err.contains("http://"));
        }
    }

    #[tokio::test]
    async fn webhook_urls_and_remote_errors_exclude_secret_components() {
        use crate::channels::{http_json_request_json, redact_url};

        let (origin, _requests) = fixture(
            StatusCode::UNAUTHORIZED,
            json!({"message": "private-response-text"}),
        )
        .await;
        let url = format!("{origin}/hooks/private-path?key=private-query#private-fragment");
        assert_eq!(redact_url(&url), origin);
        assert_eq!(
            redact_url("https://demo:private-password@example.org:444/path"),
            "https://example.org:444"
        );
        let error = http_json_request_json(
            Method::POST,
            &url,
            None,
            json!({"text": "fixture-check"}),
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(error, "auth_error: channel endpoint returned HTTP 401");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unused = listener.local_addr().unwrap();
        drop(listener);
        let failed_connection = http_json_request_json(
            Method::POST,
            &format!("http://{unused}/private-path"),
            None,
            Value::Null,
            None,
        )
        .await
        .unwrap_err();
        assert_eq!(
            failed_connection,
            "network_error: could not connect to channel endpoint"
        );
    }

    #[test]
    fn fixture_origin_is_process_only_and_local() {
        for invalid in [
            "https://example.com",
            "http://localhost:1234",
            "http://user:pass@127.0.0.1",
            "http://127.0.0.1/somewhere",
            "http://127.0.0.1/?token=private",
            "file:///tmp/invalid",
        ] {
            assert!(
                TokenBotAdapter::with_fixture(BotProvider::Telegram, invalid.into())
                    .origin()
                    .is_err()
            );
        }
        let bot = TokenBotAdapter {
            provider: BotProvider::Discord,
            fixture_origin: None,
        };
        assert_eq!(bot.origin().unwrap().as_str(), "https://discord.com/");
        assert!(bot.target("/users/123").is_err());
    }
}
