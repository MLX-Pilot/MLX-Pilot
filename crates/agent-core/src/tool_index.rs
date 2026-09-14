//! `ToolIndex` — ranking lexical rápido de um pedido contra **todas** as tools disponíveis.
//!
//! O seletor anterior era uma cascata de `if contains_any(query, &["edit", "replace", ...])`
//! com um conjunto fixo de nomes de tool embutido no código. Isso tinha três defeitos:
//!
//! 1. Tools que ninguém lembrou de citar (`memory_write`, `delegate_session`,
//!    `checkpoint_restore`, ...) nunca entravam no prompt de um modelo local.
//! 2. As palavras-chave eram majoritariamente em inglês, então pedidos em PT-BR
//!    ("altere o arquivo") não casavam com `edit_file`.
//! 3. Só a forma exata casava: "buscar" funcionava, "Procure" não.
//!
//! Este índice inverte a lógica: em vez de mapear palavra → tool fixa, ele pontua o
//! pedido contra o **nome e a descrição reais de cada tool registrada**. Tool nova entra
//! no ranking automaticamente, sem tocar neste arquivo.
//!
//! O custo é de microssegundos — é contagem de tokens em memória, sem I/O nem embeddings —
//! então dá para rodar a cada turno do loop do agente.

use mlx_ollama_core::FunctionDef;
use std::collections::HashMap;

/// Peso de um token que aparece no nome da tool.
const NAME_TOKEN_WEIGHT: f32 = 3.0;
/// Peso de um token que aparece na descrição da tool.
const DESC_TOKEN_WEIGHT: f32 = 1.0;
/// Peso relativo de um sinônimo em relação ao termo escrito pelo usuário.
const SYNONYM_WEIGHT: f32 = 0.6;
/// Peso de um casamento por prefixo (cobre plural e conjugação não prevista).
const PREFIX_WEIGHT: f32 = 0.45;
/// Bônus para quando o usuário escreve o nome exato da tool.
const EXACT_NAME_BONUS: f32 = 100.0;
/// Tamanho mínimo de token para valer casamento por prefixo.
const MIN_PREFIX_LEN: usize = 4;

/// Grupos de termos equivalentes, usados para expandir a consulta.
///
/// A expansão acontece do lado da **consulta**, não da tool: qualquer termo do grupo
/// alcança o vocabulário que a descrição da tool já usa. É isso que faz "Procure"
/// encontrar uma tool cuja descrição diz "Pesquisar".
///
/// Cada grupo inclui deliberadamente o **imperativo** além do infinitivo. Um pedido de
/// usuário vem como "leia", "altere", "grave" — e verbo irregular em português
/// ("ler" -> "leia") não é alcançável por stemming nem por prefixo.
const SYNONYM_GROUPS: &[&[&str]] = &[
    &[
        "procurar",
        "buscar",
        "pesquisar",
        "localizar",
        "encontrar",
        "achar",
        "descobrir",
        "identificar",
        "verificar",
        "checar",
        "procure",
        "busque",
        "pesquise",
        "localize",
        "encontre",
        "ache",
        "descubra",
        "identifique",
        "verifique",
        "cheque",
        "onde",
        "search",
        "grep",
        "find",
        "regex",
        "padrao",
        "pattern",
        "ocorrencia",
        "string",
    ],
    &[
        "ler",
        "abrir",
        "inspecionar",
        "ver",
        "visualizar",
        "mostrar",
        "exibir",
        "leia",
        "leio",
        "abra",
        "inspecione",
        "veja",
        "visualize",
        "mostre",
        "exiba",
        "read",
        "open",
        "view",
        "show",
        "cat",
        "conteudo",
        "content",
    ],
    &[
        "listar",
        "lista",
        "liste",
        "diretorio",
        "pasta",
        "dir",
        "list",
        "directory",
        "folder",
        "tree",
        "arvore",
        "raiz",
    ],
    &[
        "escrever", "criar", "gravar", "salvar", "gerar", "escreva", "crie", "grave", "salve",
        "gere", "novo", "write", "create", "save", "new", "append",
    ],
    &[
        "editar",
        "alterar",
        "mudar",
        "trocar",
        "substituir",
        "modificar",
        "atualizar",
        "corrigir",
        "ajustar",
        "refatorar",
        "edite",
        "altere",
        "mude",
        "troque",
        "substitua",
        "modifique",
        "atualize",
        "corrija",
        "ajuste",
        "refatore",
        "edit",
        "replace",
        "patch",
        "modify",
        "update",
        "refactor",
    ],
    &[
        "executar", "rodar", "compilar", "testar", "execute", "rode", "compile", "teste",
        "comando", "exec", "run", "shell", "command", "build", "test", "cargo", "npm", "make",
        "script", "processo",
    ],
    &[
        "memoria",
        "lembrar",
        "recordar",
        "memorizar",
        "persistir",
        "anotar",
        "lembre",
        "recorde",
        "memorize",
        "persista",
        "anote",
        "duravel",
        "memory",
        "remember",
        "recall",
    ],
    &[
        "delegar",
        "delegue",
        "subagente",
        "subsessao",
        "delegate",
        "subtask",
        "subagent",
        "spawn",
        "tarefa",
    ],
    &[
        "sessao",
        "sessoes",
        "conversa",
        "historico",
        "session",
        "history",
        "conversation",
    ],
    &[
        "checkpoint",
        "desfazer",
        "reverter",
        "restaurar",
        "desfaca",
        "reverta",
        "restaure",
        "rollback",
        "undo",
        "restore",
    ],
    &[
        "mensagem",
        "enviar",
        "notificar",
        "envie",
        "notifique",
        "message",
        "send",
        "notify",
        "canal",
        "channel",
    ],
    &[
        "arquivo",
        "arquivos",
        "file",
        "files",
        "documento",
        "codigo",
        "code",
        "fonte",
    ],
    &["glob", "wildcard", "extensao", "extension", "curinga"],
];

/// Uma tool pré-processada para ranqueamento.
#[derive(Debug, Clone)]
struct IndexedTool {
    name: String,
    /// token normalizado -> peso acumulado
    tokens: HashMap<String, f32>,
}

/// Resultado de um ranqueamento.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredTool {
    pub name: String,
    pub score: f32,
}

/// Índice invertido leve sobre o catálogo de tools ativo.
#[derive(Debug, Clone, Default)]
pub struct ToolIndex {
    entries: Vec<IndexedTool>,
}

impl ToolIndex {
    /// Constrói o índice a partir das tools que realmente estão disponíveis no runtime.
    pub fn build(tools: &[FunctionDef]) -> Self {
        let entries = tools
            .iter()
            .map(|tool| {
                let mut tokens: HashMap<String, f32> = HashMap::new();

                // O nome da tool é o sinal mais forte: `edit_file` -> ["edit", "file"].
                for token in tokenize(&tool.name.replace('_', " ")) {
                    *tokens.entry(token).or_insert(0.0) += NAME_TOKEN_WEIGHT;
                }

                // A descrição carrega o vocabulário de domínio — neste projeto ela já é
                // escrita em PT-BR, o que dá cobertura do idioma de graça.
                for token in tokenize(&tool.description) {
                    *tokens.entry(token).or_insert(0.0) += DESC_TOKEN_WEIGHT;
                }

                IndexedTool {
                    name: tool.name.clone(),
                    tokens,
                }
            })
            .collect();

        Self { entries }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Pontua todas as tools contra o pedido, da mais relevante para a menos.
    ///
    /// Sempre devolve uma entrada por tool indexada — quem chama decide onde cortar.
    /// Empates preservam a ordem original do registro, então o resultado é determinístico.
    pub fn rank(&self, query: &str) -> Vec<ScoredTool> {
        let normalized_query = normalize(query);
        let query_tokens = tokenize(query);
        let expanded = expand_query(&query_tokens);

        let mut scored = self
            .entries
            .iter()
            .enumerate()
            .map(|(idx, entry)| {
                let mut score = 0.0_f32;

                // O usuário citou a tool pelo nome: respeite isso acima de tudo.
                if normalized_query.contains(&normalize(&entry.name)) {
                    score += EXACT_NAME_BONUS;
                }

                for (term, term_weight) in &expanded {
                    let mut best = 0.0_f32;

                    if let Some(weight) = entry.tokens.get(term) {
                        best = best.max(weight * term_weight);
                    } else if term.len() >= MIN_PREFIX_LEN {
                        // Casamento por prefixo cobre conjugação e plural que os grupos de
                        // sinônimos não previram ("listagem" -> "listar").
                        for (token, weight) in &entry.tokens {
                            if token.len() >= MIN_PREFIX_LEN
                                && (token.starts_with(term.as_str())
                                    || term.starts_with(token.as_str()))
                            {
                                best = best.max(weight * term_weight * PREFIX_WEIGHT);
                            }
                        }
                    }

                    score += best;
                }

                (
                    idx,
                    ScoredTool {
                        name: entry.name.clone(),
                        score,
                    },
                )
            })
            .collect::<Vec<_>>();

        scored.sort_by(|a, b| {
            b.1.score
                .partial_cmp(&a.1.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        scored.into_iter().map(|(_, tool)| tool).collect()
    }

    /// Os `limit` nomes de tool mais relevantes, já filtrados por pontuação positiva.
    ///
    /// Quando nada pontua (pedido genérico como "me ajuda"), devolve os primeiros
    /// `limit` do registro em vez de vazio, para o modelo nunca ficar sem ferramenta.
    pub fn top_names(&self, query: &str, limit: usize) -> Vec<String> {
        if limit == 0 || self.entries.is_empty() {
            return Vec::new();
        }

        let ranked = self.rank(query);
        let relevant = ranked
            .iter()
            .filter(|tool| tool.score > 0.0)
            .take(limit)
            .map(|tool| tool.name.clone())
            .collect::<Vec<_>>();

        if !relevant.is_empty() {
            return relevant;
        }

        self.entries
            .iter()
            .take(limit)
            .map(|entry| entry.name.clone())
            .collect()
    }
}

/// Expande os tokens da consulta com seus sinônimos, mantendo o peso mais alto por termo.
fn expand_query(tokens: &[String]) -> Vec<(String, f32)> {
    let mut weights: HashMap<String, f32> = HashMap::new();

    for token in tokens {
        let entry = weights.entry(token.clone()).or_insert(0.0);
        *entry = entry.max(1.0);

        for group in SYNONYM_GROUPS {
            if !group.iter().any(|term| normalize(term) == *token) {
                continue;
            }
            for term in *group {
                let normalized = normalize(term);
                if normalized == *token {
                    continue;
                }
                let entry = weights.entry(normalized).or_insert(0.0);
                *entry = entry.max(SYNONYM_WEIGHT);
            }
        }
    }

    weights.into_iter().collect()
}

/// Minúsculas + remoção de acento, para "memória" e "memoria" serem o mesmo token.
fn normalize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            other => other.to_ascii_lowercase(),
        })
        .collect()
}

/// Quebra o texto em tokens normalizados, descartando ruído curto.
fn tokenize(text: &str) -> Vec<String> {
    normalize(text)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|token| token.len() >= 2)
        .filter(|token| !is_stopword(token))
        .map(ToString::to_string)
        .collect()
}

/// Palavras funcionais que não distinguem uma tool de outra.
fn is_stopword(token: &str) -> bool {
    matches!(
        token,
        "de" | "da"
            | "do"
            | "das"
            | "dos"
            | "no"
            | "na"
            | "nos"
            | "nas"
            | "em"
            | "um"
            | "uma"
            | "os"
            | "as"
            | "ao"
            | "aos"
            | "para"
            | "por"
            | "com"
            | "que"
            | "qual"
            | "quais"
            | "me"
            | "meu"
            | "minha"
            | "se"
            | "sua"
            | "seu"
            | "the"
            | "of"
            | "to"
            | "in"
            | "on"
            | "at"
            | "for"
            | "and"
            | "or"
            | "is"
            | "it"
            | "this"
            | "that"
            | "you"
            | "your"
            | "please"
            | "equivalente"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tool(name: &str, description: &str) -> FunctionDef {
        FunctionDef {
            name: name.to_string(),
            description: description.to_string(),
            parameters: json!({ "type": "object" }),
        }
    }

    /// Catálogo real completo, copiado de `GET /agent/tools` do daemon em execução.
    ///
    /// A lista inteira importa: o valor do ranking está em escolher bem sob competição
    /// de 21 candidatos, que é a situação real. Testar com um subconjunto pequeno
    /// esconderia justamente as colisões que derrubaram o seletor antigo.
    fn catalog() -> Vec<FunctionDef> {
        vec![
            tool(
                "session_search",
                "Pesquisar sessoes anteriores relevantes e reutilizar contexto entre conversas.",
            ),
            tool(
                "sessions_history",
                "Ler o historico de mensagens de uma sessao local.",
            ),
            tool("sessions_list", "Listar sessoes locais do agent."),
            tool(
                "sessions_send",
                "Enviar mensagem para uma sessao local existente.",
            ),
            tool(
                "sessions_status",
                "Inspecionar metadados e status atual de uma sessao local.",
            ),
            tool(
                "toolsets_list",
                "Listar toolsets nomeados disponiveis para runs Hermes-inspired e delegacao.",
            ),
            tool("message", "Enviar mensagem por um canal configurado."),
            tool(
                "checkpoint_restore",
                "Restaurar um checkpoint local para desfazer uma alteracao de arquivo.",
            ),
            tool(
                "checkpoints_list",
                "Listar checkpoints locais de rollback criados por ferramentas de arquivo.",
            ),
            tool(
                "delegate_session",
                "Executar uma subsessao delegada com contexto isolado e retornar apenas o resumo.",
            ),
            tool(
                "edit_file",
                "Aplicar uma edicao precisa de texto em arquivo. Equivalente a Edit ou MultiEdit.",
            ),
            tool(
                "exec",
                "Executar programa local no workspace com fila local e sem operadores de shell.",
            ),
            tool(
                "glob",
                "Encontrar arquivos por padrao glob no workspace. Equivalente a Glob.",
            ),
            tool(
                "grep",
                "Pesquisar texto ou regex em arquivos do workspace. Equivalente a Grep.",
            ),
            tool(
                "list_dir",
                "Listar arquivos e diretorios dentro do workspace. Equivalente a LS.",
            ),
            tool("memory_get", "Ler um artefato de memoria local pelo id."),
            tool(
                "memory_search",
                "Pesquisar memorias compactadas geradas por sessoes anteriores.",
            ),
            tool(
                "memory_write",
                "Persistir memoria local duravel para reuso em sessoes futuras.",
            ),
            tool(
                "read_file",
                "Ler o conteudo de um arquivo dentro do workspace. Equivalente a Read.",
            ),
            tool(
                "sessions_spawn",
                "Criar uma nova sessao local do agent. Equivalente funcional a Agent ou Task.",
            ),
            tool(
                "write_file",
                "Criar ou sobrescrever um arquivo no workspace. Equivalente a Write.",
            ),
        ]
    }

    fn top(query: &str, limit: usize) -> Vec<String> {
        ToolIndex::build(&catalog()).top_names(query, limit)
    }

    #[test]
    fn conjugated_portuguese_verb_still_finds_grep() {
        // Regressão: "Procure" não casava com a lista fixa que só tinha "procurar".
        let names = top("Procure a string MAX_RETRIES nos arquivos do workspace", 6);
        assert!(names.contains(&"grep".to_string()), "obtido: {names:?}");
    }

    #[test]
    fn portuguese_edit_request_finds_edit_file() {
        // Regressão: "altere" não existia em nenhuma lista de palavras-chave.
        for query in [
            "altere o valor de MAX_RETRIES de 3 para 5 no arquivo src/config.rs",
            "mude a constante no arquivo de configuracao",
            "substitua o texto dentro do arquivo",
        ] {
            let names = top(query, 6);
            assert!(
                names.contains(&"edit_file".to_string()),
                "query {query:?} -> {names:?}"
            );
        }
    }

    #[test]
    fn memory_tools_are_reachable() {
        // Regressão: memory_write não aparecia em nenhuma lista, então era inalcançável.
        let names = top("grave na memoria duravel que o projeto usa Rust", 6);
        assert!(
            names.contains(&"memory_write".to_string()),
            "obtido: {names:?}"
        );

        let names = top("pesquise na memoria o que voce sabe sobre o projeto", 6);
        assert!(
            names.contains(&"memory_search".to_string()),
            "obtido: {names:?}"
        );
    }

    #[test]
    fn delegation_tools_are_reachable() {
        let names = top("delegue para uma subsessao a tarefa de contar arquivos", 6);
        assert!(
            names.contains(&"delegate_session".to_string())
                || names.contains(&"sessions_spawn".to_string()),
            "obtido: {names:?}"
        );
    }

    #[test]
    fn checkpoint_tools_are_reachable() {
        let names = top("desfaca a ultima alteracao restaurando o checkpoint", 6);
        assert!(
            names.contains(&"checkpoint_restore".to_string()),
            "obtido: {names:?}"
        );
    }

    #[test]
    fn explicit_tool_name_wins() {
        // Regressão: citar a tool pelo nome não bastava para ela entrar no prompt.
        let names = top("use a ferramenta memory_write para gravar isso", 3);
        assert_eq!(names.first().map(String::as_str), Some("memory_write"));
    }

    #[test]
    fn read_request_ranks_read_file_first() {
        let names = top("leia o arquivo src/main.rs", 3);
        assert_eq!(names.first().map(String::as_str), Some("read_file"));
    }

    #[test]
    fn exec_request_ranks_exec_first() {
        let names = top("execute o comando cargo --version", 3);
        assert_eq!(names.first().map(String::as_str), Some("exec"));
    }

    #[test]
    fn english_queries_still_work() {
        let names = top("search for MAX_RETRIES in the files", 6);
        assert!(names.contains(&"grep".to_string()), "obtido: {names:?}");

        let names = top("write a new file with the summary", 6);
        assert!(
            names.contains(&"write_file".to_string()),
            "obtido: {names:?}"
        );
    }

    #[test]
    fn unmatched_query_falls_back_instead_of_returning_nothing() {
        let names = top("xyzzy plugh", 3);
        assert_eq!(names.len(), 3, "deveria cair no fallback: {names:?}");
    }

    #[test]
    fn accents_do_not_break_matching() {
        let names = top("grave na memória durável", 6);
        assert!(
            names.contains(&"memory_write".to_string()),
            "obtido: {names:?}"
        );
    }

    /// Os casos que o modelo local errou na bateria contra o daemon real.
    ///
    /// Cada `query` abaixo é literalmente a mensagem enviada no teste; a tool esperada é a
    /// que o seletor antigo deixou de fora do prompt, fazendo o modelo responder errado.
    #[test]
    fn regression_cases_from_the_live_agent_run() {
        let cases: &[(&str, &str)] = &[
            // T05: "Procure" nao casava com a keyword "procurar".
            (
                "Procure a string MAX_RETRIES nos arquivos do workspace e diga em qual arquivo ela esta e qual o valor.",
                "grep",
            ),
            // T06: encadeamento grep -> read.
            (
                "Descubra qual funcao tem um TODO pendente no codigo e depois leia o arquivo dela.",
                "grep",
            ),
            // T08: "altere" nao existia em nenhuma lista de keywords.
            (
                "No arquivo src/config.rs, altere o valor de MAX_RETRIES de 3 para 5.",
                "edit_file",
            ),
            // T17: memory_write era inalcancavel; o modelo caiu em write_file.
            (
                "Grave na memoria duravel a informacao: 'O projeto de teste usa Rust'.",
                "memory_write",
            ),
            // T18: memory_search idem.
            (
                "Pesquise na memoria o que voce sabe sobre 'projeto de teste' e me conte.",
                "memory_search",
            ),
            // T19: delegacao era inalcancavel; o modelo fez list_dir.
            (
                "Delegue para uma subsessao a tarefa de contar quantos arquivos .rs existem em src/.",
                "delegate_session",
            ),
        ];

        let index = ToolIndex::build(&catalog());
        // 6 é o teto real do perfil small_local depois da correção.
        for (query, expected) in cases {
            let names = index.top_names(query, 6);
            assert!(
                names.contains(&expected.to_string()),
                "{expected} deveria entrar no prompt de {query:?}, obtido: {names:?}"
            );
        }
    }

    #[test]
    fn ranking_is_deterministic() {
        let index = ToolIndex::build(&catalog());
        let first = index.top_names("liste os arquivos", 5);
        for _ in 0..5 {
            assert_eq!(index.top_names("liste os arquivos", 5), first);
        }
    }

    #[test]
    fn new_tool_is_matchable_without_touching_the_selector() {
        // O ponto central do redesenho: tool nova entra no ranking só por existir.
        let mut tools = catalog();
        tools.push(tool(
            "http_fetch",
            "Baixar o conteudo de uma URL da internet via requisicao HTTP.",
        ));
        let index = ToolIndex::build(&tools);
        let names = index.top_names("baixe o conteudo dessa url http", 4);
        assert!(
            names.contains(&"http_fetch".to_string()),
            "obtido: {names:?}"
        );
    }
}
