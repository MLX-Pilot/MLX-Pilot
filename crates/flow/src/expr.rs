//! Linguagem de expressoes de `{{ ... }}`.
//!
//! Substitui a avaliacao via motor JavaScript por um interpretador proprio,
//! deterministico e sem acesso ao sistema. Um parametro de no e sempre JSON;
//! qualquer string contendo `{{ ... }}` e tratada como template.
//!
//! Regras de tipo: se a string inteira for exatamente uma expressao, o valor
//! avaliado preserva o tipo JSON (numero continua numero). Se houver texto em
//! volta, o resultado e interpolado como string.
//!
//! Acesso a caminho inexistente devolve `null` em vez de erro (um fluxo nao
//! deve quebrar porque um campo opcional faltou), mas uma variavel desconhecida
//! e erro, para pegar erros de digitacao.

use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};

/// Erro de analise ou avaliacao de uma expressao.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExprError {
    pub expression: String,
    pub message: String,
}

impl ExprError {
    fn new(expression: &str, message: impl Into<String>) -> Self {
        Self {
            expression: expression.to_string(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ExprError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} (em `{}`)", self.message, self.expression)
    }
}

impl std::error::Error for ExprError {}

/// Saida de um no ja executado, visivel para `$node["Nome"]`.
#[derive(Debug, Clone, Default)]
pub struct NodeView {
    pub items: Vec<Value>,
}

/// Tudo que uma expressao pode enxergar.
#[derive(Debug, Clone)]
pub struct ExprScope<'a> {
    /// Item atual (`$json`).
    pub json: &'a Value,
    /// Todos os itens de entrada do no (`$items`).
    pub items: &'a [Value],
    /// Indice do item atual (`$index`).
    pub index: usize,
    /// Saidas dos nos ja executados, por nome (`$node["Nome"]`).
    pub nodes: &'a HashMap<String, NodeView>,
    /// Variaveis passadas na execucao (`$env`). Nao expoe o ambiente do processo.
    pub env: &'a Map<String, Value>,
    /// Relogio da execucao (`$now`).
    pub now: DateTime<Utc>,
    pub run_id: &'a str,
    pub flow_id: &'a str,
    pub node_name: &'a str,
}

impl<'a> ExprScope<'a> {
    /// Escopo minimo, util em testes e em nos de gatilho.
    pub fn simple(
        json: &'a Value,
        items: &'a [Value],
        nodes: &'a HashMap<String, NodeView>,
        env: &'a Map<String, Value>,
    ) -> Self {
        Self {
            json,
            items,
            index: 0,
            nodes,
            env,
            now: Utc::now(),
            run_id: "",
            flow_id: "",
            node_name: "",
        }
    }
}

// ─────────────────────────── API publica ───────────────────────────

/// Verdadeiro se o valor contem algum template a resolver.
pub fn has_template(value: &Value) -> bool {
    match value {
        Value::String(text) => text.contains("{{"),
        Value::Array(items) => items.iter().any(has_template),
        Value::Object(map) => map.values().any(has_template),
        _ => false,
    }
}

/// Resolve templates recursivamente em qualquer valor JSON.
pub fn render_value(value: &Value, scope: &ExprScope) -> Result<Value, ExprError> {
    match value {
        Value::String(text) => render_template(text, scope),
        Value::Array(items) => items
            .iter()
            .map(|item| render_value(item, scope))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => {
            let mut rendered = Map::new();
            for (key, item) in map {
                // A chave tambem pode ser dinamica.
                let rendered_key = match render_template(key, scope)? {
                    Value::String(text) => text,
                    other => stringify(&other),
                };
                rendered.insert(rendered_key, render_value(item, scope)?);
            }
            Ok(Value::Object(rendered))
        }
        other => Ok(other.clone()),
    }
}

/// Resolve um template de string. Preserva o tipo quando a string inteira e
/// uma unica expressao.
pub fn render_template(template: &str, scope: &ExprScope) -> Result<Value, ExprError> {
    let segments = split_template(template)?;

    if segments.len() == 1 {
        if let Segment::Expr(expr) = &segments[0] {
            return eval(expr, scope);
        }
    }

    let mut output = String::new();
    for segment in &segments {
        match segment {
            Segment::Text(text) => output.push_str(text),
            Segment::Expr(expr) => {
                let value = eval(expr, scope)?;
                output.push_str(&stringify(&value));
            }
        }
    }
    Ok(Value::String(output))
}

/// Avalia uma expressao isolada (sem as chaves `{{ }}`).
pub fn eval(source: &str, scope: &ExprScope) -> Result<Value, ExprError> {
    let tokens = tokenize(source)?;
    let mut parser = Parser {
        source,
        tokens,
        position: 0,
    };
    let expr = parser.parse_expression(0)?;
    parser.expect_end()?;
    eval_expr(&expr, scope, source)
}

/// Avalia uma expressao e reduz o resultado a booleano.
pub fn eval_bool(source: &str, scope: &ExprScope) -> Result<bool, ExprError> {
    eval(source, scope).map(|value| truthy(&value))
}

/// Converte um valor JSON para texto do jeito que um template espera:
/// strings sem aspas, o resto como JSON compacto.
pub fn stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Regra de veracidade usada por `!`, `&&`, `||` e pelo no condicional.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().map(|n| n != 0.0).unwrap_or(false),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

// ─────────────────────────── Template split ───────────────────────────

#[derive(Debug, PartialEq)]
enum Segment {
    Text(String),
    Expr(String),
}

/// Quebra o template em texto literal e expressoes. Respeita aspas para que
/// `{{ upper('}}') }}` nao termine cedo demais, e aceita `\{{` como escape.
fn split_template(template: &str) -> Result<Vec<Segment>, ExprError> {
    let chars: Vec<char> = template.chars().collect();
    let mut segments = Vec::new();
    let mut text = String::new();
    let mut position = 0usize;

    while position < chars.len() {
        if chars[position] == '\\' && position + 2 < chars.len() && chars[position + 1] == '{' && chars[position + 2] == '{' {
            text.push_str("{{");
            position += 3;
            continue;
        }
        if chars[position] == '{' && position + 1 < chars.len() && chars[position + 1] == '{' {
            if !text.is_empty() {
                segments.push(Segment::Text(std::mem::take(&mut text)));
            }
            let start = position + 2;
            let end = find_template_end(&chars, start).ok_or_else(|| {
                ExprError::new(template, "expressao `{{` sem `}}` correspondente")
            })?;
            let expr: String = chars[start..end].iter().collect();
            if expr.trim().is_empty() {
                return Err(ExprError::new(template, "expressao vazia entre `{{` e `}}`"));
            }
            segments.push(Segment::Expr(expr.trim().to_string()));
            position = end + 2;
            continue;
        }
        text.push(chars[position]);
        position += 1;
    }

    if !text.is_empty() {
        segments.push(Segment::Text(text));
    }
    if segments.is_empty() {
        segments.push(Segment::Text(String::new()));
    }
    Ok(segments)
}

/// Indice do `}}` que fecha a expressao iniciada em `start`.
fn find_template_end(chars: &[char], start: usize) -> Option<usize> {
    let mut position = start;
    let mut quote: Option<char> = None;

    while position < chars.len() {
        let current = chars[position];
        match quote {
            Some(open) => {
                if current == '\\' {
                    position += 2;
                    continue;
                }
                if current == open {
                    quote = None;
                }
            }
            None => {
                if current == '\'' || current == '"' {
                    quote = Some(current);
                } else if current == '}' && position + 1 < chars.len() && chars[position + 1] == '}'
                {
                    return Some(position);
                }
            }
        }
        position += 1;
    }
    None
}

// ─────────────────────────── Tokenizer ───────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    Var(String),
    Str(String),
    Num(f64),
    Dot,
    Comma,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Op(&'static str),
}

fn tokenize(source: &str) -> Result<Vec<Token>, ExprError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut position = 0usize;

    while position < chars.len() {
        let current = chars[position];

        if current.is_whitespace() {
            position += 1;
            continue;
        }

        if current == '$' || current.is_alphabetic() || current == '_' {
            let is_var = current == '$';
            let start = if is_var { position + 1 } else { position };
            let mut end = start;
            while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                end += 1;
            }
            if end == start {
                return Err(ExprError::new(source, "`$` sem nome de variavel"));
            }
            let name: String = chars[start..end].iter().collect();
            tokens.push(if is_var {
                Token::Var(name)
            } else {
                Token::Ident(name)
            });
            position = end;
            continue;
        }

        if current.is_ascii_digit() {
            let start = position;
            let mut end = position;
            while end < chars.len() && (chars[end].is_ascii_digit() || chars[end] == '.') {
                // Um ponto so faz parte do numero se vier outro digito depois.
                if chars[end] == '.' && !(end + 1 < chars.len() && chars[end + 1].is_ascii_digit()) {
                    break;
                }
                end += 1;
            }
            let text: String = chars[start..end].iter().collect();
            let number = text
                .parse::<f64>()
                .map_err(|_| ExprError::new(source, format!("numero invalido: `{text}`")))?;
            tokens.push(Token::Num(number));
            position = end;
            continue;
        }

        if current == '\'' || current == '"' {
            let quote = current;
            let mut text = String::new();
            position += 1;
            loop {
                if position >= chars.len() {
                    return Err(ExprError::new(source, "string sem aspas de fechamento"));
                }
                let character = chars[position];
                if character == '\\' {
                    position += 1;
                    if position >= chars.len() {
                        return Err(ExprError::new(source, "escape incompleto na string"));
                    }
                    text.push(match chars[position] {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        other => other,
                    });
                    position += 1;
                    continue;
                }
                if character == quote {
                    position += 1;
                    break;
                }
                text.push(character);
                position += 1;
            }
            tokens.push(Token::Str(text));
            continue;
        }

        let two: String = chars[position..(position + 2).min(chars.len())].iter().collect();
        let two_char_op = match two.as_str() {
            "==" => Some("=="),
            "!=" => Some("!="),
            "<=" => Some("<="),
            ">=" => Some(">="),
            "&&" => Some("&&"),
            "||" => Some("||"),
            _ => None,
        };
        if let Some(operator) = two_char_op {
            tokens.push(Token::Op(operator));
            position += 2;
            continue;
        }

        let single = match current {
            '.' => Some(Token::Dot),
            ',' => Some(Token::Comma),
            '(' => Some(Token::LParen),
            ')' => Some(Token::RParen),
            '[' => Some(Token::LBracket),
            ']' => Some(Token::RBracket),
            '+' => Some(Token::Op("+")),
            '-' => Some(Token::Op("-")),
            '*' => Some(Token::Op("*")),
            '/' => Some(Token::Op("/")),
            '%' => Some(Token::Op("%")),
            '!' => Some(Token::Op("!")),
            '<' => Some(Token::Op("<")),
            '>' => Some(Token::Op(">")),
            _ => None,
        };
        match single {
            Some(token) => {
                tokens.push(token);
                position += 1;
            }
            None => {
                return Err(ExprError::new(
                    source,
                    format!("caractere inesperado: `{current}`"),
                ))
            }
        }
    }

    Ok(tokens)
}

// ─────────────────────────── Parser ───────────────────────────

#[derive(Debug, Clone)]
enum Expr {
    Literal(Value),
    Var(String),
    Member(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
    Not(Box<Expr>),
    Negate(Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    position: usize,
}

/// Precedencia de operadores binarios. Maior numero liga mais forte.
fn precedence(operator: &str) -> Option<u8> {
    Some(match operator {
        "||" => 1,
        "&&" => 2,
        "==" | "!=" => 3,
        "<" | "<=" | ">" | ">=" => 4,
        "+" | "-" => 5,
        "*" | "/" | "%" => 6,
        _ => return None,
    })
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn error(&self, message: impl Into<String>) -> ExprError {
        ExprError::new(self.source, message)
    }

    fn expect_end(&self) -> Result<(), ExprError> {
        if self.position == self.tokens.len() {
            Ok(())
        } else {
            Err(self.error("sobrou conteudo depois do fim da expressao"))
        }
    }

    fn parse_expression(&mut self, min_precedence: u8) -> Result<Expr, ExprError> {
        let mut left = self.parse_unary()?;

        while let Some(Token::Op(operator)) = self.peek().cloned() {
            let Some(current_precedence) = precedence(operator) else {
                break;
            };
            if current_precedence < min_precedence {
                break;
            }
            self.position += 1;
            // Todos os operadores sao associativos a esquerda.
            let right = self.parse_expression(current_precedence + 1)?;
            left = Expr::Binary(operator, Box::new(left), Box::new(right));
        }

        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, ExprError> {
        match self.peek() {
            Some(Token::Op("!")) => {
                self.position += 1;
                Ok(Expr::Not(Box::new(self.parse_unary()?)))
            }
            Some(Token::Op("-")) => {
                self.position += 1;
                Ok(Expr::Negate(Box::new(self.parse_unary()?)))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Expr, ExprError> {
        let mut expr = self.parse_primary()?;

        loop {
            match self.peek() {
                Some(Token::Dot) => {
                    self.position += 1;
                    match self.next() {
                        Some(Token::Ident(name)) => expr = Expr::Member(Box::new(expr), name),
                        // `$json.items` nao deve falhar so porque `items` virou Var.
                        Some(Token::Var(name)) => expr = Expr::Member(Box::new(expr), name),
                        _ => return Err(self.error("esperado um nome de campo depois de `.`")),
                    }
                }
                Some(Token::LBracket) => {
                    self.position += 1;
                    let index = self.parse_expression(0)?;
                    match self.next() {
                        Some(Token::RBracket) => {}
                        _ => return Err(self.error("esperado `]`")),
                    }
                    expr = Expr::Index(Box::new(expr), Box::new(index));
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ExprError> {
        match self.next() {
            Some(Token::Num(number)) => Ok(Expr::Literal(number_value(number))),
            Some(Token::Str(text)) => Ok(Expr::Literal(Value::String(text))),
            Some(Token::Var(name)) => Ok(Expr::Var(name)),
            Some(Token::Ident(name)) => match name.as_str() {
                "true" => Ok(Expr::Literal(Value::Bool(true))),
                "false" => Ok(Expr::Literal(Value::Bool(false))),
                "null" => Ok(Expr::Literal(Value::Null)),
                _ => {
                    if matches!(self.peek(), Some(Token::LParen)) {
                        self.position += 1;
                        let mut args = Vec::new();
                        if !matches!(self.peek(), Some(Token::RParen)) {
                            loop {
                                args.push(self.parse_expression(0)?);
                                match self.peek() {
                                    Some(Token::Comma) => {
                                        self.position += 1;
                                    }
                                    _ => break,
                                }
                            }
                        }
                        match self.next() {
                            Some(Token::RParen) => {}
                            _ => return Err(self.error("esperado `)` no fim da chamada")),
                        }
                        Ok(Expr::Call(name, args))
                    } else {
                        Err(self.error(format!(
                            "identificador desconhecido: `{name}`. Variaveis comecam com `$`."
                        )))
                    }
                }
            },
            Some(Token::LParen) => {
                let inner = self.parse_expression(0)?;
                match self.next() {
                    Some(Token::RParen) => Ok(inner),
                    _ => Err(self.error("esperado `)`")),
                }
            }
            Some(Token::LBracket) => {
                let mut items = Vec::new();
                if !matches!(self.peek(), Some(Token::RBracket)) {
                    loop {
                        items.push(self.parse_expression(0)?);
                        match self.peek() {
                            Some(Token::Comma) => {
                                self.position += 1;
                            }
                            _ => break,
                        }
                    }
                }
                match self.next() {
                    Some(Token::RBracket) => Ok(Expr::Call("__array".to_string(), items)),
                    _ => Err(self.error("esperado `]` no fim da lista")),
                }
            }
            other => Err(self.error(match other {
                Some(token) => format!("token inesperado: {token:?}"),
                None => "expressao incompleta".to_string(),
            })),
        }
    }
}

// ─────────────────────────── Avaliador ───────────────────────────

fn eval_expr(expr: &Expr, scope: &ExprScope, source: &str) -> Result<Value, ExprError> {
    match expr {
        Expr::Literal(value) => Ok(value.clone()),
        Expr::Var(name) => eval_var(name, scope, source),
        Expr::Member(base, field) => {
            // `$node.Nome` e um atalho de `$node["Nome"]`.
            if let Expr::Var(name) = base.as_ref() {
                if name == "node" {
                    return Ok(node_view_value(field, scope));
                }
            }
            let value = eval_expr(base, scope, source)?;
            Ok(member(&value, field))
        }
        Expr::Index(base, index) => {
            let key = eval_expr(index, scope, source)?;
            if let Expr::Var(name) = base.as_ref() {
                if name == "node" {
                    return Ok(node_view_value(&stringify(&key), scope));
                }
            }
            let value = eval_expr(base, scope, source)?;
            Ok(index_into(&value, &key))
        }
        Expr::Call(name, args) => {
            let values = args
                .iter()
                .map(|arg| eval_expr(arg, scope, source))
                .collect::<Result<Vec<_>, _>>()?;
            call_function(name, values, scope, source)
        }
        Expr::Not(inner) => Ok(Value::Bool(!truthy(&eval_expr(inner, scope, source)?))),
        Expr::Negate(inner) => {
            let value = eval_expr(inner, scope, source)?;
            let number = as_number(&value).ok_or_else(|| {
                ExprError::new(source, "`-` exige um numero")
            })?;
            Ok(number_value(-number))
        }
        Expr::Binary(operator, left, right) => {
            // Curto-circuito antes de avaliar o lado direito.
            match *operator {
                "&&" => {
                    let left_value = eval_expr(left, scope, source)?;
                    if !truthy(&left_value) {
                        return Ok(Value::Bool(false));
                    }
                    return Ok(Value::Bool(truthy(&eval_expr(right, scope, source)?)));
                }
                "||" => {
                    let left_value = eval_expr(left, scope, source)?;
                    if truthy(&left_value) {
                        return Ok(Value::Bool(true));
                    }
                    return Ok(Value::Bool(truthy(&eval_expr(right, scope, source)?)));
                }
                _ => {}
            }

            let left_value = eval_expr(left, scope, source)?;
            let right_value = eval_expr(right, scope, source)?;
            binary(operator, &left_value, &right_value, source)
        }
    }
}

fn eval_var(name: &str, scope: &ExprScope, source: &str) -> Result<Value, ExprError> {
    match name {
        "json" => Ok(scope.json.clone()),
        "items" => Ok(Value::Array(scope.items.to_vec())),
        "index" => Ok(json!(scope.index)),
        "env" => Ok(Value::Object(scope.env.clone())),
        "now" => Ok(json!(scope.now.to_rfc3339())),
        "runId" => Ok(json!(scope.run_id)),
        "flowId" => Ok(json!(scope.flow_id)),
        "nodeName" => Ok(json!(scope.node_name)),
        "node" => {
            let mut map = Map::new();
            for (node_name, view) in scope.nodes {
                map.insert(node_name.clone(), node_view_from(view));
            }
            Ok(Value::Object(map))
        }
        other => Err(ExprError::new(
            source,
            format!(
                "variavel desconhecida: `${other}`. Disponiveis: $json, $items, $index, $node, $env, $now, $runId, $flowId, $nodeName."
            ),
        )),
    }
}

fn node_view_from(view: &NodeView) -> Value {
    json!({
        "json": view.items.first().cloned().unwrap_or(Value::Null),
        "items": view.items.clone(),
        "count": view.items.len(),
    })
}

fn node_view_value(node_name: &str, scope: &ExprScope) -> Value {
    scope
        .nodes
        .get(node_name)
        .map(node_view_from)
        .unwrap_or(Value::Null)
}

fn member(value: &Value, field: &str) -> Value {
    match value {
        Value::Object(map) => map.get(field).cloned().unwrap_or(Value::Null),
        // `.length` em arrays e string e conveniencia comum.
        Value::Array(items) if field == "length" => json!(items.len()),
        Value::String(text) if field == "length" => json!(text.chars().count()),
        _ => Value::Null,
    }
}

fn index_into(value: &Value, key: &Value) -> Value {
    match (value, key) {
        (Value::Array(items), Value::Number(_)) => {
            let Some(raw) = as_index(key) else {
                return Value::Null;
            };
            // Indice negativo conta do fim.
            let resolved = if raw < 0 {
                items.len() as i64 + raw
            } else {
                raw
            };
            if resolved < 0 {
                return Value::Null;
            }
            items.get(resolved as usize).cloned().unwrap_or(Value::Null)
        }
        (Value::Object(map), key) => map.get(&stringify(key)).cloned().unwrap_or(Value::Null),
        (Value::String(text), Value::Number(_)) => as_index(key)
            .filter(|index| *index >= 0)
            .and_then(|index| text.chars().nth(index as usize))
            .map(|character| Value::String(character.to_string()))
            .unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::Bool(flag) => Some(if *flag { 1.0 } else { 0.0 }),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Converte um `f64` em JSON preservando a forma inteira quando possivel.
///
/// Sem isso, `{{ 1 + 1 }}` viraria `2.0` e acabaria numa URL ou num corpo JSON
/// como `2.0`, que quase nunca e o que o outro lado espera. Tambem e o que
/// permite usar o resultado como indice de array.
fn number_value(number: f64) -> Value {
    if number.is_finite() && number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_992.0 {
        return Value::Number(serde_json::Number::from(number as i64));
    }
    serde_json::Number::from_f64(number)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// Indice inteiro de um valor JSON numerico, aceitando tanto inteiro quanto
/// float com parte fracionaria zero.
fn as_index(value: &Value) -> Option<i64> {
    let number = value.as_f64()?;
    if !number.is_finite() || number.fract() != 0.0 {
        return None;
    }
    Some(number as i64)
}

fn loose_equals(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Number(_), Value::Number(_)) => as_number(left) == as_number(right),
        // Numero e string comparam numericamente quando a string e numerica.
        (Value::Number(_), Value::String(_)) | (Value::String(_), Value::Number(_)) => {
            match (as_number(left), as_number(right)) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            }
        }
        _ => left == right,
    }
}

fn binary(operator: &str, left: &Value, right: &Value, source: &str) -> Result<Value, ExprError> {
    match operator {
        "==" => Ok(Value::Bool(loose_equals(left, right))),
        "!=" => Ok(Value::Bool(!loose_equals(left, right))),
        "<" | "<=" | ">" | ">=" => {
            let ordering = match (as_number(left), as_number(right)) {
                (Some(a), Some(b)) => a.partial_cmp(&b),
                _ => match (left, right) {
                    (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
                    _ => None,
                },
            }
            .ok_or_else(|| {
                ExprError::new(
                    source,
                    format!("`{operator}` so compara numeros ou strings"),
                )
            })?;
            Ok(Value::Bool(match operator {
                "<" => ordering.is_lt(),
                "<=" => ordering.is_le(),
                ">" => ordering.is_gt(),
                _ => ordering.is_ge(),
            }))
        }
        "+" => match (left, right) {
            (Value::Array(a), Value::Array(b)) => {
                let mut merged = a.clone();
                merged.extend(b.clone());
                Ok(Value::Array(merged))
            }
            (Value::String(_), _) | (_, Value::String(_)) => Ok(Value::String(format!(
                "{}{}",
                stringify(left),
                stringify(right)
            ))),
            _ => arithmetic(operator, left, right, source),
        },
        "-" | "*" | "/" | "%" => arithmetic(operator, left, right, source),
        other => Err(ExprError::new(
            source,
            format!("operador nao suportado: `{other}`"),
        )),
    }
}

fn arithmetic(operator: &str, left: &Value, right: &Value, source: &str) -> Result<Value, ExprError> {
    let (Some(a), Some(b)) = (as_number(left), as_number(right)) else {
        return Err(ExprError::new(
            source,
            format!("`{operator}` exige numeros dos dois lados"),
        ));
    };
    let result = match operator {
        "+" => a + b,
        "-" => a - b,
        "*" => a * b,
        "/" => {
            if b == 0.0 {
                return Err(ExprError::new(source, "divisao por zero"));
            }
            a / b
        }
        "%" => {
            if b == 0.0 {
                return Err(ExprError::new(source, "resto de divisao por zero"));
            }
            a % b
        }
        _ => unreachable!("operador aritmetico ja filtrado"),
    };
    Ok(number_value(result))
}

fn call_function(
    name: &str,
    args: Vec<Value>,
    scope: &ExprScope,
    source: &str,
) -> Result<Value, ExprError> {
    let arity_error = |expected: &str| {
        ExprError::new(
            source,
            format!("`{name}()` espera {expected}, recebeu {}", args.len()),
        )
    };
    let arg = |index: usize| args.get(index).cloned().unwrap_or(Value::Null);

    match name {
        "__array" => Ok(Value::Array(args)),
        "upper" => Ok(Value::String(stringify(&arg(0)).to_uppercase())),
        "lower" => Ok(Value::String(stringify(&arg(0)).to_lowercase())),
        "trim" => Ok(Value::String(stringify(&arg(0)).trim().to_string())),
        "str" => Ok(Value::String(stringify(&arg(0)))),
        "json" => Ok(Value::String(arg(0).to_string())),
        "parseJson" => {
            let text = stringify(&arg(0));
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        }
        "num" => Ok(as_number(&arg(0)).map(number_value).unwrap_or(Value::Null)),
        "bool" => Ok(Value::Bool(truthy(&arg(0)))),
        "len" => Ok(json!(match arg(0) {
            Value::String(text) => text.chars().count(),
            Value::Array(items) => items.len(),
            Value::Object(map) => map.len(),
            Value::Null => 0,
            _ => 1,
        })),
        "abs" => Ok(as_number(&arg(0))
            .map(|number| number_value(number.abs()))
            .unwrap_or(Value::Null)),
        "round" => {
            let number = as_number(&arg(0)).unwrap_or(0.0);
            let digits = as_number(&arg(1)).unwrap_or(0.0).clamp(0.0, 10.0) as i32;
            let factor = 10f64.powi(digits);
            Ok(number_value((number * factor).round() / factor))
        }
        "min" | "max" => {
            if args.len() < 2 {
                return Err(arity_error("2 argumentos"));
            }
            let a = as_number(&arg(0)).unwrap_or(f64::NAN);
            let b = as_number(&arg(1)).unwrap_or(f64::NAN);
            Ok(number_value(if name == "min" { a.min(b) } else { a.max(b) }))
        }
        "default" => {
            if args.len() < 2 {
                return Err(arity_error("2 argumentos"));
            }
            let candidate = arg(0);
            Ok(if truthy(&candidate) { candidate } else { arg(1) })
        }
        "if" => {
            if args.len() < 3 {
                return Err(arity_error("3 argumentos"));
            }
            Ok(if truthy(&arg(0)) { arg(1) } else { arg(2) })
        }
        "contains" => {
            if args.len() < 2 {
                return Err(arity_error("2 argumentos"));
            }
            Ok(Value::Bool(match arg(0) {
                Value::String(text) => text.contains(&stringify(&arg(1))),
                Value::Array(items) => items.iter().any(|item| loose_equals(item, &arg(1))),
                Value::Object(map) => map.contains_key(&stringify(&arg(1))),
                _ => false,
            }))
        }
        "split" => {
            if args.len() < 2 {
                return Err(arity_error("2 argumentos"));
            }
            let separator = stringify(&arg(1));
            let text = stringify(&arg(0));
            let parts: Vec<Value> = if separator.is_empty() {
                text.chars().map(|c| json!(c.to_string())).collect()
            } else {
                text.split(separator.as_str()).map(|p| json!(p)).collect()
            };
            Ok(Value::Array(parts))
        }
        "join" => {
            if args.len() < 2 {
                return Err(arity_error("2 argumentos"));
            }
            let separator = stringify(&arg(1));
            let parts = match arg(0) {
                Value::Array(items) => items.iter().map(stringify).collect::<Vec<_>>(),
                other => vec![stringify(&other)],
            };
            Ok(Value::String(parts.join(&separator)))
        }
        "replace" => {
            if args.len() < 3 {
                return Err(arity_error("3 argumentos"));
            }
            Ok(Value::String(stringify(&arg(0)).replace(
                stringify(&arg(1)).as_str(),
                stringify(&arg(2)).as_str(),
            )))
        }
        "keys" => Ok(match arg(0) {
            Value::Object(map) => Value::Array(map.keys().map(|key| json!(key)).collect()),
            _ => Value::Array(Vec::new()),
        }),
        "values" => Ok(match arg(0) {
            Value::Object(map) => Value::Array(map.values().cloned().collect()),
            Value::Array(items) => Value::Array(items),
            _ => Value::Array(Vec::new()),
        }),
        "get" => {
            if args.len() < 2 {
                return Err(arity_error("2 argumentos"));
            }
            let mut current = arg(0);
            for part in stringify(&arg(1)).split('.') {
                if part.is_empty() {
                    continue;
                }
                current = match part.parse::<usize>() {
                    Ok(index) => index_into(&current, &json!(index)),
                    Err(_) => member(&current, part),
                };
            }
            Ok(current)
        }
        "now" => Ok(json!(scope.now.to_rfc3339())),
        "uuid" => Ok(json!(uuid::Uuid::new_v4().to_string())),
        other => Err(ExprError::new(
            source,
            format!("funcao desconhecida: `{other}()`"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope_with<'a>(
        json: &'a Value,
        items: &'a [Value],
        nodes: &'a HashMap<String, NodeView>,
        env: &'a Map<String, Value>,
    ) -> ExprScope<'a> {
        ExprScope::simple(json, items, nodes, env)
    }

    fn eval_with(source: &str, json: Value) -> Value {
        let items = vec![json.clone()];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);
        eval(source, &scope).unwrap_or_else(|error| panic!("falhou `{source}`: {error}"))
    }

    #[test]
    fn reads_nested_paths() {
        let data = json!({ "user": { "name": "Ana", "tags": ["a", "b"] } });
        assert_eq!(eval_with("$json.user.name", data.clone()), json!("Ana"));
        assert_eq!(eval_with("$json.user.tags[1]", data.clone()), json!("b"));
        assert_eq!(eval_with("$json.user.tags[-1]", data.clone()), json!("b"));
        assert_eq!(eval_with("$json[\"user\"][\"name\"]", data), json!("Ana"));
    }

    #[test]
    fn missing_path_is_null_not_error() {
        assert_eq!(eval_with("$json.nao.existe", json!({})), Value::Null);
    }

    #[test]
    fn unknown_variable_is_an_error() {
        let json = json!({});
        let items = vec![];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);
        let error = eval("$naoExiste", &scope).unwrap_err();
        assert!(error.message.contains("variavel desconhecida"));
    }

    #[test]
    fn arithmetic_and_comparison() {
        // Resultados inteiros continuam inteiros: `14`, e nao `14.0`.
        assert_eq!(eval_with("2 + 3 * 4", json!({})), json!(14));
        assert_eq!(eval_with("(2 + 3) * 4", json!({})), json!(20));
        assert_eq!(eval_with("10 / 4", json!({})), json!(2.5));
        assert_eq!(eval_with("7 % 3", json!({})), json!(1));
        assert_eq!(eval_with("2 > 1", json!({})), json!(true));
        assert_eq!(eval_with("'a' < 'b'", json!({})), json!(true));
    }

    #[test]
    fn division_by_zero_is_an_error() {
        let json = json!({});
        let items = vec![];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);
        assert!(eval("1 / 0", &scope).is_err());
    }

    #[test]
    fn plus_concatenates_when_a_side_is_string() {
        assert_eq!(
            eval_with("'Total: ' + $json.n", json!({ "n": 5 })),
            json!("Total: 5")
        );
    }

    #[test]
    fn logical_operators_short_circuit() {
        // Se `&&` avaliasse o lado direito, `1/0` explodiria.
        assert_eq!(eval_with("false && (1 / 0)", json!({})), json!(false));
        assert_eq!(eval_with("true || (1 / 0)", json!({})), json!(true));
    }

    #[test]
    fn functions_cover_the_common_cases() {
        assert_eq!(eval_with("upper('ana')", json!({})), json!("ANA"));
        assert_eq!(eval_with("len('abc')", json!({})), json!(3));
        assert_eq!(eval_with("len($json.xs)", json!({"xs":[1,2]})), json!(2));
        assert_eq!(eval_with("default(null, 'x')", json!({})), json!("x"));
        assert_eq!(eval_with("if(1 > 2, 'a', 'b')", json!({})), json!("b"));
        assert_eq!(
            eval_with("join(split('a,b', ','), '-')", json!({})),
            json!("a-b")
        );
        assert_eq!(
            eval_with("contains($json.xs, 2)", json!({"xs":[1,2]})),
            json!(true)
        );
        assert_eq!(
            eval_with("get($json, 'a.0.b')", json!({"a":[{"b":9}]})),
            json!(9)
        );
        assert_eq!(eval_with("round(1.23456, 2)", json!({})), json!(1.23));
    }

    #[test]
    fn unknown_function_is_an_error() {
        let json = json!({});
        let items = vec![];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);
        assert!(eval("naoExiste(1)", &scope)
            .unwrap_err()
            .message
            .contains("funcao desconhecida"));
    }

    #[test]
    fn node_outputs_are_reachable_by_name() {
        let json = json!({});
        let items = vec![];
        let mut nodes = HashMap::new();
        nodes.insert(
            "Buscar dados".to_string(),
            NodeView {
                items: vec![json!({ "status": 200 }), json!({ "status": 404 })],
            },
        );
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);

        assert_eq!(
            eval("$node[\"Buscar dados\"].json.status", &scope).unwrap(),
            json!(200)
        );
        assert_eq!(
            eval("$node[\"Buscar dados\"].count", &scope).unwrap(),
            json!(2)
        );
        assert_eq!(
            eval("$node[\"Buscar dados\"].items[1].status", &scope).unwrap(),
            json!(404)
        );
        assert_eq!(eval("$node[\"Inexistente\"]", &scope).unwrap(), Value::Null);
    }

    #[test]
    fn env_is_isolated_from_process_environment() {
        let json = json!({});
        let items = vec![];
        let nodes = HashMap::new();
        let mut env = Map::new();
        env.insert("TOKEN".to_string(), json!("abc"));
        let scope = scope_with(&json, &items, &nodes, &env);

        assert_eq!(eval("$env.TOKEN", &scope).unwrap(), json!("abc"));
        // Nao vaza variavel real do processo.
        assert_eq!(eval("$env.PATH", &scope).unwrap(), Value::Null);
    }

    #[test]
    fn whole_string_template_preserves_type() {
        let json = json!({ "n": 42, "ok": true });
        let items = vec![json.clone()];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);

        assert_eq!(render_template("{{ $json.n }}", &scope).unwrap(), json!(42));
        assert_eq!(
            render_template("{{ $json.ok }}", &scope).unwrap(),
            json!(true)
        );
    }

    #[test]
    fn mixed_template_interpolates_as_string() {
        let json = json!({ "n": 42 });
        let items = vec![json.clone()];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);

        assert_eq!(
            render_template("valor: {{ $json.n }}!", &scope).unwrap(),
            json!("valor: 42!")
        );
    }

    #[test]
    fn template_respects_quotes_and_escapes() {
        let json = json!({});
        let items = vec![];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);

        // `}}` dentro de aspas nao encerra a expressao.
        assert_eq!(
            render_template("{{ upper('a}}b') }}", &scope).unwrap(),
            json!("A}}B")
        );
        // `\{{` e literal.
        assert_eq!(
            render_template("\\{{ literal }}", &scope).unwrap(),
            json!("{{ literal }}")
        );
    }

    #[test]
    fn unterminated_template_is_an_error() {
        let json = json!({});
        let items = vec![];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);
        assert!(render_template("{{ $json.a ", &scope).is_err());
    }

    #[test]
    fn render_value_walks_nested_structures() {
        let json = json!({ "name": "Ana", "id": 7 });
        let items = vec![json.clone()];
        let nodes = HashMap::new();
        let env = Map::new();
        let scope = scope_with(&json, &items, &nodes, &env);

        let rendered = render_value(
            &json!({
                "url": "https://api/{{ $json.id }}",
                "body": { "greeting": "Ola {{ $json.name }}" },
                "list": ["{{ $json.id }}", 1],
                "untouched": 5
            }),
            &scope,
        )
        .unwrap();

        assert_eq!(rendered["url"], json!("https://api/7"));
        assert_eq!(rendered["body"]["greeting"], json!("Ola Ana"));
        assert_eq!(rendered["list"][0], json!(7));
        assert_eq!(rendered["untouched"], json!(5));
    }

    #[test]
    fn truthiness_matches_documented_rules() {
        assert!(!truthy(&Value::Null));
        assert!(!truthy(&json!("")));
        assert!(!truthy(&json!(0)));
        assert!(!truthy(&json!([])));
        assert!(!truthy(&json!({})));
        assert!(truthy(&json!("a")));
        assert!(truthy(&json!(1)));
        assert!(truthy(&json!([1])));
    }

    #[test]
    fn has_template_detects_nested_templates() {
        assert!(has_template(&json!({ "a": ["x", "{{ $json.b }}"] })));
        assert!(!has_template(&json!({ "a": ["x", 1] })));
    }
}
