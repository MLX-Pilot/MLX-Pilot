//! `ToolRegistry` — registration, lookup, dispatch, and JSON Schema
//! validation of tools.

use mlx_agent_tools::{Tool, ToolContext, ToolDefinition, ToolError, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

struct RegisteredTool {
    tool: Arc<dyn Tool>,
    schema_validator: Result<jsonschema::Validator, String>,
}

/// Central registry of all available tools.
///
/// The `AgentLoop` uses this to convert tool names from the LLM into
/// actual `Tool` implementations and dispatch calls.
pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

impl ToolRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    /// Register a tool. Overwrites any existing tool with the same name.
    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        let schema_validator = jsonschema::validator_for(tool.parameters())
            .map_err(|e| format!("invalid schema for tool '{}': {}", tool.name(), e));

        self.tools.insert(
            tool.name().to_string(),
            RegisteredTool {
                tool,
                schema_validator,
            },
        );
    }

    /// Look up a tool by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.get(name).map(|entry| &entry.tool)
    }

    /// Validate params against the tool's JSON Schema, then dispatch.
    pub async fn dispatch(
        &self,
        tool_name: &str,
        params: &Value,
        ctx: &ToolContext,
    ) -> Result<ToolResult, ToolError> {
        let entry = self
            .tools
            .get(tool_name)
            .ok_or_else(|| ToolError::InvalidParams {
                details: format!("unknown tool: {tool_name}"),
            })?;

        // Validate params against JSON Schema.
        self.validate_params(entry, params)?;

        entry.tool.execute(params, ctx).await
    }

    /// Validate parameters against a tool's JSON Schema.
    fn validate_params(&self, tool: &RegisteredTool, params: &Value) -> Result<(), ToolError> {
        let validator =
            tool.schema_validator
                .as_ref()
                .map_err(|error| ToolError::InvalidParams {
                    details: error.clone(),
                })?;

        // Validate and collect errors.
        let errors: Vec<String> = validator
            .iter_errors(params)
            .map(|e| e.to_string())
            .collect();

        if !errors.is_empty() {
            return Err(ToolError::InvalidParams {
                details: format!(
                    "parameter validation failed for '{}': {}",
                    tool.tool.name(),
                    errors.join("; ")
                ),
            });
        }

        Ok(())
    }

    /// Return all tool definitions (for LLM function-calling).
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|entry| entry.tool.to_definition())
            .collect()
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Returns true when no tools are registered.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// Create a registry pre-loaded with all 5 built-in tools.
    pub fn with_builtins() -> Self {
        use mlx_agent_tools::{EditFileTool, ExecTool, ListDirTool, ReadFileTool, WriteFileTool};

        let mut registry = Self::new();
        registry.register(Arc::new(ReadFileTool::new()));
        registry.register(Arc::new(WriteFileTool::new()));
        registry.register(Arc::new(EditFileTool::new()));
        registry.register(Arc::new(ListDirTool::new()));
        registry.register(Arc::new(ExecTool::new()));
        registry
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PluginClass {
    Channel,
    Memory,
    VoiceCall,
    Diffs,
    DevicePair,
    Auth,
    AutomationHelper,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HelpMetadata {
    pub summary: String,
    pub help: String,
    #[serde(default)]
    pub docs_url: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDescriptor {
    pub id: String,
    pub name: String,
    pub class: PluginClass,
    #[serde(default)]
    pub supports_lazy_load: bool,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub docs: HelpMetadata,
    pub config_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelDescriptor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub supports_lazy_load: bool,
    pub docs: HelpMetadata,
    pub config_schema: Value,
}

pub struct PluginRegistry {
    plugins: BTreeMap<String, PluginDescriptor>,
    aliases: HashMap<String, String>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: BTreeMap::new(),
            aliases: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: PluginDescriptor) {
        let id = plugin.id.to_ascii_lowercase();
        self.aliases.insert(id.clone(), id.clone());
        for alias in plugin
            .aliases
            .iter()
            .chain(plugin.docs.aliases.iter())
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            self.aliases.insert(alias, id.clone());
        }
        self.plugins.insert(id, plugin);
    }

    pub fn get(&self, id_or_alias: &str) -> Option<&PluginDescriptor> {
        let key = self.resolve_id(id_or_alias)?;
        self.plugins.get(&key)
    }

    pub fn resolve_id(&self, id_or_alias: &str) -> Option<String> {
        let key = id_or_alias.trim().to_ascii_lowercase();
        self.aliases.get(&key).cloned()
    }

    pub fn list(&self) -> Vec<PluginDescriptor> {
        self.plugins.values().cloned().collect()
    }

    pub fn openclaw_compat() -> Self {
        let mut registry = Self::new();
        for plugin in compat_plugins() {
            registry.register(plugin);
        }
        registry
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ChannelRegistry {
    channels: BTreeMap<String, ChannelDescriptor>,
    aliases: HashMap<String, String>,
}

impl ChannelRegistry {
    pub fn new() -> Self {
        Self {
            channels: BTreeMap::new(),
            aliases: HashMap::new(),
        }
    }

    pub fn register(&mut self, channel: ChannelDescriptor) {
        let id = channel.id.to_ascii_lowercase();
        self.aliases.insert(id.clone(), id.clone());
        for alias in channel
            .aliases
            .iter()
            .chain(channel.docs.aliases.iter())
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty())
        {
            self.aliases.insert(alias, id.clone());
        }
        self.channels.insert(id, channel);
    }

    pub fn get(&self, id_or_alias: &str) -> Option<&ChannelDescriptor> {
        let key = self.resolve_id(id_or_alias)?;
        self.channels.get(&key)
    }

    pub fn resolve_id(&self, id_or_alias: &str) -> Option<String> {
        let key = id_or_alias.trim().to_ascii_lowercase();
        self.aliases.get(&key).cloned()
    }

    pub fn list(&self) -> Vec<ChannelDescriptor> {
        self.channels.values().cloned().collect()
    }

    pub fn openclaw_compat() -> Self {
        let mut registry = Self::new();
        for channel in compat_channels() {
            registry.register(channel);
        }
        registry
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn compat_plugins() -> Vec<PluginDescriptor> {
    vec![
        plugin_descriptor(
            "memory",
            "Memory",
            PluginClass::Memory,
            vec!["memories", "context-memory"],
            vec!["state", "summaries", "budget-aware-context"],
            "Persistent memory hooks for local-first context retention.",
            "Stores compact memory artifacts and summaries without assuming infinite context windows.",
        ),
        plugin_descriptor(
            "voice-call",
            "Voice Call",
            PluginClass::VoiceCall,
            vec!["voice", "call"],
            vec!["audio-io", "realtime", "transport-bridge"],
            "Voice interaction plugin family.",
            "Provides voice-call transport metadata and runtime health for local integrations.",
        ),
        plugin_descriptor(
            "diffs",
            "Diffs",
            PluginClass::Diffs,
            vec!["patches", "git-diffs"],
            vec!["patch-preview", "apply", "review"],
            "Diff and patch helpers.",
            "Exposes diff-aware helpers so UI and agent can track code changes without coupling to the main loop.",
        ),
        plugin_descriptor(
            "device-pair",
            "Device Pair",
            PluginClass::DevicePair,
            vec!["pairing", "device-link"],
            vec!["pairing", "handshake", "qr"],
            "Device pairing helpers.",
            "Tracks local pairing workflows for companion devices and channel bridges.",
        ),
        plugin_descriptor(
            "auth",
            "Auth Plugins",
            PluginClass::Auth,
            vec!["auth-plugins", "identity"],
            vec!["credentials", "token-broker", "local-auth"],
            "Authentication plugin family.",
            "Wraps auth providers and local secret hand-offs without leaking them into the agent loop.",
        ),
        plugin_descriptor(
            "automation-helpers",
            "Automation Helpers",
            PluginClass::AutomationHelper,
            vec!["automation", "helpers"],
            vec!["scheduler", "jobs", "triggers"],
            "Automation support helpers.",
            "Provides metadata and runtime state for local automation integrations.",
        ),
    ]
}

fn compat_channels() -> Vec<ChannelDescriptor> {
    let entries = vec![
        ("telegram", "Telegram", vec!["tg"]),
        ("whatsapp", "WhatsApp", vec!["wa", "whats-app"]),
        ("discord", "Discord", vec![]),
        ("irc", "IRC", vec![]),
        ("googlechat", "Google Chat", vec!["google-chat", "gchat"]),
        ("slack", "Slack", vec![]),
        ("signal", "Signal", vec![]),
        ("imessage", "iMessage", vec!["apple-messages"]),
        ("feishu", "Feishu", vec!["lark"]),
        ("nostr", "Nostr", vec![]),
        ("msteams", "Microsoft Teams", vec!["ms-teams", "teams"]),
        ("mattermost", "Mattermost", vec![]),
        (
            "nextcloud-talk",
            "Nextcloud Talk",
            vec!["nextcloud_talk", "talk"],
        ),
        ("matrix", "Matrix", vec![]),
        ("bluebubbles", "BlueBubbles", vec!["blue-bubbles"]),
        ("line", "LINE", vec![]),
        ("zalo", "Zalo", vec![]),
        ("zalouser", "Zalo User", vec!["zalo-user"]),
        (
            "synology-chat",
            "Synology Chat",
            vec!["synology_chat", "synology"],
        ),
        ("tlon", "Tlon", vec![]),
    ];

    entries
        .into_iter()
        .map(|(id, name, aliases)| channel_descriptor(id, name, aliases))
        .collect()
}

fn plugin_descriptor(
    id: &str,
    name: &str,
    class: PluginClass,
    aliases: Vec<&str>,
    capabilities: Vec<&str>,
    summary: &str,
    help: &str,
) -> PluginDescriptor {
    PluginDescriptor {
        id: id.to_string(),
        name: name.to_string(),
        class,
        supports_lazy_load: true,
        aliases: aliases.into_iter().map(ToString::to_string).collect(),
        capabilities: capabilities.into_iter().map(ToString::to_string).collect(),
        docs: HelpMetadata {
            summary: summary.to_string(),
            help: help.to_string(),
            docs_url: None,
            aliases: Vec::new(),
        },
        config_schema: compat_config_schema(name),
    }
}

fn channel_descriptor(id: &str, name: &str, aliases: Vec<&str>) -> ChannelDescriptor {
    ChannelDescriptor {
        id: id.to_string(),
        name: name.to_string(),
        aliases: aliases.into_iter().map(ToString::to_string).collect(),
        capabilities: vec![
            "send-messages".to_string(),
            "receive-messages".to_string(),
            "health".to_string(),
            "docs".to_string(),
        ],
        supports_lazy_load: true,
        docs: HelpMetadata {
            summary: format!("{name} compatibility channel for OpenClaw-style routing."),
            help: format!(
                "Registers {name} with alias resolution, local-first config, and lazy runtime activation."
            ),
            docs_url: None,
            aliases: Vec::new(),
        },
        config_schema: compat_config_schema(name),
    }
}

fn compat_config_schema(title: &str) -> Value {
    serde_json::json!({
        "title": format!("{title} Config"),
        "type": "object",
        "additionalProperties": true,
        "properties": {
            "endpoint": { "type": "string" },
            "token": { "type": "string" },
            "room": { "type": "string" },
            "enabled": { "type": "boolean" }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlx_agent_tools::{ExecutionMode, ToolContext};
    use std::path::PathBuf;

    fn test_ctx() -> ToolContext {
        ToolContext {
            workspace_root: PathBuf::from("."),
            session_id: "test".into(),
            active_skill: None,
            mode: ExecutionMode::Full,
        }
    }

    #[test]
    fn empty_registry() {
        let reg = ToolRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.get("anything").is_none());
        assert!(reg.definitions().is_empty());
    }

    #[test]
    fn with_builtins_has_five_tools() {
        let reg = ToolRegistry::with_builtins();
        assert_eq!(reg.len(), 5);
        assert!(reg.get("read_file").is_some());
        assert!(reg.get("write_file").is_some());
        assert!(reg.get("edit_file").is_some());
        assert!(reg.get("list_dir").is_some());
        assert!(reg.get("exec").is_some());
    }

    #[tokio::test]
    async fn dispatch_unknown_tool_errors() {
        let reg = ToolRegistry::new();
        let params = serde_json::json!({});
        let result = reg.dispatch("nonexistent", &params, &test_ctx()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("unknown tool"));
    }

    #[tokio::test]
    async fn dispatch_validates_schema_rejects_invalid() {
        let reg = ToolRegistry::with_builtins();
        // read_file requires "path" (string), send a number instead.
        let params = serde_json::json!({"path": 12345});
        let result = reg.dispatch("read_file", &params, &test_ctx()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("validation failed") || err.contains("invalid"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn dispatch_validates_schema_missing_required() {
        let reg = ToolRegistry::with_builtins();
        // write_file requires "path" and "content".
        let params = serde_json::json!({"path": "test.txt"});
        let result = reg.dispatch("write_file", &params, &test_ctx()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("validation failed") || err.contains("content"),
            "got: {err}"
        );
    }

    #[test]
    fn definitions_returns_all_tool_schemas() {
        let reg = ToolRegistry::with_builtins();
        let defs = reg.definitions();
        assert_eq!(defs.len(), 5);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"read_file"));
        assert!(names.contains(&"exec"));
    }

    #[test]
    fn plugin_registry_resolves_aliases() {
        let reg = PluginRegistry::openclaw_compat();
        let plugin = reg.get("context-memory").expect("plugin alias");
        assert_eq!(plugin.id, "memory");
    }

    #[test]
    fn channel_registry_contains_openclaw_catalog() {
        let reg = ChannelRegistry::openclaw_compat();
        assert_eq!(reg.list().len(), 20);
        let channel = reg.get("teams").expect("channel alias");
        assert_eq!(channel.id, "msteams");
    }
}
