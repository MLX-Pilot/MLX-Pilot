//! Offline Extism loader. Guests have no filesystem, HTTP, process or credential imports.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub permissions: Vec<String>,
    pub tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontend: Option<Frontend>,
}
/// Static browser assets, rendered in an opaque-origin sandbox by the host.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frontend {
    pub html: String,
    #[serde(default)]
    pub css: String,
    #[serde(default)]
    pub javascript: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    pub export: String,
    #[serde(default)]
    pub mutating: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Package {
    pub manifest: Manifest,
    pub wasm: Vec<u8>,
    pub grants: BTreeSet<String>,
    pub enabled: bool,
    pub sha256: String,
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 30
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
impl Manifest {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.api_version == 1, "Unsupported plugin API version");
        ensure!(
            identifier(&self.id),
            "Plugin id must be lowercase snake_case, <=30 characters"
        );
        ensure!(
            !self.name.is_empty() && self.name.len() <= 100 && self.version.len() <= 40,
            "Invalid plugin metadata"
        );
        ensure!(
            !self.tools.is_empty() && self.tools.len() <= 20,
            "Expected 1–20 tools"
        );
        ensure!(
            self.permissions
                .iter()
                .all(|p| matches!(p.as_str(), "compute" | "documents:read" | "documents:write")),
            "Unsupported permission"
        );
        if let Some(ui) = &self.frontend {
            ensure!(!ui.html.trim().is_empty(), "Frontend HTML is empty");
            ensure!(
                ui.html.len() + ui.css.len() + ui.javascript.len() <= 1024 * 1024,
                "Frontend assets exceed 1 MiB"
            );
        }
        let mut names = BTreeSet::new();
        for t in &self.tools {
            ensure!(
                identifier(&t.name) && identifier(&t.export) && names.insert(&t.name),
                "Invalid or duplicate tool name"
            );
            ensure!(t.description.len() <= 2000, "Tool description too long");
            ensure!(
                t.parameters["type"] == "object",
                "Tool schema must describe an object"
            );
            let schema = serde_json::to_string(&t.parameters)?;
            ensure!(
                schema.len() <= 16000
                    && ![
                        "\"$ref\"",
                        "\"$dynamicRef\"",
                        "\"$recursiveRef\"",
                        "\"$schema\""
                    ]
                    .iter()
                    .any(|key| schema.contains(key)),
                "Schema references and oversized schemas are unsupported"
            );
            jsonschema::validator_for(&t.parameters).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        }
        Ok(())
    }
}
impl Package {
    pub fn new(manifest: Manifest, wasm: Vec<u8>, grants: BTreeSet<String>) -> Result<Self> {
        manifest.validate()?;
        ensure!(
            wasm.len() <= 8 * 1024 * 1024 && wasm.starts_with(b"\0asm"),
            "Expected a WASM module up to 8 MB"
        );
        ensure!(
            grants.iter().all(|g| manifest.permissions.contains(g)),
            "Grant not declared by plugin"
        );
        let sha256 = format!("{:x}", Sha256::digest(&wasm));
        let p = Self {
            manifest,
            wasm,
            grants,
            enabled: true,
            sha256,
        };
        // Reject invalid modules before persisting an installation.
        p.builder()?.compile()?;
        Ok(p)
    }
    fn builder(&self) -> Result<extism::PluginBuilder<'_>> {
        // Reconstruct the Extism manifest ourselves. Never accept host paths/URLs from the package.
        let mut manifest = extism::Manifest::new([extism::Wasm::data(self.wasm.clone())]);
        // The document is bounded at 24 MiB; parsing, atomic copies and output
        // serialization need room beyond the wire size.
        manifest.memory.max_pages = Some(4096);
        manifest.timeout_ms = Some(5000);
        Ok(extism::PluginBuilder::new(manifest)
            .with_wasi(true)
            .with_fuel_limit(100_000_000))
    }
    pub fn invoke(&self, name: &str, input: &Value) -> Result<Value> {
        ensure!(self.enabled, "Plugin disabled");
        ensure!(
            self.manifest
                .permissions
                .iter()
                .all(|p| self.grants.contains(p)),
            "Plugin permissions not granted"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&self.wasm)) == self.sha256,
            "Plugin integrity check failed"
        );
        let spec = self
            .manifest
            .tools
            .iter()
            .find(|t| t.name == name)
            .context("Unknown plugin tool")?;
        let validator = jsonschema::validator_for(&spec.parameters)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        if let Err(e) = validator.validate(input) {
            bail!("Invalid tool input: {e}");
        }
        let raw = serde_json::to_vec(input)?;
        ensure!(raw.len() <= 24 * 1024 * 1024, "Plugin input exceeds 24 MB");
        let mut plugin = self.builder()?.build()?;
        let output = plugin.call::<&[u8], Vec<u8>>(&spec.export, &raw)?;
        ensure!(
            output.len() <= 24 * 1024 * 1024,
            "Plugin output exceeds 24 MB"
        );
        Ok(serde_json::from_slice(&output)?)
    }
    pub fn save(&self, path: &Path) -> Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }
    pub fn load(path: &Path) -> Result<Self> {
        ensure!(
            std::fs::metadata(path)?.len() <= 40 * 1024 * 1024,
            "Package too large"
        );
        let p: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        p.manifest.validate()?;
        ensure!(
            p.wasm.len() <= 8 * 1024 * 1024 && p.wasm.starts_with(b"\0asm"),
            "Invalid stored WASM"
        );
        ensure!(
            p.grants.iter().all(|g| p.manifest.permissions.contains(g)),
            "Undeclared stored grant"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&p.wasm)) == p.sha256,
            "Plugin integrity check failed"
        );
        Ok(p)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn manifest() -> Manifest {
        Manifest {
            api_version: 1,
            id: "example".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            permissions: vec!["compute".into()],
            frontend: None,
            tools: vec![ToolSpec {
                name: "run".into(),
                description: "Test".into(),
                parameters: json!({"type":"object"}),
                export: "run".into(),
                mutating: false,
            }],
        }
    }
    #[test]
    fn frontend_assets_are_bounded_and_backward_compatible() {
        let mut m = manifest();
        let encoded = serde_json::to_value(&m).unwrap();
        assert!(encoded.get("frontend").is_none());
        m.frontend = Some(Frontend {
            html: "<main>Hello</main>".into(),
            css: String::new(),
            javascript: String::new(),
        });
        assert!(m.validate().is_ok());
        m.frontend.as_mut().unwrap().javascript = "x".repeat(1024 * 1024);
        assert!(m.validate().is_err());
    }
    #[test]
    fn rejects_names_and_external_schemas() {
        let mut m = manifest();
        m.id = "../escape".into();
        assert!(m.validate().is_err());
        m = manifest();
        for key in ["$ref", "$dynamicRef", "$recursiveRef", "$schema"] {
            m.tools[0].parameters = json!({"type":"object",key:"http://localhost"});
            assert!(m.validate().is_err());
        }
    }
    #[test]
    fn wasm_is_bounded_and_grants_are_required() {
        let bytes = wat::parse_str(
            "(module (func (export \"run\") (result i32) (loop $forever br $forever) i32.const 0))",
        )
        .unwrap();
        let mut p = Package::new(manifest(), bytes, BTreeSet::new()).unwrap();
        assert!(p
            .invoke("run", &json!({}))
            .unwrap_err()
            .to_string()
            .contains("permissions"));
        p.grants.insert("compute".into());
        assert!(p.invoke("run", &json!({})).is_err());
    }
}
