//! Cliente do Model Context Protocol.
//!
//! O MCP e JSON-RPC 2.0 sobre um transporte. Este modulo separa as duas
//! camadas: [`McpTransport`] troca linhas de texto, e [`McpClient`] fala o
//! protocolo em cima disso. A separacao existe para o protocolo ser testavel
//! sem subir processo nenhum — o transporte de producao ([`StdioTransport`])
//! conversa com um servidor MCP por stdin/stdout.
//!
//! Cobre o subconjunto que um no de workflow precisa: `initialize`, a
//! notificacao `notifications/initialized`, `tools/list` e `tools/call`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

/// Versao do protocolo que anunciamos no handshake.
pub const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Falha ao falar com um servidor MCP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpError {
    pub message: String,
}

impl McpError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for McpError {}

/// Definicao de um servidor MCP configurado pelo usuario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Nome curto usado nos nos do fluxo. Unico.
    pub name: String,
    /// Executavel do servidor, ex.: `npx`.
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Variaveis de ambiente extras do processo do servidor.
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

fn default_true() -> bool {
    true
}

impl McpServerConfig {
    /// Nome normalizado: minusculo, sem espacos nas pontas.
    pub fn normalized_name(&self) -> String {
        self.name.trim().to_lowercase()
    }

    /// Erro legivel quando a configuracao nao da para usar.
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("o servidor MCP precisa de um nome".to_string());
        }
        if self.command.trim().is_empty() {
            return Err(format!(
                "o servidor MCP `{}` precisa de um comando",
                self.name
            ));
        }
        Ok(())
    }
}

/// Uma ferramenta anunciada por um servidor MCP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// JSON Schema dos parametros, como o servidor declarou.
    #[serde(default)]
    pub input_schema: Value,
}

/// Transporte de linhas JSON-RPC.
#[async_trait]
pub trait McpTransport: Send + Sync {
    /// Envia uma linha (sem `\n`; o transporte acrescenta).
    async fn send(&self, line: String) -> Result<(), McpError>;
    /// Espera a proxima linha recebida.
    async fn recv(&self) -> Result<String, McpError>;
    /// Encerra o transporte.
    async fn close(&self) -> Result<(), McpError>;
}

/// Cliente MCP sobre um transporte qualquer.
pub struct McpClient {
    transport: Box<dyn McpTransport>,
    next_id: AtomicI64,
    timeout: Duration,
}

impl McpClient {
    pub fn new(transport: Box<dyn McpTransport>, timeout: Duration) -> Self {
        Self {
            transport,
            next_id: AtomicI64::new(1),
            timeout,
        }
    }

    /// Handshake obrigatorio antes de qualquer outra chamada.
    pub async fn initialize(&self) -> Result<Value, McpError> {
        let result = self
            .request(
                "initialize",
                json!({
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": { "tools": {} },
                    "clientInfo": { "name": "mlx-pilot", "version": env!("CARGO_PKG_VERSION") },
                }),
            )
            .await?;

        // O servidor so aceita chamadas depois desta notificacao.
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result)
    }

    /// Ferramentas anunciadas pelo servidor.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
        let result = self.request("tools/list", json!({})).await?;
        let raw = result
            .get("tools")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        Ok(raw
            .iter()
            .filter_map(|tool| {
                let name = tool.get("name").and_then(Value::as_str)?;
                Some(McpTool {
                    name: name.to_string(),
                    description: tool
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    // Servidores usam `inputSchema`; aceita a forma snake_case
                    // tambem porque implementacoes variam.
                    input_schema: tool
                        .get("inputSchema")
                        .or_else(|| tool.get("input_schema"))
                        .cloned()
                        .unwrap_or_else(|| json!({})),
                })
            })
            .collect())
    }

    /// Executa uma ferramenta.
    pub async fn call_tool(&self, name: &str, arguments: Value) -> Result<McpToolResult, McpError> {
        let result = self
            .request(
                "tools/call",
                json!({
                    "name": name,
                    "arguments": if arguments.is_object() { arguments } else { json!({}) },
                }),
            )
            .await?;

        Ok(McpToolResult {
            text: extract_text(&result),
            is_error: result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            raw: result,
        })
    }

    pub async fn close(&self) -> Result<(), McpError> {
        self.transport.close().await
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), McpError> {
        let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
        self.transport.send(message.to_string()).await
    }

    /// Envia um pedido e espera a resposta com o mesmo id.
    async fn request(&self, method: &str, params: Value) -> Result<Value, McpError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let message = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        self.transport.send(message.to_string()).await?;

        // Notificacoes e logs do servidor podem chegar no meio; descarta o que
        // nao for a resposta deste id.
        let deadline = tokio::time::Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(McpError::new(format!(
                    "o servidor MCP nao respondeu `{method}` em {}s",
                    self.timeout.as_secs()
                )));
            }

            let line = match tokio::time::timeout(remaining, self.transport.recv()).await {
                Ok(result) => result?,
                Err(_) => {
                    return Err(McpError::new(format!(
                        "o servidor MCP nao respondeu `{method}` em {}s",
                        self.timeout.as_secs()
                    )))
                }
            };

            let Ok(value) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            if value.get("id").and_then(Value::as_i64) != Some(id) {
                continue;
            }
            if let Some(error) = value.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                let text = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("erro sem mensagem");
                return Err(McpError::new(format!("{method} falhou ({code}): {text}")));
            }
            return Ok(value.get("result").cloned().unwrap_or_else(|| json!({})));
        }
    }
}

/// Resultado de uma ferramenta MCP.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolResult {
    /// Conteudo textual concatenado, que e o caso comum.
    pub text: String,
    pub is_error: bool,
    /// Resultado cru, para quem precisar de conteudo nao textual.
    pub raw: Value,
}

/// Junta os blocos `content[].text` de uma resposta `tools/call`.
fn extract_text(result: &Value) -> String {
    let Some(content) = result.get("content").and_then(Value::as_array) else {
        return String::new();
    };
    content
        .iter()
        .filter_map(|block| block.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Transporte que conversa com um processo servidor por stdin/stdout.
pub struct StdioTransport {
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    child: Mutex<Child>,
}

impl StdioTransport {
    /// Sobe o processo do servidor.
    pub async fn spawn(config: &McpServerConfig) -> Result<Self, McpError> {
        config.validate().map_err(McpError::new)?;

        let mut command = Command::new(&config.command);
        command
            .args(&config.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // O servidor usa stderr para log; deixa passar para o log do daemon.
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        for (key, value) in &config.env {
            command.env(key, value);
        }
        if let Some(cwd) = config.cwd.as_deref().filter(|value| !value.trim().is_empty()) {
            command.current_dir(cwd);
        }

        let mut child = command.spawn().map_err(|error| {
            McpError::new(format!(
                "nao foi possivel iniciar `{}`: {error}",
                config.command
            ))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpError::new("o processo do servidor MCP nao expos stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpError::new("o processo do servidor MCP nao expos stdout"))?;

        Ok(Self {
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            child: Mutex::new(child),
        })
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn send(&self, line: String) -> Result<(), McpError> {
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|error| McpError::new(format!("falha ao escrever no servidor MCP: {error}")))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|error| McpError::new(format!("falha ao escrever no servidor MCP: {error}")))?;
        stdin
            .flush()
            .await
            .map_err(|error| McpError::new(format!("falha ao escrever no servidor MCP: {error}")))
    }

    async fn recv(&self) -> Result<String, McpError> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();
        let read = stdout
            .read_line(&mut line)
            .await
            .map_err(|error| McpError::new(format!("falha ao ler do servidor MCP: {error}")))?;
        if read == 0 {
            return Err(McpError::new("o servidor MCP encerrou a conexao"));
        }
        Ok(line.trim_end().to_string())
    }

    async fn close(&self) -> Result<(), McpError> {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
        Ok(())
    }
}

/// Sobe o servidor, faz o handshake e devolve o cliente pronto.
pub async fn connect(
    config: &McpServerConfig,
    timeout: Duration,
) -> Result<McpClient, McpError> {
    let transport = StdioTransport::spawn(config).await?;
    let client = McpClient::new(Box::new(transport), timeout);
    client.initialize().await?;
    Ok(client)
}

/// Converte um `Map` de parametros num objeto JSON de argumentos.
pub fn arguments_from(value: &Value) -> Value {
    match value {
        Value::Object(_) => value.clone(),
        Value::Null => Value::Object(Map::new()),
        other => json!({ "value": other }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    type Sent = std::sync::Arc<StdMutex<Vec<Value>>>;

    /// Transporte de teste: guarda o que foi enviado e devolve respostas
    /// programadas, sem processo nenhum.
    struct FakeTransport {
        sent: Sent,
        inbox: StdMutex<Vec<String>>,
    }

    impl FakeTransport {
        fn with(responses: Vec<String>) -> Self {
            Self {
                sent: Sent::default(),
                // `remove(0)` consome na ordem programada.
                inbox: StdMutex::new(responses),
            }
        }

        /// Transporte mais um handle para inspecionar o que foi enviado.
        fn spying(responses: Vec<String>) -> (Self, Sent) {
            let transport = Self::with(responses);
            let sent = transport.sent.clone();
            (transport, sent)
        }
    }

    #[async_trait]
    impl McpTransport for FakeTransport {
        async fn send(&self, line: String) -> Result<(), McpError> {
            self.sent
                .lock()
                .unwrap()
                .push(serde_json::from_str(&line).unwrap());
            Ok(())
        }

        async fn recv(&self) -> Result<String, McpError> {
            let mut inbox = self.inbox.lock().unwrap();
            if inbox.is_empty() {
                return Err(McpError::new("sem mais respostas"));
            }
            Ok(inbox.remove(0))
        }

        async fn close(&self) -> Result<(), McpError> {
            Ok(())
        }
    }

    fn client_with(responses: Vec<String>) -> McpClient {
        McpClient::new(
            Box::new(FakeTransport::with(responses)),
            Duration::from_secs(5),
        )
    }

    #[tokio::test]
    async fn initialize_sends_the_handshake_and_the_notification() {
        let (transport, sent_log) = FakeTransport::spying(vec![
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}"#.to_string(),
        ]);
        let client = McpClient::new(Box::new(transport), Duration::from_secs(5));

        client.initialize().await.unwrap();

        let sent = sent_log.lock().unwrap();
        assert_eq!(sent[0]["method"], json!("initialize"));
        assert_eq!(sent[0]["params"]["protocolVersion"], json!(MCP_PROTOCOL_VERSION));
        assert_eq!(sent[1]["method"], json!("notifications/initialized"));
        // A notificacao nao leva id.
        assert!(sent[1].get("id").is_none());
    }

    #[tokio::test]
    async fn list_tools_reads_both_schema_spellings() {
        let client = client_with(vec![
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[
                {"name":"read_file","description":"Le um arquivo","inputSchema":{"type":"object"}},
                {"name":"outro","input_schema":{"type":"object","properties":{}}}
            ]}}"#
                .to_string(),
        ]);

        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[0].description, "Le um arquivo");
        assert_eq!(tools[0].input_schema["type"], json!("object"));
        assert_eq!(tools[1].input_schema["type"], json!("object"));
    }

    #[tokio::test]
    async fn tools_without_a_name_are_ignored() {
        let client = client_with(vec![
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"description":"sem nome"}]}}"#
                .to_string(),
        ]);
        assert!(client.list_tools().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn call_tool_joins_the_text_blocks() {
        let client = client_with(vec![
            r#"{"jsonrpc":"2.0","id":1,"result":{"content":[
                {"type":"text","text":"linha 1"},
                {"type":"text","text":"linha 2"},
                {"type":"image","data":"..."}
            ]}}"#
                .to_string(),
        ]);

        let result = client.call_tool("x", json!({ "a": 1 })).await.unwrap();
        assert_eq!(result.text, "linha 1\nlinha 2");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn call_tool_reports_the_servers_error_flag() {
        let client = client_with(vec![
            r#"{"jsonrpc":"2.0","id":1,"result":{"isError":true,"content":[{"type":"text","text":"arquivo nao existe"}]}}"#
                .to_string(),
        ]);

        let result = client.call_tool("read", json!({})).await.unwrap();
        assert!(result.is_error);
        assert_eq!(result.text, "arquivo nao existe");
    }

    #[tokio::test]
    async fn a_jsonrpc_error_becomes_a_client_error() {
        let client = client_with(vec![
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#
                .to_string(),
        ]);

        let error = client.list_tools().await.unwrap_err();
        assert!(error.message.contains("Method not found"), "{}", error.message);
        assert!(error.message.contains("-32601"));
    }

    #[tokio::test]
    async fn notifications_arriving_first_do_not_confuse_the_response() {
        let client = client_with(vec![
            // Log do servidor, sem id: precisa ser descartado.
            r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info"}}"#
                .to_string(),
            // Resposta de outro id, tambem descartada.
            r#"{"jsonrpc":"2.0","id":99,"result":{"tools":[]}}"#.to_string(),
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"certo"}]}}"#.to_string(),
        ]);

        let tools = client.list_tools().await.unwrap();
        assert_eq!(tools[0].name, "certo");
    }

    #[tokio::test]
    async fn malformed_lines_are_skipped() {
        let client = client_with(vec![
            "isto nao e json".to_string(),
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#.to_string(),
        ]);
        assert!(client.list_tools().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_closed_transport_surfaces_as_an_error() {
        let client = client_with(Vec::new());
        assert!(client.list_tools().await.is_err());
    }

    #[test]
    fn config_validation_requires_name_and_command() {
        let base = McpServerConfig {
            name: "arquivos".to_string(),
            command: "npx".to_string(),
            args: vec!["-y".to_string()],
            env: HashMap::new(),
            cwd: None,
            enabled: true,
            description: None,
        };
        assert!(base.validate().is_ok());
        assert_eq!(base.normalized_name(), "arquivos");

        let sem_nome = McpServerConfig {
            name: "  ".to_string(),
            ..base.clone()
        };
        assert!(sem_nome.validate().is_err());

        let sem_comando = McpServerConfig {
            command: "".to_string(),
            ..base
        };
        assert!(sem_comando.validate().is_err());
    }

    #[test]
    fn arguments_are_always_an_object() {
        assert_eq!(arguments_from(&json!({ "a": 1 }))["a"], json!(1));
        assert_eq!(arguments_from(&Value::Null), json!({}));
        assert_eq!(arguments_from(&json!(5))["value"], json!(5));
    }

    #[test]
    fn extract_text_handles_missing_content() {
        assert_eq!(extract_text(&json!({})), "");
        assert_eq!(extract_text(&json!({ "content": [] })), "");
    }
}
