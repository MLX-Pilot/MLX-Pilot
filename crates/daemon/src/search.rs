//! Multi-provider web search with content extraction and SSRF protection.
//!
//! Provides:
//! - `SearchProvider` trait — DuckDuckGo (default free), SearXNG, Brave
//! - `POST /api/search` — search across providers
//! - `POST /api/search/fetch` — fetch + extract main text from a URL
//! - SSRF guard — blocks private/loopback/link-local/meta-data IPs
//! - In-memory cache with TTL per query+provider
//!
//! The existing `/web/brave/search` chip is preserved; Brave becomes one of
//! the providers implementing the shared trait.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::Json;
use futures_util::future::join_all;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};

// ── Error type ──────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    #[error("provider error: {0}")]
    Provider(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("ssrf blocked: {0}")]
    SsrfBlocked(String),
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("timeout")]
    #[allow(dead_code)]
    TimeoutDeprecated,
    #[error("rate limited")]
    #[allow(dead_code)]
    RateLimitedDeprecated,
}

impl From<SearchError> for crate::AppError {
    fn from(e: SearchError) -> Self {
        match &e {
            SearchError::SsrfBlocked(_) => crate::AppError::NotFound(e.to_string()),
            SearchError::NotConfigured(_) => crate::AppError::NotFound(e.to_string()),
            _ => crate::AppError::Provider(mlx_ollama_core::ProviderError::Unavailable {
                details: e.to_string(),
            }),
        }
    }
}

// ── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub provider: Option<String>,
    pub max_results: Option<usize>,
    pub safe_search: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub url: String,
    pub title: String,
    pub text: String,
    pub content_type: Option<String>,
    pub fetched_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchProviderInfo {
    pub id: String,
    pub label: String,
    pub requires_key: bool,
    pub configured: bool,
}

/// Configuration for the search service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    pub default_provider: String,
    pub searxng_instance: Option<String>,
    pub brave_api_key: Option<String>,
    pub safe_search: bool,
    pub max_results: usize,
    pub cache_ttl_secs: u64,
    pub fetch_timeout_secs: u64,
    pub fetch_max_bytes: u64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_provider: "duckduckgo".to_string(),
            searxng_instance: None,
            brave_api_key: None,
            safe_search: true,
            max_results: 5,
            cache_ttl_secs: 900, // 15 min
            fetch_timeout_secs: 8,
            fetch_max_bytes: 2_097_152, // 2 MB
        }
    }
}

// ── SearchProvider trait ────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait SearchProvider: Send + Sync {
    fn id(&self) -> &str;
    fn label(&self) -> &str;
    fn requires_api_key(&self) -> bool;
    fn is_configured(&self) -> bool;
    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, SearchError>;
}

// ── DuckDuckGo provider (free, no API key) ──────────────────────────────────
///
/// Scrapes the no-JS DuckDuckGo HTML search page (`html.duckduckgo.com/html/`)
/// which returns real web search results — unlike the Instant Answer API
/// (`api.duckduckgo.com`) that only returns Wikipedia-style abstracts.
/// Same approach Odysseus uses as the free fallback provider.
pub struct DuckDuckGoProvider {
    client: Client,
}

impl DuckDuckGoProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    /// Resolve a DuckDuckGo redirect URL (`//duckduckgo.com/l/?uddg=...`) to the real target.
    fn resolve_redirect(href: &str) -> String {
        let href = href.trim();
        // Already a direct URL
        if href.starts_with("http://") || href.starts_with("https://") {
            return href.to_string();
        }
        // Protocol-relative redirect: //duckduckgo.com/l/?uddg=<encoded_url>
        if href.starts_with("//") {
            let full = format!("https:{}", href);
            if let Ok(parsed) = url::Url::parse(&full) {
                // Extract the `uddg` query parameter (DuckDuckGo redirect target)
                for (key, value) in parsed.query_pairs() {
                    if key == "uddg" {
                        if let Ok(decoded) = urlencoding::decode(&value) {
                            return decoded.into_owned();
                        }
                    }
                }
            }
        }
        href.to_string()
    }
}

#[async_trait::async_trait]
impl SearchProvider for DuckDuckGoProvider {
    fn id(&self) -> &str {
        "duckduckgo"
    }
    fn label(&self) -> &str {
        "DuckDuckGo"
    }
    fn requires_api_key(&self) -> bool {
        false
    }
    fn is_configured(&self) -> bool {
        true // always available, no key needed
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
        let max_results = query.max_results.unwrap_or(5).min(20);
        // kp: 1 = strict, -1 = moderate, -2 = off
        let safe_param = if query.safe_search.unwrap_or(true) {
            "1"
        } else {
            "-1"
        };

        // ── Primary: scrape html.duckduckgo.com (same as Odysseus) ──────
        let response = self
            .client
            .get("https://html.duckduckgo.com/html/")
            .query(&[("q", query.q.as_str()), ("kp", safe_param)])
            .header("Accept", "text/html")
            .send()
            .await
            .map_err(|e| SearchError::Network(format!("DuckDuckGo request failed: {e}")))?;

        if !response.status().is_success() {
            return Err(SearchError::Provider(format!(
                "DuckDuckGo returned HTTP {}",
                response.status()
            )));
        }

        let html_text = response
            .text()
            .await
            .map_err(|e| SearchError::Provider(format!("DuckDuckGo read error: {e}")))?;

        let document = Html::parse_document(&html_text);
        let result_sel =
            Selector::parse(".result").map_err(|e| SearchError::Provider(e.to_string()))?;
        let link_sel =
            Selector::parse(".result__a").map_err(|e| SearchError::Provider(e.to_string()))?;
        let snippet_sel = Selector::parse(".result__snippet")
            .map_err(|e| SearchError::Provider(e.to_string()))?;

        let mut results = Vec::new();

        for result_el in document.select(&result_sel) {
            if results.len() >= max_results {
                break;
            }
            // Extract link
            let (title, url) = if let Some(link) = result_el.select(&link_sel).next() {
                let raw_href = link.value().attr("href").unwrap_or("");
                let resolved = Self::resolve_redirect(raw_href);
                let text = link.text().collect::<Vec<_>>().join(" ").trim().to_string();
                if resolved.is_empty() || resolved == raw_href && !resolved.starts_with("http") {
                    continue;
                }
                (text, resolved)
            } else {
                continue;
            };

            // Extract snippet
            let snippet = result_el
                .select(&snippet_sel)
                .next()
                .map(|s| s.text().collect::<Vec<_>>().join(" ").trim().to_string())
                .unwrap_or_default();

            if url.is_empty() {
                continue;
            }

            results.push(SearchResult {
                title: if title.is_empty() { url.clone() } else { title },
                url,
                snippet,
                provider: "duckduckgo".to_string(),
            });
        }

        debug!(
            "DuckDuckGo HTML search: {} results for '{}'",
            results.len(),
            query.q
        );
        Ok(results)
    }
}

// ── SearXNG provider ────────────────────────────────────────────────────────

pub struct SearxngProvider {
    client: Client,
    instance_url: String,
    configured: bool,
}

impl SearxngProvider {
    pub fn new(instance_url: Option<String>) -> Self {
        let configured = instance_url
            .as_ref()
            .map(|u| !u.trim().is_empty())
            .unwrap_or(false);
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(12))
                .user_agent("Mozilla/5.0 (compatible; MLXPilot/1.0)")
                .build()
                .expect("Failed to build HTTP client"),
            instance_url: instance_url
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string(),
            configured,
        }
    }
}

#[async_trait::async_trait]
impl SearchProvider for SearxngProvider {
    fn id(&self) -> &str {
        "searxng"
    }
    fn label(&self) -> &str {
        "SearXNG"
    }
    fn requires_api_key(&self) -> bool {
        false
    }
    fn is_configured(&self) -> bool {
        self.configured
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
        if !self.configured {
            return Err(SearchError::NotConfigured(
                "SearXNG instance not configured".to_string(),
            ));
        }

        let max_results = query.max_results.unwrap_or(5).min(20);
        let safe_search = if query.safe_search.unwrap_or(true) {
            "1"
        } else {
            "0"
        };

        let response = self
            .client
            .get(format!("{}/search", self.instance_url))
            .query(&[
                ("q", query.q.as_str()),
                ("format", "json"),
                ("safesearch", safe_search),
                ("categories", "general"),
            ])
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(SearchError::Provider(format!(
                "SearXNG returned HTTP {}",
                response.status()
            )));
        }

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| SearchError::Provider(e.to_string()))?;

        let results = json
            .pointer("/results")
            .and_then(|v| v.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let title = entry.get("title")?.as_str()?.trim().to_string();
                        let url = entry.get("url")?.as_str()?.trim().to_string();
                        let snippet = entry
                            .get("content")
                            .or_else(|| entry.get("snippet"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();

                        if title.is_empty() || url.is_empty() {
                            return None;
                        }

                        Some(SearchResult {
                            title,
                            url,
                            snippet,
                            provider: "searxng".to_string(),
                        })
                    })
                    .take(max_results)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(results)
    }
}

// ── Brave provider ──────────────────────────────────────────────────────────

pub struct BraveProvider {
    client: Client,
    api_key: String,
    configured: bool,
}

impl BraveProvider {
    pub fn new(api_key: Option<String>) -> Self {
        let configured = api_key
            .as_ref()
            .map(|k| !k.trim().is_empty())
            .unwrap_or(false);
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(18))
                .build()
                .expect("Failed to build HTTP client"),
            api_key: api_key.unwrap_or_default(),
            configured,
        }
    }
}

#[async_trait::async_trait]
impl SearchProvider for BraveProvider {
    fn id(&self) -> &str {
        "brave"
    }
    fn label(&self) -> &str {
        "Brave Search"
    }
    fn requires_api_key(&self) -> bool {
        true
    }
    fn is_configured(&self) -> bool {
        self.configured
    }

    async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
        if !self.configured {
            return Err(SearchError::NotConfigured(
                "Brave API key not configured".to_string(),
            ));
        }

        let max_results = query.max_results.unwrap_or(5).min(10);

        let response = self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[
                ("q", query.q.as_str()),
                ("count", &max_results.to_string()),
                (
                    "safesearch",
                    if query.safe_search.unwrap_or(true) {
                        "moderate"
                    } else {
                        "off"
                    },
                ),
            ])
            .header("Accept", "application/json")
            .header("X-Subscription-Token", self.api_key.as_str())
            .send()
            .await
            .map_err(|e| SearchError::Network(e.to_string()))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(SearchError::Provider(format!(
                "Brave API returned HTTP {status}: {body}"
            )));
        }

        let parsed: serde_json::Value = response
            .json()
            .await
            .map_err(|e| SearchError::Provider(e.to_string()))?;

        let results = parsed
            .pointer("/web/results")
            .and_then(|v| v.as_array())
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| {
                        let title = entry
                            .get("title")
                            .and_then(|v| v.as_str())?
                            .trim()
                            .to_string();
                        let url = entry
                            .get("url")
                            .or_else(|| entry.get("profile").and_then(|v| v.get("url")))
                            .and_then(|v| v.as_str())?
                            .trim()
                            .to_string();
                        let snippet = entry
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .trim()
                            .to_string();

                        if title.is_empty() || url.is_empty() {
                            return None;
                        }

                        Some(SearchResult {
                            title,
                            url,
                            snippet,
                            provider: "brave".to_string(),
                        })
                    })
                    .take(max_results)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(results)
    }
}

// ── In-memory cache ─────────────────────────────────────────────────────────

#[derive(Clone)]
struct CacheEntry {
    results: Vec<SearchResult>,
    cached_at: Instant,
}

struct SearchCache {
    entries: RwLock<HashMap<String, CacheEntry>>,
    ttl: Duration,
}

impl SearchCache {
    fn new(ttl_secs: u64) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    fn cache_key(provider: &str, query: &str) -> String {
        format!("{}:{}", provider, query.to_lowercase().trim())
    }

    async fn get(&self, provider: &str, query: &str) -> Option<Vec<SearchResult>> {
        let entries = self.entries.read().await;
        let key = Self::cache_key(provider, query);
        entries.get(&key).and_then(|entry| {
            if entry.cached_at.elapsed() < self.ttl {
                Some(entry.results.clone())
            } else {
                None
            }
        })
    }

    async fn set(&self, provider: &str, query: &str, results: Vec<SearchResult>) {
        let mut entries = self.entries.write().await;
        let key = Self::cache_key(provider, query);
        entries.insert(
            key,
            CacheEntry {
                results,
                cached_at: Instant::now(),
            },
        );
        // Prune expired entries occasionally (simple: prune when > 500 entries).
        if entries.len() > 500 {
            let ttl = self.ttl;
            entries.retain(|_, entry| entry.cached_at.elapsed() < ttl);
        }
    }
}

// ── SSRF guard ──────────────────────────────────────────────────────────────

/// Check if a URL points to a private/internal IP address.
/// Returns `Ok(())` if safe, or `Err(SearchError::SsrfBlocked)` if blocked.
pub fn guard_ssrf(url_str: &str) -> Result<(), SearchError> {
    let parsed = url::Url::parse(url_str)
        .map_err(|_| SearchError::SsrfBlocked("invalid URL".to_string()))?;

    // Only allow http/https
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(SearchError::SsrfBlocked(format!(
            "blocked scheme: {scheme}"
        )));
    }

    // Resolve host to IP(s) and check against private ranges.
    let host = parsed
        .host_str()
        .ok_or_else(|| SearchError::SsrfBlocked("no host in URL".to_string()))?;

    // Check for raw IP in host string first (faster path).
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_ip(&ip) {
            return Err(SearchError::SsrfBlocked(format!(
                "private IP blocked: {ip}"
            )));
        }
        return Ok(());
    }

    // For hostnames, we do a DNS resolution check. In an async context,
    // we use tokio::net::lookup_host. But DNS resolution itself is safe —
    // the check happens in the fetch path, not here. We tag the URL as
    // "needs resolution" and check post-resolution.
    //
    // For now, we also block known metadata endpoints by hostname.
    let host_lower = host.to_lowercase();
    if host_lower == "metadata.google.internal"
        || host_lower == "169.254.169.254"
        || host_lower.ends_with(".compute.internal")
    {
        return Err(SearchError::SsrfBlocked(format!(
            "metadata endpoint blocked: {host}"
        )));
    }

    // Block .local / .localhost / .internal patterns in private networks
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
    {
        return Err(SearchError::SsrfBlocked(format!(
            "local hostname blocked: {host}"
        )));
    }

    Ok(())
}

/// Check resolved IP against private/loopback ranges after DNS resolution.
pub fn guard_resolved_ip(ip: &IpAddr) -> Result<(), SearchError> {
    if is_private_ip(ip) {
        return Err(SearchError::SsrfBlocked(format!(
            "private IP blocked after resolution: {ip}"
        )));
    }
    Ok(())
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.octets() == [169, 254, 169, 254] // AWS/cloud metadata
        }
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unspecified(),
    }
}

// ── Content extraction (fetch + scrape main text) ──────────────────────────

/// Fetch a URL and extract the main text content.
pub async fn fetch_and_extract(
    url_str: &str,
    timeout_secs: u64,
    max_bytes: u64,
) -> Result<FetchResult, SearchError> {
    // SSRF guard first
    guard_ssrf(url_str)?;

    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent("Mozilla/5.0 (compatible; MLXPilot/1.0)")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| SearchError::Network(e.to_string()))?;

    let response = client
        .get(url_str)
        .send()
        .await
        .map_err(|e| SearchError::Network(e.to_string()))?;

    // Check the final resolved URL's IP (after redirects).
    // reqwest DNS resolution goes through the system resolver, but we
    // can check if we ended up at a private IP via the remote_addr.
    if let Some(remote) = response.remote_addr() {
        guard_resolved_ip(&remote.ip())?;
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Only fetch textual content
    if let Some(ref ct) = content_type {
        let ct_lower = ct.to_lowercase();
        if ct_lower.contains("application/octet-stream")
            || ct_lower.contains("video/")
            || ct_lower.contains("audio/")
            || ct_lower.contains("image/")
        {
            return Err(SearchError::Provider(format!(
                "unsupported content type: {ct}"
            )));
        }
    }

    let body = response
        .text()
        .await
        .map_err(|e| SearchError::Network(e.to_string()))?;

    // Enforce max size
    let body = if body.len() as u64 > max_bytes {
        body[..max_bytes as usize].to_string()
    } else {
        body
    };

    let (title, text) = extract_main_content(&body);

    Ok(FetchResult {
        url: url_str.to_string(),
        title,
        text,
        content_type,
        fetched_at: chrono::Utc::now(),
    })
}

/// Extract title and main text from an HTML document.
fn extract_main_content(html: &str) -> (String, String) {
    let document = Html::parse_document(html);

    // Extract title
    let title_selector = Selector::parse("title").unwrap();
    let title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<Vec<_>>().join(" ").trim().to_string())
        .unwrap_or_default();

    // Remove unwanted elements: script, style, nav, header, footer
    let unwanted = Selector::parse("script, style, nav, header, footer, iframe, noscript").unwrap();
    // Try to get article/main content first
    let main_selector = Selector::parse(
        "article, main, [role=\"main\"], .post-content, .article-content, #content, .content",
    )
    .ok();

    // Get the main content container if available, otherwise use body.
    let body_sel = Selector::parse("body").unwrap();

    // Clone the DOM to avoid borrow issues — iterate and collect text.
    let mut text_parts = Vec::new();

    // Try main content selectors first
    if let Some(ref sel) = main_selector {
        if let Some(main_el) = document.select(sel).next() {
            collect_text(main_el, &unwanted, &mut text_parts);
        }
    }

    // If we got nothing from main content, extract from body
    if text_parts.is_empty() {
        if let Some(body_el) = document.select(&body_sel).next() {
            collect_text(body_el, &unwanted, &mut text_parts);
        }
    }

    let text = text_parts
        .join("\n\n")
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    (title, text)
}

#[allow(clippy::only_used_in_recursion)]
fn collect_text(element: scraper::ElementRef<'_>, unwanted: &Selector, output: &mut Vec<String>) {
    for child in element.children() {
        match child.value() {
            scraper::node::Node::Text(text) => {
                let trimmed = text.text.trim();
                if !trimmed.is_empty() {
                    output.push(trimmed.to_string());
                }
            }
            scraper::node::Node::Element(el) => {
                // Skip unwanted elements
                let el_name = el.name();
                if el_name == "script"
                    || el_name == "style"
                    || el_name == "nav"
                    || el_name == "footer"
                    || el_name == "header"
                    || el_name == "iframe"
                    || el_name == "noscript"
                {
                    continue;
                }
                // Add paragraph breaks for block elements
                if matches!(
                    el_name,
                    "p" | "div"
                        | "section"
                        | "article"
                        | "li"
                        | "h1"
                        | "h2"
                        | "h3"
                        | "h4"
                        | "h5"
                        | "h6"
                        | "br"
                        | "blockquote"
                        | "pre"
                ) {
                    // Recurse into element
                    if let Some(el_ref) = scraper::ElementRef::wrap(child) {
                        collect_text(el_ref, unwanted, output);
                    }
                    // Add paragraph separator
                    if !output.is_empty()
                        && !output.last().map(|s| s.ends_with('\n')).unwrap_or(false)
                    {
                        output.push(String::new()); // blank line separator
                    }
                } else {
                    // Inline elements — just recurse
                    if let Some(el_ref) = scraper::ElementRef::wrap(child) {
                        collect_text(el_ref, unwanted, output);
                    }
                }
            }
            _ => {}
        }
    }
}

// ── Search service (orchestration) ──────────────────────────────────────────

pub struct SearchService {
    providers: HashMap<String, Arc<dyn SearchProvider>>,
    cache: SearchCache,
    default_provider: String,
}

impl SearchService {
    pub fn new(config: &SearchConfig, brave_api_key: Option<String>) -> Self {
        let mut providers: HashMap<String, Arc<dyn SearchProvider>> = HashMap::new();

        let ddg: Arc<dyn SearchProvider> = Arc::new(DuckDuckGoProvider::new());
        providers.insert(ddg.id().to_string(), ddg);

        let searxng: Arc<dyn SearchProvider> =
            Arc::new(SearxngProvider::new(config.searxng_instance.clone()));
        providers.insert(searxng.id().to_string(), searxng);

        let brave: Arc<dyn SearchProvider> = Arc::new(BraveProvider::new(
            brave_api_key.or_else(|| config.brave_api_key.clone()),
        ));
        providers.insert(brave.id().to_string(), brave);

        let default_provider = if providers.contains_key(&config.default_provider) {
            config.default_provider.clone()
        } else {
            "duckduckgo".to_string()
        };

        Self {
            providers,
            cache: SearchCache::new(config.cache_ttl_secs),
            default_provider,
        }
    }

    pub fn list_providers(&self) -> Vec<SearchProviderInfo> {
        self.providers
            .values()
            .map(|p| SearchProviderInfo {
                id: p.id().to_string(),
                label: p.label().to_string(),
                requires_key: p.requires_api_key(),
                configured: p.is_configured(),
            })
            .collect()
    }

    pub async fn search(&self, query: &SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
        let provider_id = query
            .provider
            .as_deref()
            .filter(|p| !p.is_empty())
            .unwrap_or(&self.default_provider);

        // Check cache first
        if let Some(cached) = self.cache.get(provider_id, &query.q).await {
            debug!("search cache hit for {provider_id}: {}", query.q);
            return Ok(cached);
        }

        let provider = self.providers.get(provider_id).ok_or_else(|| {
            SearchError::NotConfigured(format!("unknown provider: {provider_id}"))
        })?;

        if !provider.is_configured() {
            return Err(SearchError::NotConfigured(format!(
                "provider '{provider_id}' is not configured"
            )));
        }

        let results = provider.search(query).await?;

        // Update cache
        if !results.is_empty() {
            self.cache.set(provider_id, &query.q, results.clone()).await;
        }

        Ok(results)
    }

    #[allow(dead_code)]
    pub async fn multi_search(
        &self,
        query: &SearchQuery,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let configured: Vec<_> = self
            .providers
            .values()
            .filter(|p| p.is_configured())
            .collect();

        if configured.is_empty() {
            return Err(SearchError::NotConfigured(
                "no search providers configured".to_string(),
            ));
        }

        let futures: Vec<_> = configured
            .iter()
            .map(|provider| {
                let q = SearchQuery {
                    max_results: Some(query.max_results.unwrap_or(3)),
                    ..query.clone()
                };
                async move {
                    match provider.search(&q).await {
                        Ok(results) => results,
                        Err(e) => {
                            warn!("provider {} failed: {e}", provider.id());
                            Vec::new()
                        }
                    }
                }
            })
            .collect();

        let all_results: Vec<SearchResult> =
            join_all(futures).await.into_iter().flatten().collect();

        // Deduplicate by URL
        let mut seen = std::collections::HashSet::new();
        let deduped: Vec<SearchResult> = all_results
            .into_iter()
            .filter(|r| seen.insert(r.url.clone()))
            .take(query.max_results.unwrap_or(10))
            .collect();

        Ok(deduped)
    }
}

// ── Axum endpoint handlers ──────────────────────────────────────────────────

/// POST /api/search
pub async fn api_search(
    State(state): State<crate::AppState>,
    Json(query): Json<SearchQuery>,
) -> Result<Json<Vec<SearchResult>>, crate::AppError> {
    if query.q.trim().is_empty() {
        return Err(crate::AppError::Provider(
            mlx_ollama_core::ProviderError::InvalidRequest {
                details: "query cannot be empty".to_string(),
            },
        ));
    }

    let results = state
        .search_service
        .search(&query)
        .await
        .map_err(crate::AppError::from)?;

    Ok(Json(results))
}

/// POST /api/search/fetch
#[derive(Debug, Deserialize)]
pub struct FetchRequest {
    pub url: String,
}

pub async fn api_search_fetch(
    State(state): State<crate::AppState>,
    Json(req): Json<FetchRequest>,
) -> Result<Json<FetchResult>, crate::AppError> {
    if req.url.trim().is_empty() {
        return Err(crate::AppError::Provider(
            mlx_ollama_core::ProviderError::InvalidRequest {
                details: "url cannot be empty".to_string(),
            },
        ));
    }

    let result = fetch_and_extract(
        &req.url,
        state.search_config.fetch_timeout_secs,
        state.search_config.fetch_max_bytes,
    )
    .await
    .map_err(crate::AppError::from)?;

    Ok(Json(result))
}

/// GET /api/search/providers
pub async fn api_search_providers(
    State(state): State<crate::AppState>,
) -> Json<Vec<SearchProviderInfo>> {
    Json(state.search_service.list_providers())
}

/// GET /api/search/config
pub async fn api_search_config(State(state): State<crate::AppState>) -> Json<SearchConfig> {
    Json(state.search_config.clone())
}
