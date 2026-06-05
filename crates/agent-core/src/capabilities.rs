use glob_match::glob_match;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityContextKind {
    Agent,
    Ui,
    Automation,
    Background,
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CapabilityScopeRules {
    #[serde(default)]
    pub allow: Vec<String>,
    #[serde(default)]
    pub deny: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct CapabilityScopes {
    #[serde(default)]
    pub fs: CapabilityScopeRules,
    #[serde(default)]
    pub process: CapabilityScopeRules,
    #[serde(default)]
    pub network: CapabilityScopeRules,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityManifest {
    pub identifier: String,
    pub version: String,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub contexts: Vec<CapabilityContextKind>,
    #[serde(default)]
    pub windows: Vec<String>,
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub scopes: CapabilityScopes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilitySubject {
    pub context: CapabilityContextKind,
    #[serde(default)]
    pub window: Option<String>,
    pub platform: String,
}

impl CapabilitySubject {
    pub fn agent() -> Self {
        Self {
            context: CapabilityContextKind::Agent,
            window: None,
            platform: std::env::consts::OS.to_string(),
        }
    }

    pub fn plugin() -> Self {
        Self {
            context: CapabilityContextKind::Plugin,
            window: None,
            platform: std::env::consts::OS.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityBinding {
    pub manifest_id: String,
    pub subject: CapabilitySubject,
}

#[derive(Debug, thiserror::Error)]
pub enum CapabilityError {
    #[error("invalid capability manifest: {details}")]
    InvalidManifest { details: String },

    #[error("failed to read capability manifest '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("unsupported capability manifest extension for '{path}'")]
    UnsupportedFormat { path: PathBuf },

    #[error("capability denied: {reason}")]
    Denied { reason: String },
}

pub struct CapabilityAuthority {
    manifests: HashMap<String, CapabilityManifest>,
}

impl CapabilityAuthority {
    pub fn new() -> Self {
        Self {
            manifests: HashMap::new(),
        }
    }

    pub fn schema() -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["identifier", "version", "permissions"],
            "properties": {
                "identifier": { "type": "string", "minLength": 1 },
                "version": { "type": "string", "minLength": 1 },
                "permissions": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["fs:read", "fs:write", "process:spawn", "network:http"]
                    }
                },
                "contexts": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "enum": ["agent", "ui", "automation", "background", "plugin"]
                    }
                },
                "windows": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "platforms": {
                    "type": "array",
                    "items": { "type": "string" }
                },
                "scopes": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "fs": { "$ref": "#/$defs/scope_rules" },
                        "process": { "$ref": "#/$defs/scope_rules" },
                        "network": { "$ref": "#/$defs/scope_rules" }
                    }
                }
            },
            "$defs": {
                "scope_rules": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "allow": {
                            "type": "array",
                            "items": { "type": "string" }
                        },
                        "deny": {
                            "type": "array",
                            "items": { "type": "string" }
                        }
                    }
                }
            }
        })
    }

    pub fn insert_manifest(
        &mut self,
        manifest: CapabilityManifest,
    ) -> Result<&CapabilityManifest, CapabilityError> {
        validate_manifest(&manifest)?;
        let key = manifest.identifier.trim().to_ascii_lowercase();
        self.manifests.insert(key.clone(), manifest);
        self.manifests
            .get(&key)
            .ok_or_else(|| CapabilityError::InvalidManifest {
                details: "manifest was inserted but could not be retrieved".to_string(),
            })
    }

    pub fn load_manifest_path(
        &mut self,
        path: impl AsRef<Path>,
    ) -> Result<&CapabilityManifest, CapabilityError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path).map_err(|source| CapabilityError::Io {
            path: path.to_path_buf(),
            source,
        })?;

        let manifest = match path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
            .as_deref()
        {
            Some("json") => {
                serde_json::from_str::<CapabilityManifest>(&content).map_err(|error| {
                    CapabilityError::InvalidManifest {
                        details: format!("{}: {error}", path.display()),
                    }
                })?
            }
            Some("toml") => toml::from_str::<CapabilityManifest>(&content).map_err(|error| {
                CapabilityError::InvalidManifest {
                    details: format!("{}: {error}", path.display()),
                }
            })?,
            _ => {
                return Err(CapabilityError::UnsupportedFormat {
                    path: path.to_path_buf(),
                });
            }
        };

        self.insert_manifest(manifest)
    }

    pub fn authorize_tool(
        &self,
        binding: &CapabilityBinding,
        tool_name: &str,
        params: &Value,
        workspace_root: &Path,
    ) -> Result<(), CapabilityError> {
        let manifest = self
            .manifests
            .get(&binding.manifest_id.trim().to_ascii_lowercase())
            .ok_or_else(|| CapabilityError::Denied {
                reason: format!("unknown capability manifest '{}'", binding.manifest_id),
            })?;

        if !manifest_applies_to_subject(manifest, &binding.subject) {
            return Err(CapabilityError::Denied {
                reason: format!(
                    "capability '{}' does not apply to {:?} context on platform '{}'",
                    manifest.identifier, binding.subject.context, binding.subject.platform
                ),
            });
        }

        let Some(requirement) = capability_requirement(tool_name, params) else {
            return Ok(());
        };

        if !manifest
            .permissions
            .iter()
            .any(|value| value == requirement.permission())
        {
            return Err(CapabilityError::Denied {
                reason: format!(
                    "tool '{}' requires permission '{}'",
                    tool_name,
                    requirement.permission()
                ),
            });
        }

        match requirement {
            ToolCapabilityRequirement::FsRead(path) | ToolCapabilityRequirement::FsWrite(path) => {
                authorize_fs_scope(manifest, path, workspace_root)?;
            }
            ToolCapabilityRequirement::ProcessSpawn(program) => {
                authorize_process_scope(manifest, program)?;
            }
        }

        Ok(())
    }
}

impl Default for CapabilityAuthority {
    fn default() -> Self {
        Self::new()
    }
}

enum ToolCapabilityRequirement<'a> {
    FsRead(&'a str),
    FsWrite(&'a str),
    ProcessSpawn(&'a str),
}

impl ToolCapabilityRequirement<'_> {
    fn permission(&self) -> &'static str {
        match self {
            Self::FsRead(_) => "fs:read",
            Self::FsWrite(_) => "fs:write",
            Self::ProcessSpawn(_) => "process:spawn",
        }
    }
}

fn capability_requirement<'a>(
    tool_name: &str,
    params: &'a Value,
) -> Option<ToolCapabilityRequirement<'a>> {
    match tool_name {
        "read_file" => params
            .get("path")
            .and_then(Value::as_str)
            .or(Some("."))
            .map(ToolCapabilityRequirement::FsRead),
        "list_dir" => params
            .get("path")
            .and_then(Value::as_str)
            .or(Some("."))
            .map(ToolCapabilityRequirement::FsRead),
        "glob" | "grep" => params
            .get("base_path")
            .and_then(Value::as_str)
            .or(Some("."))
            .map(ToolCapabilityRequirement::FsRead),
        "write_file" | "checkpoint_restore" => params
            .get("path")
            .and_then(Value::as_str)
            .map(ToolCapabilityRequirement::FsWrite),
        "edit_file" => params
            .get("path")
            .and_then(Value::as_str)
            .map(ToolCapabilityRequirement::FsWrite),
        "exec" => resolve_program_name(params).map(ToolCapabilityRequirement::ProcessSpawn),
        _ => None,
    }
}

fn validate_manifest(manifest: &CapabilityManifest) -> Result<(), CapabilityError> {
    let value =
        serde_json::to_value(manifest).map_err(|error| CapabilityError::InvalidManifest {
            details: error.to_string(),
        })?;
    let validator = jsonschema::validator_for(&CapabilityAuthority::schema()).map_err(|error| {
        CapabilityError::InvalidManifest {
            details: format!("schema build failed: {error}"),
        }
    })?;
    let errors = validator
        .iter_errors(&value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(CapabilityError::InvalidManifest {
            details: errors.join("; "),
        })
    }
}

fn manifest_applies_to_subject(manifest: &CapabilityManifest, subject: &CapabilitySubject) -> bool {
    let context_ok = manifest.contexts.is_empty() || manifest.contexts.contains(&subject.context);
    let window_ok = manifest.windows.is_empty()
        || subject
            .window
            .as_deref()
            .map(|value| manifest.windows.iter().any(|item| item == value))
            .unwrap_or(false);
    let platform_ok = manifest.platforms.is_empty()
        || manifest
            .platforms
            .iter()
            .any(|value| value.eq_ignore_ascii_case(&subject.platform));

    context_ok && window_ok && platform_ok
}

fn authorize_fs_scope(
    manifest: &CapabilityManifest,
    path_value: &str,
    workspace_root: &Path,
) -> Result<(), CapabilityError> {
    let candidate = normalize_scope_path(resolve_scope_path(workspace_root, path_value));
    let deny = manifest
        .scopes
        .fs
        .deny
        .iter()
        .any(|rule| path_rule_matches(rule, &candidate, workspace_root));
    if deny {
        return Err(CapabilityError::Denied {
            reason: format!("filesystem scope denied '{}'", path_value),
        });
    }

    if !manifest.scopes.fs.allow.is_empty()
        && !manifest
            .scopes
            .fs
            .allow
            .iter()
            .any(|rule| path_rule_matches(rule, &candidate, workspace_root))
    {
        return Err(CapabilityError::Denied {
            reason: format!("filesystem scope does not allow '{}'", path_value),
        });
    }

    Ok(())
}

fn authorize_process_scope(
    manifest: &CapabilityManifest,
    program: &str,
) -> Result<(), CapabilityError> {
    let deny = manifest
        .scopes
        .process
        .deny
        .iter()
        .any(|rule| process_rule_matches(rule, program));
    if deny {
        return Err(CapabilityError::Denied {
            reason: format!("process scope denied '{}'", program),
        });
    }

    if !manifest.scopes.process.allow.is_empty()
        && !manifest
            .scopes
            .process
            .allow
            .iter()
            .any(|rule| process_rule_matches(rule, program))
    {
        return Err(CapabilityError::Denied {
            reason: format!("process scope does not allow '{}'", program),
        });
    }

    Ok(())
}

fn resolve_scope_path(workspace_root: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        logical_normalize(path)
    } else {
        logical_normalize(&workspace_root.join(path))
    }
}

fn normalize_scope_path(path: PathBuf) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn path_rule_matches(rule: &str, candidate: &str, workspace_root: &Path) -> bool {
    let normalized_rule = expand_path_rule(rule, workspace_root);
    glob_match(&normalized_rule, candidate)
        || candidate == normalized_rule
        || candidate.starts_with(&format!("{normalized_rule}/"))
}

fn expand_path_rule(rule: &str, workspace_root: &Path) -> String {
    let workspace = normalize_scope_path(logical_normalize(workspace_root));
    let mut value = rule.trim().replace('\\', "/");
    if value.contains("$WORKSPACE") {
        value = value.replace("$WORKSPACE", &workspace);
    } else if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            let home = normalize_scope_path(home);
            value = format!("{home}/{rest}");
        }
    } else if !Path::new(&value).is_absolute() && !value.starts_with('*') {
        value = format!("{workspace}/{}", value.trim_start_matches("./"));
    }
    value
}

fn process_rule_matches(rule: &str, program: &str) -> bool {
    let normalized_program = program.replace('\\', "/");
    let basename = Path::new(&normalized_program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&normalized_program);
    glob_match(rule, &normalized_program)
        || glob_match(rule, basename)
        || rule.eq_ignore_ascii_case(&normalized_program)
        || rule.eq_ignore_ascii_case(basename)
}

fn resolve_program_name(params: &Value) -> Option<&str> {
    if let Some(program) = params
        .get("argv")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(program);
    }

    params
        .get("command")
        .and_then(Value::as_str)
        .and_then(first_command_token)
}

fn first_command_token(command: &str) -> Option<&str> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut in_quote = false;
    for (index, ch) in trimmed.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ' ' | '\t' if !in_quote => {
                return Some(trimmed[..index].trim_matches('"'));
            }
            _ => {}
        }
    }
    Some(trimmed.trim_matches('"'))
}

fn logical_normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let _ = result.pop();
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> CapabilityManifest {
        CapabilityManifest {
            identifier: "agent-default".to_string(),
            version: "1.0.0".to_string(),
            permissions: vec![
                "fs:read".to_string(),
                "fs:write".to_string(),
                "process:spawn".to_string(),
            ],
            contexts: vec![CapabilityContextKind::Agent],
            windows: Vec::new(),
            platforms: Vec::new(),
            scopes: CapabilityScopes {
                fs: CapabilityScopeRules {
                    allow: vec!["$WORKSPACE/**/*".to_string()],
                    deny: vec!["$WORKSPACE/.secrets/**/*".to_string()],
                },
                process: CapabilityScopeRules {
                    allow: vec!["git".to_string(), "rg".to_string()],
                    deny: vec!["powershell*".to_string()],
                },
                network: CapabilityScopeRules::default(),
            },
        }
    }

    #[test]
    fn schema_accepts_valid_manifest() {
        validate_manifest(&manifest()).expect("manifest should validate");
    }

    #[test]
    fn schema_rejects_unknown_permission() {
        let mut bad = manifest();
        bad.permissions.push("fs:destroy".to_string());
        let error = validate_manifest(&bad).expect_err("manifest should fail");
        assert!(
            error.to_string().contains("fs:destroy")
                || error.to_string().contains("invalid capability manifest")
        );
    }

    #[test]
    fn authority_enforces_fs_scope() {
        let mut authority = CapabilityAuthority::new();
        authority
            .insert_manifest(manifest())
            .expect("insert manifest");
        let binding = CapabilityBinding {
            manifest_id: "agent-default".to_string(),
            subject: CapabilitySubject::agent(),
        };
        let workspace = PathBuf::from("C:/workspace/project");

        authority
            .authorize_tool(
                &binding,
                "read_file",
                &json!({"path":"src/main.rs"}),
                &workspace,
            )
            .expect("path inside workspace");

        let error = authority
            .authorize_tool(
                &binding,
                "read_file",
                &json!({"path":".secrets/token.txt"}),
                &workspace,
            )
            .expect_err("secret path should be denied");
        assert!(error.to_string().contains("filesystem scope denied"));
    }

    #[test]
    fn authority_enforces_process_scope() {
        let mut authority = CapabilityAuthority::new();
        authority
            .insert_manifest(manifest())
            .expect("insert manifest");
        let binding = CapabilityBinding {
            manifest_id: "agent-default".to_string(),
            subject: CapabilitySubject::agent(),
        };
        let workspace = PathBuf::from("C:/workspace/project");

        authority
            .authorize_tool(
                &binding,
                "exec",
                &json!({"argv":["git","status"]}),
                &workspace,
            )
            .expect("git should be allowed");

        let error = authority
            .authorize_tool(
                &binding,
                "exec",
                &json!({"argv":["python","-V"]}),
                &workspace,
            )
            .expect_err("python should be denied by allowlist");
        assert!(error.to_string().contains("process scope does not allow"));
    }

    #[test]
    fn authority_parses_json_and_toml_manifests() {
        let dir = tempfile::tempdir().expect("tempdir");
        let json_path = dir.path().join("cap.json");
        let toml_path = dir.path().join("cap.toml");
        fs::write(
            &json_path,
            serde_json::to_string(&manifest()).expect("serialize manifest"),
        )
        .expect("write json manifest");
        fs::write(
            &toml_path,
            toml::to_string(&manifest()).expect("serialize toml"),
        )
        .expect("write toml manifest");

        let mut authority = CapabilityAuthority::new();
        authority
            .load_manifest_path(&json_path)
            .expect("json manifest should load");
        authority
            .load_manifest_path(&toml_path)
            .expect("toml manifest should load");
    }
}
