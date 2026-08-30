use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::{
    env, fs,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

const DEFAULT_N8N_BASE_URL: &str = "http://127.0.0.1:5678";
const DEFAULT_MLX_DAEMON_URL: &str = "http://127.0.0.1:11435";
const DEFAULT_OLLAMA_BASE_URL: &str = "http://127.0.0.1:11434";
const DEFAULT_MODEL_ID: &str = "qwen3.5:9b";

#[derive(Debug, Deserialize)]
pub struct N8nStatusQuery {
    #[serde(default)]
    base_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct N8nApiRequest {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct N8nGenerateWorkflowRequest {
    #[serde(default)]
    base_url: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mlx_base_url: Option<String>,
    #[serde(default)]
    ollama_base_url: Option<String>,
    #[serde(default)]
    workflow_model_id: Option<String>,
    #[serde(default)]
    agent_provider: Option<String>,
    #[serde(default)]
    agent_model_id: Option<String>,
    #[serde(default)]
    agent_base_url: Option<String>,
    #[serde(default)]
    agent_api_key: Option<String>,
    #[serde(default)]
    agent_provider_profile_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct N8nStatusResponse {
    mode: String,
    base_url: String,
    api_url: String,
    health_url: String,
    editor_url: String,
    source_tree: N8nSourceStatus,
    healthy: bool,
    status_code: Option<u16>,
    message: String,
}

#[derive(Debug, Serialize)]
struct N8nSourceStatus {
    present: bool,
    path: Option<String>,
    version: Option<String>,
    package_manager: Option<String>,
    node_engine: Option<String>,
    pnpm_engine: Option<String>,
    message: String,
}

#[derive(Debug, Serialize)]
pub struct N8nWorkflowListResponse {
    base_url: String,
    api_url: String,
    workflows: Value,
}

#[derive(Debug, Serialize)]
pub struct N8nGenerateWorkflowResponse {
    created: bool,
    base_url: String,
    api_url: String,
    editor_url: Option<String>,
    workflow_id: Option<String>,
    generated_workflow: Value,
    workflow: Value,
}

#[derive(Debug, Serialize)]
struct N8nErrorResponse {
    error: String,
    details: Option<String>,
}

pub async fn status(Query(query): Query<N8nStatusQuery>) -> Json<N8nStatusResponse> {
    let base_url = normalize_base_url(query.base_url.as_deref());
    let health_url = format!("{base_url}/healthz");
    let api_url = api_url(&base_url);
    let client = http_client();
    let source_tree = n8n_source_status();

    match client.get(&health_url).send().await {
        Ok(response) => {
            let status = response.status();
            let healthy = status.is_success();
            Json(N8nStatusResponse {
                mode: "direct-api".to_string(),
                base_url: base_url.clone(),
                api_url,
                health_url,
                editor_url: base_url,
                source_tree,
                healthy,
                status_code: Some(status.as_u16()),
                message: if healthy {
                    "n8n reachable".to_string()
                } else {
                    format!("n8n health returned HTTP {}", status.as_u16())
                },
            })
        }
        Err(error) => Json(N8nStatusResponse {
            mode: "direct-api".to_string(),
            base_url: base_url.clone(),
            api_url,
            health_url,
            editor_url: base_url,
            source_tree,
            healthy: false,
            status_code: None,
            message: format!("n8n unreachable: {error}"),
        }),
    }
}

pub async fn list_workflows(Json(request): Json<N8nApiRequest>) -> Response {
    let base_url = normalize_base_url(request.base_url.as_deref());
    let api_key = match required_api_key(request.api_key.as_deref()) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let api_url = api_url(&base_url);
    let endpoint = format!("{api_url}/workflows");

    match http_client()
        .get(&endpoint)
        .header("X-N8N-API-KEY", api_key)
        .send()
        .await
    {
        Ok(response) => {
            json_response_from_n8n(response, |body| {
                Json(N8nWorkflowListResponse {
                    base_url: base_url.clone(),
                    api_url: api_url.clone(),
                    workflows: body,
                })
                .into_response()
            })
            .await
        }
        Err(error) => n8n_error(
            StatusCode::BAD_GATEWAY,
            "n8n_request_failed",
            Some(error.to_string()),
        ),
    }
}

pub async fn generate_workflow(
    State(state): State<super::AppState>,
    Json(request): Json<N8nGenerateWorkflowRequest>,
) -> Response {
    let base_url = normalize_base_url(request.base_url.as_deref());
    let api_key = match required_api_key(request.api_key.as_deref()) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let prompt = match required_prompt(request.prompt.as_deref()) {
        Ok(value) => value,
        Err(response) => return response,
    };
    let api_url = api_url(&base_url);
    let endpoint = format!("{api_url}/workflows");

    let generated_workflow =
        match generate_workflow_json_with_agent(&state, &request, &prompt).await {
            Ok(value) => value,
            Err(response) => return response,
        };
    let workflow = match normalize_generated_workflow(generated_workflow, &request) {
        Ok(value) => value,
        Err(response) => return response,
    };

    match http_client()
        .post(&endpoint)
        .header("X-N8N-API-KEY", api_key)
        .json(&workflow)
        .send()
        .await
    {
        Ok(response) => {
            json_response_from_n8n(response, |body| {
                let workflow_id = body
                    .get("id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                let editor_url = workflow_id
                    .as_deref()
                    .map(|id| format!("{}/workflow/{}", base_url, id));
                Json(N8nGenerateWorkflowResponse {
                    created: true,
                    base_url: base_url.clone(),
                    api_url: api_url.clone(),
                    editor_url,
                    workflow_id,
                    generated_workflow: workflow,
                    workflow: body,
                })
                .into_response()
            })
            .await
        }
        Err(error) => n8n_error(
            StatusCode::BAD_GATEWAY,
            "n8n_request_failed",
            Some(error.to_string()),
        ),
    }
}

async fn generate_workflow_json_with_agent(
    state: &super::AppState,
    request: &N8nGenerateWorkflowRequest,
    prompt: &str,
) -> Result<Value, Response> {
    let agent_request = workflow_generator_agent_request(request, prompt);
    let agent_response = match crate::agent_api::execute_agent_request(state, agent_request).await {
        Ok(response) => response,
        Err(error) => return Err(error.into_response()),
    };

    parse_json_object_from_text(&agent_response.content).map_err(|error| {
        n8n_error(
            StatusCode::BAD_GATEWAY,
            "workflow_generation_failed",
            Some(error),
        )
    })
}

fn workflow_generator_agent_request(
    request: &N8nGenerateWorkflowRequest,
    prompt: &str,
) -> crate::agent_api::AgentRunRequest {
    crate::agent_api::AgentRunRequest {
        session_id: None,
        message: workflow_generator_user_message(request, prompt),
        provider: request.agent_provider.clone(),
        model_id: request.agent_model_id.clone(),
        api_key: request.agent_api_key.clone(),
        base_url: workflow_generator_agent_base_url(request),
        custom_headers: None,
        streaming: Some(false),
        fallback_enabled: Some(false),
        fallback_provider: None,
        fallback_model_id: None,
        execution_mode: Some("dry_run".to_string()),
        approval_mode: Some("deny".to_string()),
        system_prompt: Some(workflow_generator_system_prompt()),
        max_iterations: Some(1),
        max_prompt_tokens: None,
        max_history_messages: Some(0),
        max_tools_in_prompt: Some(0),
        temperature: Some(0.1),
        aggressive_tool_filtering: Some(true),
        enable_tool_call_fallback: Some(false),
        runtime_variant: Some("classic".to_string()),
        persist_tool_events: Some(false),
        session_search_enabled: Some(false),
        memory_profile: Some("minimal".to_string()),
        memory_snapshot_mode: None,
        session_context: None,
        gateway_context: None,
        delegate_depth: None,
        enabled_skills: Some(Vec::new()),
        enabled_tools: Some(Vec::new()),
        toolset_id: None,
        provider_profile_id: request.agent_provider_profile_id.clone(),
        workspace_root: None,
    }
}

fn workflow_generator_agent_base_url(request: &N8nGenerateWorkflowRequest) -> Option<String> {
    if let Some(value) = request
        .agent_base_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(value.to_string());
    }

    let provider = request
        .agent_provider
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if provider.eq_ignore_ascii_case("ollama") {
        return Some(normalize_service_url(
            request.ollama_base_url.as_deref(),
            DEFAULT_OLLAMA_BASE_URL,
        ));
    }

    None
}

fn workflow_generator_system_prompt() -> String {
    [
        "You generate n8n workflow JSON for MLX Pilot.",
        "Return only one valid JSON object. Do not use markdown fences, comments, prose, or explanations.",
        "The JSON must be suitable for n8n Public API POST /api/v1/workflows.",
        "Required top-level fields: name, nodes, connections, settings.",
        "Only include top-level fields name, nodes, connections, and settings.",
        "Do not include id, active, createdAt, updatedAt, versionId, tags, staticData, pinData, meta, shared, isArchived, or triggerCount.",
        "In workflow settings, only use n8n workflow settings such as executionOrder, timezone, saveManualExecutions, saveExecutionProgress, saveDataErrorExecution, saveDataSuccessExecution, executionTimeout, callerPolicy, callerIds, binaryMode, redactionPolicy, availableInMCP.",
        "Never put active, credentials, manual, nodes, or connections inside settings.",
        "Keep workflows inactive/manual unless the user explicitly asks for a trigger.",
        "Use n8n node names as keys in connections.",
        "If a workflow calls MLX Pilot, use an HTTP Request node that POSTs to /agent/run.",
        "Do not invent credentials. If credentials are needed, create the node without credentials and make the name clear.",
    ]
    .join(" ")
}

fn workflow_generator_user_message(request: &N8nGenerateWorkflowRequest, prompt: &str) -> String {
    let name = trimmed_or(request.name.as_deref(), "MLX Pilot Generated Workflow");
    let mlx_base_url =
        normalize_service_url(request.mlx_base_url.as_deref(), DEFAULT_MLX_DAEMON_URL);
    let ollama_base_url =
        normalize_service_url(request.ollama_base_url.as_deref(), DEFAULT_OLLAMA_BASE_URL);
    let workflow_model_id = trimmed_or(request.workflow_model_id.as_deref(), DEFAULT_MODEL_ID);

    format!(
        r#"Create an n8n workflow JSON object from this request:

Workflow name: {name}
MLX Pilot base URL: {mlx_base_url}
Ollama base URL: {ollama_base_url}
Default model for MLX Pilot HTTP Request nodes: {workflow_model_id}

User request:
{prompt}

For MLX Pilot calls, use an HTTP Request node like:
{{
  "parameters": {{
    "method": "POST",
    "url": "{mlx_base_url}/agent/run",
    "sendBody": true,
    "specifyBody": "json",
    "jsonBody": "={{\"message\":\"...\",\"provider\":\"ollama\",\"model_id\":\"{workflow_model_id}\",\"base_url\":\"{ollama_base_url}\",\"max_iterations\":1}}",
    "options": {{}}
  }},
  "name": "Ask MLX Pilot",
  "type": "n8n-nodes-base.httpRequest",
  "typeVersion": 4.5,
  "position": [520, 300]
}}

Return only the workflow JSON object."#
    )
}

fn normalize_generated_workflow(
    mut workflow: Value,
    request: &N8nGenerateWorkflowRequest,
) -> Result<Value, Response> {
    let object = workflow.as_object_mut().ok_or_else(|| {
        n8n_error(
            StatusCode::BAD_GATEWAY,
            "workflow_generation_invalid",
            Some("generated JSON must be an object".to_string()),
        )
    })?;

    for key in [
        "id",
        "active",
        "createdAt",
        "updatedAt",
        "isArchived",
        "versionId",
        "tags",
        "staticData",
        "pinData",
        "meta",
        "triggerCount",
        "shared",
    ] {
        object.remove(key);
    }

    let name = trimmed_or(request.name.as_deref(), "MLX Pilot Generated Workflow");
    if object
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_none()
    {
        object.insert("name".to_string(), json!(name));
    }

    let nodes = object
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            n8n_error(
                StatusCode::BAD_GATEWAY,
                "workflow_generation_invalid",
                Some("generated workflow must include a nodes array".to_string()),
            )
        })?;
    if nodes.is_empty() {
        return Err(n8n_error(
            StatusCode::BAD_GATEWAY,
            "workflow_generation_invalid",
            Some("generated workflow must include at least one node".to_string()),
        ));
    }

    for (index, node) in nodes.iter_mut().enumerate() {
        let node_object = node.as_object_mut().ok_or_else(|| {
            n8n_error(
                StatusCode::BAD_GATEWAY,
                "workflow_generation_invalid",
                Some("every node must be a JSON object".to_string()),
            )
        })?;
        if !node_object.contains_key("id") {
            node_object.insert("id".to_string(), json!(Uuid::new_v4().to_string()));
        }
        if node_object
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            node_object.insert("id".to_string(), json!(Uuid::new_v4().to_string()));
        }
        if node_object
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            node_object.insert("name".to_string(), json!(format!("Node {}", index + 1)));
        }
        if !node_object.contains_key("position") {
            node_object.insert(
                "position".to_string(),
                json!([260 + (index as i64 * 260), 300]),
            );
        }
        if !valid_node_position(node_object.get("position")) {
            node_object.insert(
                "position".to_string(),
                json!([260 + (index as i64 * 260), 300]),
            );
        }
        retain_allowed_keys(node_object, N8N_PUBLIC_API_NODE_KEYS);
    }

    if !object.get("connections").is_some_and(Value::is_object) {
        object.insert("connections".to_string(), json!({}));
    }

    sanitize_workflow_settings(object);
    retain_allowed_keys(object, N8N_PUBLIC_API_WORKFLOW_KEYS);

    Ok(workflow)
}

const N8N_PUBLIC_API_WORKFLOW_KEYS: &[&str] = &["name", "nodes", "connections", "settings"];

const N8N_PUBLIC_API_NODE_KEYS: &[&str] = &[
    "id",
    "name",
    "webhookId",
    "disabled",
    "notesInFlow",
    "notes",
    "type",
    "typeVersion",
    "executeOnce",
    "alwaysOutputData",
    "retryOnFail",
    "maxTries",
    "waitBetweenTries",
    "continueOnFail",
    "onError",
    "position",
    "parameters",
    "credentials",
    "customTelemetryTags",
];

fn retain_allowed_keys(object: &mut Map<String, Value>, allowed: &[&str]) {
    let unknown = object
        .keys()
        .filter(|key| !allowed.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    for key in unknown {
        object.remove(&key);
    }
}

fn valid_node_position(value: Option<&Value>) -> bool {
    let Some(items) = value.and_then(Value::as_array) else {
        return false;
    };
    items.len() >= 2 && items.iter().take(2).all(Value::is_number)
}

fn sanitize_workflow_settings(workflow: &mut Map<String, Value>) {
    let mut sanitized = Map::new();
    if let Some(settings) = workflow.get("settings").and_then(Value::as_object) {
        copy_bool_setting(settings, &mut sanitized, "saveExecutionProgress");
        copy_bool_setting(settings, &mut sanitized, "saveManualExecutions");
        copy_bool_setting(settings, &mut sanitized, "availableInMCP");

        copy_number_setting(settings, &mut sanitized, "executionTimeout");
        copy_number_setting(settings, &mut sanitized, "timeSavedPerExecution");

        copy_string_setting(settings, &mut sanitized, "errorWorkflow");
        copy_string_setting(settings, &mut sanitized, "timezone");
        copy_string_setting(settings, &mut sanitized, "callerIds");

        copy_enum_setting(
            settings,
            &mut sanitized,
            "saveDataErrorExecution",
            &["all", "none"],
        );
        copy_enum_setting(
            settings,
            &mut sanitized,
            "saveDataSuccessExecution",
            &["all", "none"],
        );
        copy_enum_setting(
            settings,
            &mut sanitized,
            "binaryMode",
            &["separate", "combined"],
        );
        copy_enum_setting(
            settings,
            &mut sanitized,
            "callerPolicy",
            &[
                "any",
                "none",
                "workflowsFromAList",
                "workflowsFromSameOwner",
            ],
        );
        copy_enum_setting(
            settings,
            &mut sanitized,
            "timeSavedMode",
            &["fixed", "dynamic"],
        );
        copy_enum_setting(
            settings,
            &mut sanitized,
            "redactionPolicy",
            &["none", "non-manual", "manual-only", "all"],
        );
        copy_enum_setting(settings, &mut sanitized, "executionOrder", &["v0", "v1"]);
    }

    sanitized
        .entry("executionOrder".to_string())
        .or_insert_with(|| json!("v1"));
    workflow.insert("settings".to_string(), Value::Object(sanitized));
}

fn copy_bool_setting(source: &Map<String, Value>, target: &mut Map<String, Value>, key: &str) {
    if let Some(value) = source.get(key).and_then(Value::as_bool) {
        target.insert(key.to_string(), json!(value));
    }
}

fn copy_number_setting(source: &Map<String, Value>, target: &mut Map<String, Value>, key: &str) {
    if source.get(key).is_some_and(Value::is_number) {
        target.insert(key.to_string(), source[key].clone());
    }
}

fn copy_string_setting(source: &Map<String, Value>, target: &mut Map<String, Value>, key: &str) {
    if let Some(value) = source
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        target.insert(key.to_string(), json!(value));
    }
}

fn copy_enum_setting(
    source: &Map<String, Value>,
    target: &mut Map<String, Value>,
    key: &str,
    allowed: &[&str],
) {
    if let Some(value) = source
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| allowed.contains(value))
    {
        target.insert(key.to_string(), json!(value));
    }
}

fn parse_json_object_from_text(text: &str) -> Result<Value, String> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return ensure_json_object(value);
    }

    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if start < end {
            let candidate = &trimmed[start..=end];
            let value = serde_json::from_str::<Value>(candidate)
                .map_err(|error| format!("agent response did not contain valid JSON: {error}"))?;
            return ensure_json_object(value);
        }
    }

    Err("agent response did not contain a JSON object".to_string())
}

fn ensure_json_object(value: Value) -> Result<Value, String> {
    if value.is_object() {
        Ok(value)
    } else {
        Err("agent response JSON must be an object".to_string())
    }
}

async fn json_response_from_n8n(
    response: reqwest::Response,
    ok: impl FnOnce(Value) -> Response,
) -> Response {
    let status = response.status();
    let body_text = response.text().await.unwrap_or_default();
    let parsed =
        serde_json::from_str::<Value>(&body_text).unwrap_or_else(|_| json!({ "raw": body_text }));

    if status.is_success() {
        return ok(parsed);
    }

    n8n_error(
        StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
        "n8n_api_error",
        Some(parsed.to_string()),
    )
}

fn required_api_key(value: Option<&str>) -> Result<&str, Response> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            n8n_error(
                StatusCode::BAD_REQUEST,
                "n8n_api_key_required",
                Some("Create an API key in n8n Settings > n8n API.".to_string()),
            )
        })
}

fn required_prompt(value: Option<&str>) -> Result<String, Response> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            n8n_error(
                StatusCode::BAD_REQUEST,
                "workflow_prompt_required",
                Some("Prompt cannot be empty.".to_string()),
            )
        })
}

fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap_or_else(|_| Client::new())
}

fn normalize_base_url(value: Option<&str>) -> String {
    let base = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_N8N_BASE_URL);
    normalize_service_url(Some(base), DEFAULT_N8N_BASE_URL)
        .trim_end_matches("/api/v1")
        .to_string()
}

fn normalize_service_url(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .trim_end_matches('/')
        .to_string()
}

fn api_url(base_url: &str) -> String {
    format!("{}/api/v1", base_url.trim_end_matches('/'))
}

fn trimmed_or(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn n8n_source_status() -> N8nSourceStatus {
    let Some(source_root) = find_n8n_source_root() else {
        return N8nSourceStatus {
            present: false,
            path: None,
            version: None,
            package_manager: None,
            node_engine: None,
            pnpm_engine: None,
            message: "vendor/n8n source tree not found".to_string(),
        };
    };

    let package_json_path = source_root.join("package.json");
    let package_json = match fs::read_to_string(&package_json_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
    {
        Some(package_json) => package_json,
        None => {
            return N8nSourceStatus {
                present: true,
                path: Some(source_root.display().to_string()),
                version: None,
                package_manager: None,
                node_engine: None,
                pnpm_engine: None,
                message: "vendor/n8n exists, but package.json could not be read".to_string(),
            };
        }
    };

    let metadata = n8n_source_metadata(&package_json);
    N8nSourceStatus {
        present: true,
        path: Some(source_root.display().to_string()),
        version: metadata.version,
        package_manager: metadata.package_manager,
        node_engine: metadata.node_engine,
        pnpm_engine: metadata.pnpm_engine,
        message: "vendor/n8n source tree found".to_string(),
    }
}

fn find_n8n_source_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(cwd) = env::current_dir() {
        starts.push(cwd);
    }
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));

    for start in starts {
        if let Some(source_root) = find_n8n_source_root_from(&start) {
            return Some(source_root);
        }
    }

    None
}

fn find_n8n_source_root_from(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join("vendor").join("n8n");
        if candidate.join("package.json").is_file() {
            return Some(candidate);
        }
    }
    None
}

#[derive(Debug, PartialEq, Eq)]
struct N8nSourceMetadata {
    version: Option<String>,
    package_manager: Option<String>,
    node_engine: Option<String>,
    pnpm_engine: Option<String>,
}

fn n8n_source_metadata(package_json: &Value) -> N8nSourceMetadata {
    N8nSourceMetadata {
        version: package_json
            .get("version")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        package_manager: package_json
            .get("packageManager")
            .and_then(Value::as_str)
            .map(ToString::to_string),
        node_engine: package_json
            .get("engines")
            .and_then(|engines| engines.get("node"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        pnpm_engine: package_json
            .get("engines")
            .and_then(|engines| engines.get("pnpm"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
    }
}

fn n8n_error(status: StatusCode, error: &str, details: Option<String>) -> Response {
    (
        status,
        Json(N8nErrorResponse {
            error: error.to_string(),
            details,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generate_request() -> N8nGenerateWorkflowRequest {
        N8nGenerateWorkflowRequest {
            base_url: None,
            api_key: None,
            prompt: Some("Crie um workflow manual".to_string()),
            name: Some("Workflow teste".to_string()),
            mlx_base_url: None,
            ollama_base_url: None,
            workflow_model_id: Some("llama3.2:3b".to_string()),
            agent_provider: None,
            agent_model_id: None,
            agent_base_url: None,
            agent_api_key: None,
            agent_provider_profile_id: None,
        }
    }

    #[test]
    fn n8n_source_metadata_reads_runtime_requirements() {
        let metadata = n8n_source_metadata(&json!({
            "version": "2.37.0",
            "packageManager": "pnpm@11.22.0",
            "engines": {
                "node": ">=24.0.0",
                "pnpm": ">=11.22.0"
            }
        }));

        assert_eq!(
            metadata,
            N8nSourceMetadata {
                version: Some("2.37.0".to_string()),
                package_manager: Some("pnpm@11.22.0".to_string()),
                node_engine: Some(">=24.0.0".to_string()),
                pnpm_engine: Some(">=11.22.0".to_string()),
            }
        );
    }

    #[test]
    fn parse_json_object_from_text_accepts_markdown_wrapped_json() {
        let parsed = parse_json_object_from_text(
            r#"Here is the workflow:
```json
{"name":"Generated","nodes":[{"name":"Manual Trigger"}],"connections":{}}
```"#,
        )
        .expect("parsed workflow");

        assert_eq!(parsed["name"], "Generated");
    }

    #[test]
    fn normalize_generated_workflow_adds_required_defaults() {
        let workflow = normalize_generated_workflow(
            json!({
                "nodes": [
                    {
                        "name": "Manual Trigger",
                        "type": "n8n-nodes-base.manualTrigger",
                        "typeVersion": 1
                    }
                ]
            }),
            &generate_request(),
        )
        .expect("normalized workflow");

        assert_eq!(workflow["name"], "Workflow teste");
        assert!(workflow["nodes"][0]["id"].is_string());
        assert!(workflow["nodes"][0]["position"].is_array());
        assert!(workflow["connections"].is_object());
        assert_eq!(workflow["settings"]["executionOrder"], "v1");
    }

    #[test]
    fn normalize_generated_workflow_sanitizes_n8n_public_api_shape() {
        let workflow = normalize_generated_workflow(
            json!({
                "active": false,
                "tags": [],
                "nodes": [
                    {
                        "name": "Manual Trigger",
                        "type": "n8n-nodes-base.manualTrigger",
                        "typeVersion": 1,
                        "position": "left",
                        "settings": {
                            "manual": true
                        }
                    }
                ],
                "connections": {},
                "settings": {
                    "active": false,
                    "credentials": {},
                    "manual": true,
                    "saveManualExecutions": true,
                    "saveDataSuccessExecution": "DEFAULT",
                    "executionOrder": "v1",
                    "timezone": "America/Sao_Paulo"
                }
            }),
            &generate_request(),
        )
        .expect("normalized workflow");

        assert!(workflow.get("active").is_none());
        assert!(workflow.get("tags").is_none());
        assert!(workflow["nodes"][0].get("settings").is_none());
        assert!(workflow["nodes"][0]["position"].is_array());
        assert!(workflow["settings"].get("active").is_none());
        assert!(workflow["settings"].get("credentials").is_none());
        assert!(workflow["settings"].get("manual").is_none());
        assert!(workflow["settings"]
            .get("saveDataSuccessExecution")
            .is_none());
        assert_eq!(workflow["settings"]["saveManualExecutions"], true);
        assert_eq!(workflow["settings"]["timezone"], "America/Sao_Paulo");
        assert_eq!(workflow["settings"]["executionOrder"], "v1");
    }

    #[test]
    fn workflow_generator_message_includes_mlx_agent_request_shape() {
        let message = workflow_generator_user_message(&generate_request(), "Resuma dados");

        assert!(message.contains("/agent/run"));
        assert!(message.contains("\\\"provider\\\":\\\"ollama\\\""));
        assert!(message.contains("\\\"model_id\\\":\\\"llama3.2:3b\\\""));
    }

    #[test]
    fn workflow_generator_uses_external_ollama_base_url_for_agent() {
        let mut request = generate_request();
        request.agent_provider = Some("ollama".to_string());
        request.agent_base_url = None;
        request.ollama_base_url = Some("http://127.0.0.1:11434/".to_string());

        let agent_request = workflow_generator_agent_request(&request, "Crie workflow");

        assert_eq!(
            agent_request.base_url.as_deref(),
            Some("http://127.0.0.1:11434")
        );
    }

    #[test]
    fn workflow_generator_disables_tools_skills_and_fallback() {
        let agent_request = workflow_generator_agent_request(&generate_request(), "Crie workflow");

        assert_eq!(agent_request.enabled_tools, Some(Vec::new()));
        assert_eq!(agent_request.enabled_skills, Some(Vec::new()));
        assert_eq!(agent_request.fallback_enabled, Some(false));
        assert_eq!(agent_request.runtime_variant.as_deref(), Some("classic"));
    }
}
