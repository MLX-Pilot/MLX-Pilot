//! Sandboxed Extism loader with explicit, permission-checked host capabilities.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path, sync::Arc};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub api_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    pub permissions: Vec<String>,
    pub tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<ServiceSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frontend: Option<Frontend>,
}
/// A named endpoint. User policy must separately authorize local HTTP services.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceSpec {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub methods: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<ServiceCredential>,
}
/// Describes how the host injects a stored secret; the guest never receives it.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceCredential {
    pub id: String,
    pub header: String,
    #[serde(default)]
    pub prefix: String,
}

/// Trusted broker bound to one plugin's current grants and user configuration.
pub trait PluginHost: Send + Sync {
    fn call(&self, operation: &str, input: &Value) -> Result<Value>;
}

pub const HOST_FUNCTION: &str = "mlx_host_call";
const MAX_HOST_BYTES: usize = 24 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HostRequest {
    operation: String,
    input: Value,
}

fn operation_permission(operation: &str) -> Result<&'static str> {
    match operation {
        "http.request" => Ok("network:request"),
        "credentials.status" => Ok("credentials:use"),
        "documents.list" | "documents.get" => Ok("documents:read"),
        "documents.create" | "documents.edit" => Ok("documents:write"),
        _ => bail!("Unsupported plugin host operation"),
    }
}

fn credential_header(header: &str) -> bool {
    let name = header.to_ascii_lowercase();
    !name.is_empty()
        && name.len() <= 100
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        && !matches!(
            name.as_str(),
            "host"
                | "cookie"
                | "set-cookie"
                | "content-length"
                | "transfer-encoding"
                | "connection"
                | "keep-alive"
                | "te"
                | "trailer"
                | "upgrade"
                | "origin"
                | "referer"
                | "forwarded"
                | "expect"
                | "x-real-ip"
                | "x-http-method-override"
        )
        && !["proxy-", "sec-", "x-forwarded-"]
            .iter()
            .any(|p| name.starts_with(p))
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
            self.permissions.iter().all(|p| matches!(
                p.as_str(),
                "compute"
                    | "documents:read"
                    | "documents:write"
                    | "network:request"
                    | "credentials:use"
            )),
            "Unsupported permission"
        );
        ensure!(self.services.len() <= 20, "Expected at most 20 services");
        let mut service_ids = BTreeSet::new();
        let mut credential_ids = BTreeSet::new();
        for service in &self.services {
            ensure!(
                identifier(&service.id) && service_ids.insert(&service.id),
                "Invalid or duplicate service id"
            );
            ensure!(
                !service.name.trim().is_empty() && service.name.len() <= 100,
                "Invalid service name"
            );
            ensure!(service.base_url.len() <= 2048, "Service URL too long");
            let url = url::Url::parse(&service.base_url).context("Invalid service URL")?;
            let local = match url.host() {
                Some(url::Host::Domain(name)) => name == "localhost",
                Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
                Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
                None => false,
            };
            ensure!(url.host().is_some() && (url.scheme() == "https" || (url.scheme() == "http" && local)), "Services require HTTPS; HTTP is limited to loopback with separate user authorization");
            ensure!(
                url.username().is_empty()
                    && url.password().is_none()
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "Service URLs cannot contain credentials, queries or fragments"
            );
            let mut methods = BTreeSet::new();
            ensure!(
                !service.methods.is_empty() && service.methods.len() <= 7,
                "Declare 1–7 HTTP methods"
            );
            for method in &service.methods {
                ensure!(
                    matches!(
                        method.as_str(),
                        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
                    ) && methods.insert(method),
                    "Unsupported or duplicate HTTP method"
                );
            }
            ensure!(
                self.permissions.iter().any(|p| p == "network:request"),
                "Services require network:request permission"
            );
            if let Some(credential) = &service.credential {
                ensure!(
                    identifier(&credential.id) && credential_ids.insert(&credential.id),
                    "Invalid or duplicate credential id"
                );
                ensure!(
                    credential_header(&credential.header),
                    "Unsafe credential header"
                );
                ensure!(
                    credential.prefix.len() <= 128
                        && !credential.prefix.chars().any(char::is_control),
                    "Invalid credential prefix"
                );
                ensure!(
                    self.permissions.iter().any(|p| p == "credentials:use"),
                    "Service credentials require credentials:use permission"
                );
            }
        }
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
        p.builder(None)?.compile()?;
        Ok(p)
    }
    fn builder(&self, host: Option<Arc<dyn PluginHost>>) -> Result<extism::PluginBuilder<'_>> {
        // Reconstruct the Extism manifest ourselves. Never accept host paths/URLs from the package.
        let mut manifest = extism::Manifest::new([extism::Wasm::data(self.wasm.clone())]);
        // The document is bounded at 24 MiB; parsing, atomic copies and output
        // serialization need room beyond the wire size.
        manifest.memory.max_pages = Some(4096);
        manifest.timeout_ms = Some(if host.is_some() { 60_000 } else { 5000 });
        let grants = self.grants.clone();
        let services = self.manifest.services.clone();
        Ok(extism::PluginBuilder::new(manifest)
            .with_wasi(true)
            .with_fuel_limit(100_000_000)
            .with_function(
                HOST_FUNCTION,
                [extism::PTR],
                [extism::PTR],
                extism::UserData::new(()),
                move |plugin, inputs, outputs, _| {
                    let result = (|| -> Result<Value> {
                        let bytes: &[u8] = plugin.memory_get_val(&inputs[0])?;
                        ensure!(
                            bytes.len() <= MAX_HOST_BYTES,
                            "Plugin host request exceeds 24 MiB"
                        );
                        let request: HostRequest =
                            serde_json::from_slice(bytes).context("Invalid plugin host request")?;
                        ensure!(
                            request.input.is_object(),
                            "Plugin host input must be an object"
                        );
                        ensure!(
                            grants.contains(operation_permission(&request.operation)?),
                            "Plugin host permission not granted"
                        );
                        if request.operation == "http.request"
                            || request.operation == "credentials.status"
                        {
                            let service = services
                                .iter()
                                .find(|service| {
                                    request.input["service"].as_str() == Some(&service.id)
                                })
                                .context("Unknown plugin service")?;
                            if request.operation == "http.request" && service.credential.is_some() {
                                ensure!(
                                    grants.contains("credentials:use"),
                                    "Plugin credential permission not granted"
                                );
                            }
                        }
                        host.as_ref()
                            .context("Plugin host unavailable")?
                            .call(&request.operation, &request.input)
                    })();
                    let response = match result {
                        Ok(result) => serde_json::json!({ "result": result }),
                        Err(error) => serde_json::json!({ "error": error.to_string() }),
                    };
                    let mut bytes = serde_json::to_vec(&response)?;
                    if bytes.len() > MAX_HOST_BYTES {
                        bytes = serde_json::to_vec(
                            &serde_json::json!({ "error": "Plugin host response exceeds 24 MiB" }),
                        )?;
                    }
                    plugin.memory_set_val(&mut outputs[0], bytes.as_slice())?;
                    Ok(())
                },
            ))
    }
    pub fn invoke(&self, name: &str, input: &Value) -> Result<Value> {
        self.invoke_internal(name, input, None)
    }
    pub fn invoke_with_host(
        &self,
        name: &str,
        input: &Value,
        host: Arc<dyn PluginHost>,
    ) -> Result<Value> {
        self.invoke_internal(name, input, Some(host))
    }
    fn invoke_internal(
        &self,
        name: &str,
        input: &Value,
        host: Option<Arc<dyn PluginHost>>,
    ) -> Result<Value> {
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
        let mut plugin = self.builder(host)?.build()?;
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
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn manifest() -> Manifest {
        Manifest {
            api_version: 1,
            id: "example".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            permissions: vec!["compute".into()],
            frontend: None,
            services: vec![],
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

    fn service() -> ServiceSpec {
        ServiceSpec {
            id: "research".into(),
            name: "Research API".into(),
            base_url: "https://api.example.org/v1/".into(),
            methods: vec!["GET".into(), "POST".into()],
            credential: Some(ServiceCredential {
                id: "research_key".into(),
                header: "Authorization".into(),
                prefix: "Bearer ".into(),
            }),
        }
    }

    fn service_manifest() -> Manifest {
        let mut m = manifest();
        m.permissions
            .extend(["network:request".into(), "credentials:use".into()]);
        m.services.push(service());
        m
    }

    #[test]
    fn service_manifest_is_backward_compatible_and_accepts_explicit_loopback() {
        let original = manifest();
        let encoded = serde_json::to_value(&original).unwrap();
        assert!(encoded.get("services").is_none());
        assert!(serde_json::from_value::<Manifest>(encoded)
            .unwrap()
            .services
            .is_empty());
        for base in [
            "https://api.example.org/v1/",
            "http://localhost:8080/",
            "http://127.0.0.1:11434/",
            "http://[::1]:8080/",
        ] {
            let mut m = service_manifest();
            m.services[0].base_url = base.into();
            m.validate().unwrap();
        }
    }

    #[test]
    fn service_metadata_rejects_unsafe_endpoints_and_capabilities() {
        for base in [
            "http://api.example.org",
            "http://192.168.1.1",
            "http://localhost.evil.test",
            "file:///etc/passwd",
            "https://secret@api.example.org",
            "https://api.example.org?api_key=secret",
            "https://api.example.org#key",
        ] {
            let mut m = service_manifest();
            m.services[0].base_url = base.into();
            assert!(m.validate().is_err(), "accepted {base}");
        }
        for method in ["CONNECT", "TRACE", "get", "POST\r\nHost:evil.test"] {
            let mut m = service_manifest();
            m.services[0].methods = vec![method.into()];
            assert!(m.validate().is_err(), "accepted {method}");
        }
        let mut m = service_manifest();
        m.services.push(service());
        assert!(m.validate().is_err());
        m = service_manifest();
        m.permissions.retain(|p| p != "network:request");
        assert!(m.validate().is_err());
        m = service_manifest();
        m.permissions.retain(|p| p != "credentials:use");
        assert!(m.validate().is_err());
    }

    #[test]
    fn credential_headers_cannot_override_routing_cookies_or_message_framing() {
        for header in [
            "Host",
            "Cookie",
            "Set-Cookie",
            "Content-Length",
            "Transfer-Encoding",
            "Connection",
            "Proxy-Authorization",
            "X-Forwarded-Host",
            "Sec-Fetch-Site",
            "X-API-Key\r\nCookie",
        ] {
            let mut m = service_manifest();
            m.services[0].credential.as_mut().unwrap().header = header.into();
            assert!(m.validate().is_err(), "accepted {header}");
        }
        for header in ["Authorization", "x-api-key", "X-Goog-Api-Key"] {
            let mut m = service_manifest();
            m.services[0].credential.as_mut().unwrap().header = header.into();
            m.validate().unwrap();
        }
        let mut m = service_manifest();
        m.services[0].credential.as_mut().unwrap().prefix = "Bearer\r\n".into();
        assert!(m.validate().is_err());
    }

    fn host_bridge_wasm() -> Vec<u8> {
        wat::parse_str(r#"(module
          (import "extism:host/env" "input_length" (func $input_length (result i64)))
          (import "extism:host/env" "input_load_u8" (func $input_load_u8 (param i64) (result i32)))
          (import "extism:host/env" "alloc" (func $alloc (param i64) (result i64)))
          (import "extism:host/env" "store_u8" (func $store_u8 (param i64 i32)))
          (import "extism:host/env" "length" (func $length (param i64) (result i64)))
          (import "extism:host/env" "output_set" (func $output_set (param i64 i64)))
          (import "extism:host/user" "mlx_host_call" (func $host (param i64) (result i64)))
          (func (export "run") (result i32)
            (local $ptr i64) (local $len i64) (local $i i64) (local $response i64)
            (local.set $len (call $input_length))
            (local.set $ptr (call $alloc (local.get $len)))
            (block $done (loop $copy
              (br_if $done (i64.ge_u (local.get $i) (local.get $len)))
              (call $store_u8 (i64.add (local.get $ptr) (local.get $i)) (call $input_load_u8 (local.get $i)))
              (local.set $i (i64.add (local.get $i) (i64.const 1)))
              (br $copy)))
            (local.set $response (call $host (local.get $ptr)))
            (call $output_set (local.get $response) (call $length (local.get $response)))
            (i32.const 0)))"#).unwrap()
    }

    struct RecordingHost(AtomicUsize);
    impl PluginHost for RecordingHost {
        fn call(&self, operation: &str, input: &Value) -> Result<Value> {
            self.0.fetch_add(1, Ordering::SeqCst);
            if input["fail"] == true {
                bail!("Broker denied this operation");
            }
            Ok(json!({ "operation": operation, "input": input }))
        }
    }

    #[test]
    fn wasm_imports_compile_before_install_and_host_calls_round_trip_dynamically() {
        let mut m = manifest();
        m.permissions.push("documents:read".into());
        let grants = m.permissions.iter().cloned().collect();
        let p = Package::new(m, host_bridge_wasm(), grants).unwrap();
        let request =
            json!({ "operation": "documents.get", "input": { "id": "user-approved-document" } });
        let unavailable = p.invoke("run", &request).unwrap();
        assert!(unavailable["error"]
            .as_str()
            .unwrap()
            .contains("host unavailable"));
        let host = Arc::new(RecordingHost(AtomicUsize::new(0)));
        let response = p.invoke_with_host("run", &request, host.clone()).unwrap();
        assert_eq!(response, json!({ "result": request }));
        assert_eq!(host.0.load(Ordering::SeqCst), 1);
        let failure = p
            .invoke_with_host(
                "run",
                &json!({ "operation": "documents.get", "input": { "fail": true } }),
                host,
            )
            .unwrap();
        assert_eq!(failure, json!({ "error": "Broker denied this operation" }));
    }

    #[test]
    fn host_operations_require_grants_and_declared_services_before_dispatch() {
        let m = service_manifest();
        let grants = m.permissions.iter().cloned().collect();
        let p = Package::new(m, host_bridge_wasm(), grants).unwrap();
        let host = Arc::new(RecordingHost(AtomicUsize::new(0)));
        for request in [
            json!({ "operation": "documents.list", "input": {} }),
            json!({ "operation": "credentials.read", "input": { "service": "research" } }),
            json!({ "operation": "http.request", "input": { "service": "undeclared", "path": "/" } }),
            json!({ "operation": "http.request", "input": null }),
            json!({ "operation": "http.request", "input": {}, "plugin_id": "another_plugin" }),
        ] {
            let result = p.invoke_with_host("run", &request, host.clone()).unwrap();
            assert!(result.get("error").is_some());
        }
        assert_eq!(host.0.load(Ordering::SeqCst), 0);
        let request = json!({ "operation": "http.request", "input": { "service": "research", "path": "/search" } });
        assert_eq!(
            p.invoke_with_host("run", &request, host.clone()).unwrap()["result"],
            request
        );
        assert_eq!(host.0.load(Ordering::SeqCst), 1);
    }
}
