use std::collections::{BTreeMap, BTreeSet};
use std::io::{ErrorKind, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::fs;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::process::Command;
use tokio::time::{sleep, timeout};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct OpenClawRuntimeConfig {
    pub node_command: String,
    pub cli_path: PathBuf,
    pub state_dir: PathBuf,
    pub gateway_token: String,
    pub session_key: String,
    pub timeout: Duration,
    pub gateway_log: PathBuf,
    pub error_log: PathBuf,
    pub sync_log: PathBuf,
}

#[derive(Debug, Clone)]
pub struct OpenClawRuntime {
    cfg: OpenClawRuntimeConfig,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenClawCloudModel {
    pub reference: String,
    pub provider: String,
    pub model: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenClawCurrentModel {
    pub source: String,
    pub reference: String,
    pub provider: String,
    pub model: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpenClawModelsStateResponse {
    pub session_key: String,
    pub current: OpenClawCurrentModel,
    pub cloud_models: Vec<OpenClawCloudModel>,
}

#[derive(Debug, Deserialize)]
pub struct OpenClawSetModelRequest {
    pub source: String,
    pub model_reference: Option<String>,
    pub local_model_path: Option<String>,
    pub local_model_name: Option<String>,
}

#[derive(Debug)]
struct OpenClawConfigSnapshot {
    hash: String,
    parsed: Value,
}

#[derive(Debug)]
struct OpenClawSessionState {
    key: String,
    model_provider: Option<String>,
    model: Option<String>,
}

impl OpenClawRuntime {
    pub fn new(cfg: OpenClawRuntimeConfig) -> Self {
        Self { cfg }
    }

    pub async fn status(&self) -> OpenClawStatusResponse {
        let mut response = OpenClawStatusResponse {
            available: false,
            cli_path: self.cfg.cli_path.display().to_string(),
            state_dir: self.cfg.state_dir.display().to_string(),
            session_key: self.cfg.session_key.clone(),
            gateway_log: self.cfg.gateway_log.display().to_string(),
            error_log: self.cfg.error_log.display().to_string(),
            sync_log: self.cfg.sync_log.display().to_string(),
            health: None,
            error: None,
        };

        if !self.cfg.cli_path.exists() {
            response.error = Some(format!(
                "openclaw cli nao encontrado em {}",
                self.cfg.cli_path.display()
            ));
            return response;
        }

        let health_timeout = Duration::from_secs(12);
        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "--json".to_string(),
            "health".to_string(),
        ];

        match self.run_command_json(args, health_timeout).await {
            Ok(health) => {
                response.available = true;
                response.health = Some(health);
            }
            Err(error) => {
                response.error = Some(error.to_string());
            }
        }

        response
    }

    pub async fn install(&self) -> Result<String, OpenClawError> {
        let parent_dir = self
            .cfg
            .cli_path
            .parent()
            .ok_or_else(|| OpenClawError::BadRequest("caminho openclaw_cli_path invalido".to_string()))?;

        if !parent_dir.exists() {
            tokio::fs::create_dir_all(parent_dir)
                .await
                .map_err(|e| OpenClawError::Io {
                    context: "falha criando diretorio pai do openclaw".to_string(),
                    source: e.to_string(),
                })?;
        }

        // 1. git clone se nao existir o .git
        let git_dir = parent_dir.join(".git");
        if !git_dir.exists() {
            let output = Command::new("git")
                .arg("clone")
                .arg("https://github.com/kaike/openclaw.git")
                .arg(parent_dir)
                .output()
                .await
                .map_err(|e| OpenClawError::Io {
                    context: "falha rodando git clone".to_string(),
                    source: e.to_string(),
                })?;

            if !output.status.success() {
                return Err(OpenClawError::CommandFailed {
                    command: "git clone".to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                });
            }
        }

        // 2. npm install
        let output = Command::new(&self.cfg.node_command)
            .arg(Self::npm_command_name())
            .arg("install")
            .current_dir(parent_dir)
            .output()
            .await
            .map_err(|e| OpenClawError::Io {
                context: "falha rodando npm install".to_string(),
                source: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(OpenClawError::CommandFailed {
                command: "npm install".to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        Ok("OpenClaw instalado com sucesso!".to_string())
    }

    pub async fn models_state(&self) -> Result<OpenClawModelsStateResponse, OpenClawError> {
        let _ = self.ensure_runtime_compatibility().await;
        let alias_map = self.load_alias_map().await?;
        let current = self.read_current_model(&alias_map).await?;

        let mut cloud_models = self.list_configured_cloud_models(&alias_map).await?;
        if current.source == "cloud"
            && !cloud_models
                .iter()
                .any(|entry| entry.reference == current.reference)
        {
            cloud_models.insert(
                0,
                OpenClawCloudModel {
                    reference: current.reference.clone(),
                    provider: current.provider.clone(),
                    model: current.model.clone(),
                    label: current.label.clone(),
                    alias: alias_map.get(&current.reference).cloned(),
                },
            );
        }

        Ok(OpenClawModelsStateResponse {
            session_key: self.cfg.session_key.clone(),
            current,
            cloud_models,
        })
    }

    pub async fn set_model(
        &self,
        request: OpenClawSetModelRequest,
    ) -> Result<OpenClawCurrentModel, OpenClawError> {
        let _ = self.ensure_runtime_compatibility().await;
        let source = request.source.trim().to_lowercase();

        if source == "cloud" {
            let reference = request
                .model_reference
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    OpenClawError::BadRequest(
                        "model_reference e obrigatorio para source=cloud".to_string(),
                    )
                })?
                .to_string();
            return self.set_cloud_model(reference).await;
        }

        if source == "local" {
            let path = request
                .local_model_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    OpenClawError::BadRequest(
                        "local_model_path e obrigatorio para source=local".to_string(),
                    )
                })?
                .to_string();

            return self.set_local_model(path, request.local_model_name).await;
        }

        Err(OpenClawError::BadRequest(
            "source invalido: use cloud ou local".to_string(),
        ))
    }

    async fn set_cloud_model(
        &self,
        model_reference: String,
    ) -> Result<OpenClawCurrentModel, OpenClawError> {
        self.apply_model_with_retry(&model_reference, 6).await?;
        let alias_map = self.load_alias_map().await?;
        self.read_current_model(&alias_map).await
    }

    async fn set_local_model(
        &self,
        local_model_path: String,
        local_model_name: Option<String>,
    ) -> Result<OpenClawCurrentModel, OpenClawError> {
        self.ensure_local_model_registered(&local_model_path, local_model_name)
            .await?;

        let model_reference = format!("openai/{local_model_path}");
        self.apply_model_with_retry(&model_reference, 12).await?;

        let alias_map = self.load_alias_map().await?;
        self.read_current_model(&alias_map).await
    }

    async fn apply_model_with_retry(
        &self,
        model_reference: &str,
        attempts: usize,
    ) -> Result<(), OpenClawError> {
        self.patch_default_primary_model(model_reference).await?;

        let retries = attempts.max(1);
        let mut last_error = "erro desconhecido".to_string();

        for attempt in 0..retries {
            match self.patch_active_sessions_model(model_reference).await {
                Ok(()) => return Ok(()),
                Err(error) => {
                    last_error = error.to_string();

                    if attempt + 1 >= retries {
                        break;
                    }

                    sleep(Duration::from_millis(750 + (attempt as u64 * 250))).await;
                }
            }
        }

        Err(OpenClawError::Unavailable(format!(
            "nao foi possivel aplicar modelo '{model_reference}' nas sessoes ativas: {last_error}"
        )))
    }

    async fn patch_default_primary_model(
        &self,
        model_reference: &str,
    ) -> Result<(), OpenClawError> {
        let snapshot = self.fetch_config_snapshot().await?;
        let current_primary = snapshot
            .parsed
            .pointer("/agents/defaults/model/primary")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or("");

        if current_primary == model_reference {
            return Ok(());
        }

        let patch = json!({
            "agents": {
                "defaults": {
                    "model": {
                        "primary": model_reference
                    }
                }
            }
        });

        self.apply_config_patch(
            snapshot.hash,
            patch,
            "mlx-pilot model switch",
            200,
            Duration::from_secs(30),
        )
        .await
    }

    async fn patch_active_sessions_model(
        &self,
        model_reference: &str,
    ) -> Result<(), OpenClawError> {
        let sessions = self.list_sessions(800).await?;
        let mut target_keys = BTreeSet::new();

        if sessions.is_empty() {
            target_keys.insert(self.cfg.session_key.clone());
        } else {
            for session in sessions {
                if !session_matches_target_model(&session, model_reference) {
                    target_keys.insert(session.key);
                }
            }

            target_keys.insert(self.cfg.session_key.clone());
        }

        let mut errors = Vec::new();
        for session_key in target_keys {
            if let Err(error) = self
                .patch_single_session_model(&session_key, model_reference)
                .await
            {
                let details = error.to_string();
                if looks_missing_session_error(&details) {
                    continue;
                }
                errors.push(format!("{session_key}: {details}"));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(OpenClawError::Unavailable(format!(
                "falhas em sessions.patch: {}",
                errors.join(" | ")
            )))
        }
    }

    async fn list_sessions(
        &self,
        limit: usize,
    ) -> Result<Vec<OpenClawSessionState>, OpenClawError> {
        let params = json!({
            "limit": limit.clamp(50, 2000),
        });
        let params_json = serde_json::to_string(&params).map_err(|error| OpenClawError::Parse {
            details: format!("falha serializando params sessions.list: {error}"),
        })?;

        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "sessions.list".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            "12000".to_string(),
            "--params".to_string(),
            params_json,
        ];

        let response = self.run_command_json(args, Duration::from_secs(18)).await?;
        let mut sessions = Vec::new();

        if let Some(entries) = response.pointer("/sessions").and_then(Value::as_array) {
            for entry in entries {
                let Some(key) = entry
                    .get("key")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };

                let model_provider = entry
                    .get("modelProvider")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);

                let model = entry
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);

                sessions.push(OpenClawSessionState {
                    key: key.to_string(),
                    model_provider,
                    model,
                });
            }
        }

        Ok(sessions)
    }

    async fn patch_single_session_model(
        &self,
        session_key: &str,
        model_reference: &str,
    ) -> Result<(), OpenClawError> {
        let params = json!({
            "key": session_key,
            "model": model_reference,
        });
        let params_json = serde_json::to_string(&params).map_err(|error| OpenClawError::Parse {
            details: format!("falha serializando params sessions.patch: {error}"),
        })?;

        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "sessions.patch".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            "12000".to_string(),
            "--params".to_string(),
            params_json,
        ];

        let response = self.run_command_json(args, Duration::from_secs(18)).await?;
        if let Some(false) = response.get("ok").and_then(Value::as_bool) {
            let reason = get_string(&response, "/error/message")
                .or_else(|| get_string(&response, "/error"))
                .unwrap_or_else(|| "gateway recusou sessions.patch".to_string());
            return Err(OpenClawError::BadRequest(format!(
                "gateway recusou sessions.patch para '{session_key}': {reason}"
            )));
        }

        Ok(())
    }

    async fn ensure_local_model_registered(
        &self,
        local_model_path: &str,
        local_model_name: Option<String>,
    ) -> Result<(), OpenClawError> {
        let snapshot = self.fetch_config_snapshot().await?;
        let mut patch_needed = false;

        if snapshot
            .parsed
            .pointer("/models/providers/openai")
            .and_then(Value::as_object)
            .is_none()
        {
            return Err(OpenClawError::BadRequest(
                "OpenClaw nao possui provider openai configurado para modelos locais".to_string(),
            ));
        }

        let mut openai_models = snapshot
            .parsed
            .pointer("/models/providers/openai/models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let exists = openai_models.iter().any(|entry| {
            entry
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .is_some_and(|value| value == local_model_path)
        });

        if !exists {
            patch_needed = true;
            openai_models.push(local_model_catalog_entry(
                local_model_path,
                local_model_name.clone(),
            ));
        }

        let model_reference = format!("openai/{local_model_path}");
        let mut defaults_models = snapshot
            .parsed
            .pointer("/agents/defaults/models")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(Map::new);

        if !defaults_models.contains_key(&model_reference) {
            patch_needed = true;
            let alias = local_model_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| format!("Local MLX {}", file_label(local_model_path)));

            defaults_models.insert(model_reference.clone(), json!({ "alias": alias }));
        }

        if !patch_needed {
            return Ok(());
        }

        let patch = json!({
            "models": {
                "providers": {
                    "openai": {
                        "models": openai_models,
                    }
                }
            },
            "agents": {
                "defaults": {
                    "models": defaults_models,
                }
            }
        });

        self.apply_config_patch(
            snapshot.hash,
            patch,
            "mlx-pilot local model sync",
            300,
            Duration::from_secs(35),
        )
        .await
    }

    async fn ensure_runtime_compatibility(&self) -> Result<(), OpenClawError> {
        let snapshot = self.fetch_config_snapshot().await?;
        let mut root_patch = Map::<String, Value>::new();

        let state_dir = self.cfg.state_dir.display().to_string();
        let skills_dir = format!("{state_dir}/workspace/skills");

        let path_prepend_original = snapshot
            .parsed
            .pointer("/tools/exec/pathPrepend")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        if !path_prepend_original.is_empty() {
            let normalized = path_prepend_original
                .iter()
                .map(|value| normalize_host_path(value, &state_dir, &skills_dir))
                .collect::<Vec<_>>();

            if normalized != path_prepend_original {
                root_patch.insert(
                    "tools".to_string(),
                    json!({ "exec": { "pathPrepend": normalized } }),
                );
            }
        }

        let extra_dirs_original = snapshot
            .parsed
            .pointer("/skills/load/extraDirs")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let mut skills_patch = Map::<String, Value>::new();
        if !extra_dirs_original.is_empty() {
            let normalized = extra_dirs_original
                .iter()
                .map(|value| normalize_host_path(value, &state_dir, &skills_dir))
                .collect::<Vec<_>>();
            if normalized != extra_dirs_original {
                skills_patch.insert("load".to_string(), json!({ "extraDirs": normalized }));
            }
        }

        let mut sherpa_env_patch = Map::<String, Value>::new();
        let runtime_dir_original = snapshot
            .parsed
            .pointer("/skills/entries/sherpa-onnx-tts/env/SHERPA_ONNX_RUNTIME_DIR")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(value) = runtime_dir_original.as_deref() {
            let normalized = normalize_host_path(value, &state_dir, &skills_dir);
            if normalized != value {
                sherpa_env_patch.insert(
                    "SHERPA_ONNX_RUNTIME_DIR".to_string(),
                    Value::String(normalized),
                );
            }
        }

        let model_dir_original = snapshot
            .parsed
            .pointer("/skills/entries/sherpa-onnx-tts/env/SHERPA_ONNX_MODEL_DIR")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(value) = model_dir_original.as_deref() {
            let normalized = normalize_host_path(value, &state_dir, &skills_dir);
            if normalized != value {
                sherpa_env_patch.insert(
                    "SHERPA_ONNX_MODEL_DIR".to_string(),
                    Value::String(normalized),
                );
            }
        }

        if !sherpa_env_patch.is_empty() {
            skills_patch.insert(
                "entries".to_string(),
                json!({
                    "sherpa-onnx-tts": {
                        "env": sherpa_env_patch
                    }
                }),
            );
        }

        if !skills_patch.is_empty() {
            root_patch.insert("skills".to_string(), Value::Object(skills_patch));
        }

        if let Some(base_url) = snapshot
            .parsed
            .pointer("/models/providers/openai/baseUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let normalized_url = normalize_openai_base_url(base_url);
            if normalized_url != base_url {
                root_patch.insert(
                    "models".to_string(),
                    json!({
                        "providers": {
                            "openai": {
                                "baseUrl": normalized_url
                            }
                        }
                    }),
                );
            }
        }

        if root_patch.is_empty() {
            return Ok(());
        }

        self.apply_config_patch(
            snapshot.hash,
            Value::Object(root_patch),
            "mlx-pilot runtime compatibility repair",
            250,
            Duration::from_secs(30),
        )
        .await
    }

    async fn apply_config_patch(
        &self,
        base_hash: String,
        patch: Value,
        note: &str,
        restart_delay_ms: u64,
        timeout_limit: Duration,
    ) -> Result<(), OpenClawError> {
        let raw_patch = serde_json::to_string(&patch).map_err(|error| OpenClawError::Parse {
            details: format!("falha serializando config.patch: {error}"),
        })?;

        let params = json!({
            "raw": raw_patch,
            "baseHash": base_hash,
            "sessionKey": self.cfg.session_key,
            "note": note,
            "restartDelayMs": restart_delay_ms,
        });

        let params_json = serde_json::to_string(&params).map_err(|error| OpenClawError::Parse {
            details: format!("falha serializando params config.patch: {error}"),
        })?;

        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "config.patch".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            "25000".to_string(),
            "--params".to_string(),
            params_json,
        ];

        self.run_command_json(args, timeout_limit).await?;
        self.wait_for_gateway(Duration::from_secs(20)).await
    }

    async fn wait_for_gateway(&self, total_timeout: Duration) -> Result<(), OpenClawError> {
        let attempts = (total_timeout.as_millis() / 1000).max(1) as usize;
        let mut last_error = "gateway indisponivel".to_string();

        for _ in 0..attempts {
            let args = vec![
                "gateway".to_string(),
                "call".to_string(),
                "--json".to_string(),
                "health".to_string(),
            ];

            match self.run_command_json(args, Duration::from_secs(6)).await {
                Ok(_) => return Ok(()),
                Err(error) => {
                    last_error = error.to_string();
                    sleep(Duration::from_millis(900)).await;
                }
            }
        }

        Err(OpenClawError::Unavailable(format!(
            "gateway nao voltou apos config.patch: {last_error}"
        )))
    }

    async fn fetch_config_snapshot(&self) -> Result<OpenClawConfigSnapshot, OpenClawError> {
        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "config.get".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            "18000".to_string(),
        ];

        let response = self.run_command_json(args, Duration::from_secs(24)).await?;
        let hash = get_string(&response, "/hash").ok_or_else(|| OpenClawError::Parse {
            details: "config.get sem hash".to_string(),
        })?;

        let parsed = if let Some(value) = response.pointer("/parsed") {
            value.clone()
        } else if let Some(raw) = response.pointer("/raw").and_then(Value::as_str) {
            serde_json::from_str::<Value>(raw).map_err(|error| OpenClawError::Parse {
                details: format!("falha parseando config.raw: {error}"),
            })?
        } else {
            Value::Null
        };

        if !parsed.is_object() {
            return Err(OpenClawError::Parse {
                details: "config.get sem objeto parsed".to_string(),
            });
        }

        Ok(OpenClawConfigSnapshot { hash, parsed })
    }

    async fn load_alias_map(&self) -> Result<BTreeMap<String, String>, OpenClawError> {
        let snapshot = self.fetch_config_snapshot().await?;
        let mut aliases = BTreeMap::new();

        collect_model_aliases(
            snapshot
                .parsed
                .pointer("/agents/defaults/models")
                .and_then(Value::as_object),
            &mut aliases,
        );

        if let Some(agents) = snapshot
            .parsed
            .pointer("/agents")
            .and_then(Value::as_object)
        {
            for (agent_id, agent_value) in agents {
                if agent_id == "defaults" {
                    continue;
                }
                collect_model_aliases(
                    agent_value.get("models").and_then(Value::as_object),
                    &mut aliases,
                );
            }
        }
        Ok(aliases)
    }

fn npm_command_name() -> &'static str {
    if cfg!(windows) {
        "npm.cmd"
    } else {
        "npm"
    }
}
    async fn list_configured_cloud_models(
        &self,
        alias_map: &BTreeMap<String, String>,
    ) -> Result<Vec<OpenClawCloudModel>, OpenClawError> {
        let snapshot = self.fetch_config_snapshot().await?;
        let mut refs = BTreeSet::new();

        if let Some(primary) = snapshot
            .parsed
            .pointer("/agents/defaults/model/primary")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            refs.insert(primary.to_string());
        }

        if let Some(defaults_models) = snapshot
            .parsed
            .pointer("/agents/defaults/models")
            .and_then(Value::as_object)
        {
            refs.extend(defaults_models.keys().cloned());
        }

        if let Some(agents) = snapshot
            .parsed
            .pointer("/agents")
            .and_then(Value::as_object)
        {
            for (agent_id, agent_value) in agents {
                if agent_id == "defaults" {
                    continue;
                }

                if let Some(primary) = agent_value
                    .pointer("/model/primary")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                {
                    refs.insert(primary.to_string());
                }

                if let Some(models) = agent_value.get("models").and_then(Value::as_object) {
                    refs.extend(models.keys().cloned());
                }
            }
        }

        let mut entries = refs
            .into_iter()
            .filter(|reference| !looks_local_model_ref(reference))
            .filter_map(|reference| {
                parse_model_reference(&reference).map(|(provider, model)| {
                    let alias = alias_map.get(&reference).cloned();
                    let label = alias.clone().unwrap_or_else(|| reference.clone());
                    OpenClawCloudModel {
                        reference,
                        provider,
                        model,
                        label,
                        alias,
                    }
                })
            })
            .collect::<Vec<_>>();

        entries.sort_by(|left, right| {
            let by_provider = left.provider.cmp(&right.provider);
            if by_provider.is_eq() {
                return left.label.cmp(&right.label);
            }
            by_provider
        });

        Ok(entries)
    }

    async fn read_current_model(
        &self,
        alias_map: &BTreeMap<String, String>,
    ) -> Result<OpenClawCurrentModel, OpenClawError> {
        let params = json!({ "limit": 400 });
        let params_json = serde_json::to_string(&params).map_err(|error| OpenClawError::Parse {
            details: format!("falha serializando params sessions.list: {error}"),
        })?;

        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "sessions.list".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            "12000".to_string(),
            "--params".to_string(),
            params_json,
        ];

        let response = self.run_command_json(args, Duration::from_secs(18)).await?;

        let default_provider = get_string(&response, "/defaults/modelProvider")
            .unwrap_or_else(|| "deepseek".to_string());
        let default_model =
            get_string(&response, "/defaults/model").unwrap_or_else(|| "deepseek-chat".to_string());

        let mut provider = default_provider.clone();
        let mut model = default_model.clone();

        if let Some(sessions) = response.pointer("/sessions").and_then(Value::as_array) {
            for entry in sessions {
                if entry
                    .get("key")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == self.cfg.session_key)
                {
                    if let Some(current_provider) = entry
                        .get("modelProvider")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        provider = current_provider.to_string();
                    }

                    if let Some(current_model) = entry
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        model = current_model.to_string();
                    }

                    break;
                }
            }
        }

        let reference = format!("{provider}/{model}");
        let source = if looks_local_model_path(&model) {
            "local".to_string()
        } else {
            "cloud".to_string()
        };
        let label = alias_map
            .get(&reference)
            .cloned()
            .unwrap_or_else(|| reference.clone());

        Ok(OpenClawCurrentModel {
            source,
            reference,
            provider,
            model,
            label,
        })
    }

    pub async fn read_logs(
        &self,
        query: OpenClawLogQuery,
    ) -> Result<OpenClawLogChunkResponse, OpenClawError> {
        let stream = query
            .stream
            .unwrap_or_else(|| "gateway".to_string())
            .to_lowercase();
        let path = self.resolve_log_path(&stream)?;
        let requested_cursor = query.cursor.unwrap_or(0);
        let max_bytes = query.max_bytes.unwrap_or(65536).clamp(1024, 262144);

        let metadata = match fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(OpenClawLogChunkResponse {
                    stream,
                    path: path.display().to_string(),
                    exists: false,
                    cursor: requested_cursor,
                    next_cursor: requested_cursor,
                    file_size: 0,
                    truncated: false,
                    content: String::new(),
                });
            }
            Err(error) => {
                return Err(OpenClawError::Io {
                    context: format!("falha ao acessar {}", path.display()),
                    source: error.to_string(),
                });
            }
        };

        let file_size = metadata.len();
        let truncated = requested_cursor > file_size;
        let cursor = if truncated {
            0
        } else {
            requested_cursor.min(file_size)
        };
        let bytes_to_read = (file_size.saturating_sub(cursor) as usize).min(max_bytes);

        if bytes_to_read == 0 {
            return Ok(OpenClawLogChunkResponse {
                stream,
                path: path.display().to_string(),
                exists: true,
                cursor,
                next_cursor: cursor,
                file_size,
                truncated,
                content: String::new(),
            });
        }

        let mut file = fs::File::open(&path)
            .await
            .map_err(|error| OpenClawError::Io {
                context: format!("falha ao abrir {}", path.display()),
                source: error.to_string(),
            })?;
        file.seek(SeekFrom::Start(cursor))
            .await
            .map_err(|error| OpenClawError::Io {
                context: format!("falha ao buscar offset em {}", path.display()),
                source: error.to_string(),
            })?;

        let mut buffer = vec![0_u8; bytes_to_read];
        file.read_exact(&mut buffer)
            .await
            .map_err(|error| OpenClawError::Io {
                context: format!("falha lendo {}", path.display()),
                source: error.to_string(),
            })?;

        let content = String::from_utf8_lossy(&buffer).to_string();
        let next_cursor = cursor + bytes_to_read as u64;

        Ok(OpenClawLogChunkResponse {
            stream,
            path: path.display().to_string(),
            exists: true,
            cursor,
            next_cursor,
            file_size,
            truncated,
            content,
        })
    }

    pub async fn chat(
        &self,
        request: OpenClawChatRequest,
    ) -> Result<OpenClawChatResponse, OpenClawError> {
        let _ = self.ensure_runtime_compatibility().await;
        let message = request.message.trim();
        if message.is_empty() {
            return Err(OpenClawError::BadRequest(
                "message nao pode ser vazio".to_string(),
            ));
        }

        let idempotency_key = request
            .idempotency_key
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(generate_idempotency_key);

        let session_key = request
            .session_key
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| self.cfg.session_key.clone());

        let timeout_ms = request
            .timeout_ms
            .unwrap_or(self.cfg.timeout.as_millis() as u64)
            .clamp(1000, 900000);
        let timeout_limit = Duration::from_millis(timeout_ms + 2000);

        let params = json!({
            "message": message,
            "idempotencyKey": idempotency_key,
            "sessionKey": session_key,
        });
        let params_json = serde_json::to_string(&params).map_err(|error| OpenClawError::Parse {
            details: format!("falha serializando params: {error}"),
        })?;

        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "agent".to_string(),
            "--expect-final".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            timeout_ms.to_string(),
            "--params".to_string(),
            params_json,
        ];

        let response = self.run_command_json(args, timeout_limit).await?;
        Ok(normalize_chat_response(response))
    }

    pub async fn observability(&self) -> Result<OpenClawObservabilityResponse, OpenClawError> {
        let _ = self.ensure_runtime_compatibility().await;
        let history = self.read_recent_history(120).await.ok();
        let skills_status = self.read_skills_status().await.ok();

        if history.is_none() && skills_status.is_none() {
            return Err(OpenClawError::Unavailable(
                "nao foi possivel carregar observabilidade do openclaw".to_string(),
            ));
        }

        let mut provider = None;
        let mut model = None;
        let mut usage = None;
        let mut updated_at = None;
        let mut tools = Vec::new();
        let mut skills = Vec::new();

        if let Some(history_value) = history.as_ref() {
            let latest = latest_assistant_message(history_value);
            if let Some(assistant) = latest {
                provider = assistant
                    .get("provider")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);
                model = assistant
                    .get("model")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string);
                usage = assistant.get("usage").and_then(build_usage);
                updated_at = assistant.get("timestamp").and_then(Value::as_u64);
            }

            tools = extract_tools_from_history(history_value);
        }

        if let Some(skills_value) = skills_status.as_ref() {
            skills = extract_eligible_skills(skills_value);
        }

        let mut config_snapshot = None;
        if skills.is_empty() || tools.is_empty() {
            config_snapshot = self.fetch_config_snapshot().await.ok();
        }

        if skills.is_empty() {
            if let Some(snapshot) = config_snapshot.as_ref() {
                skills = extract_skills_from_config(&snapshot.parsed);
            }
        }

        if tools.is_empty() {
            if let Some(snapshot) = config_snapshot.as_ref() {
                tools = extract_tools_from_config(&snapshot.parsed);
            }
        }

        if tools.is_empty() {
            tools.push("exec".to_string());
        }
        if skills.is_empty() {
            skills.push("default".to_string());
        }

        Ok(OpenClawObservabilityResponse {
            session_key: self.cfg.session_key.clone(),
            provider,
            model,
            usage,
            skills,
            tools,
            updated_at,
        })
    }

    pub async fn runtime_status(&self) -> Result<OpenClawRuntimeStateResponse, OpenClawError> {
        let raw = self.read_gateway_status().await?;
        Ok(parse_runtime_status(&raw))
    }

    pub async fn runtime_action(
        &self,
        request: OpenClawRuntimeActionRequest,
    ) -> Result<OpenClawRuntimeActionResponse, OpenClawError> {
        let action = request.action.trim().to_lowercase();
        let (args, timeout_limit) = match action.as_str() {
            "start" => (
                vec![
                    "gateway".to_string(),
                    "start".to_string(),
                    "--json".to_string(),
                ],
                Duration::from_secs(45),
            ),
            "stop" => (
                vec![
                    "gateway".to_string(),
                    "stop".to_string(),
                    "--json".to_string(),
                ],
                Duration::from_secs(35),
            ),
            "restart" => (
                vec![
                    "gateway".to_string(),
                    "restart".to_string(),
                    "--json".to_string(),
                ],
                Duration::from_secs(45),
            ),
            _ => {
                return Err(OpenClawError::BadRequest(
                    "acao invalida: use start, stop ou restart".to_string(),
                ));
            }
        };

        self.run_command_json(args, timeout_limit).await?;
        let runtime = self.runtime_status().await?;

        Ok(OpenClawRuntimeActionResponse { action, runtime })
    }

    async fn read_recent_history(&self, limit: usize) -> Result<Value, OpenClawError> {
        let params = json!({
            "sessionKey": self.cfg.session_key,
            "limit": limit.clamp(10, 500),
        });
        let params_json = serde_json::to_string(&params).map_err(|error| OpenClawError::Parse {
            details: format!("falha serializando params chat.history: {error}"),
        })?;

        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "chat.history".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            "15000".to_string(),
            "--params".to_string(),
            params_json,
        ];

        self.run_command_json(args, Duration::from_secs(20)).await
    }

    async fn read_skills_status(&self) -> Result<Value, OpenClawError> {
        let args = vec![
            "gateway".to_string(),
            "call".to_string(),
            "skills.status".to_string(),
            "--json".to_string(),
            "--timeout".to_string(),
            "15000".to_string(),
        ];

        self.run_command_json(args, Duration::from_secs(20)).await
    }

    async fn read_gateway_status(&self) -> Result<Value, OpenClawError> {
        let args = vec![
            "gateway".to_string(),
            "status".to_string(),
            "--json".to_string(),
        ];

        self.run_command_json(args, Duration::from_secs(25)).await
    }

    fn resolve_log_path(&self, stream: &str) -> Result<PathBuf, OpenClawError> {
        let path = match stream {
            "gateway" => self.cfg.gateway_log.clone(),
            "error" => self.cfg.error_log.clone(),
            "sync" => self.cfg.sync_log.clone(),
            _ => {
                return Err(OpenClawError::BadRequest(
                    "stream invalido: use gateway, error ou sync".to_string(),
                ));
            }
        };

        Ok(path)
    }

    async fn run_command_json(
        &self,
        args: Vec<String>,
        timeout_limit: Duration,
    ) -> Result<Value, OpenClawError> {
        let mut command = Command::new(&self.cfg.node_command);
        command
            .arg(&self.cfg.cli_path)
            .args(&args)
            .env("OPENCLAW_STATE_DIR", &self.cfg.state_dir)
            .env("OPENCLAW_GATEWAY_TOKEN", &self.cfg.gateway_token)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let command_preview = format!(
            "{} {} {}",
            self.cfg.node_command,
            self.cfg.cli_path.display(),
            args.join(" ")
        );

        let output = timeout(timeout_limit, command.output())
            .await
            .map_err(|_| OpenClawError::Timeout {
                seconds: timeout_limit.as_secs().max(1),
            })?
            .map_err(|error| OpenClawError::Io {
                context: format!("falha executando {command_preview}"),
                source: error.to_string(),
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

        if !output.status.success() {
            return Err(OpenClawError::CommandFailed {
                command: command_preview,
                stderr: if stderr.is_empty() {
                    "sem stderr".to_string()
                } else {
                    stderr
                },
            });
        }

        parse_json_output(&stdout).map_err(|details| OpenClawError::Parse {
            details: format!("{details}; stderr: {stderr}"),
        })
    }
}

#[derive(Debug)]
pub enum OpenClawError {
    BadRequest(String),
    Io { context: String, source: String },
    CommandFailed { command: String, stderr: String },
    Parse { details: String },
    Timeout { seconds: u64 },
    Unavailable(String),
}

impl std::fmt::Display for OpenClawError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenClawError::BadRequest(message) => write!(formatter, "{message}"),
            OpenClawError::Io { context, source } => write!(formatter, "{context}: {source}"),
            OpenClawError::CommandFailed { command, stderr } => {
                write!(formatter, "comando falhou ({command}): {stderr}")
            }
            OpenClawError::Parse { details } => write!(formatter, "{details}"),
            OpenClawError::Timeout { seconds } => {
                write!(formatter, "operacao expirou apos {seconds}s")
            }
            OpenClawError::Unavailable(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for OpenClawError {}

#[derive(Debug, Deserialize)]
pub struct OpenClawLogQuery {
    pub stream: Option<String>,
    pub cursor: Option<u64>,
    pub max_bytes: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct OpenClawLogChunkResponse {
    pub stream: String,
    pub path: String,
    pub exists: bool,
    pub cursor: u64,
    pub next_cursor: u64,
    pub file_size: u64,
    pub truncated: bool,
    pub content: String,
}

#[derive(Debug, Serialize)]
pub struct OpenClawStatusResponse {
    pub available: bool,
    pub cli_path: String,
    pub state_dir: String,
    pub session_key: String,
    pub gateway_log: String,
    pub error_log: String,
    pub sync_log: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub health: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OpenClawChatRequest {
    pub message: String,
    pub session_key: Option<String>,
    pub idempotency_key: Option<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct OpenClawChatResponse {
    pub run_id: Option<String>,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub reply: String,
    pub payloads: Vec<String>,
    pub duration_ms: Option<u64>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: Option<OpenClawUsage>,
    pub skills: Vec<String>,
    pub tools: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct OpenClawUsage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct OpenClawObservabilityResponse {
    pub session_key: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub usage: Option<OpenClawUsage>,
    pub skills: Vec<String>,
    pub tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct OpenClawRuntimeStateResponse {
    pub service_status: String,
    pub service_state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u64>,
    pub rpc_ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port_status: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct OpenClawRuntimeActionRequest {
    pub action: String,
}

#[derive(Debug, Serialize)]
pub struct OpenClawRuntimeActionResponse {
    pub action: String,
    pub runtime: OpenClawRuntimeStateResponse,
}

fn latest_assistant_message(root: &Value) -> Option<&Value> {
    root.pointer("/messages")
        .and_then(Value::as_array)
        .and_then(|messages| {
            messages
                .iter()
                .rev()
                .find(|entry| entry.get("role").and_then(Value::as_str) == Some("assistant"))
        })
}

fn extract_tools_from_history(root: &Value) -> Vec<String> {
    let mut tools = Vec::new();

    if let Some(messages) = root.pointer("/messages").and_then(Value::as_array) {
        for message in messages.iter().rev() {
            if let Some(name) = message
                .get("toolName")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                push_unique_name(&mut tools, name);
            }

            if let Some(contents) = message.get("content").and_then(Value::as_array) {
                for content in contents {
                    if let Some(name) = content
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                    {
                        push_unique_name(&mut tools, name);
                    }
                }
            }
        }
    }

    tools
}

fn extract_eligible_skills(root: &Value) -> Vec<String> {
    let mut skills = Vec::new();

    if let Some(entries) = root.pointer("/skills").and_then(Value::as_array) {
        for entry in entries {
            let eligible = entry
                .get("eligible")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let disabled = entry
                .get("disabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);

            if !eligible || disabled {
                continue;
            }

            if let Some(name) = entry
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                push_unique_name(&mut skills, name);
            }
        }
    }

    skills
}

fn extract_skills_from_config(root: &Value) -> Vec<String> {
    let mut skills = Vec::new();

    if let Some(entries) = root.pointer("/skills/entries").and_then(Value::as_object) {
        for name in entries.keys() {
            let normalized = name.trim();
            if !normalized.is_empty() {
                push_unique_name(&mut skills, normalized);
            }
        }
    }

    skills
}

fn extract_tools_from_config(root: &Value) -> Vec<String> {
    let mut tools = Vec::new();
    let ignored = ["allow", "deny", "profile", "elevated"];

    if let Some(entries) = root.pointer("/tools").and_then(Value::as_object) {
        for name in entries.keys() {
            let normalized = name.trim();
            if normalized.is_empty() || ignored.iter().any(|value| value == &normalized) {
                continue;
            }
            push_unique_name(&mut tools, normalized);
        }
    }

    tools
}

fn parse_runtime_status(root: &Value) -> OpenClawRuntimeStateResponse {
    let service_status =
        get_string(root, "/service/runtime/status").unwrap_or_else(|| "unknown".to_string());
    let service_state =
        get_string(root, "/service/runtime/state").unwrap_or_else(|| "unknown".to_string());
    let pid = get_u64(root, "/service/runtime/pid");
    let rpc_ok = root
        .pointer("/rpc/ok")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let port_status = get_string(root, "/port/status");
    let issues = root
        .pointer("/service/configAudit/issues")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("message").and_then(Value::as_str))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    OpenClawRuntimeStateResponse {
        service_status,
        service_state,
        pid,
        rpc_ok,
        port_status,
        issues,
    }
}

fn push_unique_name(values: &mut Vec<String>, candidate: &str) {
    if values.len() >= 48 {
        return;
    }

    let normalized = candidate.trim();
    if normalized.is_empty() {
        return;
    }

    if !values.iter().any(|value| value == normalized) {
        values.push(normalized.to_string());
    }
}

fn local_model_catalog_entry(local_model_path: &str, local_model_name: Option<String>) -> Value {
    let name = local_model_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| local_model_path.to_string());

    json!({
        "id": local_model_path,
        "name": name,
        "reasoning": false,
        "input": ["text"],
        "cost": {
            "input": 0,
            "output": 0,
            "cacheRead": 0,
            "cacheWrite": 0
        },
        "contextWindow": 200000,
        "maxTokens": 8192,
    })
}

fn parse_model_reference(reference: &str) -> Option<(String, String)> {
    let trimmed = reference.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (provider, model) = trimmed.split_once('/')?;
    let provider = provider.trim();
    let model = model.trim();
    if provider.is_empty() || model.is_empty() {
        return None;
    }

    Some((provider.to_string(), model.to_string()))
}

fn session_matches_target_model(session: &OpenClawSessionState, model_reference: &str) -> bool {
    let Some((target_provider, target_model)) = parse_model_reference(model_reference) else {
        return false;
    };

    session
        .model_provider
        .as_deref()
        .map(str::trim)
        .is_some_and(|provider| provider == target_provider)
        && session
            .model
            .as_deref()
            .map(str::trim)
            .is_some_and(|model| model == target_model)
}

fn looks_missing_session_error(details: &str) -> bool {
    let normalized = details.to_lowercase();
    normalized.contains("session not found")
        || normalized.contains("unknown session")
        || normalized.contains("nao encontrado")
}

fn looks_local_model_path(model: &str) -> bool {
    let trimmed = model.trim();
    trimmed.starts_with('/') || trimmed.contains("/Users/") || trimmed.contains('\\')
}

fn looks_local_model_ref(reference: &str) -> bool {
    parse_model_reference(reference)
        .map(|(_, model)| looks_local_model_path(&model))
        .unwrap_or(false)
}

fn collect_model_aliases(input: Option<&Map<String, Value>>, out: &mut BTreeMap<String, String>) {
    let Some(entries) = input else {
        return;
    };

    for (reference, value) in entries {
        let alias = value
            .get("alias")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|entry| !entry.is_empty());

        if let Some(alias) = alias {
            out.insert(reference.to_string(), alias.to_string());
        }
    }
}

fn file_label(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string())
}

fn normalize_openai_base_url(value: &str) -> String {
    value.replace("host.docker.internal", "127.0.0.1")
}

fn normalize_host_path(value: &str, state_dir: &str, skills_dir: &str) -> String {
    let mut normalized = value.replace("/home/node/.openclaw/workspace/skills", skills_dir);
    normalized = normalized.replace("/home/node/.openclaw", state_dir);
    normalized = normalized.replace("/app/skills", skills_dir);
    normalized
}

fn generate_idempotency_key() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("mlx-pilot-openclaw-{millis}-{counter}")
}

fn parse_json_output(raw: &str) -> Result<Value, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("stdout vazio".to_string());
    }

    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }

    if let Some(extracted) = extract_last_json_object(trimmed) {
        if let Ok(value) = serde_json::from_str::<Value>(extracted) {
            return Ok(value);
        }
    }

    Err("retorno nao e JSON valido".to_string())
}

fn extract_last_json_object(raw: &str) -> Option<&str> {
    for (index, ch) in raw.char_indices().rev() {
        if ch != '{' {
            continue;
        }
        let candidate = raw.get(index..)?;
        if serde_json::from_str::<Value>(candidate).is_ok() {
            return Some(candidate);
        }
    }

    None
}

fn normalize_chat_response(raw: Value) -> OpenClawChatResponse {
    let run_id = get_string(&raw, "/runId");
    let status = get_string(&raw, "/status");
    let summary = get_string(&raw, "/summary");
    let duration_ms = get_u64(&raw, "/result/meta/durationMs");
    let provider = get_string(&raw, "/result/meta/agentMeta/provider")
        .or_else(|| get_string(&raw, "/result/meta/systemPromptReport/provider"));
    let model = get_string(&raw, "/result/meta/agentMeta/model")
        .or_else(|| get_string(&raw, "/result/meta/systemPromptReport/model"));

    let payloads = raw
        .pointer("/result/payloads")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.get("text").and_then(Value::as_str))
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let reply = if payloads.is_empty() {
        String::new()
    } else {
        payloads.join("\n\n")
    };

    let usage = build_usage(
        raw.pointer("/result/meta/agentMeta/usage")
            .unwrap_or(&Value::Null),
    );

    let skills = list_entry_names(&raw, "/result/meta/systemPromptReport/skills/entries");
    let tools = list_entry_names(&raw, "/result/meta/systemPromptReport/tools/entries");

    OpenClawChatResponse {
        run_id,
        status,
        summary,
        reply,
        payloads,
        duration_ms,
        provider,
        model,
        usage,
        skills,
        tools,
    }
}

fn build_usage(value: &Value) -> Option<OpenClawUsage> {
    if !value.is_object() {
        return None;
    }

    let usage = OpenClawUsage {
        input: value.get("input").and_then(Value::as_u64),
        output: value.get("output").and_then(Value::as_u64),
        cache_read: value.get("cacheRead").and_then(Value::as_u64),
        cache_write: value.get("cacheWrite").and_then(Value::as_u64),
        total: value
            .get("total")
            .and_then(Value::as_u64)
            .or_else(|| value.get("totalTokens").and_then(Value::as_u64)),
    };

    if usage.input.is_none()
        && usage.output.is_none()
        && usage.cache_read.is_none()
        && usage.cache_write.is_none()
        && usage.total.is_none()
    {
        return None;
    }

    Some(usage)
}

fn list_entry_names(root: &Value, pointer: &str) -> Vec<String> {
    let mut names = Vec::new();

    if let Some(entries) = root.pointer(pointer).and_then(Value::as_array) {
        for entry in entries {
            if let Some(name) = entry.get("name").and_then(Value::as_str) {
                let normalized = name.trim();
                if !normalized.is_empty() && !names.iter().any(|value| value == normalized) {
                    names.push(normalized.to_string());
                }
            }
        }
    }

    names
}

fn get_string(root: &Value, pointer: &str) -> Option<String> {
    root.pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn get_u64(root: &Value, pointer: &str) -> Option<u64> {
    root.pointer(pointer).and_then(Value::as_u64)
}
