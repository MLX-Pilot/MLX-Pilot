//! Executable probe for the proposed inference ABI. This loader is test-only.
use anyhow::{ensure, Result};
use extism::{Manifest, Plugin, PluginBuilder, Wasm};
use mlx_ollama_core::{ChatMessage, ChatRequest, ChatResponse, MessageRole, ModelDescriptor};
use serde_json::{json, Value};

const METADATA: &str = include_str!("fixtures/provider-v1.json");
const GUEST: &str = include_str!("fixtures/provider-v1.wat");

fn load_probe(metadata: &Value, wasm: Vec<u8>) -> Result<Plugin> {
    ensure!(
        metadata["kind"] == "inference_provider"
            && metadata["api_version"] == 2
            && metadata["provider_abi"] == 1,
        "Unsupported provider contract"
    );
    ensure!(
        metadata["permissions"] == json!(["compute"]),
        "Expected compute-only fixture"
    );
    let mut manifest = Manifest::new([Wasm::data(wasm)]);
    manifest.memory.max_pages = Some(64);
    manifest.timeout_ms = Some(5000);
    let guest = PluginBuilder::new(manifest)
        .with_wasi(false)
        .with_fuel_limit(1_000_000)
        .build()?;
    for export in ["provider_list_models", "provider_chat"] {
        ensure!(guest.function_exists(export), "Missing export {export}");
    }
    if metadata["capabilities"]["streaming"] == true {
        ensure!(
            guest.function_exists("provider_chat_stream"),
            "Missing stream export"
        );
    }
    Ok(guest)
}

#[test]
fn loads_provider_and_reads_core_wire_types() -> Result<()> {
    let metadata: Value = serde_json::from_str(METADATA)?;
    let mut guest = load_probe(&metadata, wat::parse_str(GUEST)?)?;
    let config = json!({});
    let schema = jsonschema::validator_for(&metadata["config_schema"])
        .map_err(|err| anyhow::anyhow!(err.to_string()))?;
    assert!(schema.is_valid(&config));
    assert!(!schema.is_valid(&json!({"unexpected": true})));

    let output: String = guest.call(
        "provider_list_models",
        json!({"config": config}).to_string(),
    )?;
    let catalog: Value = serde_json::from_str(&output)?;
    let models: Vec<ModelDescriptor> = serde_json::from_value(catalog["result"].clone())?;
    let provider = format!("plugin.{}", metadata["id"].as_str().unwrap());
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].provider, provider);
    assert!(models[0].is_available);

    let request = ChatRequest {
        model_id: models[0].id.clone(),
        messages: vec![ChatMessage::text(MessageRole::User, "ping")],
        options: Default::default(),
    };
    let input = json!({"config": config, "request": request, "tools": []});
    let output: String = guest.call("provider_chat", input.to_string())?;
    let reply: Value = serde_json::from_str(&output)?;
    let response: ChatResponse = serde_json::from_value(reply["result"].clone())?;
    assert_eq!(response.provider, provider);
    assert_eq!(response.model_id, request.model_id);
    assert_eq!(response.message.role, MessageRole::Assistant);
    assert_eq!(response.message.content, "pong");
    Ok(())
}

#[test]
fn rejects_incompatible_version_and_missing_exports() -> Result<()> {
    let mut metadata: Value = serde_json::from_str(METADATA)?;
    metadata["provider_abi"] = json!(2);
    assert!(load_probe(&metadata, vec![])
        .err()
        .unwrap()
        .to_string()
        .contains("Unsupported provider contract"));
    metadata["provider_abi"] = json!(1);
    let incomplete = wat::parse_str(
        "(module (func (export \"provider_list_models\") (result i32) i32.const 0))",
    )?;
    assert!(load_probe(&metadata, incomplete)
        .err()
        .unwrap()
        .to_string()
        .contains("Missing export provider_chat"));
    metadata["capabilities"]["streaming"] = json!(true);
    assert!(load_probe(&metadata, wat::parse_str(GUEST)?)
        .err()
        .unwrap()
        .to_string()
        .contains("Missing stream export"));
    Ok(())
}
