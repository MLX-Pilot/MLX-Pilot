//! User-configured capabilities. Guests never receive a vault key or an unrestricted client.
use super::*;
use crate::secrets_vault::SecretsVault;
use anyhow::{bail, ensure, Context, Result};
use mlx_plugin_runtime::PluginHost;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    time::{Duration, Instant},
};
use url::Url;

const MAX_HTTP_BYTES: usize = 1024 * 1024;
const MAX_HOST_BYTES: usize = 24 * 1024 * 1024;

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ServiceAccess {
    enabled: bool,
    base_url: String,
    allow_private_network: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct DocumentAccess {
    ids: BTreeSet<String>,
    allow_create: bool,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Access {
    manifest_hash: String,
    services: BTreeMap<String, ServiceAccess>,
    documents: DocumentAccess,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceUpdate {
    enabled: bool,
    base_url: String,
    #[serde(default)]
    allow_private_network: bool,
    credential: Option<String>,
    #[serde(default)]
    clear_credential: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfigurationUpdate {
    grants: BTreeSet<String>,
    services: BTreeMap<String, ServiceUpdate>,
    documents: DocumentAccess,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HostRequest {
    operation: String,
    input: Value,
}

fn scope(workspace: &FsPath) -> Result<String> {
    let canonical = workspace.canonicalize()?;
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    ))
}
fn fingerprint(package: &Package) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&json!({
            "services":package.manifest.services,"permissions":package.manifest.permissions
        }))?)
    ))
}
fn private_directory(path: &FsPath) -> Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

impl StudioService {
    fn access_path(&self, workspace: &FsPath, id: &str) -> Result<PathBuf> {
        self.package(id).map_err(anyhow::Error::msg)?;
        Ok(self
            .root
            .join("plugin-access")
            .join(scope(workspace)?)
            .join(format!("{id}.json")))
    }
    fn access(&self, workspace: &FsPath, package: &Package) -> Result<Access> {
        let path = self.access_path(workspace, &package.manifest.id)?;
        let hash = fingerprint(package)?;
        let mut access = if path.exists() {
            ensure!(
                std::fs::metadata(&path)?.len() <= 64 * 1024,
                "Plugin configuration too large"
            );
            serde_json::from_slice::<Access>(&std::fs::read(path)?)?
        } else {
            Access::default()
        };
        // A changed service declaration must receive fresh user authorization.
        if access.manifest_hash != hash {
            access = Access {
                manifest_hash: hash,
                ..Default::default()
            };
        }
        for service in &package.manifest.services {
            access
                .services
                .entry(service.id.clone())
                .or_insert_with(|| ServiceAccess {
                    base_url: service.base_url.clone(),
                    ..Default::default()
                });
        }
        Ok(access)
    }
    fn save_access(&self, workspace: &FsPath, id: &str, access: &Access) -> Result<()> {
        let path = self.access_path(workspace, id)?;
        private_directory(path.parent().context("Missing configuration directory")?)?;
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, serde_json::to_vec(access)?)?;
        std::fs::rename(temp, path)?;
        Ok(())
    }
    fn plugin_vault(&self, id: &str) -> Result<SecretsVault> {
        self.package(id).map_err(anyhow::Error::msg)?;
        let path = self.root.join("credentials").join(id);
        private_directory(&path)?;
        Ok(SecretsVault::open(&path)?)
    }
    fn secret_key(workspace: &FsPath, service: &mlx_plugin_runtime::ServiceSpec) -> Result<String> {
        let credential = service
            .credential
            .as_ref()
            .context("Service has no credential slot")?;
        Ok(format!(
            "{}:{}:{}",
            scope(workspace)?,
            service.id,
            credential.id
        ))
    }
    pub(super) fn remove_plugin_access(&self, id: &str) -> Result<(), String> {
        let vault = self.root.join("credentials").join(id);
        if vault.exists() {
            std::fs::remove_dir_all(vault).map_err(|e| e.to_string())?;
        }
        let root = self.root.join("plugin-access");
        if root.exists() {
            for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
                let path = entry
                    .map_err(|e| e.to_string())?
                    .path()
                    .join(format!("{id}.json"));
                if path.is_file() {
                    std::fs::remove_file(path).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(())
    }
    fn configuration(&self, workspace: &FsPath, id: &str) -> Result<Value> {
        let package = self.package(id).map_err(anyhow::Error::msg)?;
        ensure!(
            id != "presentation",
            "Configure the bundled Studio through its own editor"
        );
        let access = self.access(workspace, &package)?;
        let vault = self.plugin_vault(id)?;
        let mut services = BTreeMap::new();
        for service in &package.manifest.services {
            let configured = if service.credential.is_some() {
                vault
                    .get_secret(&Self::secret_key(workspace, service)?)?
                    .is_some()
            } else {
                false
            };
            let mut value = serde_json::to_value(&access.services[&service.id])?;
            value["credential_set"] = json!(configured);
            services.insert(service.id.clone(), value);
        }
        Ok(json!({
            "grants":package.grants,"services":services,"documents":access.documents,
            "available_documents":self.list(workspace).unwrap_or_else(|_| json!([]))
        }))
    }
    fn configure(
        &self,
        workspace: &FsPath,
        id: &str,
        update: ConfigurationUpdate,
    ) -> Result<Value> {
        let _access = self
            .host_gate
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let mut package = self.package(id).map_err(anyhow::Error::msg)?;
        ensure!(
            id != "presentation",
            "Bundled Studio configuration is managed by the editor"
        );
        ensure!(
            update
                .grants
                .iter()
                .all(|p| package.manifest.permissions.contains(p)),
            "Undeclared permission grant"
        );
        ensure!(
            update.services.len() == package.manifest.services.len(),
            "Include all declared services"
        );
        ensure!(
            update.documents.ids.len() <= 200,
            "Too many document grants"
        );
        for id in &update.documents.ids {
            ensure!(valid_id(id), "Invalid document grant");
            self.get(workspace, id).map_err(anyhow::Error::msg)?;
        }
        // Validate the entire update before writing any credential.
        for service in &package.manifest.services {
            let input = update
                .services
                .get(&service.id)
                .context("Unknown or missing service configuration")?;
            validate_base(
                &input.base_url,
                input.allow_private_network || !input.enabled,
            )?;
            if let Some(secret) = &input.credential {
                ensure!(
                    service.credential.is_some(),
                    "Service does not declare a credential"
                );
                ensure!(
                    !secret.is_empty()
                        && secret.len() <= 8192
                        && !secret.chars().any(char::is_control),
                    "Invalid credential"
                );
                ensure!(
                    !input.clear_credential,
                    "Choose replacement or removal of credential"
                );
            }
        }
        let vault = self.plugin_vault(id)?;
        let mut access = Access {
            manifest_hash: fingerprint(&package)?,
            documents: update.documents,
            services: BTreeMap::new(),
        };
        for service in &package.manifest.services {
            let input = &update.services[&service.id];
            if service.credential.is_some() {
                let key = Self::secret_key(workspace, service)?;
                if input.clear_credential {
                    vault.remove_secret(&key)?;
                }
                if let Some(secret) = &input.credential {
                    vault.set_secret(&key, secret)?;
                }
            }
            access.services.insert(
                service.id.clone(),
                ServiceAccess {
                    enabled: input.enabled,
                    base_url: input.base_url.clone(),
                    allow_private_network: input.allow_private_network,
                },
            );
        }
        self.save_access(workspace, id, &access)?;
        let _save = self
            .gate
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        package.grants = update.grants;
        package.save(&self.root.join("plugins").join(format!("{id}.json")))?;
        self.packages
            .write()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?
            .insert(id.into(), package);
        self.configuration(workspace, id)
    }
    pub(super) fn invoke_plugin(
        self: &Arc<Self>,
        workspace: &FsPath,
        id: &str,
        tool: &str,
        input: &Value,
        allow_mutation: bool,
    ) -> Result<Value, String> {
        let package = self.package(id)?;
        let spec = package
            .manifest
            .tools
            .iter()
            .find(|s| s.name == tool)
            .ok_or("Unknown tool")?;
        let host = Arc::new(ScopedHost {
            studio: self.clone(),
            workspace: workspace.into(),
            id: id.into(),
            manifest_hash: fingerprint(&package).map_err(|e| e.to_string())?,
            allow_mutation: allow_mutation && spec.mutating,
            started: Instant::now(),
            requests: Mutex::new(0),
        });
        package
            .invoke_with_host(tool, input, host)
            .map_err(|e| e.to_string())
    }
}

struct ScopedHost {
    studio: Arc<StudioService>,
    workspace: PathBuf,
    id: String,
    manifest_hash: String,
    allow_mutation: bool,
    started: Instant,
    requests: Mutex<u8>,
}
impl PluginHost for ScopedHost {
    fn call(&self, operation: &str, input: &Value) -> Result<Value> {
        ensure!(
            self.started.elapsed() < Duration::from_secs(45),
            "Plugin host deadline exceeded"
        );
        ensure!(
            serde_json::to_vec(input)?.len() <= MAX_HOST_BYTES,
            "Host input too large"
        );
        let _access = self
            .studio
            .host_gate
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let package = self.studio.package(&self.id).map_err(anyhow::Error::msg)?;
        ensure!(package.enabled, "Plugin disabled");
        ensure!(
            package
                .manifest
                .permissions
                .iter()
                .all(|p| package.grants.contains(p)),
            "Plugin permissions not granted"
        );
        ensure!(
            fingerprint(&package)? == self.manifest_hash,
            "Plugin permissions changed; retry the invocation"
        );
        let access = self.studio.access(&self.workspace, &package)?;
        let permission = match operation {
            "http.request" => "network:request",
            "credentials.status" => "credentials:use",
            "documents.list" | "documents.get" => "documents:read",
            "documents.create" | "documents.edit" => "documents:write",
            _ => bail!("Unsupported host operation"),
        };
        ensure!(
            package.grants.contains(permission)
                && package.manifest.permissions.iter().any(|p| p == permission),
            "Host permission not granted: {permission}"
        );
        let result = match operation {
            "http.request" => self.request(&package, &access, input)?,
            "credentials.status" => {
                let service = service(&package, input)?;
                let configured = if service.credential.is_some() {
                    self.studio
                        .plugin_vault(&self.id)?
                        .get_secret(&StudioService::secret_key(&self.workspace, service)?)?
                        .is_some()
                } else {
                    false
                };
                json!({"configured":configured})
            }
            "documents.list" => {
                let list = self
                    .studio
                    .list(&self.workspace)
                    .map_err(anyhow::Error::msg)?;
                json!(list
                    .as_array()
                    .context("Invalid document inventory")?
                    .iter()
                    .filter(|d| d["id"]
                        .as_str()
                        .is_some_and(|id| access.documents.ids.contains(id)))
                    .collect::<Vec<_>>())
            }
            "documents.get" => {
                let id = authorized_document(input, &access)?;
                let mut doc = serde_json::to_value(
                    self.studio
                        .get(&self.workspace, id)
                        .map_err(anyhow::Error::msg)?,
                )?;
                if let Some(assets) = doc["assets"].as_array_mut() {
                    for asset in assets {
                        asset
                            .as_object_mut()
                            .context("Invalid asset")?
                            .remove("data_url");
                    }
                }
                doc
            }
            "documents.create" => {
                ensure!(
                    self.allow_mutation,
                    "Document writes require a mutating tool in full execution mode"
                );
                ensure!(
                    access.documents.allow_create,
                    "Document creation is not authorized for this plugin"
                );
                ensure!(
                    access.documents.ids.len() < 200,
                    "Document grant limit reached; remove unused selections first"
                );
                let title = input["title"].as_str().context("A title is required")?;
                ensure!(
                    !title.trim().is_empty() && title.len() <= 300,
                    "Invalid document title"
                );
                let doc = self
                    .studio
                    .save(
                        &self.workspace,
                        Document::new(uuid::Uuid::new_v4().to_string(), title.into()),
                        None,
                    )
                    .map_err(anyhow::Error::msg)?;
                let mut access = access;
                access.documents.ids.insert(doc.id.clone());
                self.studio
                    .save_access(&self.workspace, &self.id, &access)?;
                json!(doc)
            }
            "documents.edit" => {
                ensure!(
                    self.allow_mutation,
                    "Document writes require a mutating tool in full execution mode"
                );
                let id = authorized_document(input, &access)?;
                let base = input["base_revision"]
                    .as_u64()
                    .context("base_revision is required")?;
                let operations = input
                    .get("operations")
                    .context("operations are required")?
                    .clone();
                let doc = self
                    .studio
                    .edit(&self.workspace, id, base, operations)
                    .map_err(anyhow::Error::msg)?;
                json!({"id":doc.id,"revision":doc.revision,"slides":doc.slides.len(),"diagnostics":doc.validate().map_err(anyhow::Error::msg)?})
            }
            _ => unreachable!(),
        };
        ensure!(
            serde_json::to_vec(&result)?.len() <= MAX_HOST_BYTES,
            "Host response too large"
        );
        Ok(result)
    }
}
fn service<'a>(package: &'a Package, input: &Value) -> Result<&'a mlx_plugin_runtime::ServiceSpec> {
    let id = input["service"].as_str().context("service is required")?;
    package
        .manifest
        .services
        .iter()
        .find(|s| s.id == id)
        .context("Service not declared by this plugin")
}
fn authorized_document<'a>(input: &'a Value, access: &Access) -> Result<&'a str> {
    let id = input["id"].as_str().context("id is required")?;
    ensure!(
        valid_id(id) && access.documents.ids.contains(id),
        "Document not authorized for this plugin"
    );
    Ok(id)
}
fn validate_base(value: &str, allow_private: bool) -> Result<Url> {
    ensure!(value.len() <= 2048, "Service URL too long");
    let url = Url::parse(value).context("Invalid service URL")?;
    ensure!(
        url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "Use a service URL without credentials, query or fragment"
    );
    ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && allow_private),
        "HTTPS required; HTTP is allowed only with explicit local-network access"
    );
    Ok(url)
}
fn allowed_ip(ip: IpAddr, allow_private: bool) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            if v.is_unspecified()
                || v.is_multicast()
                || v.is_link_local()
                || v.is_broadcast()
                || o[0] == 0
                || o[0] >= 240
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || v.is_documentation()
                || (o[0] == 192 && o[1] == 0 && o[2] == 0)
                || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
            {
                return false;
            }
            allow_private || !(v.is_private() || v.is_loopback())
        }
        IpAddr::V6(v) => {
            if let Some(v4) = v.to_ipv4_mapped() {
                return allowed_ip(IpAddr::V4(v4), allow_private);
            }
            if v.is_unspecified() || v.is_multicast() || v.is_unicast_link_local() {
                return false;
            }
            let s = v.segments();
            if v.is_loopback() || v.is_unique_local() {
                return allow_private;
            }
            // Only global unicast; exclude documentation and transition/embedded ranges.
            (s[0] & 0xe000) == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] <= 0x1ff || s[1] == 0xdb8))
        }
    }
}
fn pinned_addresses(url: &Url, allow_private: bool) -> Result<Vec<SocketAddr>> {
    let host = url
        .host_str()
        .context("Missing service host")?
        .trim_matches(['[', ']']);
    let port = url
        .port_or_known_default()
        .context("Invalid service port")?;
    let hostname = host.to_owned();
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let resolved = (hostname.as_str(), port)
            .to_socket_addrs()
            .map(|a| a.collect::<Vec<_>>());
        let _ = sender.send(resolved);
    });
    let addresses = receiver
        .recv_timeout(Duration::from_secs(3))
        .context("Service DNS resolution timed out")?
        .context("Could not resolve service host")?;
    ensure!(
        !addresses.is_empty() && addresses.iter().all(|a| allowed_ip(a.ip(), allow_private)),
        "Service resolves to a prohibited address; local addresses require explicit permission"
    );
    if url.scheme() == "http" {
        ensure!(
            addresses.iter().all(|a| match a.ip() {
                IpAddr::V4(v) => v.is_private() || v.is_loopback(),
                IpAddr::V6(v) => v.is_unique_local() || v.is_loopback(),
            }),
            "HTTP is limited to authorized local addresses"
        );
    }
    Ok(addresses)
}
impl ScopedHost {
    fn request(&self, package: &Package, access: &Access, input: &Value) -> Result<Value> {
        let spec = service(package, input)?;
        let config = access
            .services
            .get(&spec.id)
            .context("Service not configured")?;
        ensure!(
            config.enabled,
            "Enable this service in the plugin configuration first"
        );
        let base = validate_base(&config.base_url, config.allow_private_network)?;
        let path = input["path"]
            .as_str()
            .context("Service-relative path is required")?;
        ensure!(
            path.starts_with('/')
                && !path.starts_with("//")
                && !path.contains('#')
                && path.len() <= 4096,
            "Use a relative service path starting with a single slash"
        );
        let url = base.join(path)?;
        ensure!(
            url.origin() == base.origin() && url.username().is_empty() && url.password().is_none(),
            "Request must stay within the configured service origin"
        );
        let method = input["method"]
            .as_str()
            .unwrap_or("GET")
            .to_ascii_uppercase();
        ensure!(
            spec.methods.contains(&method),
            "HTTP method not declared by plugin"
        );
        if !matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS") {
            ensure!(
                self.allow_mutation,
                "HTTP writes require a mutating tool in full execution mode"
            );
        }
        let mut requests = self
            .requests
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        ensure!(
            *requests < 3,
            "At most three service requests per invocation"
        );
        *requests += 1;
        let addresses = pinned_addresses(&url, config.allow_private_network)?;
        let remaining = Duration::from_secs(45).saturating_sub(self.started.elapsed());
        ensure!(!remaining.is_zero(), "Plugin host deadline exceeded");
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(remaining.min(Duration::from_secs(15)))
            .resolve_to_addrs(url.host_str().context("Missing service host")?, &addresses)
            .build()?;
        let mut request = client.request(reqwest::Method::from_bytes(method.as_bytes())?, url);
        if let Some(headers) = input.get("headers") {
            let headers = headers.as_object().context("headers must be an object")?;
            ensure!(headers.len() <= 20, "Too many request headers");
            for (name, value) in headers {
                let lower = name.to_ascii_lowercase();
                ensure!(
                    !matches!(
                        lower.as_str(),
                        "authorization"
                            | "cookie"
                            | "host"
                            | "connection"
                            | "content-length"
                            | "transfer-encoding"
                            | "upgrade"
                            | "x-api-key"
                            | "origin"
                            | "referer"
                            | "forwarded"
                            | "expect"
                            | "x-real-ip"
                            | "x-http-method-override"
                            | "x-method-override"
                    ) && !lower.starts_with("proxy-")
                        && !lower.starts_with("sec-")
                        && !lower.starts_with("x-forwarded-")
                        && spec
                            .credential
                            .as_ref()
                            .is_none_or(|c| !name.eq_ignore_ascii_case(&c.header)),
                    "Reserved request header"
                );
                let value = value.as_str().context("Header value must be a string")?;
                ensure!(value.len() <= 4096, "Request header too long");
                request = request.header(
                    reqwest::header::HeaderName::from_bytes(name.as_bytes())?,
                    reqwest::header::HeaderValue::from_str(value)?,
                );
            }
        }
        let mut secret = None;
        if let Some(credential) = &spec.credential {
            ensure!(
                package.grants.contains("credentials:use"),
                "Credential permission not granted"
            );
            let token = self
                .studio
                .plugin_vault(&self.id)?
                .get_secret(&StudioService::secret_key(&self.workspace, spec)?)?
                .context("Configure a credential for this service first")?;
            request = request.header(
                reqwest::header::HeaderName::from_bytes(credential.header.as_bytes())?,
                reqwest::header::HeaderValue::from_str(&format!("{}{}", credential.prefix, token))?,
            );
            secret = Some(token);
        }
        if let Some(body) = input.get("body") {
            ensure!(
                serde_json::to_vec(body)?.len() <= MAX_HTTP_BYTES,
                "Service request body exceeds 1 MiB"
            );
            request = if let Some(text) = body.as_str() {
                request.body(text.to_owned())
            } else {
                request.json(body)
            };
        }
        let mut response = request
            .send()
            .map_err(|_| anyhow::anyhow!("Service request failed or timed out"))?;
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= MAX_HTTP_BYTES as u64),
            "Service response exceeds 1 MiB"
        );
        let status = response.status().as_u16();
        let mut content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        if let Some(token) = &secret {
            content_type = content_type.replace(token, "[REDACTED]");
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut response)
            .take((MAX_HTTP_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= MAX_HTTP_BYTES,
            "Service response exceeds 1 MiB"
        );
        // Services may echo auth headers. Never return configured token bytes to guests.
        let encoding = input["response_type"].as_str().unwrap_or("json");
        let body = match encoding {
            "base64" => {
                ensure!(
                    secret
                        .as_ref()
                        .is_none_or(|s| !bytes.windows(s.len()).any(|b| b == s.as_bytes())),
                    "Service response contains protected credentials"
                );
                json!(STANDARD.encode(bytes))
            }
            "text" | "json" => {
                let mut text = String::from_utf8(bytes)
                    .context("Service response is not UTF-8; request base64")?;
                if let Some(token) = &secret {
                    text = text.replace(token, "[REDACTED]");
                }
                if encoding == "json" {
                    let mut value = serde_json::from_str(&text)
                        .context("Service did not return JSON; request text or base64")?;
                    if let Some(token) = &secret {
                        redact_json(&mut value, token);
                    }
                    value
                } else {
                    json!(text)
                }
            }
            _ => bail!("response_type must be json, text or base64"),
        };
        Ok(
            json!({"status":status,"headers":{"content-type":content_type},"body":body,"encoding":encoding}),
        )
    }
}

fn redact_json(value: &mut Value, token: &str) {
    match value {
        Value::String(s) => *s = s.replace(token, "[REDACTED]"),
        Value::Array(items) => items.iter_mut().for_each(|v| redact_json(v, token)),
        Value::Object(object) => {
            let old = std::mem::take(object);
            for (key, mut value) in old {
                redact_json(&mut value, token);
                object.insert(key.replace(token, "[REDACTED]"), value);
            }
        }
        _ => {}
    }
}

pub(crate) async fn plugin_configuration(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult {
    let studio = s.studio.clone();
    let workspace = s.agent_state.default_workspace.clone();
    tokio::task::spawn_blocking(move || {
        let _access = studio
            .host_gate
            .lock()
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
        studio.configuration(&workspace, &id)
    })
    .await
    .map_err(error)?
    .map(Json)
    .map_err(error)
}
pub(crate) async fn configure_plugin(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(update): Json<ConfigurationUpdate>,
) -> ApiResult {
    let studio = s.studio.clone();
    let workspace = s.agent_state.default_workspace.clone();
    tokio::task::spawn_blocking(move || studio.configure(&workspace, &id, update))
        .await
        .map_err(error)?
        .map(Json)
        .map_err(error)
}
pub(crate) async fn host_operation(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(request): Json<HostRequest>,
) -> ApiResult {
    let studio = s.studio.clone();
    let workspace = s.agent_state.default_workspace.clone();
    tokio::task::spawn_blocking(move || {
        let package = studio.package(&id).map_err(anyhow::Error::msg)?;
        ensure!(
            id != "presentation",
            "Use the Studio's scoped document routes"
        );
        ScopedHost {
            studio: studio.clone(),
            workspace,
            id,
            manifest_hash: fingerprint(&package)?,
            allow_mutation: true,
            started: Instant::now(),
            requests: Mutex::new(0),
        }
        .call(&request.operation, &request.input)
    })
    .await
    .map_err(error)?
    .map(Json)
    .map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlx_plugin_runtime::{ServiceCredential, ServiceSpec, ToolSpec};
    use std::io::Write;

    fn setup() -> (tempfile::TempDir, Arc<StudioService>, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        let studio = Arc::new(StudioService::new(dir.path().join("state")).unwrap());
        let manifest = Manifest {
            api_version: 1,
            id: "connected".into(),
            name: "Connected".into(),
            version: "1.0.0".into(),
            permissions: [
                "compute",
                "network:request",
                "credentials:use",
                "documents:read",
                "documents:write",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            frontend: None,
            services: vec![ServiceSpec {
                id: "api".into(),
                name: "Example API".into(),
                base_url: "https://example.com".into(),
                methods: vec!["GET".into(), "POST".into()],
                credential: Some(ServiceCredential {
                    id: "token".into(),
                    header: "Authorization".into(),
                    prefix: "Bearer ".into(),
                }),
            }],
            tools: vec![ToolSpec {
                name: "run".into(),
                description: "Test".into(),
                parameters: json!({"type":"object"}),
                export: "run".into(),
                mutating: true,
            }],
        };
        let grants = manifest.permissions.iter().cloned().collect();
        studio
            .install(manifest, b"\0asm\x01\0\0\0".to_vec(), grants)
            .unwrap();
        (dir, studio, workspace)
    }
    fn configure(studio: &StudioService, workspace: &FsPath, body: Value) -> Value {
        studio
            .configure(
                workspace,
                "connected",
                serde_json::from_value(body).unwrap(),
            )
            .unwrap()
    }
    fn update(studio: &StudioService, workspace: &FsPath) -> Value {
        let mut value = studio.configuration(workspace, "connected").unwrap();
        value.as_object_mut().unwrap().remove("available_documents");
        for service in value["services"].as_object_mut().unwrap().values_mut() {
            service.as_object_mut().unwrap().remove("credential_set");
        }
        value
    }
    fn host(studio: &Arc<StudioService>, workspace: &FsPath, mutation: bool) -> ScopedHost {
        ScopedHost {
            studio: studio.clone(),
            workspace: workspace.into(),
            id: "connected".into(),
            manifest_hash: fingerprint(&studio.package("connected").unwrap()).unwrap(),
            allow_mutation: mutation,
            started: Instant::now(),
            requests: Mutex::new(0),
        }
    }
    #[test]
    fn document_grants_are_explicit_workspace_scoped_and_revision_safe() {
        let (_dir, studio, workspace) = setup();
        let a = studio
            .save(
                &workspace,
                Document::new("allowed".into(), "Allowed".into()),
                None,
            )
            .unwrap();
        studio
            .save(
                &workspace,
                Document::new("hidden".into(), "Hidden".into()),
                None,
            )
            .unwrap();
        let mut settings = update(&studio, &workspace);
        settings["documents"] = json!({"ids":[a.id],"allow_create":true});
        configure(&studio, &workspace, settings);
        let host = host(&studio, &workspace, true);
        assert_eq!(
            host.call("documents.list", &json!({}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(host.call("documents.get", &json!({"id":"hidden"})).is_err());
        let operations = json!([{"type":"set_text","slide_id":"slide-1","element_id":"title-1","text":"Revised"}]);
        let request = json!({"id":a.id,"base_revision":a.revision,"operations":operations});
        assert_eq!(
            host.call("documents.edit", &request).unwrap()["revision"],
            2
        );
        assert!(host
            .call("documents.edit", &request)
            .unwrap_err()
            .to_string()
            .contains("REVISION_CONFLICT"));
        assert_eq!(
            studio.get(&workspace, &a.id).unwrap().slides[0].elements[0].text,
            "Revised"
        );
        let created = host
            .call("documents.create", &json!({"title":"Created by plugin"}))
            .unwrap();
        assert!(host
            .call("documents.get", &json!({"id":created["id"]}))
            .is_ok());
        let other = workspace.parent().unwrap().join("other");
        std::fs::create_dir(&other).unwrap();
        assert!(super::tests::host(&studio, &other, true)
            .call("documents.get", &json!({"id":a.id}))
            .is_err());
        assert!(super::tests::host(&studio, &other, true)
            .call("documents.create", &json!({"title":"Denied"}))
            .is_err());
    }
    #[test]
    fn reads_cannot_smuggle_writes_and_revocation_rechecks_existing_hosts() {
        let (_dir, studio, workspace) = setup();
        let mut settings = update(&studio, &workspace);
        settings["documents"]["allow_create"] = json!(true);
        configure(&studio, &workspace, settings);
        let reader = host(&studio, &workspace, false);
        assert!(reader
            .call("documents.create", &json!({"title":"Blocked"}))
            .is_err());
        let mut settings = update(&studio, &workspace);
        settings["grants"] = json!(["compute"]);
        configure(&studio, &workspace, settings);
        assert!(reader.call("documents.list", &json!({})).is_err());
        studio.enable("connected", false).unwrap();
        assert!(reader
            .call("credentials.status", &json!({"service":"api"}))
            .is_err());
    }
    #[test]
    fn credentials_are_encrypted_scoped_write_only_and_removed_on_uninstall() {
        let (dir, studio, workspace) = setup();
        let mut settings = update(&studio, &workspace);
        settings["services"]["api"]["credential"] = json!("protected-demo-token-123");
        let response = configure(&studio, &workspace, settings);
        assert_eq!(response["services"]["api"]["credential_set"], true);
        assert!(!response.to_string().contains("protected-demo-token-123"));
        let file = std::fs::read_to_string(
            dir.path()
                .join("state/credentials/connected/agent_secrets.v1.json"),
        )
        .unwrap();
        assert!(!file.contains("protected-demo-token-123"));
        assert_eq!(
            host(&studio, &workspace, false)
                .call("credentials.status", &json!({"service":"api"}))
                .unwrap(),
            json!({"configured":true})
        );
        let other = workspace.parent().unwrap().join("other");
        std::fs::create_dir(&other).unwrap();
        assert_eq!(
            host(&studio, &other, false)
                .call("credentials.status", &json!({"service":"api"}))
                .unwrap(),
            json!({"configured":false})
        );
        studio.remove("connected").unwrap();
        assert!(!dir.path().join("state/credentials/connected").exists());
    }
    #[test]
    fn network_is_pinned_opt_in_method_scoped_and_redacts_echoed_credentials() {
        let (_dir, studio, workspace) = setup();
        assert!(host(&studio, &workspace, true)
            .call("http.request", &json!({"service":"api","path":"/"}))
            .is_err());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut settings = update(&studio, &workspace);
        settings["services"]["api"] = json!({"base_url":format!("http://{address}"),"enabled":true,"allow_private_network":true,"credential":"demo-protected-key"});
        configure(&studio, &workspace, settings);
        let host = host(&studio, &workspace, false);
        for input in [
            json!({"service":"api","path":"//evil.example"}),
            json!({"service":"api","path":"/","method":"DELETE"}),
            json!({"service":"api","path":"/","method":"POST","body":{}}),
            json!({"service":"unlisted","path":"/"}),
        ] {
            assert!(host.call("http.request", &input).is_err());
        }
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            let n = stream.read(&mut bytes).unwrap();
            assert!(String::from_utf8_lossy(&bytes[..n]).contains("Bearer demo-protected-key"));
            let body = r#"{"echo":"Bearer demo\u002dprotected\u002dkey","demo\u002dprotected\u002dkey":{"nested":["demo-protected-key"]},"value":42}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json; echoed=demo-protected-key\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let response = host
            .call("http.request", &json!({"service":"api","path":"/data"}))
            .unwrap();
        assert_eq!(response["body"]["value"], 42);
        assert!(!response.to_string().contains("demo-protected-key"));
        assert_eq!(response["body"]["echo"], "Bearer [REDACTED]");
        server.join().unwrap();
        let mut settings = update(&studio, &workspace);
        settings["services"]["api"]["allow_private_network"] = json!(false);
        assert!(studio
            .configure(
                &workspace,
                "connected",
                serde_json::from_value(settings).unwrap()
            )
            .is_err());
    }
    #[test]
    fn metadata_targets_and_untrusted_origins_are_blocked() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "0.0.0.0",
            "::1",
            "fc00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
            "2002:7f00:1::",
            "::127.0.0.1",
        ] {
            assert!(!allowed_ip(ip.parse().unwrap(), false), "{ip}");
        }
        assert!(!allowed_ip("169.254.169.254".parse().unwrap(), true));
        assert!(allowed_ip("127.0.0.1".parse().unwrap(), true));
        assert!(allowed_ip("8.8.8.8".parse().unwrap(), false));
        for url in [
            "https://user:password@example.com",
            "https://example.com?key=secret",
            "file:///tmp/test",
            "http://example.com",
        ] {
            assert!(validate_base(url, false).is_err());
        }
        for origin in [
            "https://evil.example",
            "null",
            "http://localhost.evil.example",
        ] {
            assert!(!crate::trusted_ui_origin(&origin.parse().unwrap()));
        }
        for origin in [
            "tauri://localhost",
            "http://127.0.0.1:8765",
            "http://tauri.localhost",
            "https://tauri.localhost",
            "http://localhost:8000",
        ] {
            assert!(crate::trusted_ui_origin(&origin.parse().unwrap()));
        }
    }
    #[test]
    fn server_rejects_remote_origins_and_rebinding_hosts_without_relying_on_cors() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("host", "127.0.0.1:11435".parse().unwrap());
        assert!(crate::trusted_request_headers(&headers));
        headers.insert("origin", "https://untrusted.example".parse().unwrap());
        assert!(!crate::trusted_request_headers(&headers));
        headers.remove("origin");
        headers.insert("host", "rebind.example:11435".parse().unwrap());
        assert!(!crate::trusted_request_headers(&headers));
        headers.insert("host", "localhost@rebind.example".parse().unwrap());
        assert!(!crate::trusted_request_headers(&headers));
        headers.insert("host", "[::1]:11435".parse().unwrap());
        headers.insert("origin", "tauri://localhost".parse().unwrap());
        assert!(crate::trusted_request_headers(&headers));
    }
    #[test]
    fn changed_service_manifest_requires_fresh_authorization() {
        let (_dir, studio, workspace) = setup();
        let mut settings = update(&studio, &workspace);
        settings["services"]["api"]["enabled"] = json!(true);
        configure(&studio, &workspace, settings);
        let old_host = host(&studio, &workspace, false);
        let mut package = studio.package("connected").unwrap();
        package.manifest.services[0].base_url = "https://other.example".into();
        studio
            .install(package.manifest, package.wasm, package.grants)
            .unwrap();
        let config = studio.configuration(&workspace, "connected").unwrap();
        assert_eq!(config["services"]["api"]["enabled"], false);
        assert!(old_host
            .call("credentials.status", &json!({"service":"api"}))
            .is_err());
    }
}
