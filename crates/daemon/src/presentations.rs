//! Presentation Studio persistence, plugin lifecycle and local-model editing.
use crate::AppState;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use mlx_agent_tools::{ExecutionMode, Tool, ToolContext, ToolError, ToolResult};
use mlx_plugin_runtime::{Manifest, Package};
use mlx_presentation::{valid_id, Document, MAX_DOCUMENT_BYTES};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path as FsPath, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
fn error(e: impl ToString) -> (StatusCode, Json<Value>) {
    let message = e.to_string();
    let status = if message.starts_with("REVISION_CONFLICT") {
        StatusCode::CONFLICT
    } else {
        StatusCode::BAD_REQUEST
    };
    (status, Json(json!({"error":message})))
}

pub struct StudioService {
    root: PathBuf,
    gate: Mutex<()>,
    packages: RwLock<BTreeMap<String, Package>>,
}
impl StudioService {
    pub fn new(root: PathBuf) -> anyhow::Result<Self> {
        std::fs::create_dir_all(root.join("plugins"))?;
        let mut packages = BTreeMap::new();
        for entry in std::fs::read_dir(root.join("plugins"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|x| x == "json") {
                match Package::load(&path) {
                    Ok(p) => {
                        packages.insert(p.manifest.id.clone(), p);
                    }
                    Err(e) => tracing::warn!("Ignoring invalid plugin: {e}"),
                }
            }
        }
        let bundled = include_bytes!("../../../plugins/presentation-studio/plugin.wasm");
        let bundled_hash = format!("{:x}", Sha256::digest(bundled));
        if !root.join("plugins/presentation.removed").exists()
            && packages
                .get("presentation")
                .is_none_or(|p: &Package| p.sha256 != bundled_hash)
        {
            let manifest: Manifest = serde_json::from_str(include_str!(
                "../../../plugins/presentation-studio/plugin.json"
            ))?;
            let wasm = include_bytes!("../../../plugins/presentation-studio/plugin.wasm").to_vec();
            let grants = packages
                .get("presentation")
                .map(|p| p.grants.clone())
                .unwrap_or_else(|| manifest.permissions.iter().cloned().collect());
            let enabled = packages.get("presentation").is_none_or(|p| p.enabled);
            let mut p = Package::new(manifest, wasm, grants)?;
            p.enabled = enabled;
            p.save(&root.join("plugins/presentation.json"))?;
            packages.insert("presentation".into(), p);
        }
        Ok(Self {
            root,
            gate: Mutex::new(()),
            packages: RwLock::new(packages),
        })
    }
    fn directory(&self, workspace: &FsPath) -> Result<PathBuf, String> {
        self.require_presentation()?;
        let path = workspace.canonicalize().map_err(|e| e.to_string())?;
        let scope = format!("{:x}", Sha256::digest(path.to_string_lossy().as_bytes()));
        let dir = self.root.join("projects").join(scope);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        Ok(dir)
    }
    fn path(&self, workspace: &FsPath, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("invalid project id".into());
        }
        Ok(self.directory(workspace)?.join(format!("{id}.json")))
    }
    pub fn get(&self, workspace: &FsPath, id: &str) -> Result<Document, String> {
        let p = self.path(workspace, id)?;
        let bytes = std::fs::read(p).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_DOCUMENT_BYTES {
            return Err("document too large".into());
        }
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())
    }
    pub fn list(&self, workspace: &FsPath) -> Result<Value, String> {
        let mut docs = vec![];
        for e in std::fs::read_dir(self.directory(workspace)?).map_err(|e| e.to_string())? {
            let path = e.map_err(|e| e.to_string())?.path();
            if path.extension().is_some_and(|v| v == "json") {
                if let Some(id) = path.file_stem().and_then(|x| x.to_str()) {
                    if let Ok(d) = self.get(workspace, id) {
                        docs.push(json!({"id":d.id,"title":d.title,"revision":d.revision,"slides":d.slides.len()}));
                    }
                }
            }
        }
        Ok(json!(docs))
    }
    pub fn save(
        &self,
        workspace: &FsPath,
        mut doc: Document,
        base: Option<u64>,
    ) -> Result<Document, String> {
        let _lock = self.gate.lock().map_err(|e| e.to_string())?;
        doc.validate()?;
        let path = self.path(workspace, &doc.id)?;
        if path.exists() {
            let old = self.get(workspace, &doc.id)?;
            if base != Some(old.revision) {
                return Err(format!(
                    "REVISION_CONFLICT: expected {}, received {:?}",
                    old.revision, base
                ));
            }
            let history = self.directory(workspace)?.join(&doc.id);
            std::fs::create_dir_all(&history).map_err(|e| e.to_string())?;
            std::fs::write(
                history.join(format!("{}.json", old.revision)),
                serde_json::to_vec(&old).unwrap(),
            )
            .map_err(|e| e.to_string())?;
            doc.revision = old.revision + 1;
            if doc.revision > 20 {
                let _ = std::fs::remove_file(history.join(format!("{}.json", doc.revision - 21)));
            }
        } else {
            if base.is_some() {
                return Err("project does not exist".into());
            }
            doc.revision = 1;
        }
        let temp = path.with_extension("tmp");
        std::fs::write(&temp, serde_json::to_vec(&doc).unwrap()).map_err(|e| e.to_string())?;
        std::fs::rename(temp, path).map_err(|e| e.to_string())?;
        Ok(doc)
    }
    pub fn inventory(&self) -> Value {
        json!(self
            .packages
            .read()
            .unwrap()
            .values()
            .map(|p| {
                let mut manifest = serde_json::to_value(&p.manifest).unwrap();
                manifest.as_object_mut().unwrap().remove("frontend");
                json!({"manifest":manifest,"enabled":p.enabled,"grants":p.grants,"sha256":p.sha256,
                "has_frontend":p.manifest.frontend.is_some() || p.manifest.id == "presentation",
                "bundled":p.manifest.id == "presentation"})
            })
            .collect::<Vec<_>>())
    }
    pub fn package(&self, id: &str) -> Result<Package, String> {
        self.packages
            .read()
            .map_err(|e| e.to_string())?
            .get(id)
            .cloned()
            .ok_or("unknown plugin".into())
    }
    pub fn install(
        &self,
        m: Manifest,
        wasm: Vec<u8>,
        grants: BTreeSet<String>,
    ) -> Result<Value, String> {
        if m.id == "presentation" {
            return Err("bundled plugin cannot be replaced by third-party packages".into());
        }
        let _guard = self.gate.lock().map_err(|e| e.to_string())?;
        let p = Package::new(m, wasm, grants).map_err(|e| e.to_string())?;
        p.save(
            &self
                .root
                .join("plugins")
                .join(format!("{}.json", p.manifest.id)),
        )
        .map_err(|e| e.to_string())?;
        self.packages
            .write()
            .unwrap()
            .insert(p.manifest.id.clone(), p);
        Ok(self.inventory())
    }
    pub fn enable(&self, id: &str, enabled: bool) -> Result<Value, String> {
        let _guard = self.gate.lock().map_err(|e| e.to_string())?;
        let mut p = self.package(id)?;
        p.enabled = enabled;
        p.save(&self.root.join("plugins").join(format!("{id}.json")))
            .map_err(|e| e.to_string())?;
        self.packages.write().unwrap().insert(id.into(), p);
        Ok(self.inventory())
    }
    fn require_presentation(&self) -> Result<(), String> {
        let p = self.package("presentation")?;
        if !p.enabled || !p.manifest.permissions.iter().all(|g| p.grants.contains(g)) {
            return Err(
                "Presentation Studio desativado ou sem permissoes. Ative-o em Plugins.".into(),
            );
        }
        Ok(())
    }
    pub fn remove(&self, id: &str) -> Result<Value, String> {
        let _guard = self.gate.lock().map_err(|e| e.to_string())?;
        self.package(id)?;
        if id == "presentation" {
            std::fs::write(self.root.join("plugins/presentation.removed"), b"removed")
                .map_err(|e| e.to_string())?;
        }
        std::fs::remove_file(self.root.join("plugins").join(format!("{id}.json")))
            .map_err(|e| e.to_string())?;
        self.packages.write().unwrap().remove(id);
        Ok(self.inventory())
    }
    pub fn restore_bundled(&self) -> Result<Value, String> {
        let _guard = self.gate.lock().map_err(|e| e.to_string())?;
        let manifest: Manifest = serde_json::from_str(include_str!(
            "../../../plugins/presentation-studio/plugin.json"
        ))
        .map_err(|e| e.to_string())?;
        let grants = manifest.permissions.iter().cloned().collect();
        let p = Package::new(
            manifest,
            include_bytes!("../../../plugins/presentation-studio/plugin.wasm").to_vec(),
            grants,
        )
        .map_err(|e| e.to_string())?;
        p.save(&self.root.join("plugins/presentation.json"))
            .map_err(|e| e.to_string())?;
        let marker = self.root.join("plugins/presentation.removed");
        if marker.exists() {
            std::fs::remove_file(marker).map_err(|e| e.to_string())?;
        }
        self.packages
            .write()
            .unwrap()
            .insert("presentation".into(), p);
        Ok(self.inventory())
    }
    pub fn frontend(&self, id: &str) -> Result<Value, String> {
        let p = self.package(id)?;
        if !p.enabled || !p.manifest.permissions.iter().all(|g| p.grants.contains(g)) {
            return Err("Plugin disabled or permissions missing".into());
        }
        p.manifest
            .frontend
            .map(|ui| json!(ui))
            .ok_or("Plugin has no packaged frontend".into())
    }
    pub fn edit(
        &self,
        workspace: &FsPath,
        id: &str,
        base: u64,
        operations: Value,
    ) -> Result<Document, String> {
        let mut doc = self.get(workspace, id)?;
        if doc.revision != base {
            return Err("REVISION_CONFLICT: reload the project before editing".into());
        }
        // Editing changes asset references, not pixel data. Keep large binary assets
        // in the host so a title edit never spends WASM fuel parsing megabytes of base64.
        let assets = std::mem::take(&mut doc.assets);
        doc.assets = assets.iter().map(|a| mlx_presentation::Asset {
            id: a.id.clone(), name: a.name.clone(), description: a.description.clone(),
            data_url: "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=".into(),
        }).collect();
        let result = self
            .package("presentation")?
            .invoke("edit", &json!({"document":doc,"operations":operations}))
            .map_err(|e| e.to_string())?;
        let mut edited: Document =
            serde_json::from_value(result["document"].clone()).map_err(|e| e.to_string())?;
        if edited.id != id {
            return Err("plugin changed project identity".into());
        }
        edited.assets = assets;
        self.save(workspace, edited, Some(base))
    }
    pub fn readonly_tool_names(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = [
            "presentation_list",
            "presentation_get",
            "presentation_validate",
            "presentation_instructions",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        for (id, t) in self.tool_specs() {
            if !t.mutating {
                names.insert(format!("{}__{}", id, t.name));
            }
        }
        names
    }
    pub fn tool_specs(&self) -> Vec<(String, mlx_plugin_runtime::ToolSpec)> {
        self.packages
            .read()
            .unwrap()
            .values()
            .filter(|p| {
                p.enabled
                    && p.manifest.permissions.iter().all(|x| p.grants.contains(x))
                    && p.manifest.id != "presentation"
            })
            .flat_map(|p| {
                p.manifest
                    .tools
                    .iter()
                    .cloned()
                    .map(|t| (p.manifest.id.clone(), t))
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

pub(crate) async fn list(State(s): State<AppState>) -> ApiResult {
    s.studio
        .list(&s.agent_state.default_workspace)
        .map(Json)
        .map_err(error)
}
pub(crate) async fn create(State(s): State<AppState>, Json(v): Json<Value>) -> ApiResult {
    let title = v["title"].as_str().unwrap_or("Nova apresentação");
    let d = Document::new(uuid::Uuid::new_v4().to_string(), title.into());
    s.studio
        .save(&s.agent_state.default_workspace, d, None)
        .map(|d| Json(json!(d)))
        .map_err(error)
}
pub(crate) async fn get(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.studio
        .get(&s.agent_state.default_workspace, &id)
        .map(|d| Json(json!(d)))
        .map_err(error)
}
#[derive(Deserialize)]
pub(crate) struct SaveRequest {
    base_revision: u64,
    document: Document,
}
pub(crate) async fn save(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<SaveRequest>,
) -> ApiResult {
    if id != v.document.id {
        return Err(error("project id mismatch"));
    }
    s.studio
        .save(
            &s.agent_state.default_workspace,
            v.document,
            Some(v.base_revision),
        )
        .map(|d| Json(json!(d)))
        .map_err(error)
}
#[derive(Deserialize)]
pub(crate) struct EditRequest {
    base_revision: u64,
    operations: Value,
}
pub(crate) async fn edit(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<EditRequest>,
) -> ApiResult {
    let service = s.studio.clone();
    let workspace = s.agent_state.default_workspace.clone();
    tokio::task::spawn_blocking(move || {
        service.edit(&workspace, &id, v.base_revision, v.operations)
    })
    .await
    .map_err(error)?
    .map(|d| Json(json!(d)))
    .map_err(error)
}
pub(crate) async fn validate(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let doc = s
        .studio
        .get(&s.agent_state.default_workspace, &id)
        .map_err(error)?;
    Ok(Json(json!({"diagnostics":doc.validate().map_err(error)?})))
}
pub(crate) async fn history(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    let d = s
        .studio
        .get(&s.agent_state.default_workspace, &id)
        .map_err(error)?;
    let start = d.revision.saturating_sub(20).max(1);
    Ok(Json(
        json!({"current":d.revision,"revisions":(start..d.revision).collect::<Vec<_>>()}),
    ))
}
pub(crate) async fn restore(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> ApiResult {
    let current = s
        .studio
        .get(&s.agent_state.default_workspace, &id)
        .map_err(error)?;
    let revision = v["revision"]
        .as_u64()
        .ok_or_else(|| error("missing revision"))?;
    let base = v["base_revision"]
        .as_u64()
        .ok_or_else(|| error("missing base_revision"))?;
    let path = s
        .studio
        .directory(&s.agent_state.default_workspace)
        .map_err(error)?
        .join(&id)
        .join(format!("{revision}.json"));
    let mut d: Document =
        serde_json::from_slice(&std::fs::read(path).map_err(error)?).map_err(error)?;
    d.id = current.id;
    s.studio
        .save(&s.agent_state.default_workspace, d, Some(base))
        .map(|d| Json(json!(d)))
        .map_err(error)
}
pub(crate) async fn plugins(State(s): State<AppState>) -> ApiResult {
    Ok(Json(s.studio.inventory()))
}
#[derive(Deserialize)]
pub(crate) struct Install {
    manifest: Manifest,
    wasm_base64: String,
    grants: BTreeSet<String>,
}
pub(crate) async fn install(State(s): State<AppState>, Json(v): Json<Install>) -> ApiResult {
    let bytes = STANDARD.decode(&v.wasm_base64).map_err(error)?;
    let service = s.studio.clone();
    tokio::task::spawn_blocking(move || service.install(v.manifest, bytes, v.grants))
        .await
        .map_err(error)?
        .map(Json)
        .map_err(error)
}
pub(crate) async fn toggle(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> ApiResult {
    s.studio
        .enable(
            &id,
            v["enabled"]
                .as_bool()
                .ok_or_else(|| error("enabled must be boolean"))?,
        )
        .map(Json)
        .map_err(error)
}
pub(crate) async fn uninstall(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.studio.remove(&id).map(Json).map_err(error)
}
pub(crate) async fn restore_plugin(State(s): State<AppState>) -> ApiResult {
    let service = s.studio.clone();
    tokio::task::spawn_blocking(move || service.restore_bundled())
        .await
        .map_err(error)?
        .map(Json)
        .map_err(error)
}
pub(crate) async fn frontend(State(s): State<AppState>, Path(id): Path<String>) -> ApiResult {
    s.studio.frontend(&id).map(Json).map_err(error)
}
pub(crate) async fn invoke(
    State(s): State<AppState>,
    Path((id, tool)): Path<(String, String)>,
    Json(v): Json<Value>,
) -> ApiResult {
    if id == "presentation" {
        return Err(error("Use the scoped project edit endpoint"));
    }
    let p = s.studio.package(&id).map_err(error)?;
    tokio::task::spawn_blocking(move || p.invoke(&tool, &v))
        .await
        .map_err(error)?
        .map(Json)
        .map_err(error)
}

pub(crate) async fn instructions() -> Json<Value> {
    Json(json!({"instructions":include_str!("../../../plugins/presentation-studio/AGENT.md")}))
}

// Registered agent tools share the same project store and the plugin permission boundary.
pub struct StudioTool {
    name: String,
    description: String,
    schema: Value,
    service: Arc<StudioService>,
    plugin: Option<(String, String)>,
    mutating: bool,
}
impl StudioTool {
    pub fn builtins(service: Arc<StudioService>) -> Vec<Arc<dyn Tool>> {
        if service.require_presentation().is_err() {
            return vec![];
        }
        [("presentation_instructions","Read the presentation editing contract, operations, design rules and examples before editing slides.",json!({"type":"object","properties":{},"additionalProperties":false}),false),
  ("presentation_list","List saved presentations in this workspace.",json!({"type":"object","properties":{},"additionalProperties":false}),false),
  ("presentation_create","Read presentation_instructions first. Create a presentation. Then use presentation_edit to add slides. Coordinates are 1280x720.",json!({"type":"object","properties":{"title":{"type":"string","maxLength":300}},"required":["title"],"additionalProperties":false}),true),
  ("presentation_get","Read a presentation without image bytes. Use its revision in edits. Assets include descriptions.",json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false}),false),
  ("presentation_edit","Edit a presentation atomically. Operations: set_text, update_element, add_element, add_slide, remove_slide, set_theme, set_notes. Use base_revision from get. Elements: text/image/shape/chart/table; x,y,w,h in 1280x720; animation none/fade/rise/slide/zoom.",json!({"type":"object","properties":{"project_id":{"type":"string"},"base_revision":{"type":"integer","minimum":1},"operations":{"type":"array","minItems":1,"maxItems":100,"items":{"type":"object"}}},"required":["project_id","base_revision","operations"],"additionalProperties":false}),true),
  ("presentation_validate","Validate slide geometry and document structure.",json!({"type":"object","properties":{"project_id":{"type":"string"}},"required":["project_id"],"additionalProperties":false}),false)]
   .into_iter().map(|(n,d,s,m)|Arc::new(Self{name:n.into(),description:d.into(),schema:s,service:service.clone(),plugin:None,mutating:m}) as Arc<dyn Tool>).collect()
    }
    pub fn plugins(service: Arc<StudioService>) -> Vec<Arc<dyn Tool>> {
        service
            .tool_specs()
            .into_iter()
            .map(|(id, t)| {
                Arc::new(Self {
                    name: format!("{}__{}", id, t.name),
                    description: t.description,
                    schema: t.parameters,
                    service: service.clone(),
                    plugin: Some((id, t.name)),
                    mutating: t.mutating,
                }) as Arc<dyn Tool>
            })
            .collect()
    }
}
#[async_trait::async_trait]
impl Tool for StudioTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn parameters(&self) -> &Value {
        &self.schema
    }
    async fn execute(&self, p: &Value, ctx: &ToolContext) -> Result<ToolResult, ToolError> {
        if ctx.mode == ExecutionMode::Locked
            || self.mutating && matches!(ctx.mode, ExecutionMode::ReadOnly | ExecutionMode::DryRun)
        {
            return Err(ToolError::ModeRestriction { mode: ctx.mode });
        }
        let service = self.service.clone();
        let workspace = ctx.workspace_root.clone();
        let name = self.name.clone();
        let plugin = self.plugin.clone();
        let p = p.clone();
        let value=tokio::task::spawn_blocking(move||->Result<Value,String>{
   if let Some((id,tool))=plugin{return service.package(&id)?.invoke(&tool,&p).map_err(|e|e.to_string());}
   let builtin=service.package("presentation")?;if !builtin.enabled || !builtin.manifest.permissions.iter().all(|x|builtin.grants.contains(x)){return Err("Presentation plugin disabled or permission revoked".into());}
   let id=p["project_id"].as_str().unwrap_or("");
   match name.as_str(){
    "presentation_instructions"=>Ok(json!({"instructions":include_str!("../../../plugins/presentation-studio/AGENT.md"),"agent_usage":"Call presentation_get, then pass operations to presentation_edit with its revision. Call presentation_validate afterwards. Use the Presentation Studio UI for rendering and exporting PDF, PPTX and animated HTML."})),
    "presentation_list"=>service.list(&workspace),
    "presentation_create"=>Ok(json!(service.save(&workspace,Document::new(uuid::Uuid::new_v4().to_string(),p["title"].as_str().unwrap_or("Presentation").into()),None)?)),
    "presentation_get"=>{let d=service.get(&workspace,id)?;let mut v=json!(d);if let Some(assets)=v["assets"].as_array_mut(){for a in assets{a.as_object_mut().unwrap().remove("data_url");}}Ok(v)},
    "presentation_edit"=>{let d=service.edit(&workspace,id,p["base_revision"].as_u64().ok_or("missing revision")?,p["operations"].clone())?;Ok(json!({"id":d.id,"revision":d.revision,"slides":d.slides.len(),"diagnostics":d.validate()?}))},
    "presentation_validate"=>Ok(json!(service.get(&workspace,id)?.validate()?)),_=>Err("unknown tool".into())
   }
  }).await.map_err(|e|ToolError::ExecutionFailed{message:e.to_string()})?.map_err(|e|ToolError::ExecutionFailed{message:e})?;
        Ok(ToolResult {
            output: serde_json::to_string(&value).unwrap(),
            is_error: false,
            metadata: Default::default(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saves_conflicts_history_and_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let service = StudioService::new(dir.path().join("state")).unwrap();
        let ws = dir.path();
        let d = service
            .save(ws, Document::new("one".into(), "One".into()), None)
            .unwrap();
        assert_eq!(d.revision, 1);
        let mut changed = d.clone();
        changed.title = "Two".into();
        let saved = service.save(ws, changed, Some(1)).unwrap();
        assert_eq!(saved.revision, 2);
        assert!(service
            .save(ws, d, Some(1))
            .unwrap_err()
            .starts_with("REVISION_CONFLICT"));
        assert!(service.get(ws, "../escape").is_err());
        let ws2 = dir.path().join("other");
        std::fs::create_dir(&ws2).unwrap();
        assert!(service.get(&ws2, "one").is_err());
    }
    #[test]
    fn edits_execute_real_wasm() {
        let dir = tempfile::tempdir().unwrap();
        let service = StudioService::new(dir.path().join("state")).unwrap();
        let d = service
            .save(
                dir.path(),
                Document::new("one".into(), "Before".into()),
                None,
            )
            .unwrap();
        let edited=service.edit(dir.path(),&d.id,d.revision,json!([{"type":"set_text","slide_id":"slide-1","element_id":"title-1","text":"After"}])).unwrap();
        assert_eq!(edited.slides[0].elements[0].text, "After");
        service.enable("presentation", false).unwrap();
        assert!(service
            .edit(
                dir.path(),
                &d.id,
                edited.revision,
                json!([{"type":"set_title","text":"Denied"}])
            )
            .is_err());
    }
    #[test]
    fn wasm_edits_preserve_large_opaque_assets_without_spending_fuel_on_pixels() {
        let dir = tempfile::tempdir().unwrap();
        let service = StudioService::new(dir.path().join("state")).unwrap();
        let mut d = Document::new("images".into(), "Images".into());
        let data_url = format!("data:image/png;base64,{}", "A".repeat(6 * 1024 * 1024));
        d.assets.push(mlx_presentation::Asset {
            id: "large".into(),
            name: "large.png".into(),
            description: "Reference".into(),
            data_url: data_url.clone(),
        });
        let d = service.save(dir.path(), d, None).unwrap();
        let edited=service.edit(dir.path(),&d.id,d.revision,json!([{"type":"set_text","slide_id":"slide-1","element_id":"title-1","text":"Updated with images"}])).unwrap();
        assert_eq!(edited.assets[0].data_url, data_url);
        assert_eq!(edited.slides[0].elements[0].text, "Updated with images");
    }
    #[test]
    fn plugin_removal_persists_and_reinstall_restores_documents() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("state");
        let service = StudioService::new(root.clone()).unwrap();
        let doc = service
            .save(
                dir.path(),
                Document::new("kept".into(), "Preserved".into()),
                None,
            )
            .unwrap();
        service.enable("presentation", false).unwrap();
        assert!(service
            .save(dir.path(), doc.clone(), Some(doc.revision))
            .is_err());
        assert!(service.list(dir.path()).is_err());
        drop(service);
        let service = StudioService::new(root.clone()).unwrap();
        assert!(!service.package("presentation").unwrap().enabled);
        service.remove("presentation").unwrap();
        drop(service);
        let service = StudioService::new(root).unwrap();
        assert!(service.package("presentation").is_err());
        assert!(service.remove("../escape").is_err());
        service.restore_bundled().unwrap();
        assert_eq!(service.get(dir.path(), "kept").unwrap().title, "Preserved");
    }
    #[test]
    fn frontend_requires_enabled_package_and_grants() {
        let dir = tempfile::tempdir().unwrap();
        let service = StudioService::new(dir.path().join("state")).unwrap();
        let mut manifest: Manifest = serde_json::from_str(include_str!(
            "../../../plugins/presentation-studio/plugin.json"
        ))
        .unwrap();
        manifest.id = "example".into();
        manifest.frontend = Some(mlx_plugin_runtime::Frontend {
            html: "<h1>Hello</h1>".into(),
            css: String::new(),
            javascript: String::new(),
        });
        let wasm = include_bytes!("../../../plugins/presentation-studio/plugin.wasm").to_vec();
        service
            .install(manifest.clone(), wasm.clone(), BTreeSet::new())
            .unwrap();
        assert!(service.frontend("example").is_err());
        let grants = manifest.permissions.iter().cloned().collect();
        service.install(manifest, wasm, grants).unwrap();
        assert_eq!(
            service.frontend("example").unwrap()["html"],
            "<h1>Hello</h1>"
        );
        service.enable("example", false).unwrap();
        assert!(service.frontend("example").is_err());
        service.remove("example").unwrap();
        assert!(service.frontend("example").is_err());
    }
}
