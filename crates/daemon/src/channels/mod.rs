pub(crate) mod protocol;

use crate::config::{
    AppConfig, ChannelAccountHealthState, ChannelAccountPersistedState, ChannelAccountPolicy,
    ChannelPersistedState,
};
use crate::secrets_vault::SecretsVault;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use mlx_agent_core::{ChannelDescriptor, ChannelRegistry, HelpMetadata};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use uuid::Uuid;

use self::protocol::{
    bridge_channel_capabilities, family_for_channel, validate_account_payload,
    CHANNEL_PROTOCOL_VERSION,
};

const CHANNEL_SESSIONS_DIR: &str = "channel-sessions";
const CHANNEL_AUDIT_DIR: &str = "channel-audit";

#[derive(Debug, Deserialize)]
pub struct ChannelUpsertAccountRequest {
    pub channel: String,
    pub account_id: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub credentials: Option<Value>,
    #[serde(default)]
    pub credentials_ref: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub routing_defaults: BTreeMap<String, String>,
    #[serde(default)]
    pub limits: Option<ChannelAccountPolicy>,
    #[serde(default)]
    pub adapter_config: Value,
    #[serde(default)]
    pub set_as_default: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChannelRemoveAccountRequest {
    pub channel: String,
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ChannelAuthRequest {
    pub channel: String,
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
pub struct ChannelProbeRequest {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub all_accounts: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChannelResolveRequest {
    pub channel: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub preferred_account_id: Option<String>,
    pub target: String,
}

#[derive(Debug, Deserialize)]
pub struct MessageSendRequest {
    pub channel: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub preferred_account_id: Option<String>,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct ChannelLogsQuery {
    #[serde(default)]
    pub channel: Option<String>,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub struct LegacyChannelUpsertRequest {
    pub channel_id: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub alias: Option<String>,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
pub struct LegacyChannelRemoveRequest {
    pub channel_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelAccountView {
    pub account_id: String,
    pub enabled: bool,
    pub configured: bool,
    pub is_default: bool,
    pub credentials_ref: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub routing_defaults: BTreeMap<String, String>,
    pub health_state: ChannelAccountHealthState,
    pub limits: ChannelAccountPolicy,
    pub adapter_config: Value,
    pub session: ChannelSessionState,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelView {
    pub id: String,
    pub name: String,
    pub protocol_family: String,
    pub protocol_version: String,
    pub protocol_schema: Value,
    pub aliases: Vec<String>,
    pub capabilities: Vec<String>,
    pub supports_lazy_load: bool,
    pub docs: HelpMetadata,
    pub config_schema: Value,
    pub default_account_id: Option<String>,
    pub ambiguity_warning: Option<String>,
    pub accounts: Vec<ChannelAccountView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelCapabilityView {
    pub id: String,
    pub name: String,
    pub protocol_family: String,
    pub protocol_version: String,
    pub protocol_schema: Value,
    pub aliases: Vec<String>,
    pub capabilities: Vec<String>,
    pub accounts: Vec<ChannelCapabilityAccountView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelCapabilityAccountView {
    pub account_id: String,
    pub enabled: bool,
    pub health_status: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelActionResponse {
    pub channel: String,
    pub account_id: String,
    pub protocol_family: String,
    pub protocol_version: String,
    pub status: String,
    pub message: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChannelResolveResponse {
    pub channel: String,
    pub account_id: String,
    pub protocol_family: String,
    pub protocol_version: String,
    pub requested_target: String,
    pub resolved_target: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MessageSendResponse {
    pub channel: String,
    pub account_id: String,
    pub protocol_family: String,
    pub protocol_version: String,
    pub target: String,
    pub message_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelSessionState {
    pub status: String,
    #[serde(default)]
    pub session_dir: Option<String>,
    #[serde(default)]
    pub qr_code: Option<String>,
    #[serde(default)]
    pub connected_at_epoch_ms: Option<u128>,
    #[serde(default)]
    pub disconnected_at_epoch_ms: Option<u128>,
}

impl Default for ChannelSessionState {
    fn default() -> Self {
        Self {
            status: "not_logged_in".to_string(),
            session_dir: None,
            qr_code: None,
            connected_at_epoch_ms: None,
            disconnected_at_epoch_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAuditEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub channel: String,
    pub account_id: String,
    pub protocol_family: String,
    pub protocol_version: String,
    pub action: String,
    pub result: String,
    pub operation: String,
    pub status: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct AdapterContext {
    channel: String,
    account_id: String,
    session_root: PathBuf,
    _metadata: BTreeMap<String, String>,
    _routing_defaults: BTreeMap<String, String>,
    _adapter_config: Value,
    credentials: Option<Value>,
}

#[derive(Debug, Clone)]
struct AdapterResponse {
    status: String,
    message: String,
    details: Value,
    session_state: Option<ChannelSessionState>,
}

#[derive(Debug, Clone)]
struct ProbeResult {
    status: String,
    message: String,
}

#[derive(Debug, Clone)]
struct ResolveResult {
    resolved_target: String,
}

#[derive(Debug, Clone)]
struct SendResult {
    message_id: String,
}

#[derive(Debug, Clone, Default)]
struct AccountThrottleState {
    window_started_epoch_ms: u128,
    request_count: u32,
}

#[derive(Debug, Default)]
struct ChannelRuntimeState {
    throttles: HashMap<String, AccountThrottleState>,
}

#[async_trait]
trait ChannelAdapter: Send + Sync {
    fn capabilities(&self) -> Vec<String>;

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String>;
    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String>;
    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String>;
    async fn resolve(&self, ctx: AdapterContext, target: String) -> Result<ResolveResult, String>;
    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        message: String,
    ) -> Result<SendResult, String>;
}

struct WhatsAppAdapter;
struct TokenBotAdapter {
    channel_id: &'static str,
    capabilities: &'static [&'static str],
}
struct WebhookAdapter {
    channel_id: &'static str,
    capabilities: &'static [&'static str],
    kind: WebhookKind,
}
struct HttpBridgeAdapter {
    _channel_id: &'static str,
    capabilities: &'static [&'static str],
}
struct MatrixAdapter;
struct IrcAdapter;

#[derive(Clone, Copy)]
enum WebhookKind {
    GoogleChat,
    Feishu,
    Teams,
    Mattermost,
    Synology,
}

pub(crate) struct ChannelService {
    settings_path: PathBuf,
    settings_dir: PathBuf,
    registry: ChannelRegistry,
    adapters: HashMap<String, Arc<dyn ChannelAdapter>>,
    runtime: Mutex<ChannelRuntimeState>,
    audit_log: ChannelAuditLog,
}

impl ChannelService {
    pub(crate) fn new(settings_path: PathBuf) -> Self {
        let settings_dir = settings_path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let registry = ChannelRegistry::openclaw_compat();
        let mut adapters: HashMap<String, Arc<dyn ChannelAdapter>> = HashMap::new();

        adapters.insert("whatsapp".to_string(), Arc::new(WhatsAppAdapter));
        adapters.insert(
            "telegram".to_string(),
            Arc::new(TokenBotAdapter {
                channel_id: "telegram",
                capabilities: &["send-messages", "receive-messages", "bot-token", "probe"],
            }),
        );
        adapters.insert(
            "discord".to_string(),
            Arc::new(TokenBotAdapter {
                channel_id: "discord",
                capabilities: &["send-messages", "receive-messages", "bot-token", "probe"],
            }),
        );
        adapters.insert(
            "slack".to_string(),
            Arc::new(TokenBotAdapter {
                channel_id: "slack",
                capabilities: &["send-messages", "workspace-app", "bot-token", "probe"],
            }),
        );
        adapters.insert("irc".to_string(), Arc::new(IrcAdapter));
        adapters.insert(
            "googlechat".to_string(),
            Arc::new(WebhookAdapter {
                channel_id: "googlechat",
                capabilities: &["send-messages", "webhook", "probe"],
                kind: WebhookKind::GoogleChat,
            }),
        );
        adapters.insert(
            "feishu".to_string(),
            Arc::new(WebhookAdapter {
                channel_id: "feishu",
                capabilities: &["send-messages", "webhook", "probe"],
                kind: WebhookKind::Feishu,
            }),
        );
        adapters.insert(
            "msteams".to_string(),
            Arc::new(WebhookAdapter {
                channel_id: "msteams",
                capabilities: &["send-messages", "webhook", "probe"],
                kind: WebhookKind::Teams,
            }),
        );
        adapters.insert(
            "mattermost".to_string(),
            Arc::new(WebhookAdapter {
                channel_id: "mattermost",
                capabilities: &["send-messages", "webhook", "probe"],
                kind: WebhookKind::Mattermost,
            }),
        );
        adapters.insert(
            "synology-chat".to_string(),
            Arc::new(WebhookAdapter {
                channel_id: "synology-chat",
                capabilities: &["send-messages", "webhook", "probe"],
                kind: WebhookKind::Synology,
            }),
        );
        adapters.insert("matrix".to_string(), Arc::new(MatrixAdapter));

        for channel_id in [
            "signal",
            "imessage",
            "bluebubbles",
            "nostr",
            "nextcloud-talk",
            "line",
            "zalo",
            "zalouser",
            "tlon",
        ] {
            adapters.insert(
                channel_id.to_string(),
                Arc::new(HttpBridgeAdapter {
                    _channel_id: Box::leak(channel_id.to_string().into_boxed_str()),
                    capabilities: &["send-messages", "bridge-http", "probe", "multi-account"],
                }),
            );
        }

        for descriptor in registry.list() {
            assert!(
                adapters.contains_key(&descriptor.id),
                "missing adapter for channel {}",
                descriptor.id
            );
        }

        Self {
            audit_log: ChannelAuditLog::new(settings_dir.join(CHANNEL_AUDIT_DIR)),
            settings_path,
            settings_dir,
            registry,
            adapters,
            runtime: Mutex::new(ChannelRuntimeState::default()),
        }
    }

    pub(crate) async fn list_channels(&self) -> Result<Vec<ChannelView>, String> {
        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        let runtime = self.runtime.lock().await;
        Ok(self
            .registry
            .list()
            .into_iter()
            .map(|descriptor| self.build_channel_view(&cfg, &runtime, descriptor))
            .collect())
    }

    pub(crate) async fn channel_capabilities(&self) -> Result<Vec<ChannelCapabilityView>, String> {
        let views = self.list_channels().await?;
        Ok(views
            .into_iter()
            .map(|channel| ChannelCapabilityView {
                id: channel.id,
                name: channel.name,
                protocol_family: channel.protocol_family,
                protocol_version: channel.protocol_version,
                protocol_schema: channel.protocol_schema,
                aliases: channel.aliases,
                capabilities: channel.capabilities,
                accounts: channel
                    .accounts
                    .into_iter()
                    .map(|account| ChannelCapabilityAccountView {
                        account_id: account.account_id,
                        enabled: account.enabled,
                        health_status: account.health_state.status,
                        capabilities: account.capabilities,
                    })
                    .collect(),
            })
            .collect())
    }

    pub(crate) async fn upsert_account(
        &self,
        request: ChannelUpsertAccountRequest,
    ) -> Result<ChannelView, String> {
        let channel_id = self.resolve_channel_id(&request.channel)?;
        let provided_credentials = request.credentials.clone();
        let referenced_credentials = if let Some(secret_ref) = request.credentials_ref.as_deref() {
            self.load_credentials(Some(secret_ref))?
        } else {
            None
        };
        validate_account_payload(
            &channel_id,
            &request.account_id,
            provided_credentials
                .as_ref()
                .or(referenced_credentials.as_ref()),
            &request.routing_defaults,
        )?;
        self.registry
            .get(&channel_id)
            .ok_or_else(|| format!("unknown channel '{}'", request.channel))?;

        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        let entry = cfg
            .compatibility
            .channels
            .entry(channel_id.clone())
            .or_insert_with(ChannelPersistedState::default);

        let account = entry
            .accounts
            .entry(request.account_id.clone())
            .or_insert_with(ChannelAccountPersistedState::default);
        account.enabled = request.enabled.unwrap_or(account.enabled);
        account.metadata = request.metadata;
        account.routing_defaults = request.routing_defaults;
        account.limits = request.limits.unwrap_or_else(|| account.limits.clone());
        if !request.adapter_config.is_null() {
            account.adapter_config = request.adapter_config;
        }
        if let Some(credentials) = request.credentials {
            let secret_key = channel_secret_key(&channel_id, &request.account_id);
            open_channel_vault(&self.settings_dir)
                .map_err(|error| error.to_string())?
                .set_secret(&secret_key, &credentials.to_string())
                .map_err(|error| error.to_string())?;
            account.credentials_ref = Some(secret_key);
        } else if let Some(credentials_ref) = request.credentials_ref {
            account.credentials_ref = Some(credentials_ref);
        }
        if request.set_as_default {
            entry.default_account_id = Some(request.account_id.clone());
        }
        self.save_config(&cfg).map_err(|error| error.to_string())?;

        let runtime = self.runtime.lock().await;
        let descriptor = self
            .registry
            .get(&channel_id)
            .cloned()
            .ok_or_else(|| format!("unknown channel '{}'", channel_id))?;
        Ok(self.build_channel_view(&cfg, &runtime, descriptor))
    }

    pub(crate) async fn remove_account(
        &self,
        request: ChannelRemoveAccountRequest,
    ) -> Result<(), String> {
        let channel_id = self.resolve_channel_id(&request.channel)?;
        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        let Some(entry) = cfg.compatibility.channels.get_mut(&channel_id) else {
            return Err(format!("channel '{channel_id}' has no configured accounts"));
        };
        let removed = entry
            .accounts
            .remove(&request.account_id)
            .ok_or_else(|| format!("account '{}' not found", request.account_id))?;
        if entry.default_account_id.as_deref() == Some(request.account_id.as_str()) {
            entry.default_account_id = entry.accounts.keys().next().cloned();
        }
        if let Some(secret_ref) = removed.credentials_ref {
            let _ = open_channel_vault(&self.settings_dir)
                .and_then(|vault| vault.remove_secret(&secret_ref));
        }
        self.remove_session_state(&channel_id, &request.account_id)?;
        self.save_config(&cfg).map_err(|error| error.to_string())?;
        self.audit(
            "remove_account",
            &channel_id,
            &request.account_id,
            "success",
            None,
            None,
        )
        .await;
        Ok(())
    }

    pub(crate) async fn login(
        &self,
        request: ChannelAuthRequest,
    ) -> Result<ChannelActionResponse, String> {
        let channel_id = self.resolve_channel_id(&request.channel)?;
        let protocol_family = family_for_channel(&channel_id).as_str().to_string();
        let response = self
            .run_account_operation(
                &channel_id,
                &request.account_id,
                "login",
                None,
                |ctx, adapter| async move { adapter.login(ctx).await },
            )
            .await?;
        if let Some(session_state) = response.session_state.clone() {
            self.save_session_state(&channel_id, &request.account_id, &session_state)?;
        }
        Ok(ChannelActionResponse {
            channel: channel_id,
            account_id: request.account_id,
            protocol_family,
            protocol_version: CHANNEL_PROTOCOL_VERSION.to_string(),
            status: response.status,
            message: response.message,
            details: response.details,
        })
    }

    pub(crate) async fn logout(
        &self,
        request: ChannelAuthRequest,
    ) -> Result<ChannelActionResponse, String> {
        let channel_id = self.resolve_channel_id(&request.channel)?;
        let protocol_family = family_for_channel(&channel_id).as_str().to_string();
        let response = self
            .run_account_operation(
                &channel_id,
                &request.account_id,
                "logout",
                None,
                |ctx, adapter| async move { adapter.logout(ctx).await },
            )
            .await?;
        if let Some(session_state) = response.session_state.clone() {
            self.save_session_state(&channel_id, &request.account_id, &session_state)?;
        } else {
            self.remove_session_state(&channel_id, &request.account_id)?;
        }
        Ok(ChannelActionResponse {
            channel: channel_id,
            account_id: request.account_id,
            protocol_family,
            protocol_version: CHANNEL_PROTOCOL_VERSION.to_string(),
            status: response.status,
            message: response.message,
            details: response.details,
        })
    }

    pub(crate) async fn probe(
        &self,
        request: ChannelProbeRequest,
    ) -> Result<Vec<ChannelActionResponse>, String> {
        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        let mut targets = Vec::new();

        if let Some(channel) = request.channel.as_deref() {
            let channel_id = self.resolve_channel_id(channel)?;
            let entry = cfg
                .compatibility
                .channels
                .get(&channel_id)
                .cloned()
                .unwrap_or_default();
            if request.all_accounts {
                targets.extend(
                    entry
                        .accounts
                        .keys()
                        .cloned()
                        .map(|account_id| (channel_id.clone(), account_id)),
                );
            } else if let Some(account_id) = request.account_id {
                targets.push((channel_id, account_id));
            } else {
                let selected = self.select_account_id(&channel_id, &entry, None, None)?;
                targets.push((channel_id, selected));
            }
        } else {
            for (channel_id, entry) in &cfg.compatibility.channels {
                if request.all_accounts {
                    targets.extend(
                        entry
                            .accounts
                            .keys()
                            .cloned()
                            .map(|account_id| (channel_id.clone(), account_id)),
                    );
                }
            }
        }

        let mut results = Vec::new();
        for (channel_id, account_id) in targets {
            let protocol_family = family_for_channel(&channel_id).as_str().to_string();
            let response = self
                .run_account_operation(
                    &channel_id,
                    &account_id,
                    "probe",
                    None,
                    move |ctx, adapter| {
                        let protocol_family = protocol_family.clone();
                        async move {
                            let probe = adapter.probe(ctx.clone()).await?;
                            Ok(ChannelActionResponse {
                                channel: ctx.channel,
                                account_id: ctx.account_id,
                                protocol_family,
                                protocol_version: CHANNEL_PROTOCOL_VERSION.to_string(),
                                status: probe.status,
                                message: probe.message,
                                details: Value::Null,
                            })
                        }
                    },
                )
                .await?;
            results.push(response);
        }
        Ok(results)
    }

    pub(crate) async fn resolve(
        &self,
        request: ChannelResolveRequest,
    ) -> Result<ChannelResolveResponse, String> {
        let channel_id = self.resolve_channel_id(&request.channel)?;
        let account_id = self.resolve_account_for_request(
            &channel_id,
            request.account_id.as_deref(),
            request.preferred_account_id.as_deref(),
        )?;
        let protocol_family = family_for_channel(&channel_id).as_str().to_string();
        self.run_account_operation(
            &channel_id,
            &account_id,
            "resolve",
            Some(request.target.clone()),
            {
                let request_target = request.target.clone();
                let protocol_family = protocol_family.clone();
                move |ctx, adapter| {
                    let request_target = request_target.clone();
                    let protocol_family = protocol_family.clone();
                    async move {
                        let resolved = adapter.resolve(ctx.clone(), request_target.clone()).await?;
                        Ok(ChannelResolveResponse {
                            channel: ctx.channel,
                            account_id: ctx.account_id,
                            protocol_family,
                            protocol_version: CHANNEL_PROTOCOL_VERSION.to_string(),
                            requested_target: request_target,
                            resolved_target: resolved.resolved_target,
                            status: "resolved".to_string(),
                        })
                    }
                }
            },
        )
        .await
    }

    pub(crate) async fn send_message(
        &self,
        request: MessageSendRequest,
    ) -> Result<MessageSendResponse, String> {
        let channel_id = self.resolve_channel_id(&request.channel)?;
        let account_id = self.resolve_account_for_request(
            &channel_id,
            request.account_id.as_deref(),
            request.preferred_account_id.as_deref(),
        )?;
        let protocol_family = family_for_channel(&channel_id).as_str().to_string();
        self.run_account_operation(
            &channel_id,
            &account_id,
            "send_message",
            Some(request.target.clone()),
            {
                let request_target = request.target.clone();
                let request_message = request.message.clone();
                let protocol_family = protocol_family.clone();
                move |ctx, adapter| {
                    let request_target = request_target.clone();
                    let request_message = request_message.clone();
                    let protocol_family = protocol_family.clone();
                    async move {
                        let send = adapter
                            .send_message(ctx.clone(), request_target.clone(), request_message)
                            .await?;
                        Ok(MessageSendResponse {
                            channel: ctx.channel,
                            account_id: ctx.account_id,
                            protocol_family,
                            protocol_version: CHANNEL_PROTOCOL_VERSION.to_string(),
                            target: request_target,
                            message_id: send.message_id,
                            status: "sent".to_string(),
                        })
                    }
                }
            },
        )
        .await
    }

    pub(crate) async fn logs(
        &self,
        query: ChannelLogsQuery,
    ) -> Result<Vec<ChannelAuditEntry>, String> {
        self.audit_log
            .read_recent(
                query.limit.unwrap_or(50).clamp(1, 200),
                query.channel.as_deref(),
                query.account_id.as_deref(),
            )
            .await
            .map_err(|error| error.to_string())
    }

    pub(crate) async fn legacy_upsert(
        &self,
        request: LegacyChannelUpsertRequest,
    ) -> Result<ChannelView, String> {
        self.upsert_account(ChannelUpsertAccountRequest {
            channel: request.channel_id,
            account_id: request.alias.unwrap_or_else(|| "default".to_string()),
            enabled: request.enabled,
            credentials: None,
            credentials_ref: None,
            metadata: request.metadata,
            routing_defaults: BTreeMap::new(),
            limits: None,
            adapter_config: request.config,
            set_as_default: true,
        })
        .await
    }

    pub(crate) async fn legacy_remove(
        &self,
        request: LegacyChannelRemoveRequest,
    ) -> Result<(), String> {
        let channel_id = self.resolve_channel_id(&request.channel_id)?;
        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        cfg.compatibility.channels.remove(&channel_id);
        self.save_config(&cfg).map_err(|error| error.to_string())
    }

    fn resolve_channel_id(&self, value: &str) -> Result<String, String> {
        self.registry
            .resolve_id(value)
            .ok_or_else(|| format!("unknown channel '{value}'"))
    }

    fn resolve_account_for_request(
        &self,
        channel_id: &str,
        account_id: Option<&str>,
        preferred_account_id: Option<&str>,
    ) -> Result<String, String> {
        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        let entry = cfg
            .compatibility
            .channels
            .get(channel_id)
            .cloned()
            .unwrap_or_default();
        self.select_account_id(channel_id, &entry, account_id, preferred_account_id)
    }

    fn select_account_id(
        &self,
        channel_id: &str,
        entry: &ChannelPersistedState,
        account_id: Option<&str>,
        preferred_account_id: Option<&str>,
    ) -> Result<String, String> {
        if let Some(account_id) = account_id {
            if entry.accounts.contains_key(account_id) {
                return Ok(account_id.to_string());
            }
            return Err(format!(
                "account '{account_id}' not found for channel '{channel_id}'"
            ));
        }
        if let Some(preferred) = preferred_account_id {
            if entry.accounts.contains_key(preferred) {
                return Ok(preferred.to_string());
            }
        }
        if let Some(default_account_id) = entry.default_account_id.as_deref() {
            if entry.accounts.contains_key(default_account_id) {
                return Ok(default_account_id.to_string());
            }
        }

        let active_accounts = entry
            .accounts
            .iter()
            .filter(|(_, account)| account.enabled)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();

        if active_accounts.len() == 1 {
            return Ok(active_accounts[0].clone());
        }
        if active_accounts.is_empty() {
            return Err(format!("channel '{channel_id}' has no active accounts"));
        }

        Err(format!(
            "ambiguous account selection for channel '{channel_id}'; available accounts: {}",
            active_accounts.join(", ")
        ))
    }

    fn build_channel_view(
        &self,
        cfg: &AppConfig,
        runtime: &ChannelRuntimeState,
        descriptor: ChannelDescriptor,
    ) -> ChannelView {
        let persisted = cfg
            .compatibility
            .channels
            .get(&descriptor.id)
            .cloned()
            .unwrap_or_default();

        let mut accounts = persisted
            .accounts
            .iter()
            .map(|(account_id, account)| ChannelAccountView {
                account_id: account_id.clone(),
                enabled: account.enabled,
                configured: account.is_configured(),
                is_default: persisted.default_account_id.as_deref() == Some(account_id.as_str()),
                credentials_ref: account.credentials_ref.clone(),
                metadata: account.metadata.clone(),
                routing_defaults: account.routing_defaults.clone(),
                health_state: account.health_state.clone(),
                limits: account.limits.clone(),
                adapter_config: account.adapter_config.clone(),
                session: self
                    .load_session_state(&descriptor.id, account_id)
                    .unwrap_or_default(),
                capabilities: match family_for_channel(&descriptor.id) {
                    protocol::ChannelTransportFamily::BridgeHttpV1 => {
                        bridge_channel_capabilities(&descriptor.id)
                            .into_iter()
                            .map(ToString::to_string)
                            .collect()
                    }
                    _ => self
                        .adapters
                        .get(&descriptor.id)
                        .map(|adapter| adapter.capabilities())
                        .unwrap_or_else(|| descriptor.capabilities.clone()),
                },
            })
            .collect::<Vec<_>>();
        accounts.sort_by(|a, b| a.account_id.cmp(&b.account_id));

        let ambiguity_warning = {
            let active_count = accounts.iter().filter(|account| account.enabled).count();
            if active_count > 1 && persisted.default_account_id.is_none() {
                Some("Mais de uma conta ativa sem default_account_id.".to_string())
            } else {
                None
            }
        };

        let _ = runtime;
        let family = family_for_channel(&descriptor.id);
        let protocol_family = family.as_str().to_string();
        ChannelView {
            id: descriptor.id,
            name: descriptor.name,
            protocol_family,
            protocol_version: CHANNEL_PROTOCOL_VERSION.to_string(),
            protocol_schema: family.schema(),
            aliases: descriptor.aliases,
            capabilities: descriptor.capabilities,
            supports_lazy_load: descriptor.supports_lazy_load,
            docs: descriptor.docs,
            config_schema: descriptor.config_schema,
            default_account_id: persisted.default_account_id,
            ambiguity_warning,
            accounts,
        }
    }

    fn load_config(&self) -> AppConfig {
        AppConfig::load_settings_from(&self.settings_path)
    }

    fn save_config(&self, cfg: &AppConfig) -> Result<(), std::io::Error> {
        cfg.save_settings_to(&self.settings_path)
    }

    fn migrate_legacy_channels(&self, cfg: &mut AppConfig) {
        for channel in cfg.compatibility.channels.values_mut() {
            if channel.accounts.is_empty()
                && (json_value_is_present(&channel.config)
                    || !channel.metadata.is_empty()
                    || channel.alias.is_some())
            {
                let account_id = channel
                    .alias
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| "default".to_string());
                channel.accounts.insert(
                    account_id.clone(),
                    ChannelAccountPersistedState {
                        enabled: true,
                        credentials_ref: None,
                        metadata: channel.metadata.clone(),
                        routing_defaults: BTreeMap::new(),
                        health_state: ChannelAccountHealthState::default(),
                        limits: ChannelAccountPolicy::default(),
                        adapter_config: channel.config.clone(),
                    },
                );
                channel.default_account_id = Some(account_id);
            }
        }
    }

    fn session_dir(&self, channel: &str, account_id: &str) -> PathBuf {
        self.settings_dir
            .join(CHANNEL_SESSIONS_DIR)
            .join(channel)
            .join(account_id)
    }

    fn load_session_state(
        &self,
        channel: &str,
        account_id: &str,
    ) -> Result<ChannelSessionState, String> {
        let path = self.session_dir(channel, account_id).join("session.json");
        if !path.exists() {
            return Ok(ChannelSessionState::default());
        }
        let raw = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        serde_json::from_str(&raw).map_err(|error| error.to_string())
    }

    fn save_session_state(
        &self,
        channel: &str,
        account_id: &str,
        session_state: &ChannelSessionState,
    ) -> Result<(), String> {
        let dir = self.session_dir(channel, account_id);
        std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        let path = dir.join("session.json");
        let raw = serde_json::to_string_pretty(session_state).map_err(|error| error.to_string())?;
        std::fs::write(path, raw).map_err(|error| error.to_string())
    }

    fn remove_session_state(&self, channel: &str, account_id: &str) -> Result<(), String> {
        let dir = self.session_dir(channel, account_id);
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    async fn run_account_operation<T, F, Fut>(
        &self,
        channel: &str,
        account_id: &str,
        operation: &str,
        target: Option<String>,
        action: F,
    ) -> Result<T, String>
    where
        F: Fn(AdapterContext, Arc<dyn ChannelAdapter>) -> Fut + Clone,
        Fut: Future<Output = Result<T, String>>,
    {
        let channel_id = self.resolve_channel_id(channel)?;
        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        let entry = cfg
            .compatibility
            .channels
            .get(&channel_id)
            .cloned()
            .ok_or_else(|| format!("channel '{channel_id}' has no configured accounts"))?;
        let account = entry.accounts.get(account_id).cloned().ok_or_else(|| {
            format!("account '{account_id}' not found for channel '{channel_id}'")
        })?;
        if !account.enabled {
            return Err(format!("account '{account_id}' is disabled"));
        }

        self.enforce_rate_limit(&channel_id, account_id, &account.limits)
            .await?;
        self.enforce_circuit_breaker(&account.health_state)?;

        let adapter = self
            .adapters
            .get(&channel_id)
            .cloned()
            .ok_or_else(|| format!("no adapter registered for channel '{channel_id}'"))?;
        let ctx = AdapterContext {
            channel: channel_id.clone(),
            account_id: account_id.to_string(),
            session_root: self.session_dir(&channel_id, account_id),
            _metadata: account.metadata.clone(),
            _routing_defaults: account.routing_defaults.clone(),
            _adapter_config: account.adapter_config.clone(),
            credentials: self.load_credentials(account.credentials_ref.as_deref())?,
        };

        let result = self
            .execute_with_policy(
                &channel_id,
                account_id,
                operation,
                target.clone(),
                &account.limits,
                {
                    let action = action.clone();
                    move || action(ctx.clone(), adapter.clone())
                },
            )
            .await;

        match &result {
            Ok(_) => {
                self.update_health(&channel_id, account_id, |health| {
                    health.status = "healthy".to_string();
                    health.last_error = None;
                    health.failure_count = 0;
                    health.circuit_open_until_epoch_ms = None;
                    health.last_checked_epoch_ms = Some(epoch_ms_now());
                    health.last_connected_epoch_ms = Some(epoch_ms_now());
                })?;
                self.audit(operation, &channel_id, account_id, "success", target, None)
                    .await;
            }
            Err(error) => {
                self.update_health(&channel_id, account_id, |health| {
                    health.status = "degraded".to_string();
                    health.last_error = Some(error.clone());
                    health.failure_count = health.failure_count.saturating_add(1);
                    health.last_checked_epoch_ms = Some(epoch_ms_now());
                    if health.failure_count >= account.limits.circuit_breaker_threshold {
                        health.status = "circuit_open".to_string();
                        health.circuit_open_until_epoch_ms = Some(
                            epoch_ms_now() + u128::from(account.limits.circuit_breaker_open_ms),
                        );
                    }
                })?;
                self.audit(
                    operation,
                    &channel_id,
                    account_id,
                    "error",
                    target,
                    Some(error.clone()),
                )
                .await;
            }
        }

        result
    }

    async fn execute_with_policy<T, F, Fut>(
        &self,
        channel: &str,
        account_id: &str,
        operation: &str,
        target: Option<String>,
        policy: &ChannelAccountPolicy,
        mut action: F,
    ) -> Result<T, String>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        let mut last_error = None;
        for attempt in 0..=policy.max_retries {
            let timed =
                tokio::time::timeout(Duration::from_millis(policy.timeout_ms), action()).await;
            match timed {
                Ok(Ok(value)) => return Ok(value),
                Ok(Err(error)) => last_error = Some(error),
                Err(_) => last_error = Some(format!("operation '{operation}' timed out")),
            }
            if attempt < policy.max_retries {
                let backoff = policy.backoff_base_ms.saturating_mul(2_u64.pow(attempt));
                tokio::time::sleep(Duration::from_millis(backoff)).await;
            }
        }
        let error = last_error.unwrap_or_else(|| "operation failed".to_string());
        self.audit(
            operation,
            channel,
            account_id,
            "retry_exhausted",
            target,
            Some(error.clone()),
        )
        .await;
        Err(error)
    }

    async fn enforce_rate_limit(
        &self,
        channel: &str,
        account_id: &str,
        policy: &ChannelAccountPolicy,
    ) -> Result<(), String> {
        let mut runtime = self.runtime.lock().await;
        let key = format!("{channel}:{account_id}");
        let throttle = runtime.throttles.entry(key).or_default();
        let now = epoch_ms_now();
        if now.saturating_sub(throttle.window_started_epoch_ms) >= 60_000 {
            throttle.window_started_epoch_ms = now;
            throttle.request_count = 0;
        }
        if throttle.request_count >= policy.rate_limit_per_minute {
            return Err(format!("rate limit exceeded for {channel}:{account_id}"));
        }
        throttle.request_count = throttle.request_count.saturating_add(1);
        Ok(())
    }

    fn enforce_circuit_breaker(&self, health: &ChannelAccountHealthState) -> Result<(), String> {
        if let Some(until) = health.circuit_open_until_epoch_ms {
            if until > epoch_ms_now() {
                return Err("circuit breaker open for this account".to_string());
            }
        }
        Ok(())
    }

    fn update_health<F>(&self, channel: &str, account_id: &str, mutate: F) -> Result<(), String>
    where
        F: FnOnce(&mut ChannelAccountHealthState),
    {
        let mut cfg = self.load_config();
        self.migrate_legacy_channels(&mut cfg);
        let health = cfg
            .compatibility
            .channels
            .get_mut(channel)
            .and_then(|entry| entry.accounts.get_mut(account_id))
            .map(|account| &mut account.health_state)
            .ok_or_else(|| format!("account '{account_id}' not found for channel '{channel}'"))?;
        mutate(health);
        self.save_config(&cfg).map_err(|error| error.to_string())
    }

    fn load_credentials(&self, credentials_ref: Option<&str>) -> Result<Option<Value>, String> {
        let Some(credentials_ref) = credentials_ref else {
            return Ok(None);
        };
        let raw = open_channel_vault(&self.settings_dir)
            .map_err(|error| error.to_string())?
            .get_secret(credentials_ref)
            .map_err(|error| error.to_string())?;
        let Some(raw) = raw else {
            return Ok(None);
        };
        serde_json::from_str(&raw)
            .map(Some)
            .or_else(|_| Ok(Some(json!({ "token": raw }))))
    }

    async fn audit(
        &self,
        operation: &str,
        channel: &str,
        account_id: &str,
        status: &str,
        target: Option<String>,
        error: Option<String>,
    ) {
        let (error_code, error_message) = split_error_code(error);
        let entry = ChannelAuditEntry {
            id: Uuid::new_v4().to_string(),
            timestamp: Utc::now(),
            channel: channel.to_string(),
            account_id: account_id.to_string(),
            protocol_family: family_for_channel(channel).as_str().to_string(),
            protocol_version: CHANNEL_PROTOCOL_VERSION.to_string(),
            action: operation.to_string(),
            result: status.to_string(),
            operation: operation.to_string(),
            status: status.to_string(),
            target,
            summary: None,
            error_code,
            error: error_message,
        };
        let _ = self.audit_log.append(&entry).await;
    }
}

#[async_trait]
impl ChannelAdapter for WhatsAppAdapter {
    fn capabilities(&self) -> Vec<String> {
        vec![
            "send-messages".to_string(),
            "receive-messages".to_string(),
            "qr-login".to_string(),
            "multi-account".to_string(),
        ]
    }

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        std::fs::create_dir_all(ctx.session_root.join("auth"))
            .map_err(|error| error.to_string())?;
        let qr_code = format!("wa://link/{}-{}", ctx.account_id, Uuid::new_v4());
        Ok(AdapterResponse {
            status: "connected".to_string(),
            message: "WhatsApp account linked locally.".to_string(),
            details: json!({ "qr_code": qr_code }),
            session_state: Some(ChannelSessionState {
                status: "connected".to_string(),
                session_dir: Some(ctx.session_root.display().to_string()),
                qr_code: Some(qr_code),
                connected_at_epoch_ms: Some(epoch_ms_now()),
                disconnected_at_epoch_ms: None,
            }),
        })
    }

    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        Ok(AdapterResponse {
            status: "logged_out".to_string(),
            message: "WhatsApp account logged out.".to_string(),
            details: Value::Null,
            session_state: Some(ChannelSessionState {
                status: "logged_out".to_string(),
                session_dir: Some(ctx.session_root.display().to_string()),
                qr_code: None,
                connected_at_epoch_ms: None,
                disconnected_at_epoch_ms: Some(epoch_ms_now()),
            }),
        })
    }

    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String> {
        let session = ctx.session_root.join("session.json");
        if !session.exists() {
            return Ok(ProbeResult {
                status: "not_logged_in".to_string(),
                message: "WhatsApp account is not linked yet.".to_string(),
            });
        }
        Ok(ProbeResult {
            status: "healthy".to_string(),
            message: "WhatsApp account session is isolated and ready.".to_string(),
        })
    }

    async fn resolve(&self, _ctx: AdapterContext, target: String) -> Result<ResolveResult, String> {
        Ok(ResolveResult {
            resolved_target: normalize_target(target),
        })
    }

    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        _message: String,
    ) -> Result<SendResult, String> {
        if !ctx.session_root.join("session.json").exists() {
            return Err("WhatsApp account is not logged in".to_string());
        }
        Ok(SendResult {
            message_id: format!("wa-{}", hash_message_target(&target)),
        })
    }
}

#[async_trait]
impl ChannelAdapter for TokenBotAdapter {
    fn capabilities(&self) -> Vec<String> {
        self.capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect()
    }

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        require_token_credentials(&ctx)?;
        std::fs::create_dir_all(&ctx.session_root).map_err(|error| error.to_string())?;
        Ok(AdapterResponse {
            status: "connected".to_string(),
            message: format!("{} account authenticated.", self.channel_id),
            details: Value::Null,
            session_state: Some(ChannelSessionState {
                status: "connected".to_string(),
                session_dir: Some(ctx.session_root.display().to_string()),
                qr_code: None,
                connected_at_epoch_ms: Some(epoch_ms_now()),
                disconnected_at_epoch_ms: None,
            }),
        })
    }

    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        Ok(AdapterResponse {
            status: "logged_out".to_string(),
            message: format!("{} account disconnected.", self.channel_id),
            details: Value::Null,
            session_state: Some(ChannelSessionState {
                status: "logged_out".to_string(),
                session_dir: Some(ctx.session_root.display().to_string()),
                qr_code: None,
                connected_at_epoch_ms: None,
                disconnected_at_epoch_ms: Some(epoch_ms_now()),
            }),
        })
    }

    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String> {
        require_token_credentials(&ctx)?;
        Ok(ProbeResult {
            status: "healthy".to_string(),
            message: format!("{} token/account metadata looks valid.", self.channel_id),
        })
    }

    async fn resolve(&self, _ctx: AdapterContext, target: String) -> Result<ResolveResult, String> {
        Ok(ResolveResult {
            resolved_target: normalize_target(target),
        })
    }

    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        _message: String,
    ) -> Result<SendResult, String> {
        require_token_credentials(&ctx)?;
        Ok(SendResult {
            message_id: format!("{}-{}", self.channel_id, hash_message_target(&target)),
        })
    }
}

#[async_trait]
impl ChannelAdapter for WebhookAdapter {
    fn capabilities(&self) -> Vec<String> {
        self.capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect()
    }

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        let webhook_url = require_webhook_url(&ctx)?;
        validate_url(&webhook_url)?;
        std::fs::create_dir_all(&ctx.session_root).map_err(|error| error.to_string())?;
        Ok(AdapterResponse {
            status: "connected".to_string(),
            message: format!("{} webhook registered for account.", self.channel_id),
            details: json!({ "webhook_url": redact_url(&webhook_url) }),
            session_state: Some(connected_session_state(&ctx)),
        })
    }

    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        Ok(AdapterResponse {
            status: "logged_out".to_string(),
            message: format!("{} webhook account disconnected.", self.channel_id),
            details: Value::Null,
            session_state: Some(logged_out_session_state(&ctx)),
        })
    }

    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String> {
        let webhook_url = require_webhook_url(&ctx)?;
        let payload = webhook_payload(
            self.kind,
            ctx._routing_defaults
                .get("probe_target")
                .cloned()
                .unwrap_or_else(|| "probe".to_string()),
            format!("[MLX-Pilot probe] {}", self.channel_id),
        );
        http_json_request_json(reqwest::Method::POST, &webhook_url, None, payload, None).await?;
        Ok(ProbeResult {
            status: "healthy".to_string(),
            message: format!("{} webhook accepted the probe request.", self.channel_id),
        })
    }

    async fn resolve(&self, _ctx: AdapterContext, target: String) -> Result<ResolveResult, String> {
        Ok(ResolveResult {
            resolved_target: normalize_target(target),
        })
    }

    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        message: String,
    ) -> Result<SendResult, String> {
        let webhook_url = require_webhook_url(&ctx)?;
        let payload = webhook_payload(self.kind, target.clone(), message);
        http_json_request_json(reqwest::Method::POST, &webhook_url, None, payload, None).await?;
        Ok(SendResult {
            message_id: format!("{}-{}", self.channel_id, hash_message_target(&target)),
        })
    }
}

#[async_trait]
impl ChannelAdapter for HttpBridgeAdapter {
    fn capabilities(&self) -> Vec<String> {
        self.capabilities
            .iter()
            .map(|value| (*value).to_string())
            .collect()
    }

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        let (base_url, bearer) = require_bridge_endpoint(&ctx)?;
        let response = http_json_request_json(
            reqwest::Method::POST,
            &format!("{}/login", base_url.trim_end_matches('/')),
            bearer.as_deref(),
            json!({
                "channel": ctx.channel.clone(),
                "account_id": ctx.account_id.clone(),
                "metadata": ctx._metadata.clone(),
            }),
            None,
        )
        .await?;
        std::fs::create_dir_all(&ctx.session_root).map_err(|error| error.to_string())?;
        Ok(AdapterResponse {
            status: response
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("connected")
                .to_string(),
            message: response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("bridge login ok")
                .to_string(),
            details: sanitize_json_value(response),
            session_state: Some(connected_session_state(&ctx)),
        })
    }

    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        let (base_url, bearer) = require_bridge_endpoint(&ctx)?;
        let response = http_json_request_json(
            reqwest::Method::POST,
            &format!("{}/logout", base_url.trim_end_matches('/')),
            bearer.as_deref(),
            json!({
                "channel": ctx.channel.clone(),
                "account_id": ctx.account_id.clone()
            }),
            None,
        )
        .await?;
        Ok(AdapterResponse {
            status: response
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("logged_out")
                .to_string(),
            message: response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("bridge logout ok")
                .to_string(),
            details: sanitize_json_value(response),
            session_state: Some(logged_out_session_state(&ctx)),
        })
    }

    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String> {
        let (base_url, bearer) = require_bridge_endpoint(&ctx)?;
        let response = http_json_request_json(
            reqwest::Method::POST,
            &format!("{}/probe", base_url.trim_end_matches('/')),
            bearer.as_deref(),
            json!({
                "channel": ctx.channel.clone(),
                "account_id": ctx.account_id.clone()
            }),
            None,
        )
        .await?;
        Ok(ProbeResult {
            status: response
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("healthy")
                .to_string(),
            message: response
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("bridge probe ok")
                .to_string(),
        })
    }

    async fn resolve(&self, ctx: AdapterContext, target: String) -> Result<ResolveResult, String> {
        let (base_url, bearer) = require_bridge_endpoint(&ctx)?;
        let response = http_json_request_json(
            reqwest::Method::POST,
            &format!("{}/resolve", base_url.trim_end_matches('/')),
            bearer.as_deref(),
            json!({
                "channel": ctx.channel.clone(),
                "account_id": ctx.account_id.clone(),
                "target": target
            }),
            None,
        )
        .await?;
        Ok(ResolveResult {
            resolved_target: response
                .get("resolved_target")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        })
    }

    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        message: String,
    ) -> Result<SendResult, String> {
        let (base_url, bearer) = require_bridge_endpoint(&ctx)?;
        let response = http_json_request_json(
            reqwest::Method::POST,
            &format!("{}/send", base_url.trim_end_matches('/')),
            bearer.as_deref(),
            json!({
                "channel": ctx.channel.clone(),
                "account_id": ctx.account_id.clone(),
                "target": target,
                "message": message
            }),
            None,
        )
        .await?;
        Ok(SendResult {
            message_id: response
                .get("message_id")
                .and_then(Value::as_str)
                .unwrap_or("bridge-message")
                .to_string(),
        })
    }
}

#[async_trait]
impl ChannelAdapter for MatrixAdapter {
    fn capabilities(&self) -> Vec<String> {
        vec![
            "send-messages".to_string(),
            "resolve-room".to_string(),
            "rest-api".to_string(),
            "access-token".to_string(),
        ]
    }

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        let (homeserver, bearer) = require_api_base_and_token(&ctx, "homeserver")?;
        let response = http_json_request_json(
            reqwest::Method::GET,
            &format!(
                "{}/_matrix/client/v3/account/whoami",
                homeserver.trim_end_matches('/')
            ),
            Some(&bearer),
            Value::Null,
            None,
        )
        .await?;
        std::fs::create_dir_all(&ctx.session_root).map_err(|error| error.to_string())?;
        Ok(AdapterResponse {
            status: "connected".to_string(),
            message: "matrix account authenticated".to_string(),
            details: sanitize_json_value(response),
            session_state: Some(connected_session_state(&ctx)),
        })
    }

    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        Ok(AdapterResponse {
            status: "logged_out".to_string(),
            message: "matrix account disconnected".to_string(),
            details: Value::Null,
            session_state: Some(logged_out_session_state(&ctx)),
        })
    }

    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String> {
        let (homeserver, bearer) = require_api_base_and_token(&ctx, "homeserver")?;
        http_json_request_json(
            reqwest::Method::GET,
            &format!(
                "{}/_matrix/client/v3/account/whoami",
                homeserver.trim_end_matches('/')
            ),
            Some(&bearer),
            Value::Null,
            None,
        )
        .await?;
        Ok(ProbeResult {
            status: "healthy".to_string(),
            message: "matrix whoami succeeded".to_string(),
        })
    }

    async fn resolve(&self, ctx: AdapterContext, target: String) -> Result<ResolveResult, String> {
        if target.starts_with('!') {
            return Ok(ResolveResult {
                resolved_target: target,
            });
        }
        if !target.starts_with('#') {
            return Err(channel_error(
                "invalid_target",
                "matrix room aliases must start with '#' or use room ids",
            ));
        }
        let (homeserver, bearer) = require_api_base_and_token(&ctx, "homeserver")?;
        let encoded = urlencoding::encode(&target);
        let response = http_json_request_json(
            reqwest::Method::GET,
            &format!(
                "{}/_matrix/client/v3/directory/room/{}",
                homeserver.trim_end_matches('/'),
                encoded
            ),
            Some(&bearer),
            Value::Null,
            None,
        )
        .await?;
        let room_id = response
            .get("room_id")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                channel_error("provider_error", "matrix resolve response missing room_id")
            })?;
        Ok(ResolveResult {
            resolved_target: room_id.to_string(),
        })
    }

    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        message: String,
    ) -> Result<SendResult, String> {
        let resolved = self.resolve(ctx.clone(), target).await?;
        let (homeserver, bearer) = require_api_base_and_token(&ctx, "homeserver")?;
        let txn_id = Uuid::new_v4().to_string();
        http_json_request_json(
            reqwest::Method::PUT,
            &format!(
                "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
                homeserver.trim_end_matches('/'),
                urlencoding::encode(&resolved.resolved_target),
                txn_id
            ),
            Some(&bearer),
            json!({
                "msgtype": "m.text",
                "body": message
            }),
            None,
        )
        .await?;
        Ok(SendResult { message_id: txn_id })
    }
}

#[async_trait]
impl ChannelAdapter for IrcAdapter {
    fn capabilities(&self) -> Vec<String> {
        vec![
            "send-messages".to_string(),
            "tcp-socket".to_string(),
            "probe".to_string(),
            "multi-account".to_string(),
        ]
    }

    async fn login(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        let mut stream = connect_irc(&ctx).await?;
        initialize_irc_session(&mut stream, &ctx).await?;
        write_irc_line(&mut stream, "QUIT :MLX-Pilot login").await?;
        std::fs::create_dir_all(&ctx.session_root).map_err(|error| error.to_string())?;
        Ok(AdapterResponse {
            status: "connected".to_string(),
            message: "irc account connected".to_string(),
            details: Value::Null,
            session_state: Some(connected_session_state(&ctx)),
        })
    }

    async fn logout(&self, ctx: AdapterContext) -> Result<AdapterResponse, String> {
        Ok(AdapterResponse {
            status: "logged_out".to_string(),
            message: "irc account disconnected".to_string(),
            details: Value::Null,
            session_state: Some(logged_out_session_state(&ctx)),
        })
    }

    async fn probe(&self, ctx: AdapterContext) -> Result<ProbeResult, String> {
        let mut stream = connect_irc(&ctx).await?;
        initialize_irc_session(&mut stream, &ctx).await?;
        write_irc_line(&mut stream, "QUIT :MLX-Pilot probe").await?;
        Ok(ProbeResult {
            status: "healthy".to_string(),
            message: "irc socket/auth probe succeeded".to_string(),
        })
    }

    async fn resolve(&self, _ctx: AdapterContext, target: String) -> Result<ResolveResult, String> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Err(channel_error(
                "invalid_target",
                "irc target cannot be empty",
            ));
        }
        Ok(ResolveResult {
            resolved_target: trimmed.to_string(),
        })
    }

    async fn send_message(
        &self,
        ctx: AdapterContext,
        target: String,
        message: String,
    ) -> Result<SendResult, String> {
        let resolved = self.resolve(ctx.clone(), target.clone()).await?;
        let mut stream = connect_irc(&ctx).await?;
        initialize_irc_session(&mut stream, &ctx).await?;
        if resolved.resolved_target.starts_with('#') {
            write_irc_line(&mut stream, &format!("JOIN {}", resolved.resolved_target)).await?;
        }
        write_irc_line(
            &mut stream,
            &format!(
                "PRIVMSG {} :{}",
                resolved.resolved_target,
                sanitize_irc_message(&message)
            ),
        )
        .await?;
        write_irc_line(&mut stream, "QUIT :MLX-Pilot send").await?;
        Ok(SendResult {
            message_id: format!("irc-{}", hash_message_target(&resolved.resolved_target)),
        })
    }
}

struct ChannelAuditLog {
    dir: PathBuf,
}

impl ChannelAuditLog {
    fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    async fn append(&self, entry: &ChannelAuditEntry) -> Result<(), std::io::Error> {
        let path = self
            .dir
            .join(format!("{}.jsonl", entry.timestamp.format("%Y-%m-%d")));
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let raw = serde_json::to_string(entry).unwrap_or_default();
        if raw.is_empty() {
            return Ok(());
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        use tokio::io::AsyncWriteExt;
        file.write_all(format!("{raw}\n").as_bytes()).await
    }

    async fn read_recent(
        &self,
        limit: usize,
        channel: Option<&str>,
        account_id: Option<&str>,
    ) -> Result<Vec<ChannelAuditEntry>, std::io::Error> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut files = std::fs::read_dir(&self.dir)?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
            .collect::<Vec<_>>();
        files.sort();
        files.reverse();

        let mut entries = Vec::new();
        for path in files {
            let raw = std::fs::read_to_string(path)?;
            for line in raw.lines().rev() {
                if let Ok(entry) = serde_json::from_str::<ChannelAuditEntry>(line) {
                    if channel
                        .map(|value| value.eq_ignore_ascii_case(&entry.channel))
                        .unwrap_or(true)
                        && account_id
                            .map(|value| value.eq_ignore_ascii_case(&entry.account_id))
                            .unwrap_or(true)
                    {
                        entries.push(entry);
                        if entries.len() >= limit {
                            return Ok(entries);
                        }
                    }
                }
            }
        }
        Ok(entries)
    }
}

fn open_channel_vault(settings_dir: &Path) -> Result<SecretsVault, std::io::Error> {
    SecretsVault::open(settings_dir)
}

fn channel_secret_key(channel: &str, account_id: &str) -> String {
    format!("channels.{channel}.{account_id}.credentials")
}

fn require_token_credentials(ctx: &AdapterContext) -> Result<(), String> {
    let credentials = require_credentials(ctx)?;
    let token = credentials
        .get("token")
        .and_then(Value::as_str)
        .or_else(|| credentials.get("bot_token").and_then(Value::as_str));
    if token
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        return Err(channel_error(
            "auth_error",
            &format!(
                "account '{}' for channel '{}' requires token credentials",
                ctx.account_id, ctx.channel
            ),
        ));
    }
    Ok(())
}

fn require_credentials(ctx: &AdapterContext) -> Result<&Value, String> {
    ctx.credentials.as_ref().ok_or_else(|| {
        channel_error(
            "auth_error",
            &format!(
                "account '{}' for channel '{}' is missing credentials",
                ctx.account_id, ctx.channel
            ),
        )
    })
}

fn require_token_value(ctx: &AdapterContext) -> Result<String, String> {
    require_token_credentials(ctx)?;
    let credentials = require_credentials(ctx)?;
    Ok(credentials
        .get("token")
        .and_then(Value::as_str)
        .or_else(|| credentials.get("bot_token").and_then(Value::as_str))
        .unwrap_or_default()
        .trim()
        .to_string())
}

fn require_api_base_and_token(
    ctx: &AdapterContext,
    base_key: &str,
) -> Result<(String, String), String> {
    let credentials = require_credentials(ctx)?;
    let base = credentials
        .get(base_key)
        .and_then(Value::as_str)
        .or_else(|| credentials.get("base_url").and_then(Value::as_str))
        .or_else(|| credentials.get("api_base_url").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            channel_error(
                "auth_error",
                &format!("missing '{base_key}' for channel '{}'", ctx.channel),
            )
        })?
        .to_string();
    validate_url(&base)?;
    Ok((base, require_token_value(ctx)?))
}

fn require_bridge_endpoint(ctx: &AdapterContext) -> Result<(String, Option<String>), String> {
    let credentials = require_credentials(ctx)?;
    let base = credentials
        .get("base_url")
        .and_then(Value::as_str)
        .or_else(|| credentials.get("bridge_url").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            channel_error(
                "auth_error",
                &format!("missing bridge_url/base_url for channel '{}'", ctx.channel),
            )
        })?
        .to_string();
    validate_url(&base)?;
    let bearer = credentials
        .get("token")
        .and_then(Value::as_str)
        .or_else(|| credentials.get("access_token").and_then(Value::as_str))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Ok((base, bearer))
}

fn require_webhook_url(ctx: &AdapterContext) -> Result<String, String> {
    let credentials = require_credentials(ctx)?;
    let webhook_url = credentials
        .get("webhook_url")
        .and_then(Value::as_str)
        .or_else(|| credentials.get("url").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            channel_error(
                "auth_error",
                &format!("missing webhook_url for channel '{}'", ctx.channel),
            )
        })?
        .to_string();
    Ok(webhook_url)
}

fn validate_url(value: &str) -> Result<(), String> {
    reqwest::Url::parse(value)
        .map(|_| ())
        .map_err(|error| channel_error("provider_error", &format!("invalid url: {error}")))
}

async fn http_json_request_json(
    method: reqwest::Method,
    url: &str,
    bearer: Option<&str>,
    body: Value,
    extra_headers: Option<&[(&str, &str)]>,
) -> Result<Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| channel_error("provider_error", &format!("http client error: {error}")))?;
    let mut request = client.request(method, url);
    if let Some(token) = bearer {
        request = request.bearer_auth(token);
    }
    if let Some(headers) = extra_headers {
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
    }
    if !matches!(body, Value::Null) {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(map_reqwest_error)?;
    let status = response.status();
    let text = response.text().await.map_err(map_reqwest_error)?;
    if !status.is_success() {
        let code = match status.as_u16() {
            401 | 403 => "auth_error",
            404 => "invalid_target",
            408 => "network_error",
            429 => "rate_limited",
            400 => "provider_error",
            500..=599 => "provider_error",
            _ => "provider_error",
        };
        return Err(channel_error(
            code,
            &format!("http {status} from {url}: {}", truncate_text(&text, 200)),
        ));
    }
    if text.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&text).or_else(|_| Ok(json!({ "message": truncate_text(&text, 400) })))
}

fn map_reqwest_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        return channel_error("network_error", "request timed out");
    }
    if error.is_connect() {
        return channel_error("network_error", &format!("connection failed: {error}"));
    }
    channel_error("provider_error", &error.to_string())
}

fn channel_error(code: &str, message: &str) -> String {
    format!("{code}: {message}")
}

fn split_error_code(error: Option<String>) -> (Option<String>, Option<String>) {
    let Some(error) = error else {
        return (None, None);
    };
    if let Some((code, message)) = error.split_once(": ") {
        (Some(code.to_string()), Some(message.to_string()))
    } else {
        (Some("provider_error".to_string()), Some(error))
    }
}

fn connected_session_state(ctx: &AdapterContext) -> ChannelSessionState {
    ChannelSessionState {
        status: "connected".to_string(),
        session_dir: Some(ctx.session_root.display().to_string()),
        qr_code: None,
        connected_at_epoch_ms: Some(epoch_ms_now()),
        disconnected_at_epoch_ms: None,
    }
}

fn logged_out_session_state(ctx: &AdapterContext) -> ChannelSessionState {
    ChannelSessionState {
        status: "logged_out".to_string(),
        session_dir: Some(ctx.session_root.display().to_string()),
        qr_code: None,
        connected_at_epoch_ms: None,
        disconnected_at_epoch_ms: Some(epoch_ms_now()),
    }
}

fn redact_url(url: &str) -> String {
    reqwest::Url::parse(url)
        .map(|parsed| {
            let mut out = parsed;
            let _ = out.set_password(None);
            out.set_query(None);
            out.to_string()
        })
        .unwrap_or_else(|_| "<invalid-url>".to_string())
}

fn webhook_payload(kind: WebhookKind, target: String, message: String) -> Value {
    let text = if target.trim().is_empty() {
        message
    } else {
        format!("[{target}] {message}")
    };
    match kind {
        WebhookKind::GoogleChat => json!({ "text": text }),
        WebhookKind::Feishu => json!({
            "msg_type": "text",
            "content": { "text": text }
        }),
        WebhookKind::Teams => json!({ "text": text }),
        WebhookKind::Mattermost => json!({ "text": text, "channel": target }),
        WebhookKind::Synology => json!({ "text": text }),
    }
}

async fn connect_irc(ctx: &AdapterContext) -> Result<TcpStream, String> {
    let credentials = require_credentials(ctx)?;
    let server = credentials
        .get("server")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| channel_error("auth_error", "irc credentials require 'server'"))?;
    let port = credentials
        .get("port")
        .and_then(Value::as_u64)
        .unwrap_or(6667);
    TcpStream::connect((server, port as u16))
        .await
        .map_err(|error| channel_error("network_error", &format!("irc connect failed: {error}")))
}

async fn initialize_irc_session(
    stream: &mut TcpStream,
    ctx: &AdapterContext,
) -> Result<(), String> {
    let credentials = require_credentials(ctx)?;
    if let Some(password) = credentials.get("password").and_then(Value::as_str) {
        if !password.trim().is_empty() {
            write_irc_line(stream, &format!("PASS {}", password.trim())).await?;
        }
    }
    let nick = credentials
        .get("nick")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| channel_error("auth_error", "irc credentials require 'nick'"))?;
    let user = credentials
        .get("username")
        .and_then(Value::as_str)
        .or_else(|| credentials.get("user").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(nick);
    write_irc_line(stream, &format!("NICK {nick}")).await?;
    write_irc_line(stream, &format!("USER {user} 0 * :MLX Pilot")).await?;
    Ok(())
}

async fn write_irc_line(stream: &mut TcpStream, line: &str) -> Result<(), String> {
    stream
        .write_all(format!("{line}\r\n").as_bytes())
        .await
        .map_err(|error| channel_error("network_error", &format!("irc write failed: {error}")))?;
    let mut buffer = [0u8; 256];
    let _ = tokio::time::timeout(Duration::from_millis(150), stream.read(&mut buffer)).await;
    Ok(())
}

fn sanitize_irc_message(message: &str) -> String {
    message.replace(['\r', '\n'], " ")
}

fn sanitize_json_value(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            for key in ["token", "access_token", "authorization", "webhook_url"] {
                if map.contains_key(key) {
                    map.insert(key.to_string(), Value::String("<redacted>".to_string()));
                }
            }
            Value::Object(map)
        }
        other => other,
    }
}

fn truncate_text(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    text.chars().take(limit).collect::<String>() + "..."
}

fn normalize_target(target: String) -> String {
    target.trim().to_ascii_lowercase()
}

fn hash_message_target(target: &str) -> String {
    let mut out = 0u64;
    for byte in target.as_bytes() {
        out = out.wrapping_mul(131).wrapping_add(u64::from(*byte));
    }
    format!("{out:x}")
}

fn epoch_ms_now() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

fn json_value_is_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Object(map) => !map.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::String(text) => !text.trim().is_empty(),
        Value::Bool(flag) => *flag,
        Value::Number(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc as StdArc, Mutex as StdMutex};
    use tokio::net::TcpListener;

    fn token_credentials(token: &str) -> Value {
        json!({ "token": token })
    }

    async fn spawn_json_server(
        responses: StdArc<StdMutex<Vec<(String, String, String)>>>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let responses = responses.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let size = socket.read(&mut buf).await.expect("read");
                    let req = String::from_utf8_lossy(&buf[..size]).to_string();
                    let line = req.lines().next().unwrap_or_default().to_string();
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    let body = req.split("\r\n\r\n").nth(1).unwrap_or_default().to_string();
                    responses.lock().expect("lock").push((
                        line.clone(),
                        path.clone(),
                        body.clone(),
                    ));
                    let payload = match path.as_str() {
                        "/login" => json!({"status":"connected","message":"ok"}),
                        "/logout" => json!({"status":"logged_out","message":"bye"}),
                        "/probe" => json!({"status":"healthy","message":"probe-ok"}),
                        "/resolve" => json!({"resolved_target":"canonical-target"}),
                        "/send" => json!({"message_id":"msg-123"}),
                        "/_matrix/client/v3/account/whoami" => {
                            json!({"user_id":"@bot:example.test"})
                        }
                        "/_matrix/client/v3/directory/room/%23ops%3Aexample.test" => {
                            json!({"room_id":"!ops:example.test"})
                        }
                        _ if path.contains("/send/m.room.message/") => json!({"event_id":"$evt"}),
                        _ => json!({"status":"healthy","message":"ok"}),
                    };
                    let body = payload.to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    socket.write_all(response.as_bytes()).await.expect("write");
                });
            }
        });
        format!("http://{}", addr)
    }

    async fn spawn_irc_server(lines: StdArc<StdMutex<Vec<String>>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let lines = lines.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    loop {
                        match socket.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(size) => {
                                let req = String::from_utf8_lossy(&buf[..size]).to_string();
                                for line in req.split("\r\n").filter(|line| !line.is_empty()) {
                                    lines.lock().expect("lock").push(line.to_string());
                                    if line.starts_with("QUIT ") {
                                        break;
                                    }
                                }
                                let _ = socket.write_all(b":server 001 bot :welcome\r\n").await;
                            }
                            Err(_) => break,
                        }
                    }
                });
            }
        });
        addr.to_string()
    }

    #[tokio::test]
    async fn multi_whatsapp_accounts_keep_isolated_sessions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = ChannelService::new(dir.path().join("settings.json"));

        service
            .upsert_account(ChannelUpsertAccountRequest {
                channel: "whatsapp".to_string(),
                account_id: "work".to_string(),
                enabled: Some(true),
                credentials: Some(token_credentials("wa-work")),
                credentials_ref: None,
                metadata: BTreeMap::new(),
                routing_defaults: BTreeMap::new(),
                limits: None,
                adapter_config: Value::Null,
                set_as_default: true,
            })
            .await
            .expect("upsert work");
        service
            .upsert_account(ChannelUpsertAccountRequest {
                channel: "whatsapp".to_string(),
                account_id: "personal".to_string(),
                enabled: Some(true),
                credentials: Some(token_credentials("wa-personal")),
                credentials_ref: None,
                metadata: BTreeMap::new(),
                routing_defaults: BTreeMap::new(),
                limits: None,
                adapter_config: Value::Null,
                set_as_default: false,
            })
            .await
            .expect("upsert personal");

        service
            .login(ChannelAuthRequest {
                channel: "whatsapp".to_string(),
                account_id: "work".to_string(),
            })
            .await
            .expect("login work");
        service
            .login(ChannelAuthRequest {
                channel: "whatsapp".to_string(),
                account_id: "personal".to_string(),
            })
            .await
            .expect("login personal");

        let channels = service.list_channels().await.expect("list channels");
        let whatsapp = channels
            .into_iter()
            .find(|channel| channel.id == "whatsapp")
            .expect("whatsapp view");
        assert_eq!(whatsapp.accounts.len(), 2);
        assert!(whatsapp
            .accounts
            .iter()
            .all(|account| account.session.status == "connected"));

        service
            .logout(ChannelAuthRequest {
                channel: "whatsapp".to_string(),
                account_id: "work".to_string(),
            })
            .await
            .expect("logout work");

        let channels = service.list_channels().await.expect("list channels");
        let whatsapp = channels
            .into_iter()
            .find(|channel| channel.id == "whatsapp")
            .expect("whatsapp view");
        let work = whatsapp
            .accounts
            .iter()
            .find(|account| account.account_id == "work")
            .expect("work account");
        let personal = whatsapp
            .accounts
            .iter()
            .find(|account| account.account_id == "personal")
            .expect("personal account");
        assert_eq!(work.session.status, "logged_out");
        assert_eq!(personal.session.status, "connected");
    }

    #[tokio::test]
    async fn telegram_multi_account_requires_explicit_resolution_when_ambiguous() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = ChannelService::new(dir.path().join("settings.json"));

        for account_id in ["bot-a", "bot-b"] {
            service
                .upsert_account(ChannelUpsertAccountRequest {
                    channel: "telegram".to_string(),
                    account_id: account_id.to_string(),
                    enabled: Some(true),
                    credentials: Some(token_credentials(account_id)),
                    credentials_ref: None,
                    metadata: BTreeMap::new(),
                    routing_defaults: BTreeMap::new(),
                    limits: None,
                    adapter_config: Value::Null,
                    set_as_default: false,
                })
                .await
                .expect("upsert telegram");
            service
                .login(ChannelAuthRequest {
                    channel: "telegram".to_string(),
                    account_id: account_id.to_string(),
                })
                .await
                .expect("login telegram");
        }

        let err = service
            .send_message(MessageSendRequest {
                channel: "telegram".to_string(),
                account_id: None,
                preferred_account_id: None,
                target: "@team".to_string(),
                message: "ping".to_string(),
            })
            .await
            .expect_err("should be ambiguous");
        assert!(err.contains("ambiguous account selection"));

        let sent = service
            .send_message(MessageSendRequest {
                channel: "telegram".to_string(),
                account_id: Some("bot-a".to_string()),
                preferred_account_id: None,
                target: "@team".to_string(),
                message: "ping".to_string(),
            })
            .await
            .expect("send explicit account");
        assert_eq!(sent.account_id, "bot-a");
    }

    #[tokio::test]
    async fn webhook_channels_probe_and_send_with_real_http_transport() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = ChannelService::new(dir.path().join("settings.json"));
        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let base_url = spawn_json_server(requests.clone()).await;

        service
            .upsert_account(ChannelUpsertAccountRequest {
                channel: "googlechat".to_string(),
                account_id: "default".to_string(),
                enabled: Some(true),
                credentials: Some(json!({ "webhook_url": format!("{base_url}/send") })),
                credentials_ref: None,
                metadata: BTreeMap::new(),
                routing_defaults: BTreeMap::new(),
                limits: None,
                adapter_config: Value::Null,
                set_as_default: true,
            })
            .await
            .expect("upsert googlechat");

        let logged_in = service
            .login(ChannelAuthRequest {
                channel: "googlechat".to_string(),
                account_id: "default".to_string(),
            })
            .await
            .expect("login googlechat");
        assert_eq!(logged_in.status, "connected");

        let probe = service
            .probe(ChannelProbeRequest {
                channel: Some("googlechat".to_string()),
                account_id: Some("default".to_string()),
                all_accounts: false,
            })
            .await
            .expect("probe googlechat");
        assert_eq!(probe[0].status, "healthy");

        let sent = service
            .send_message(MessageSendRequest {
                channel: "googlechat".to_string(),
                account_id: Some("default".to_string()),
                preferred_account_id: None,
                target: "space".to_string(),
                message: "hello".to_string(),
            })
            .await
            .expect("send googlechat");
        assert!(sent.message_id.starts_with("googlechat-"));
        assert!(requests.lock().expect("lock").len() >= 2);
    }

    #[tokio::test]
    async fn bridge_channels_use_http_bridge_transport() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = ChannelService::new(dir.path().join("settings.json"));
        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let base_url = spawn_json_server(requests.clone()).await;

        service
            .upsert_account(ChannelUpsertAccountRequest {
                channel: "signal".to_string(),
                account_id: "phone-a".to_string(),
                enabled: Some(true),
                credentials: Some(json!({ "base_url": base_url, "token": "secret" })),
                credentials_ref: None,
                metadata: BTreeMap::new(),
                routing_defaults: BTreeMap::new(),
                limits: None,
                adapter_config: Value::Null,
                set_as_default: true,
            })
            .await
            .expect("upsert signal");

        service
            .login(ChannelAuthRequest {
                channel: "signal".to_string(),
                account_id: "phone-a".to_string(),
            })
            .await
            .expect("login signal");
        let sent = service
            .send_message(MessageSendRequest {
                channel: "signal".to_string(),
                account_id: Some("phone-a".to_string()),
                preferred_account_id: None,
                target: "+551199999999".to_string(),
                message: "ping".to_string(),
            })
            .await
            .expect("send signal");
        assert_eq!(sent.message_id, "msg-123");
        assert!(requests
            .lock()
            .expect("lock")
            .iter()
            .any(|(_, path, _)| path == "/login"));
    }

    #[tokio::test]
    async fn matrix_adapter_calls_real_client_server_paths() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = ChannelService::new(dir.path().join("settings.json"));
        let requests = StdArc::new(StdMutex::new(Vec::new()));
        let base_url = spawn_json_server(requests.clone()).await;

        service
            .upsert_account(ChannelUpsertAccountRequest {
                channel: "matrix".to_string(),
                account_id: "bot".to_string(),
                enabled: Some(true),
                credentials: Some(json!({
                    "homeserver": base_url,
                    "token": "matrix-token"
                })),
                credentials_ref: None,
                metadata: BTreeMap::new(),
                routing_defaults: BTreeMap::new(),
                limits: None,
                adapter_config: Value::Null,
                set_as_default: true,
            })
            .await
            .expect("upsert matrix");

        service
            .login(ChannelAuthRequest {
                channel: "matrix".to_string(),
                account_id: "bot".to_string(),
            })
            .await
            .expect("login matrix");
        let resolved = service
            .resolve(ChannelResolveRequest {
                channel: "matrix".to_string(),
                account_id: Some("bot".to_string()),
                preferred_account_id: None,
                target: "#ops:example.test".to_string(),
            })
            .await
            .expect("resolve matrix");
        assert_eq!(resolved.resolved_target, "!ops:example.test");

        let sent = service
            .send_message(MessageSendRequest {
                channel: "matrix".to_string(),
                account_id: Some("bot".to_string()),
                preferred_account_id: None,
                target: "#ops:example.test".to_string(),
                message: "hello".to_string(),
            })
            .await
            .expect("send matrix");
        assert!(!sent.message_id.is_empty());
    }

    #[tokio::test]
    async fn irc_adapter_uses_tcp_transport() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = ChannelService::new(dir.path().join("settings.json"));
        let commands = StdArc::new(StdMutex::new(Vec::new()));
        let addr = spawn_irc_server(commands.clone()).await;
        let (host, port) = addr.split_once(':').expect("host:port");

        service
            .upsert_account(ChannelUpsertAccountRequest {
                channel: "irc".to_string(),
                account_id: "freenode".to_string(),
                enabled: Some(true),
                credentials: Some(json!({
                    "server": host,
                    "port": port.parse::<u16>().expect("port"),
                    "nick": "mlxbot",
                    "username": "mlxbot"
                })),
                credentials_ref: None,
                metadata: BTreeMap::new(),
                routing_defaults: BTreeMap::new(),
                limits: None,
                adapter_config: Value::Null,
                set_as_default: true,
            })
            .await
            .expect("upsert irc");

        service
            .send_message(MessageSendRequest {
                channel: "irc".to_string(),
                account_id: Some("freenode".to_string()),
                preferred_account_id: None,
                target: "#mlx".to_string(),
                message: "hello".to_string(),
            })
            .await
            .expect("send irc");

        let joined = commands.lock().expect("lock").join("\n");
        assert!(joined.contains("NICK mlxbot"));
        assert!(joined.contains("PRIVMSG #mlx :hello"));
    }

    #[tokio::test]
    async fn all_catalog_channels_have_operational_adapters() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = ChannelService::new(dir.path().join("settings.json"));
        let capabilities = service.channel_capabilities().await.expect("capabilities");
        assert_eq!(capabilities.len(), 20);
        assert!(capabilities.iter().all(|channel| !channel
            .capabilities
            .iter()
            .any(|cap| cap == "not-configured")));
        assert!(capabilities
            .iter()
            .all(|channel| channel.protocol_version == "v1"));
        assert!(capabilities.iter().all(|channel| channel
            .protocol_schema
            .get("protocol_version")
            .and_then(Value::as_str)
            == Some("v1")));
    }
}
