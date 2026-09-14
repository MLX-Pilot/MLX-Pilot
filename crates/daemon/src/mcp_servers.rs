//! Registro de servidores MCP configurados no MLX Pilot.
//!
//! Guarda a lista em `<config>/mcp-servers.json` e mantem um cache das
//! ferramentas que cada servidor anuncia, para a UI montar o seletor do no
//! `mcp.call` sem subir o processo a cada renderizacao.
//!
//! A conexao e por chamada: o servidor sobe, responde e e encerrado. E mais
//! lento que manter o processo vivo, mas nao deixa processo orfao quando o
//! daemon cai, e um no de workflow nao e um caminho quente.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mlx_flow::mcp::{self, McpServerConfig, McpTool};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::warn;

/// Teto de tempo de uma chamada a um servidor MCP.
const MCP_TIMEOUT: Duration = Duration::from_secs(60);

/// Servidor com o resultado da ultima sondagem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatus {
    #[serde(flatten)]
    pub config: McpServerConfig,
    /// Ferramentas anunciadas na ultima sondagem bem-sucedida.
    #[serde(default)]
    pub tools: Vec<McpTool>,
    /// `None` enquanto o servidor nunca foi sondado.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reachable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Arquivo de configuracao.
#[derive(Debug, Default, Serialize, Deserialize)]
struct McpServerFile {
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

/// Repositorio dos servidores MCP.
#[derive(Clone)]
pub struct McpRegistry {
    path: PathBuf,
    /// Ferramentas por servidor, preenchido pelas sondagens.
    tools: Arc<RwLock<HashMap<String, Vec<McpTool>>>>,
}

impl std::fmt::Debug for McpRegistry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("McpRegistry")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl McpRegistry {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            path: root.as_ref().join("mcp-servers.json"),
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Servidores configurados. Lista vazia quando o arquivo nao existe.
    pub async fn list(&self) -> io::Result<Vec<McpServerConfig>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = tokio::fs::read_to_string(&self.path).await?;
        let file: McpServerFile = serde_json::from_str(&raw)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(file.servers)
    }

    /// Servidores com o cache de ferramentas anexado.
    pub async fn list_with_tools(&self) -> io::Result<Vec<McpServerStatus>> {
        let cache = self.tools.read().await;
        Ok(self
            .list()
            .await?
            .into_iter()
            .map(|config| {
                let tools = cache
                    .get(&config.normalized_name())
                    .cloned()
                    .unwrap_or_default();
                McpServerStatus {
                    config,
                    tools,
                    reachable: None,
                    error: None,
                }
            })
            .collect())
    }

    /// Um servidor pelo nome, ignorando caixa.
    pub async fn get(&self, name: &str) -> io::Result<Option<McpServerConfig>> {
        let wanted = name.trim().to_lowercase();
        Ok(self
            .list()
            .await?
            .into_iter()
            .find(|server| server.normalized_name() == wanted))
    }

    /// Cria ou substitui um servidor pelo nome.
    pub async fn save(&self, server: McpServerConfig) -> Result<Vec<McpServerConfig>, String> {
        server.validate()?;
        let mut servers = self.list().await.map_err(|error| error.to_string())?;
        let name = server.normalized_name();
        servers.retain(|existing| existing.normalized_name() != name);
        servers.push(server);
        servers.sort_by_key(|server| server.normalized_name());
        self.write(&servers).await.map_err(|e| e.to_string())?;
        Ok(servers)
    }

    /// Remove um servidor. `false` quando ele nao existia.
    pub async fn delete(&self, name: &str) -> io::Result<bool> {
        let wanted = name.trim().to_lowercase();
        let mut servers = self.list().await?;
        let before = servers.len();
        servers.retain(|server| server.normalized_name() != wanted);
        if servers.len() == before {
            return Ok(false);
        }
        self.write(&servers).await?;
        self.tools.write().await.remove(&wanted);
        Ok(true)
    }

    /// Sobe o servidor, lista as ferramentas e guarda no cache.
    pub async fn probe(&self, name: &str) -> McpServerStatus {
        let config = match self.get(name).await {
            Ok(Some(config)) => config,
            Ok(None) => {
                return McpServerStatus {
                    config: McpServerConfig {
                        name: name.to_string(),
                        command: String::new(),
                        args: Vec::new(),
                        env: HashMap::new(),
                        cwd: None,
                        enabled: false,
                        description: None,
                    },
                    tools: Vec::new(),
                    reachable: Some(false),
                    error: Some(format!("nao existe servidor MCP chamado `{name}`")),
                }
            }
            Err(error) => {
                return McpServerStatus {
                    config: McpServerConfig {
                        name: name.to_string(),
                        command: String::new(),
                        args: Vec::new(),
                        env: HashMap::new(),
                        cwd: None,
                        enabled: false,
                        description: None,
                    },
                    tools: Vec::new(),
                    reachable: Some(false),
                    error: Some(error.to_string()),
                }
            }
        };

        match self.fetch_tools(&config).await {
            Ok(tools) => {
                self.tools
                    .write()
                    .await
                    .insert(config.normalized_name(), tools.clone());
                McpServerStatus {
                    config,
                    tools,
                    reachable: Some(true),
                    error: None,
                }
            }
            Err(error) => McpServerStatus {
                config,
                tools: Vec::new(),
                reachable: Some(false),
                error: Some(error),
            },
        }
    }

    /// Executa uma ferramenta de um servidor configurado.
    pub async fn call_tool(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<mcp::McpToolResult, String> {
        let config = self
            .get(server)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("nao existe servidor MCP chamado `{server}`"))?;
        if !config.enabled {
            return Err(format!("o servidor MCP `{server}` esta desativado"));
        }

        let client = mcp::connect(&config, MCP_TIMEOUT)
            .await
            .map_err(|error| error.to_string())?;
        let result = client.call_tool(tool, arguments).await;
        // Encerra o processo independentemente do resultado.
        let _ = client.close().await;
        result.map_err(|error| error.to_string())
    }

    async fn fetch_tools(&self, config: &McpServerConfig) -> Result<Vec<McpTool>, String> {
        let client = mcp::connect(config, MCP_TIMEOUT)
            .await
            .map_err(|error| error.to_string())?;
        let tools = client.list_tools().await.map_err(|error| error.to_string());
        let _ = client.close().await;
        tools
    }

    async fn write(&self, servers: &[McpServerConfig]) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = McpServerFile {
            servers: servers.to_vec(),
        };
        let serialized = serde_json::to_vec_pretty(&file)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let temp = self.path.with_extension("json.tmp");
        tokio::fs::write(&temp, serialized).await?;
        if let Err(error) = tokio::fs::rename(&temp, &self.path).await {
            let _ = tokio::fs::remove_file(&temp).await;
            warn!(%error, "nao foi possivel gravar a lista de servidores MCP");
            return Err(error);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> (tempfile::TempDir, McpRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let registry = McpRegistry::new(dir.path());
        (dir, registry)
    }

    fn server(name: &str) -> McpServerConfig {
        McpServerConfig {
            name: name.to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "@modelcontextprotocol/server-filesystem".to_string()],
            env: HashMap::new(),
            cwd: None,
            enabled: true,
            description: None,
        }
    }

    #[tokio::test]
    async fn listing_an_absent_file_is_empty_not_an_error() {
        let (_dir, registry) = registry();
        assert!(registry.list().await.unwrap().is_empty());
        assert!(registry.list_with_tools().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn save_round_trips_and_sorts_by_name() {
        let (_dir, registry) = registry();
        registry.save(server("zeta")).await.unwrap();
        registry.save(server("alfa")).await.unwrap();

        let servers = registry.list().await.unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "alfa");
        assert_eq!(servers[1].args.len(), 2);
    }

    #[tokio::test]
    async fn save_replaces_by_name_ignoring_case() {
        let (_dir, registry) = registry();
        registry.save(server("Arquivos")).await.unwrap();
        let mut updated = server("arquivos");
        updated.command = "node".to_string();
        registry.save(updated).await.unwrap();

        let servers = registry.list().await.unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].command, "node");
    }

    #[tokio::test]
    async fn save_rejects_an_invalid_config() {
        let (_dir, registry) = registry();
        let mut broken = server("x");
        broken.command = String::new();
        assert!(registry.save(broken).await.is_err());
    }

    #[tokio::test]
    async fn get_and_delete_ignore_case() {
        let (_dir, registry) = registry();
        registry.save(server("Arquivos")).await.unwrap();

        assert!(registry.get("ARQUIVOS").await.unwrap().is_some());
        assert!(registry.delete("arquivos").await.unwrap());
        assert!(!registry.delete("arquivos").await.unwrap());
        assert!(registry.get("Arquivos").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn calling_a_disabled_server_is_refused_before_spawning() {
        let (_dir, registry) = registry();
        let mut disabled = server("desligado");
        disabled.enabled = false;
        registry.save(disabled).await.unwrap();

        let error = registry
            .call_tool("desligado", "x", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(error.contains("desativado"), "{error}");
    }

    #[tokio::test]
    async fn calling_an_unknown_server_is_reported() {
        let (_dir, registry) = registry();
        let error = registry
            .call_tool("fantasma", "x", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(error.contains("fantasma"));
    }

    #[tokio::test]
    async fn probing_an_unknown_server_reports_unreachable() {
        let (_dir, registry) = registry();
        let status = registry.probe("fantasma").await;
        assert_eq!(status.reachable, Some(false));
        assert!(status.error.unwrap().contains("fantasma"));
    }

    #[tokio::test]
    async fn probing_a_server_that_cannot_start_reports_the_reason() {
        let (_dir, registry) = registry();
        let mut broken = server("quebrado");
        broken.command = "binario-que-nao-existe-xyz".to_string();
        broken.args = Vec::new();
        registry.save(broken).await.unwrap();

        let status = registry.probe("quebrado").await;
        assert_eq!(status.reachable, Some(false));
        assert!(status.error.is_some());
        assert!(status.tools.is_empty());
    }

    #[tokio::test]
    async fn no_temp_file_is_left_behind() {
        let (dir, registry) = registry();
        registry.save(server("a")).await.unwrap();
        let mut entries = tokio::fs::read_dir(dir.path()).await.unwrap();
        while let Some(entry) = entries.next_entry().await.unwrap() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(!name.ends_with(".tmp"), "sobrou temporario: {name}");
        }
    }
}
