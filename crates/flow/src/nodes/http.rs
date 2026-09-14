//! No de requisicao HTTP.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method};
use serde_json::{json, Map, Value};

use crate::expr::stringify;
use crate::model::MAIN_PORT;
use crate::registry::{
    FieldKind, FieldSpec, NodeContext, NodeDescriptor, NodeError, NodeExecutor, NodeOutput,
    SelectOption,
};

use super::{param_bool, param_object, param_str, param_str_or, param_u64};

/// Teto do timeout por requisicao.
const MAX_TIMEOUT_SECS: u64 = 300;

/// Executa uma requisicao HTTP por item de entrada.
pub struct HttpRequestNode {
    client: Client,
}

impl Default for HttpRequestNode {
    fn default() -> Self {
        Self {
            client: Client::builder()
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

#[async_trait]
impl NodeExecutor for HttpRequestNode {
    fn descriptor(&self) -> NodeDescriptor {
        NodeDescriptor {
            kind: "http.request".to_string(),
            label: "Requisicao HTTP".to_string(),
            group: "Acoes".to_string(),
            description: "Chama uma URL e devolve status, cabecalhos e corpo.".to_string(),
            color: "#53a9ff".to_string(),
            glyph: "HTTP".to_string(),
            inputs: vec![MAIN_PORT.to_string()],
            outputs: vec![MAIN_PORT.to_string()],
            defaults: json!({
                "method": "GET",
                "url": "https://example.com",
                "headers": {},
                "query": {},
                "body_type": "none",
                "body": "",
                "timeout_secs": 30,
                "response_format": "auto",
                "fail_on_error_status": true
            }),
            fields: vec![
                FieldSpec::new("method", "Metodo", FieldKind::Select).options(vec![
                    SelectOption::new("GET", "GET"),
                    SelectOption::new("POST", "POST"),
                    SelectOption::new("PUT", "PUT"),
                    SelectOption::new("PATCH", "PATCH"),
                    SelectOption::new("DELETE", "DELETE"),
                    SelectOption::new("HEAD", "HEAD"),
                ]),
                FieldSpec::new("url", "URL", FieldKind::Text)
                    .required()
                    .placeholder("https://api.exemplo.com/itens/{{ $json.id }}"),
                FieldSpec::new("headers", "Cabecalhos", FieldKind::Json)
                    .help("Objeto JSON. Os valores aceitam expressoes."),
                FieldSpec::new("query", "Parametros de query", FieldKind::Json)
                    .help("Objeto JSON anexado a URL."),
                FieldSpec::new("body_type", "Tipo do corpo", FieldKind::Select).options(vec![
                    SelectOption::new("none", "Sem corpo"),
                    SelectOption::new("json", "JSON"),
                    SelectOption::new("text", "Texto"),
                ]),
                FieldSpec::new("body", "Corpo", FieldKind::Json)
                    .help("Para JSON, aceita objeto ou string com JSON valido."),
                FieldSpec::new("timeout_secs", "Timeout (s)", FieldKind::Number),
                FieldSpec::new("response_format", "Formato da resposta", FieldKind::Select).options(
                    vec![
                        SelectOption::new("auto", "Automatico pelo content-type"),
                        SelectOption::new("json", "Sempre JSON"),
                        SelectOption::new("text", "Sempre texto"),
                    ],
                ),
                FieldSpec::new(
                    "fail_on_error_status",
                    "Falhar em status 4xx/5xx",
                    FieldKind::Boolean,
                )
                .help("Desligue para tratar o erro no proprio fluxo, com um condicional."),
            ],
        }
    }

    async fn execute(&self, ctx: &NodeContext<'_>) -> Result<NodeOutput, NodeError> {
        let items = ctx.items_or_single_empty();
        let mut output = Vec::with_capacity(items.len());

        for index in 0..items.len() {
            let params = ctx.params(index)?;
            let spec = RequestSpec::from_params(&params)?;

            let mut request = self
                .client
                .request(spec.method.clone(), spec.url.clone())
                .timeout(Duration::from_secs(spec.timeout_secs));

            for (key, value) in &spec.headers {
                request = request.header(key, value);
            }
            if !spec.query.is_empty() {
                let query: Vec<(String, String)> = spec.query.clone().into_iter().collect();
                request = request.query(&query);
            }
            match &spec.body {
                RequestBody::None => {}
                RequestBody::Json(value) => {
                    request = request
                        .header("content-type", "application/json")
                        .body(value.to_string());
                }
                RequestBody::Text(text) => {
                    request = request.body(text.clone());
                }
            }

            let response = request.send().await.map_err(|error| {
                NodeError::with_details(
                    format!("a requisicao para {} falhou: {error}", spec.url),
                    json!({ "url": spec.url, "method": spec.method.as_str() }),
                )
            })?;

            let status = response.status();
            let headers: Map<String, Value> = response
                .headers()
                .iter()
                .filter_map(|(name, value)| {
                    value
                        .to_str()
                        .ok()
                        .map(|text| (name.as_str().to_string(), json!(text)))
                })
                .collect();
            let content_type = headers
                .get("content-type")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let raw = response.text().await.unwrap_or_default();

            let body = match spec.response_format.as_str() {
                "text" => Value::String(raw),
                "json" => serde_json::from_str(&raw).map_err(|error| {
                    NodeError::new(format!("a resposta nao era JSON valido: {error}"))
                })?,
                _ => {
                    if content_type.contains("json") {
                        serde_json::from_str(&raw).unwrap_or(Value::String(raw))
                    } else {
                        Value::String(raw)
                    }
                }
            };

            if spec.fail_on_error_status && !status.is_success() {
                return Err(NodeError::with_details(
                    format!("{} respondeu HTTP {}", spec.url, status.as_u16()),
                    json!({ "status": status.as_u16(), "body": body }),
                ));
            }

            output.push(json!({
                "status": status.as_u16(),
                "ok": status.is_success(),
                "headers": Value::Object(headers),
                "body": body,
            }));
        }

        Ok(NodeOutput::main(output))
    }
}

/// Corpo ja resolvido da requisicao.
#[derive(Debug, Clone, PartialEq)]
enum RequestBody {
    None,
    Json(Value),
    Text(String),
}

/// Requisicao pronta, derivada dos parametros do no.
#[derive(Debug, Clone)]
struct RequestSpec {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    query: Vec<(String, String)>,
    body: RequestBody,
    timeout_secs: u64,
    response_format: String,
    fail_on_error_status: bool,
}

impl RequestSpec {
    fn from_params(params: &Value) -> Result<Self, NodeError> {
        let url = param_str(params, "url")
            .ok_or_else(|| NodeError::new("informe a URL da requisicao"))?;
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(NodeError::new(format!(
                "a URL precisa comecar com http:// ou https://: `{url}`"
            )));
        }

        let raw_method = param_str_or(params, "method", "GET").to_uppercase();
        let method = Method::from_bytes(raw_method.as_bytes())
            .map_err(|_| NodeError::new(format!("metodo HTTP invalido: `{raw_method}`")))?;

        let headers = param_object(params, "headers")
            .into_iter()
            .map(|(key, value)| (key, stringify(&value)))
            .collect();
        let query = param_object(params, "query")
            .into_iter()
            .map(|(key, value)| (key, stringify(&value)))
            .collect();

        let body = match param_str_or(params, "body_type", "none").as_str() {
            "json" => match params.get("body") {
                None | Some(Value::Null) => RequestBody::None,
                // Uma string com JSON valido vira JSON; senao, vai como texto.
                Some(Value::String(text)) if text.trim().is_empty() => RequestBody::None,
                Some(Value::String(text)) => match serde_json::from_str::<Value>(text) {
                    Ok(value) => RequestBody::Json(value),
                    Err(error) => {
                        return Err(NodeError::new(format!(
                            "o corpo JSON nao pode ser interpretado: {error}"
                        )))
                    }
                },
                Some(other) => RequestBody::Json(other.clone()),
            },
            "text" => match params.get("body") {
                None | Some(Value::Null) => RequestBody::None,
                Some(value) => RequestBody::Text(stringify(value)),
            },
            _ => RequestBody::None,
        };

        Ok(Self {
            method,
            url,
            headers,
            query,
            body,
            timeout_secs: param_u64(params, "timeout_secs", 30).clamp(1, MAX_TIMEOUT_SECS),
            response_format: param_str_or(params, "response_format", "auto"),
            fail_on_error_status: param_bool(params, "fail_on_error_status", true),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_requires_an_absolute_url() {
        let error = RequestSpec::from_params(&json!({ "url": "exemplo.com" })).unwrap_err();
        assert!(error.message.contains("http://"));

        let missing = RequestSpec::from_params(&json!({})).unwrap_err();
        assert!(missing.message.contains("URL"));
    }

    #[test]
    fn spec_rejects_an_invalid_method() {
        let error =
            RequestSpec::from_params(&json!({ "url": "https://a.dev", "method": "GET POST" }))
                .unwrap_err();
        assert!(error.message.contains("metodo HTTP invalido"));
    }

    #[test]
    fn spec_defaults_to_get_without_body() {
        let spec = RequestSpec::from_params(&json!({ "url": "https://a.dev" })).unwrap();
        assert_eq!(spec.method, Method::GET);
        assert_eq!(spec.body, RequestBody::None);
        assert_eq!(spec.timeout_secs, 30);
        assert!(spec.fail_on_error_status);
    }

    #[test]
    fn json_body_accepts_object_and_json_string() {
        let from_object = RequestSpec::from_params(&json!({
            "url": "https://a.dev",
            "body_type": "json",
            "body": { "a": 1 }
        }))
        .unwrap();
        assert_eq!(from_object.body, RequestBody::Json(json!({ "a": 1 })));

        let from_string = RequestSpec::from_params(&json!({
            "url": "https://a.dev",
            "body_type": "json",
            "body": "{\"a\":1}"
        }))
        .unwrap();
        assert_eq!(from_string.body, RequestBody::Json(json!({ "a": 1 })));
    }

    #[test]
    fn malformed_json_body_is_reported() {
        let error = RequestSpec::from_params(&json!({
            "url": "https://a.dev",
            "body_type": "json",
            "body": "{nao json}"
        }))
        .unwrap_err();
        assert!(error.message.contains("corpo JSON"));
    }

    #[test]
    fn text_body_stringifies_non_strings() {
        let spec = RequestSpec::from_params(&json!({
            "url": "https://a.dev",
            "body_type": "text",
            "body": 42
        }))
        .unwrap();
        assert_eq!(spec.body, RequestBody::Text("42".to_string()));
    }

    #[test]
    fn timeout_is_clamped() {
        let low = RequestSpec::from_params(&json!({ "url": "https://a.dev", "timeout_secs": 0 }))
            .unwrap();
        assert_eq!(low.timeout_secs, 1);

        let high =
            RequestSpec::from_params(&json!({ "url": "https://a.dev", "timeout_secs": 99999 }))
                .unwrap();
        assert_eq!(high.timeout_secs, MAX_TIMEOUT_SECS);
    }

    #[test]
    fn headers_and_query_are_stringified() {
        let spec = RequestSpec::from_params(&json!({
            "url": "https://a.dev",
            "headers": { "x-token": 123 },
            "query": { "page": 2 }
        }))
        .unwrap();
        assert_eq!(spec.headers, vec![("x-token".to_string(), "123".to_string())]);
        assert_eq!(spec.query, vec![("page".to_string(), "2".to_string())]);
    }
}
