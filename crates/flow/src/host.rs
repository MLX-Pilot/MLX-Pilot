//! Ponte entre o motor e o processo que o hospeda.
//!
//! O crate `mlx-flow` nao conhece o daemon: nos que precisam do agente ou das
//! ferramentas do MLX Pilot chamam este trait. O daemon implementa `FlowHost`
//! sobre o `AppState` e injeta a implementacao no `FlowEngine`, o que mantem o
//! motor testavel sem subir provedor de modelo nenhum.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Pedido de inferencia disparado por um no `agent.run`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentNodeRequest {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    /// Ferramentas liberadas para esta chamada. `None` usa o padrao do agente;
    /// `Some(vec![])` roda sem ferramenta nenhuma.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled_tools: Option<Vec<String>>,
}

/// Resposta de uma chamada ao agente.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentNodeResult {
    pub content: String,
    #[serde(default)]
    pub session_id: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub model_id: String,
    #[serde(default)]
    pub total_tokens: usize,
    #[serde(default)]
    pub latency_ms: u64,
}

/// Pedido de execucao de uma ferramenta registrada no MLX Pilot.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolNodeRequest {
    pub name: String,
    #[serde(default)]
    pub params: Value,
    /// Raiz do workspace para ferramentas de arquivo. `None` usa o padrao.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    /// `read_only` bloqueia escrita e execucao.
    #[serde(default)]
    pub read_only: bool,
}

/// Resultado de uma ferramenta.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolNodeResult {
    pub output: String,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub metadata: Map<String, Value>,
}

/// Capacidades que o motor pede ao processo hospedeiro.
#[async_trait]
pub trait FlowHost: Send + Sync {
    /// Executa uma volta do agente e devolve a resposta final.
    async fn run_agent(&self, request: AgentNodeRequest) -> Result<AgentNodeResult, String>;

    /// Executa uma ferramenta do registro do agente.
    async fn call_tool(&self, request: ToolNodeRequest) -> Result<ToolNodeResult, String>;

    /// Nomes de ferramentas disponiveis, usado pelo catalogo da UI.
    fn available_tools(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Host que recusa qualquer chamada. Usado em testes e quando o motor roda sem
/// o daemon por tras.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableHost;

#[async_trait]
impl FlowHost for UnavailableHost {
    async fn run_agent(&self, _request: AgentNodeRequest) -> Result<AgentNodeResult, String> {
        Err("o agente do MLX Pilot nao esta disponivel neste contexto".to_string())
    }

    async fn call_tool(&self, _request: ToolNodeRequest) -> Result<ToolNodeResult, String> {
        Err("as ferramentas do MLX Pilot nao estao disponiveis neste contexto".to_string())
    }
}
