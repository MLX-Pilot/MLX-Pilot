//! Ciclo OODA (Observe-Orient-Decide-Act) para tarefas multi-etapa.
//!
//! O `AgentLoop` é reativo: o modelo emite `tool_calls`, o loop executa e devolve o
//! resultado, até o modelo decidir parar. Isso funciona bem para um pedido direto, mas
//! num pedido de várias etapas o modelo pequeno perde o fio — esquece o que já fez,
//! repete passos, ou para no meio achando que terminou.
//!
//! Este módulo põe um plano explícito por cima:
//!
//! - **Observe**: resume o estado atual — o que já foi feito, o que falhou, o que resta.
//! - **Orient**: pede ao modelo um plano de passos curtos, e o revisa quando algo falha.
//! - **Decide**: escolhe deterministicamente a próxima ação a partir do estado e dos
//!   limites. É função pura, então dá para testar sem modelo nenhum.
//! - **Act**: executa o passo com as ferramentas disponíveis.
//!
//! A fase Act delega ao `AgentLoop`, então política, aprovação, auditoria e seleção de
//! ferramentas continuam valendo — o OODA orquestra, não substitui.

use crate::agent_loop::AgentError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Tetos de execução de um run OODA.
///
/// Existem para que uma tarefa multi-etapa não vire um loop caro e silencioso: cada
/// limite tem um `StopReason` correspondente, então o motivo da parada sempre aparece na
/// resposta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OodaLimits {
    /// Máximo de ciclos Observe-Orient-Decide-Act.
    pub max_cycles: usize,
    /// Máximo de passos que um plano pode ter.
    pub max_steps: usize,
    /// Tentativas por passo antes de marcá-lo como falho.
    pub max_attempts_per_step: usize,
    /// Teto global de chamadas de ferramenta no run inteiro.
    pub max_tool_calls: usize,
    /// Teto de replanejamentos, para o modelo não ficar reescrevendo o plano.
    pub max_replans: usize,
    /// Tempo de parede máximo do run.
    #[serde(with = "duration_secs")]
    pub deadline: Duration,
}

impl Default for OodaLimits {
    fn default() -> Self {
        Self {
            max_cycles: 12,
            max_steps: 8,
            max_attempts_per_step: 2,
            max_tool_calls: 40,
            max_replans: 2,
            deadline: Duration::from_secs(600),
        }
    }
}

mod duration_secs {
    use super::Duration;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(value.as_secs())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Duration, D::Error> {
        let secs = u64::deserialize(deserializer)?;
        Ok(Duration::from_secs(secs))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanStepStatus {
    Pending,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: usize,
    pub description: String,
    pub status: PlanStepStatus,
    pub attempts: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

impl PlanStep {
    fn new(id: usize, description: String) -> Self {
        Self {
            id,
            description,
            status: PlanStepStatus::Pending,
            attempts: 0,
            result: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OodaPlan {
    pub objective: String,
    pub steps: Vec<PlanStep>,
}

impl OodaPlan {
    pub fn next_pending(&self) -> Option<&PlanStep> {
        self.steps
            .iter()
            .find(|step| step.status == PlanStepStatus::Pending)
    }

    pub fn is_complete(&self) -> bool {
        !self.steps.is_empty()
            && self
                .steps
                .iter()
                .all(|step| step.status != PlanStepStatus::Pending)
    }

    pub fn failed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|step| step.status == PlanStepStatus::Failed)
            .count()
    }
}

/// Por que o run parou.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OodaStopReason {
    /// Todos os passos terminaram.
    Completed,
    /// O plano acabou, mas com passos falhos.
    CompletedWithFailures,
    /// Bateu `max_cycles`.
    MaxCycles,
    /// Bateu `max_tool_calls`.
    MaxToolCalls,
    /// Estourou o `deadline`.
    Deadline,
    /// O modelo não conseguiu produzir nenhum passo.
    EmptyPlan,
    /// A fase Act devolveu erro irrecuperável.
    Failed,
}

/// Ação escolhida pela fase Decide.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OodaDecision {
    /// Executar o passo indicado.
    Execute { step_id: usize },
    /// Revisar o plano antes de continuar.
    Replan { reason: String },
    /// Encerrar com o motivo indicado.
    Stop { reason: OodaStopReason },
}

/// Estado observável de um run em andamento.
#[derive(Debug, Clone)]
pub struct OodaState {
    pub plan: OodaPlan,
    pub cycle: usize,
    pub tool_calls_used: usize,
    pub replans: usize,
    pub elapsed: Duration,
    /// Passo que falhou no ciclo anterior, se houver.
    pub last_failure: Option<String>,
}

impl OodaState {
    pub fn new(objective: String) -> Self {
        Self {
            plan: OodaPlan {
                objective,
                steps: Vec::new(),
            },
            cycle: 0,
            tool_calls_used: 0,
            replans: 0,
            elapsed: Duration::ZERO,
            last_failure: None,
        }
    }

    /// Visão do estado para a fase **Act**: o que já foi feito, e nada do que vem depois.
    ///
    /// Mostrar o plano inteiro faz o modelo pequeno se adiantar — observado na prática:
    /// com os passos futuros à vista, no passo "ler cada arquivo" ele já pulava para
    /// "escrever o relatório", e o relatório saía com conteúdo inventado porque os
    /// arquivos nunca foram lidos. O histórico serve para não repetir trabalho; o futuro
    /// só distrai.
    pub fn observation_for_act(&self, current_step_id: usize) -> String {
        self.render(|step| step.id <= current_step_id)
    }

    /// Fase **Observe**: resumo textual do estado, injetado no prompt do próximo passo.
    ///
    /// É o que impede o modelo pequeno de repetir um passo já concluído.
    pub fn observation(&self) -> String {
        self.render(|_| true)
    }

    fn render(&self, include: impl Fn(&PlanStep) -> bool) -> String {
        if self.plan.steps.is_empty() {
            return format!(
                "Objetivo: {}\nNenhum passo executado ainda.",
                self.plan.objective
            );
        }

        let mut lines = vec![format!("Objetivo: {}", self.plan.objective), String::new()];
        lines.push("Estado do plano:".to_string());
        for step in self.plan.steps.iter().filter(|step| include(step)) {
            let marker = match step.status {
                PlanStepStatus::Done => "[x]",
                PlanStepStatus::Failed => "[!]",
                PlanStepStatus::Pending => "[ ]",
            };
            let mut line = format!("{marker} {}. {}", step.id, step.description);
            if let Some(result) = &step.result {
                line.push_str(&format!(" -> {}", truncate(result, 160)));
            }
            lines.push(line);
        }

        if let Some(failure) = &self.last_failure {
            lines.push(String::new());
            lines.push(format!(
                "Falha no ciclo anterior: {}",
                truncate(failure, 200)
            ));
        }

        lines.join("\n")
    }
}

/// Fase **Decide**: função pura sobre estado e limites.
///
/// Separada do I/O de propósito — é a regra de parada do agente, e regra de parada
/// precisa ser testável sem subir modelo nem daemon.
pub fn decide(state: &OodaState, limits: &OodaLimits) -> OodaDecision {
    if state.elapsed >= limits.deadline {
        return OodaDecision::Stop {
            reason: OodaStopReason::Deadline,
        };
    }

    if state.tool_calls_used >= limits.max_tool_calls {
        return OodaDecision::Stop {
            reason: OodaStopReason::MaxToolCalls,
        };
    }

    if state.cycle >= limits.max_cycles {
        return OodaDecision::Stop {
            reason: OodaStopReason::MaxCycles,
        };
    }

    if state.plan.steps.is_empty() {
        return OodaDecision::Stop {
            reason: OodaStopReason::EmptyPlan,
        };
    }

    if let Some(step) = state.plan.next_pending() {
        // Um passo que falhou e ainda tem tentativa sobrando merece um plano revisado
        // antes de tentar de novo — repetir a mesma instrução tende a repetir a falha.
        if state.last_failure.is_some() && state.replans < limits.max_replans {
            return OodaDecision::Replan {
                reason: state
                    .last_failure
                    .clone()
                    .unwrap_or_else(|| "passo anterior falhou".to_string()),
            };
        }
        return OodaDecision::Execute { step_id: step.id };
    }

    if state.plan.failed_count() > 0 {
        return OodaDecision::Stop {
            reason: OodaStopReason::CompletedWithFailures,
        };
    }

    OodaDecision::Stop {
        reason: OodaStopReason::Completed,
    }
}

/// Resultado de executar um passo.
#[derive(Debug, Clone)]
pub struct ActOutcome {
    pub output: String,
    pub tool_calls: usize,
    pub is_error: bool,
}

/// Ponte entre o controlador e o runtime que de fato fala com o modelo.
///
/// Existe como trait para o controlador ser testável com um executor falso: a lógica de
/// ciclo, estado e limites é verificável sem provider nem rede.
#[async_trait::async_trait]
pub trait OodaExecutor: Send {
    /// Executa uma instrução com ferramentas disponíveis (fase Act).
    async fn act(&mut self, instruction: &str) -> Result<ActOutcome, AgentError>;

    /// Consulta o modelo sem executar ferramentas (fases Orient e síntese final).
    async fn deliberate(&mut self, prompt: &str) -> Result<String, AgentError>;
}

/// Executor concreto: delega cada fase ao `AgentLoop`.
///
/// A fase **Act** roda com as ferramentas normais, então política, aprovação e auditoria
/// continuam no caminho. A fase **Orient** e a síntese final rodam com as ferramentas
/// desligadas — planejar não deve mexer no workspace, e um modelo pequeno com ferramentas
/// à mão tenta executar o passo em vez de listá-lo.
pub struct AgentLoopExecutor<'a> {
    loop_runner: &'a mut crate::AgentLoop,
}

impl<'a> AgentLoopExecutor<'a> {
    pub fn new(loop_runner: &'a mut crate::AgentLoop) -> Self {
        Self { loop_runner }
    }
}

#[async_trait::async_trait]
impl OodaExecutor for AgentLoopExecutor<'_> {
    async fn act(&mut self, instruction: &str) -> Result<ActOutcome, AgentError> {
        let before = self.loop_runner.tool_calls_total();
        match self.loop_runner.run(instruction).await {
            Ok(response) => {
                let tool_calls = self
                    .loop_runner
                    .tool_calls_total()
                    .saturating_sub(before)
                    .max(response.tool_calls_made);
                Ok(ActOutcome {
                    output: response.content,
                    tool_calls,
                    is_error: false,
                })
            }
            // Estourar o limite de iterações de um passo não derruba o plano: vira uma
            // falha daquele passo, e o ciclo decide se replaneja ou desiste.
            Err(AgentError::MaxIterations { max }) => Ok(ActOutcome {
                output: format!("passo nao concluiu em {max} iteracoes"),
                tool_calls: 0,
                is_error: true,
            }),
            Err(AgentError::ToolError { tool, message }) => Ok(ActOutcome {
                output: format!("ferramenta {tool} falhou: {message}"),
                tool_calls: 0,
                is_error: true,
            }),
            Err(AgentError::PolicyDenied { reason }) => Ok(ActOutcome {
                output: format!("bloqueado por politica: {reason}"),
                tool_calls: 0,
                is_error: true,
            }),
            // Erro de provider é infraestrutura, não do passo: propaga e encerra o run.
            Err(error) => Err(error),
        }
    }

    async fn deliberate(&mut self, prompt: &str) -> Result<String, AgentError> {
        let response = self.loop_runner.run_without_tools(prompt).await?;
        Ok(response.content)
    }
}

/// Um ciclo registrado, para telemetria e depuração.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OodaCycleRecord {
    pub cycle: usize,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_description: Option<String>,
    pub tool_calls: usize,
    pub elapsed_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

/// Resultado completo de um run OODA.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OodaOutcome {
    pub final_response: String,
    pub plan: OodaPlan,
    pub cycles: Vec<OodaCycleRecord>,
    pub stop_reason: OodaStopReason,
    pub tool_calls_made: usize,
    pub replans: usize,
    pub elapsed_ms: u64,
}

/// Orquestrador do ciclo.
pub struct OodaController {
    limits: OodaLimits,
}

impl OodaController {
    pub fn new(limits: OodaLimits) -> Self {
        Self { limits }
    }

    pub fn limits(&self) -> &OodaLimits {
        &self.limits
    }

    /// Roda o ciclo completo até concluir ou bater um limite.
    pub async fn run<E: OodaExecutor>(
        &self,
        executor: &mut E,
        objective: &str,
    ) -> Result<OodaOutcome, AgentError> {
        let started = std::time::Instant::now();
        let mut state = OodaState::new(objective.to_string());
        let mut cycles: Vec<OodaCycleRecord> = Vec::new();

        // Orient inicial: sem plano não há o que decidir.
        let plan_text = executor.deliberate(&orient_prompt(&state, None)).await?;
        state.plan.steps = parse_plan_steps(&plan_text, self.limits.max_steps);

        loop {
            state.elapsed = started.elapsed();
            let decision = decide(&state, &self.limits);
            let cycle_started = std::time::Instant::now();

            match decision {
                OodaDecision::Stop { reason } => {
                    let final_response = self
                        .synthesize(executor, &state, reason)
                        .await
                        .unwrap_or_else(|_| fallback_summary(&state));

                    return Ok(OodaOutcome {
                        final_response,
                        plan: state.plan,
                        cycles,
                        stop_reason: reason,
                        tool_calls_made: state.tool_calls_used,
                        replans: state.replans,
                        elapsed_ms: started.elapsed().as_millis() as u64,
                    });
                }

                OodaDecision::Replan { reason } => {
                    state.cycle += 1;
                    state.replans += 1;
                    let revised = executor
                        .deliberate(&orient_prompt(&state, Some(&reason)))
                        .await?;
                    let revised_steps = parse_plan_steps(&revised, self.limits.max_steps);
                    apply_revised_plan(&mut state.plan, revised_steps, self.limits.max_steps);
                    state.last_failure = None;

                    cycles.push(OodaCycleRecord {
                        cycle: state.cycle,
                        decision: "replan".to_string(),
                        step_id: None,
                        step_description: None,
                        tool_calls: 0,
                        elapsed_ms: cycle_started.elapsed().as_millis() as u64,
                        outcome: Some(truncate(&reason, 200)),
                    });
                }

                OodaDecision::Execute { step_id } => {
                    state.cycle += 1;
                    let (description, attempts) =
                        match state.plan.steps.iter_mut().find(|step| step.id == step_id) {
                            Some(step) => {
                                step.attempts += 1;
                                (step.description.clone(), step.attempts)
                            }
                            None => continue,
                        };

                    let instruction = act_prompt(&state, step_id, &description);
                    let result = executor.act(&instruction).await;

                    let (outcome_text, tool_calls, failed) = match result {
                        Ok(outcome) => {
                            let failed = outcome.is_error;
                            (outcome.output, outcome.tool_calls, failed)
                        }
                        Err(error) => (error.to_string(), 0, true),
                    };

                    state.tool_calls_used += tool_calls;

                    if let Some(step) = state.plan.steps.iter_mut().find(|s| s.id == step_id) {
                        step.result = Some(truncate(&outcome_text, 400));
                        if failed {
                            // Só marca como falho depois de esgotar as tentativas; até lá
                            // o passo volta para a fila e o Decide pode pedir replan.
                            if attempts >= self.limits.max_attempts_per_step {
                                step.status = PlanStepStatus::Failed;
                            } else {
                                step.status = PlanStepStatus::Pending;
                            }
                        } else {
                            step.status = PlanStepStatus::Done;
                        }
                    }

                    state.last_failure = if failed {
                        Some(format!("passo {step_id}: {}", truncate(&outcome_text, 200)))
                    } else {
                        None
                    };

                    cycles.push(OodaCycleRecord {
                        cycle: state.cycle,
                        decision: "execute".to_string(),
                        step_id: Some(step_id),
                        step_description: Some(description),
                        tool_calls,
                        elapsed_ms: cycle_started.elapsed().as_millis() as u64,
                        outcome: Some(truncate(&outcome_text, 200)),
                    });
                }
            }
        }
    }

    async fn synthesize<E: OodaExecutor>(
        &self,
        executor: &mut E,
        state: &OodaState,
        reason: OodaStopReason,
    ) -> Result<String, AgentError> {
        if matches!(reason, OodaStopReason::EmptyPlan) {
            return Ok(
                "Nao consegui decompor o pedido em passos executaveis. Reformule com mais detalhes."
                    .to_string(),
            );
        }

        let prompt = format!(
            "{}\n\nCom base apenas no que foi apurado acima, responda ao objetivo do usuario \
             de forma direta. Nao invente resultados que nao aparecem no plano. \
             Nao descreva o plano; responda o que foi pedido.",
            state.observation()
        );
        let answer = executor.deliberate(&prompt).await?;
        if answer.trim().is_empty() {
            return Ok(fallback_summary(state));
        }
        Ok(answer)
    }
}

/// Prompt da fase Orient.
fn orient_prompt(state: &OodaState, revision_reason: Option<&str>) -> String {
    match revision_reason {
        None => format!(
            "Objetivo: {}\n\n\
             Liste os passos necessarios para cumprir esse objetivo.\n\
             Regras:\n\
             - um passo por linha, no formato `1. acao`\n\
             - cada passo deve ser uma acao concreta e verificavel\n\
             - no maximo 6 passos\n\
             - nao explique, nao escreva nada alem da lista",
            state.objective_line()
        ),
        Some(reason) => format!(
            "{}\n\n\
             O passo anterior falhou: {}\n\n\
             Reescreva APENAS os passos que ainda faltam, corrigindo a abordagem.\n\
             Regras:\n\
             - um passo por linha, no formato `1. acao`\n\
             - nao repita passos ja concluidos\n\
             - no maximo 6 passos\n\
             - nao explique, nao escreva nada alem da lista",
            state.observation(),
            truncate(reason, 200)
        ),
    }
}

/// Prompt da fase Act: o que já foi apurado + o passo a executar.
fn act_prompt(state: &OodaState, step_id: usize, step_description: &str) -> String {
    format!(
        "{}\n\n\
         Execute agora, e somente, este passo: {}\n\
         Use as ferramentas disponiveis para apurar os dados reais; nao invente conteudo. \
         Nao adiante passos seguintes. \
         Ao terminar, responda em uma frase o que foi apurado.",
        state.observation_for_act(step_id),
        step_description
    )
}

impl OodaState {
    fn objective_line(&self) -> &str {
        &self.plan.objective
    }
}

/// Extrai passos de uma resposta em texto livre.
///
/// Modelos pequenos não produzem JSON confiável, então o formato pedido é uma lista
/// numerada e o parser aceita as variações que eles costumam emitir (`1.`, `1)`, `-`,
/// `*`), ignorando preâmbulo e linhas de conversa.
pub fn parse_plan_steps(text: &str, max_steps: usize) -> Vec<PlanStep> {
    let mut steps = Vec::new();

    for raw_line in text.lines() {
        if steps.len() >= max_steps {
            break;
        }
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        let Some(content) = strip_list_marker(line) else {
            continue;
        };
        let content = content.trim();
        if content.is_empty() || content.chars().count() < 3 {
            continue;
        }

        steps.push(PlanStep::new(steps.len() + 1, content.to_string()));
    }

    steps
}

/// Remove o marcador de lista de uma linha, devolvendo `None` se não houver marcador.
fn strip_list_marker(line: &str) -> Option<&str> {
    if let Some(rest) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
        return Some(rest);
    }

    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 2 {
        return None;
    }

    let rest = &line[digits..];
    rest.strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))
        .or_else(|| rest.strip_prefix(" - "))
}

/// Substitui os passos pendentes pelo plano revisado, preservando os já executados.
fn apply_revised_plan(plan: &mut OodaPlan, revised: Vec<PlanStep>, max_steps: usize) {
    if revised.is_empty() {
        return;
    }

    let mut kept: Vec<PlanStep> = plan
        .steps
        .iter()
        .filter(|step| step.status != PlanStepStatus::Pending)
        .cloned()
        .collect();

    for step in revised {
        if kept.len() >= max_steps {
            break;
        }
        let id = kept.len() + 1;
        let mut new_step = PlanStep::new(id, step.description);
        // Se o modelo devolveu o mesmo passo, é o mesmo passo: manter o contador de
        // tentativas. Zerá-lo permitia que um passo insistentemente falho consumisse
        // tentativas para sempre, porque `max_attempts_per_step` nunca era alcançado.
        if let Some(previous) = plan
            .steps
            .iter()
            .find(|old| same_step(&old.description, &new_step.description))
        {
            new_step.attempts = previous.attempts;
        }
        kept.push(new_step);
    }

    // Reindexa para os ids continuarem contíguos depois da fusão.
    for (index, step) in kept.iter_mut().enumerate() {
        step.id = index + 1;
    }

    plan.steps = kept;
}

/// Dois passos são "o mesmo" se a descrição normalizada coincide.
fn same_step(left: &str, right: &str) -> bool {
    let normalize = |value: &str| {
        value
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}

/// Resposta usada quando o modelo não consegue sintetizar.
fn fallback_summary(state: &OodaState) -> String {
    let done = state
        .plan
        .steps
        .iter()
        .filter(|step| step.status == PlanStepStatus::Done)
        .count();
    format!(
        "Executei {done} de {} passos do plano.\n\n{}",
        state.plan.steps.len(),
        state.observation()
    )
}

fn truncate(value: &str, max_chars: usize) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= max_chars {
        return compact;
    }
    let kept = compact
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    format!("{kept}...")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // ── Parsing de plano ────────────────────────────────────────────

    #[test]
    fn parses_numbered_list() {
        let steps = parse_plan_steps("1. Ler o arquivo\n2. Contar as linhas\n", 6);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].description, "Ler o arquivo");
        assert_eq!(steps[1].id, 2);
    }

    #[test]
    fn ignores_preamble_and_trailing_chatter() {
        // O que um modelo 7B realmente devolve quando você pede "só a lista".
        let text = "Claro! Aqui esta o plano:\n\n1. Localizar o arquivo\n2. Ler o conteudo\n\nPosso ajudar em algo mais?";
        let steps = parse_plan_steps(text, 6);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].description, "Localizar o arquivo");
    }

    #[test]
    fn accepts_dash_and_paren_markers() {
        let steps = parse_plan_steps("- Primeiro passo\n2) Segundo passo\n* Terceiro passo", 6);
        assert_eq!(steps.len(), 3);
    }

    #[test]
    fn respects_max_steps() {
        let text = (1..=20)
            .map(|i| format!("{i}. passo {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(parse_plan_steps(&text, 5).len(), 5);
    }

    #[test]
    fn empty_answer_yields_no_steps() {
        assert!(parse_plan_steps("Nao sei como fazer isso.", 6).is_empty());
    }

    // ── Fase Decide (limites) ───────────────────────────────────────

    fn state_with_steps(count: usize) -> OodaState {
        let mut state = OodaState::new("objetivo".to_string());
        state.plan.steps = (1..=count)
            .map(|i| PlanStep::new(i, format!("passo {i}")))
            .collect();
        state
    }

    #[test]
    fn decide_executes_the_first_pending_step() {
        let state = state_with_steps(3);
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Execute { step_id: 1 }
        );
    }

    #[test]
    fn decide_skips_completed_steps() {
        let mut state = state_with_steps(3);
        state.plan.steps[0].status = PlanStepStatus::Done;
        state.plan.steps[1].status = PlanStepStatus::Done;
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Execute { step_id: 3 }
        );
    }

    #[test]
    fn decide_stops_when_plan_is_complete() {
        let mut state = state_with_steps(2);
        for step in &mut state.plan.steps {
            step.status = PlanStepStatus::Done;
        }
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Stop {
                reason: OodaStopReason::Completed
            }
        );
    }

    #[test]
    fn decide_reports_failures_separately_from_success() {
        let mut state = state_with_steps(2);
        state.plan.steps[0].status = PlanStepStatus::Done;
        state.plan.steps[1].status = PlanStepStatus::Failed;
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Stop {
                reason: OodaStopReason::CompletedWithFailures
            }
        );
    }

    #[test]
    fn decide_enforces_cycle_limit() {
        let mut state = state_with_steps(5);
        state.cycle = 4;
        let limits = OodaLimits {
            max_cycles: 4,
            ..OodaLimits::default()
        };
        assert_eq!(
            decide(&state, &limits),
            OodaDecision::Stop {
                reason: OodaStopReason::MaxCycles
            }
        );
    }

    #[test]
    fn decide_enforces_tool_call_budget() {
        let mut state = state_with_steps(5);
        state.tool_calls_used = 40;
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Stop {
                reason: OodaStopReason::MaxToolCalls
            }
        );
    }

    #[test]
    fn decide_enforces_deadline() {
        let mut state = state_with_steps(5);
        state.elapsed = Duration::from_secs(601);
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Stop {
                reason: OodaStopReason::Deadline
            }
        );
    }

    #[test]
    fn decide_stops_on_empty_plan() {
        let state = OodaState::new("objetivo".to_string());
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Stop {
                reason: OodaStopReason::EmptyPlan
            }
        );
    }

    #[test]
    fn decide_replans_after_a_failure() {
        let mut state = state_with_steps(3);
        state.last_failure = Some("nao encontrou o arquivo".to_string());
        assert!(matches!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Replan { .. }
        ));
    }

    #[test]
    fn decide_stops_replanning_after_the_limit() {
        let mut state = state_with_steps(3);
        state.last_failure = Some("falhou de novo".to_string());
        state.replans = 2;
        // Sem replan disponivel, segue executando em vez de travar.
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Execute { step_id: 1 }
        );
    }

    #[test]
    fn limits_are_checked_before_the_plan() {
        // Deadline vence mesmo com plano vazio: parar por tempo é mais informativo.
        let mut state = OodaState::new("objetivo".to_string());
        state.elapsed = Duration::from_secs(999);
        assert_eq!(
            decide(&state, &OodaLimits::default()),
            OodaDecision::Stop {
                reason: OodaStopReason::Deadline
            }
        );
    }

    // ── Observe ─────────────────────────────────────────────────────

    #[test]
    fn act_observation_hides_future_steps() {
        // Regressão observada com qwen2.5:7b: enxergando "escrever o relatorio" logo
        // abaixo, o modelo pulava o passo de leitura e escrevia conteudo inventado.
        let mut state = state_with_steps(4);
        state.plan.steps[0].status = PlanStepStatus::Done;
        state.plan.steps[0].result = Some("2 arquivos".to_string());

        let view = state.observation_for_act(2);
        assert!(view.contains("passo 1"));
        assert!(view.contains("passo 2"));
        assert!(!view.contains("passo 3"), "vazou passo futuro:\n{view}");
        assert!(!view.contains("passo 4"), "vazou passo futuro:\n{view}");

        // O Observe completo (usado em Orient e na sintese) segue mostrando tudo.
        assert!(state.observation().contains("passo 4"));
    }

    #[test]
    fn observation_marks_progress_so_steps_are_not_repeated() {
        let mut state = state_with_steps(3);
        state.plan.steps[0].status = PlanStepStatus::Done;
        state.plan.steps[0].result = Some("achei 2 arquivos".to_string());
        state.plan.steps[1].status = PlanStepStatus::Failed;

        let observation = state.observation();
        assert!(observation.contains("[x] 1."));
        assert!(observation.contains("achei 2 arquivos"));
        assert!(observation.contains("[!] 2."));
        assert!(observation.contains("[ ] 3."));
    }

    // ── Replanejamento ──────────────────────────────────────────────

    #[test]
    fn revised_plan_preserves_finished_steps() {
        let mut plan = OodaPlan {
            objective: "obj".to_string(),
            steps: vec![
                PlanStep {
                    id: 1,
                    description: "feito".to_string(),
                    status: PlanStepStatus::Done,
                    attempts: 1,
                    result: Some("ok".to_string()),
                },
                PlanStep::new(2, "pendente antigo".to_string()),
            ],
        };

        apply_revised_plan(
            &mut plan,
            vec![PlanStep::new(1, "nova abordagem".to_string())],
            8,
        );

        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.steps[0].description, "feito");
        assert_eq!(plan.steps[0].status, PlanStepStatus::Done);
        assert_eq!(plan.steps[1].description, "nova abordagem");
        assert_eq!(plan.steps[1].id, 2);
    }

    #[test]
    fn empty_revision_leaves_the_plan_untouched() {
        let mut plan = OodaPlan {
            objective: "obj".to_string(),
            steps: vec![PlanStep::new(1, "original".to_string())],
        };
        apply_revised_plan(&mut plan, Vec::new(), 8);
        assert_eq!(plan.steps.len(), 1);
        assert_eq!(plan.steps[0].description, "original");
    }

    // ── Controlador completo, com executor falso ────────────────────

    #[derive(Default)]
    struct ScriptedExecutor {
        plan_reply: String,
        act_replies: Vec<Result<ActOutcome, String>>,
        act_index: usize,
        final_reply: String,
        instructions_seen: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl OodaExecutor for ScriptedExecutor {
        async fn act(&mut self, instruction: &str) -> Result<ActOutcome, AgentError> {
            self.instructions_seen
                .lock()
                .unwrap()
                .push(instruction.to_string());
            let reply = self
                .act_replies
                .get(self.act_index)
                .cloned()
                .unwrap_or(Ok(ActOutcome {
                    output: "ok".to_string(),
                    tool_calls: 1,
                    is_error: false,
                }));
            self.act_index += 1;
            reply.map_err(|message| AgentError::Other(anyhow::anyhow!(message)))
        }

        async fn deliberate(&mut self, prompt: &str) -> Result<String, AgentError> {
            // O prompt de síntese pede resposta ao objetivo; o de Orient pede a lista.
            if prompt.contains("responda ao objetivo") {
                Ok(self.final_reply.clone())
            } else {
                Ok(self.plan_reply.clone())
            }
        }
    }

    #[tokio::test]
    async fn controller_runs_every_planned_step_in_order() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut executor = ScriptedExecutor {
            plan_reply: "1. Contar arquivos\n2. Somar valores".to_string(),
            act_replies: vec![
                Ok(ActOutcome {
                    output: "2 arquivos".to_string(),
                    tool_calls: 1,
                    is_error: false,
                }),
                Ok(ActOutcome {
                    output: "soma 42".to_string(),
                    tool_calls: 2,
                    is_error: false,
                }),
            ],
            act_index: 0,
            final_reply: "Sao 2 arquivos e a soma e 42.".to_string(),
            instructions_seen: seen.clone(),
        };

        let outcome = OodaController::new(OodaLimits::default())
            .run(&mut executor, "analise o projeto")
            .await
            .unwrap();

        assert_eq!(outcome.stop_reason, OodaStopReason::Completed);
        assert_eq!(outcome.plan.steps.len(), 2);
        assert!(outcome
            .plan
            .steps
            .iter()
            .all(|step| step.status == PlanStepStatus::Done));
        assert_eq!(outcome.tool_calls_made, 3);
        assert_eq!(outcome.final_response, "Sao 2 arquivos e a soma e 42.");

        // O segundo passo precisa enxergar o resultado do primeiro.
        let instructions = seen.lock().unwrap().clone();
        assert_eq!(instructions.len(), 2);
        assert!(instructions[1].contains("2 arquivos"));
    }

    #[tokio::test]
    async fn controller_stops_at_the_tool_call_budget() {
        let mut executor = ScriptedExecutor {
            plan_reply: "1. Listar arquivos\n2. Ler conteudo\n3. Resumir".to_string(),
            act_replies: vec![Ok(ActOutcome {
                output: "consumiu muito".to_string(),
                tool_calls: 10,
                is_error: false,
            })],
            act_index: 0,
            final_reply: "parcial".to_string(),
            instructions_seen: Arc::new(Mutex::new(Vec::new())),
        };

        let limits = OodaLimits {
            max_tool_calls: 5,
            ..OodaLimits::default()
        };
        let outcome = OodaController::new(limits)
            .run(&mut executor, "objetivo")
            .await
            .unwrap();

        assert_eq!(outcome.stop_reason, OodaStopReason::MaxToolCalls);
        // Parou cedo: os passos seguintes nao rodaram.
        assert!(outcome
            .plan
            .steps
            .iter()
            .any(|s| s.status == PlanStepStatus::Pending));
    }

    #[tokio::test]
    async fn controller_stops_when_the_model_cannot_plan() {
        let mut executor = ScriptedExecutor {
            plan_reply: "Nao entendi o pedido.".to_string(),
            final_reply: String::new(),
            instructions_seen: Arc::new(Mutex::new(Vec::new())),
            ..Default::default()
        };

        let outcome = OodaController::new(OodaLimits::default())
            .run(&mut executor, "???")
            .await
            .unwrap();

        assert_eq!(outcome.stop_reason, OodaStopReason::EmptyPlan);
        assert!(outcome.final_response.contains("passos executaveis"));
    }

    #[tokio::test]
    async fn failed_step_is_retried_then_marked_failed() {
        let mut executor = ScriptedExecutor {
            plan_reply: "1. passo unico".to_string(),
            act_replies: vec![
                Ok(ActOutcome {
                    output: "erro 1".to_string(),
                    tool_calls: 1,
                    is_error: true,
                }),
                Ok(ActOutcome {
                    output: "erro 2".to_string(),
                    tool_calls: 1,
                    is_error: true,
                }),
            ],
            act_index: 0,
            final_reply: "nao consegui".to_string(),
            instructions_seen: Arc::new(Mutex::new(Vec::new())),
        };

        let limits = OodaLimits {
            max_attempts_per_step: 2,
            ..OodaLimits::default()
        };
        let outcome = OodaController::new(limits)
            .run(&mut executor, "objetivo")
            .await
            .unwrap();

        assert_eq!(outcome.stop_reason, OodaStopReason::CompletedWithFailures);
        assert_eq!(outcome.plan.steps[0].status, PlanStepStatus::Failed);
        assert_eq!(outcome.plan.steps[0].attempts, 2);
        // Uma falha dispara replanejamento antes da segunda tentativa.
        assert!(outcome.replans >= 1);
    }

    #[tokio::test]
    async fn controller_never_exceeds_the_cycle_limit() {
        let mut executor = ScriptedExecutor {
            plan_reply:
                "1. Passo um\n2. Passo dois\n3. Passo tres\n4. Passo quatro\n5. Passo cinco"
                    .to_string(),
            act_replies: Vec::new(),
            act_index: 0,
            final_reply: "fim".to_string(),
            instructions_seen: Arc::new(Mutex::new(Vec::new())),
        };

        let limits = OodaLimits {
            max_cycles: 3,
            ..OodaLimits::default()
        };
        let outcome = OodaController::new(limits)
            .run(&mut executor, "objetivo")
            .await
            .unwrap();

        assert_eq!(outcome.stop_reason, OodaStopReason::MaxCycles);
        assert_eq!(outcome.cycles.len(), 3);
    }
}
