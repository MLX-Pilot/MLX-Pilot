use crate::config::{AppConfig, PluginPersistedState};
use mlx_agent_core::{
    HelpMetadata, LazyRuntimeRegistry, PluginClass, PluginDescriptor, PluginRegistry, RuntimeStatus,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
pub struct PluginToggleRequest {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginView {
    pub id: String,
    pub name: String,
    pub class: PluginClass,
    pub aliases: Vec<String>,
    pub capabilities: Vec<String>,
    pub supports_lazy_load: bool,
    pub docs: HelpMetadata,
    pub config_schema: Value,
    pub enabled: bool,
    pub configured: bool,
    pub loaded: bool,
    pub health: mlx_agent_core::RuntimeHealth,
    pub errors: Vec<String>,
    pub config: Value,
}

pub(crate) struct PluginManager {
    settings_path: PathBuf,
    registry: PluginRegistry,
    runtime: Mutex<LazyRuntimeRegistry>,
}

impl PluginManager {
    pub(crate) fn new(settings_path: PathBuf) -> Self {
        Self {
            settings_path,
            registry: PluginRegistry::openclaw_compat(),
            runtime: Mutex::new(LazyRuntimeRegistry::new()),
        }
    }

    pub(crate) async fn list_plugins(&self) -> Vec<PluginView> {
        let cfg = self.load_config();
        let runtime = self.runtime.lock().await;
        self.registry
            .list()
            .into_iter()
            .map(|descriptor| {
                let persisted = cfg.compatibility.plugins.get(&descriptor.id);
                let enabled = persisted.map(|value| value.enabled).unwrap_or(false);
                let configured = persisted
                    .map(|value| value.is_configured())
                    .unwrap_or(false);
                let status = runtime.snapshot(&descriptor.id, enabled, configured);
                build_plugin_view(descriptor, persisted, status)
            })
            .collect()
    }

    pub(crate) async fn set_plugin_enabled(
        &self,
        id_or_alias: &str,
        enabled: bool,
    ) -> Result<PluginView, String> {
        let plugin_id = self
            .registry
            .resolve_id(id_or_alias)
            .ok_or_else(|| format!("unknown plugin '{id_or_alias}'"))?;
        let descriptor = self
            .registry
            .get(&plugin_id)
            .cloned()
            .ok_or_else(|| format!("unknown plugin '{id_or_alias}'"))?;

        let mut cfg = self.load_config();
        let entry = cfg
            .compatibility
            .plugins
            .entry(plugin_id.clone())
            .or_insert_with(PluginPersistedState::default);
        entry.enabled = enabled;
        let persisted = entry.clone();
        self.save_config(&cfg).map_err(|error| error.to_string())?;

        let mut runtime = self.runtime.lock().await;
        runtime.mark_unloaded(&plugin_id);
        if enabled {
            runtime.clear_errors(&plugin_id);
        }
        let status = runtime.snapshot(&plugin_id, persisted.enabled, persisted.is_configured());
        Ok(build_plugin_view(descriptor, Some(&persisted), status))
    }

    pub(crate) fn load_config_from(path: &Path) -> AppConfig {
        AppConfig::load_settings_from(path)
    }

    fn load_config(&self) -> AppConfig {
        Self::load_config_from(&self.settings_path)
    }

    fn save_config(&self, cfg: &AppConfig) -> Result<(), std::io::Error> {
        cfg.save_settings_to(&self.settings_path)
    }
}

fn build_plugin_view(
    descriptor: PluginDescriptor,
    persisted: Option<&PluginPersistedState>,
    status: RuntimeStatus,
) -> PluginView {
    PluginView {
        id: descriptor.id,
        name: descriptor.name,
        class: descriptor.class,
        aliases: descriptor.aliases,
        capabilities: descriptor.capabilities,
        supports_lazy_load: descriptor.supports_lazy_load,
        docs: descriptor.docs,
        config_schema: descriptor.config_schema,
        enabled: status.enabled,
        configured: status.configured,
        loaded: status.loaded,
        health: status.health,
        errors: status.errors,
        config: persisted
            .map(|value| value.config.clone())
            .unwrap_or(Value::Object(Default::default())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn enable_disable_persists_plugin_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let settings_path = dir.path().join("settings.json");
        let manager = PluginManager::new(settings_path.clone());

        let enabled = manager
            .set_plugin_enabled("memory", true)
            .await
            .expect("enable plugin");
        assert!(enabled.enabled);

        let loaded = PluginManager::load_config_from(&settings_path);
        assert!(
            loaded
                .compatibility
                .plugins
                .get("memory")
                .expect("plugin config")
                .enabled
        );

        let disabled = manager
            .set_plugin_enabled("memory", false)
            .await
            .expect("disable plugin");
        assert!(!disabled.enabled);
    }
}
