//! Compile the release-gate WAT fixture with the workspace's locked wat version.
fn main() -> anyhow::Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("usage: plugin_security_fixture OUTPUT_JSON"))?;
    let wasm = wat::parse_str(include_str!(
        "../../../scripts/release-gate/fixtures/plugin-security/broker.wat"
    ))?;
    std::fs::write(
        output,
        serde_json::to_vec(&serde_json::json!({ "wasm": wasm }))?,
    )?;
    Ok(())
}
