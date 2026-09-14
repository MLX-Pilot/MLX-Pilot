//! Construcao e validacao do DAG.
//!
//! A ordenacao topologica usa o algoritmo de Kahn com camadas: cada camada
//! reune os nos cujas dependencias ja foram satisfeitas, e todos os nos de uma
//! mesma camada podem executar em paralelo. A implementacao e propria (em vez
//! de `petgraph::algo::toposort`) por dois motivos: nao adiciona dependencia de
//! build ao workspace, e permite reportar o caminho completo de um ciclo em vez
//! de apenas um no envolvido, que e o que a UI precisa mostrar.

use std::collections::{BTreeSet, HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::Flow;

/// Gravidade de um problema encontrado na validacao.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    /// Impede a execucao.
    Error,
    /// Nao impede a execucao, mas provavelmente nao e o que o usuario quis.
    Warning,
}

/// Um problema encontrado na validacao do fluxo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationIssue {
    pub severity: Severity,
    /// Codigo estavel, adequado para i18n na UI.
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_ids: Vec<String>,
}

impl ValidationIssue {
    fn error(code: &str, message: impl Into<String>, node_ids: Vec<String>) -> Self {
        Self {
            severity: Severity::Error,
            code: code.to_string(),
            message: message.into(),
            node_ids,
        }
    }

    fn warning(code: &str, message: impl Into<String>, node_ids: Vec<String>) -> Self {
        Self {
            severity: Severity::Warning,
            code: code.to_string(),
            message: message.into(),
            node_ids,
        }
    }
}

/// Resultado de uma validacao.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// Apenas os problemas bloqueantes.
    pub fn errors(&self) -> impl Iterator<Item = &ValidationIssue> {
        self.issues
            .iter()
            .filter(|issue| issue.severity == Severity::Error)
    }

    /// Mensagem unica com todos os erros, para respostas HTTP.
    pub fn error_summary(&self) -> String {
        self.errors()
            .map(|issue| issue.message.clone())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Grafo indexado, pronto para execucao.
#[derive(Debug, Clone)]
pub struct FlowGraph {
    /// Ids dos nos, na ordem de declaracao do fluxo.
    node_ids: Vec<String>,
    index_by_id: HashMap<String, usize>,
    /// Para cada no, os indices das arestas que saem dele.
    outgoing: Vec<Vec<usize>>,
    /// Para cada no, os indices das arestas que chegam nele.
    incoming: Vec<Vec<usize>>,
    /// Camadas topologicas: `levels[k]` so depende de camadas anteriores.
    levels: Vec<Vec<usize>>,
}

impl FlowGraph {
    /// Constroi o grafo validando a estrutura. `known_kind` decide se um tipo de
    /// no existe no registro; passe `|_| true` para pular essa checagem.
    pub fn build<F>(flow: &Flow, known_kind: F) -> Result<Self, ValidationReport>
    where
        F: Fn(&str) -> bool,
    {
        let report = validate(flow, known_kind);
        if !report.valid {
            return Err(report);
        }

        let node_ids: Vec<String> = flow.nodes.iter().map(|node| node.id.clone()).collect();
        let index_by_id: HashMap<String, usize> = node_ids
            .iter()
            .enumerate()
            .map(|(index, id)| (id.clone(), index))
            .collect();

        let mut outgoing = vec![Vec::new(); node_ids.len()];
        let mut incoming = vec![Vec::new(); node_ids.len()];
        for (edge_index, edge) in flow.edges.iter().enumerate() {
            // `validate` ja garantiu que ambos os lados existem.
            let from = index_by_id[&edge.from];
            let to = index_by_id[&edge.to];
            outgoing[from].push(edge_index);
            incoming[to].push(edge_index);
        }

        let levels = topological_levels(&outgoing, &incoming, flow)
            .expect("validate ja rejeitou grafos ciclicos");

        Ok(Self {
            node_ids,
            index_by_id,
            outgoing,
            incoming,
            levels,
        })
    }

    /// Quantidade de nos.
    pub fn len(&self) -> usize {
        self.node_ids.len()
    }

    /// Verdadeiro quando o fluxo nao tem nos.
    pub fn is_empty(&self) -> bool {
        self.node_ids.is_empty()
    }

    /// Indice interno de um no pelo id.
    pub fn index_of(&self, node_id: &str) -> Option<usize> {
        self.index_by_id.get(node_id).copied()
    }

    /// Id do no em um indice interno.
    pub fn node_id(&self, index: usize) -> &str {
        &self.node_ids[index]
    }

    /// Camadas topologicas. Nos de uma mesma camada sao independentes entre si.
    pub fn levels(&self) -> &[Vec<usize>] {
        &self.levels
    }

    /// Indices das arestas que chegam em um no.
    pub fn incoming(&self, index: usize) -> &[usize] {
        &self.incoming[index]
    }

    /// Indices das arestas que saem de um no.
    pub fn outgoing(&self, index: usize) -> &[usize] {
        &self.outgoing[index]
    }

    /// Nos alcancaveis a partir de um no de origem, incluindo ele mesmo.
    pub fn reachable_from(&self, flow: &Flow, start: usize) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut stack = vec![start];
        while let Some(index) = stack.pop() {
            if !seen.insert(index) {
                continue;
            }
            for &edge_index in &self.outgoing[index] {
                let target = self.index_by_id[&flow.edges[edge_index].to];
                if !seen.contains(&target) {
                    stack.push(target);
                }
            }
        }
        seen
    }
}

/// Valida a estrutura do fluxo sem construir o grafo.
pub fn validate<F>(flow: &Flow, known_kind: F) -> ValidationReport
where
    F: Fn(&str) -> bool,
{
    let mut issues = Vec::new();

    if flow.name.trim().is_empty() {
        issues.push(ValidationIssue::error(
            "flow_name_required",
            "O fluxo precisa de um nome.",
            Vec::new(),
        ));
    }

    if flow.nodes.is_empty() {
        issues.push(ValidationIssue::error(
            "flow_empty",
            "O fluxo precisa de pelo menos um no.",
            Vec::new(),
        ));
        return ValidationReport {
            valid: false,
            issues,
        };
    }

    let mut seen_ids: HashSet<&str> = HashSet::new();
    let mut seen_names: HashSet<&str> = HashSet::new();
    for node in &flow.nodes {
        if node.id.trim().is_empty() {
            issues.push(ValidationIssue::error(
                "node_id_required",
                format!("O no \"{}\" esta sem id.", node.name),
                Vec::new(),
            ));
            continue;
        }
        if !seen_ids.insert(node.id.as_str()) {
            issues.push(ValidationIssue::error(
                "node_id_duplicated",
                format!("Id de no repetido: \"{}\".", node.id),
                vec![node.id.clone()],
            ));
        }
        if node.name.trim().is_empty() {
            issues.push(ValidationIssue::error(
                "node_name_required",
                format!("O no \"{}\" esta sem nome.", node.id),
                vec![node.id.clone()],
            ));
        } else if !seen_names.insert(node.name.as_str()) {
            // Nomes duplicados quebrariam `$node["Nome"]` de forma silenciosa.
            issues.push(ValidationIssue::error(
                "node_name_duplicated",
                format!(
                    "Nome de no repetido: \"{}\". Expressoes $node[\"...\"] precisam de nomes unicos.",
                    node.name
                ),
                vec![node.id.clone()],
            ));
        }
        if !known_kind(&node.kind) {
            issues.push(ValidationIssue::error(
                "node_kind_unknown",
                format!(
                    "O no \"{}\" usa um tipo desconhecido: \"{}\".",
                    node.name, node.kind
                ),
                vec![node.id.clone()],
            ));
        }
        if !node.parameters.is_object() {
            issues.push(ValidationIssue::error(
                "node_parameters_invalid",
                format!("Os parametros do no \"{}\" precisam ser um objeto.", node.name),
                vec![node.id.clone()],
            ));
        }
    }

    let valid_ids: HashSet<&str> = flow
        .nodes
        .iter()
        .filter(|node| !node.id.trim().is_empty())
        .map(|node| node.id.as_str())
        .collect();

    let mut seen_edges: HashSet<(&str, &str, &str, &str)> = HashSet::new();
    for edge in &flow.edges {
        if !valid_ids.contains(edge.from.as_str()) {
            issues.push(ValidationIssue::error(
                "edge_source_unknown",
                format!("Conexao aponta para um no de origem inexistente: \"{}\".", edge.from),
                Vec::new(),
            ));
            continue;
        }
        if !valid_ids.contains(edge.to.as_str()) {
            issues.push(ValidationIssue::error(
                "edge_target_unknown",
                format!("Conexao aponta para um no de destino inexistente: \"{}\".", edge.to),
                Vec::new(),
            ));
            continue;
        }
        if edge.from == edge.to {
            issues.push(ValidationIssue::error(
                "edge_self_loop",
                format!("O no \"{}\" esta conectado a ele mesmo.", edge.from),
                vec![edge.from.clone()],
            ));
            continue;
        }
        let key = (
            edge.from.as_str(),
            edge.from_port.as_str(),
            edge.to.as_str(),
            edge.to_port.as_str(),
        );
        if !seen_edges.insert(key) {
            issues.push(ValidationIssue::warning(
                "edge_duplicated",
                format!(
                    "Conexao duplicada entre \"{}\" e \"{}\". Os itens chegariam repetidos.",
                    edge.from, edge.to
                ),
                vec![edge.from.clone(), edge.to.clone()],
            ));
        }
    }

    // So vale a pena procurar ciclos se as arestas forem coerentes.
    let structural_ok = issues.iter().all(|issue| issue.severity != Severity::Error);
    if structural_ok {
        if let Some(cycle) = find_cycle(flow) {
            let names: Vec<String> = cycle
                .iter()
                .map(|id| {
                    flow.node(id)
                        .map(|node| node.name.clone())
                        .unwrap_or_else(|| id.clone())
                })
                .collect();
            issues.push(ValidationIssue::error(
                "flow_has_cycle",
                format!("O fluxo tem um ciclo: {}.", names.join(" -> ")),
                cycle,
            ));
        }
    }

    let triggers = flow.triggers();
    if triggers.is_empty() {
        issues.push(ValidationIssue::warning(
            "flow_without_trigger",
            "O fluxo nao tem nenhum gatilho habilitado. Ele so podera ser executado manualmente a partir de um no sem entradas.",
            Vec::new(),
        ));
    }

    let valid = issues.iter().all(|issue| issue.severity != Severity::Error);
    ValidationReport { valid, issues }
}

/// Camadas topologicas via algoritmo de Kahn. `None` quando ha ciclo.
fn topological_levels(
    outgoing: &[Vec<usize>],
    incoming: &[Vec<usize>],
    flow: &Flow,
) -> Option<Vec<Vec<usize>>> {
    let node_count = outgoing.len();
    let index_by_id: HashMap<&str, usize> = flow
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();

    let mut remaining_indegree: Vec<usize> =
        (0..node_count).map(|index| incoming[index].len()).collect();

    let mut current: Vec<usize> = (0..node_count)
        .filter(|index| remaining_indegree[*index] == 0)
        .collect();

    let mut levels: Vec<Vec<usize>> = Vec::new();
    let mut placed = 0usize;

    while !current.is_empty() {
        placed += current.len();
        let mut next: BTreeSet<usize> = BTreeSet::new();
        for &index in &current {
            for &edge_index in &outgoing[index] {
                let target = index_by_id[flow.edges[edge_index].to.as_str()];
                remaining_indegree[target] -= 1;
                if remaining_indegree[target] == 0 {
                    next.insert(target);
                }
            }
        }
        levels.push(current);
        current = next.into_iter().collect();
    }

    if placed == node_count {
        Some(levels)
    } else {
        None
    }
}

/// Busca um ciclo por DFS e devolve o caminho fechado (ids de nos).
fn find_cycle(flow: &Flow) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mark {
        Unvisited,
        InStack,
        Done,
    }

    let index_by_id: HashMap<&str, usize> = flow
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();

    let mut adjacency: Vec<Vec<usize>> = vec![Vec::new(); flow.nodes.len()];
    for edge in &flow.edges {
        let (Some(&from), Some(&to)) = (
            index_by_id.get(edge.from.as_str()),
            index_by_id.get(edge.to.as_str()),
        ) else {
            continue;
        };
        adjacency[from].push(to);
    }

    let mut marks = vec![Mark::Unvisited; flow.nodes.len()];
    let mut stack: Vec<usize> = Vec::new();

    fn dfs(
        node: usize,
        adjacency: &[Vec<usize>],
        marks: &mut [Mark],
        stack: &mut Vec<usize>,
    ) -> Option<Vec<usize>> {
        marks[node] = Mark::InStack;
        stack.push(node);
        for &next in &adjacency[node] {
            match marks[next] {
                Mark::InStack => {
                    // Fecha o ciclo: recorta o trecho do stack a partir de `next`.
                    let start = stack.iter().position(|item| *item == next).unwrap_or(0);
                    let mut cycle = stack[start..].to_vec();
                    cycle.push(next);
                    return Some(cycle);
                }
                Mark::Unvisited => {
                    if let Some(cycle) = dfs(next, adjacency, marks, stack) {
                        return Some(cycle);
                    }
                }
                Mark::Done => {}
            }
        }
        stack.pop();
        marks[node] = Mark::Done;
        None
    }

    for index in 0..flow.nodes.len() {
        if marks[index] == Mark::Unvisited {
            if let Some(cycle) = dfs(index, &adjacency, &mut marks, &mut stack) {
                return Some(
                    cycle
                        .into_iter()
                        .map(|item| flow.nodes[item].id.clone())
                        .collect(),
                );
            }
            stack.clear();
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Edge, Node};

    fn flow_with(nodes: &[(&str, &str)], edges: &[(&str, &str)]) -> Flow {
        let mut flow = Flow::new("Teste");
        for (id, kind) in nodes {
            flow.nodes.push(Node::new(*id, *id, *kind));
        }
        for (from, to) in edges {
            flow.edges.push(Edge::new(*from, *to));
        }
        flow
    }

    #[test]
    fn linear_flow_produces_one_node_per_level() {
        let flow = flow_with(
            &[
                ("a", "trigger.manual"),
                ("b", "data.set"),
                ("c", "debug.log"),
            ],
            &[("a", "b"), ("b", "c")],
        );
        let graph = FlowGraph::build(&flow, |_| true).unwrap();
        let levels: Vec<Vec<&str>> = graph
            .levels()
            .iter()
            .map(|level| level.iter().map(|i| graph.node_id(*i)).collect())
            .collect();
        assert_eq!(levels, vec![vec!["a"], vec!["b"], vec!["c"]]);
    }

    #[test]
    fn independent_branches_share_a_level() {
        let flow = flow_with(
            &[
                ("a", "trigger.manual"),
                ("b", "data.set"),
                ("c", "data.set"),
                ("d", "flow.merge"),
            ],
            &[("a", "b"), ("a", "c"), ("b", "d"), ("c", "d")],
        );
        let graph = FlowGraph::build(&flow, |_| true).unwrap();
        let levels: Vec<Vec<&str>> = graph
            .levels()
            .iter()
            .map(|level| level.iter().map(|i| graph.node_id(*i)).collect())
            .collect();
        assert_eq!(levels, vec![vec!["a"], vec!["b", "c"], vec!["d"]]);
    }

    #[test]
    fn join_node_waits_for_the_longest_path() {
        // a -> b -> c -> d  e  a -> d: `d` deve cair na ultima camada.
        let mut flow = flow_with(
            &[
                ("a", "trigger.manual"),
                ("b", "data.set"),
                ("c", "data.set"),
                ("d", "flow.merge"),
            ],
            &[("a", "b"), ("b", "c"), ("c", "d")],
        );
        flow.edges.push(Edge::new("a", "d"));
        let graph = FlowGraph::build(&flow, |_| true).unwrap();
        let last = graph.levels().last().unwrap();
        assert_eq!(last.iter().map(|i| graph.node_id(*i)).collect::<Vec<_>>(), vec!["d"]);
    }

    #[test]
    fn cycle_is_reported_with_the_full_path() {
        let flow = flow_with(
            &[
                ("a", "trigger.manual"),
                ("b", "data.set"),
                ("c", "data.set"),
            ],
            &[("a", "b"), ("b", "c"), ("c", "b")],
        );
        let report = FlowGraph::build(&flow, |_| true).unwrap_err();
        assert!(!report.valid);
        let issue = report
            .issues
            .iter()
            .find(|issue| issue.code == "flow_has_cycle")
            .expect("ciclo reportado");
        assert!(issue.message.contains("b -> c -> b"), "{}", issue.message);
    }

    #[test]
    fn self_loop_is_rejected() {
        let flow = flow_with(&[("a", "trigger.manual")], &[("a", "a")]);
        let report = FlowGraph::build(&flow, |_| true).unwrap_err();
        assert!(report.issues.iter().any(|i| i.code == "edge_self_loop"));
    }

    #[test]
    fn duplicated_node_name_is_rejected() {
        let mut flow = Flow::new("Teste");
        flow.nodes.push(Node::new("a", "Mesmo", "trigger.manual"));
        flow.nodes.push(Node::new("b", "Mesmo", "data.set"));
        let report = validate(&flow, |_| true);
        assert!(!report.valid);
        assert!(report
            .issues
            .iter()
            .any(|i| i.code == "node_name_duplicated"));
    }

    #[test]
    fn unknown_kind_is_rejected() {
        let flow = flow_with(&[("a", "nao.existe")], &[]);
        let report = validate(&flow, |kind| kind == "trigger.manual");
        assert!(!report.valid);
        assert!(report.issues.iter().any(|i| i.code == "node_kind_unknown"));
    }

    #[test]
    fn dangling_edge_is_rejected() {
        let flow = flow_with(&[("a", "trigger.manual")], &[("a", "fantasma")]);
        let report = validate(&flow, |_| true);
        assert!(!report.valid);
        assert!(report.issues.iter().any(|i| i.code == "edge_target_unknown"));
    }

    #[test]
    fn missing_trigger_is_only_a_warning() {
        let flow = flow_with(&[("a", "data.set")], &[]);
        let report = validate(&flow, |_| true);
        assert!(report.valid);
        assert!(report
            .issues
            .iter()
            .any(|i| i.code == "flow_without_trigger" && i.severity == Severity::Warning));
    }

    #[test]
    fn reachable_from_follows_edges_forward_only() {
        let flow = flow_with(
            &[
                ("a", "trigger.manual"),
                ("b", "data.set"),
                ("c", "data.set"),
            ],
            &[("a", "b")],
        );
        let graph = FlowGraph::build(&flow, |_| true).unwrap();
        let reachable = graph.reachable_from(&flow, graph.index_of("a").unwrap());
        assert!(reachable.contains(&graph.index_of("b").unwrap()));
        assert!(!reachable.contains(&graph.index_of("c").unwrap()));
    }
}
