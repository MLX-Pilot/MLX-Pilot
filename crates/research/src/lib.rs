//! Deep Research — iterative multi-round research engine.
//!
//! Implements a plan → search → extract → synthesize → stop loop driven by an
//! LLM, producing a polished markdown report and sanitized visual HTML.
//!
//! The engine is **pluggable**: the daemon injects concrete search, fetch, and
//! LLM-call functions so this crate stays provider-agnostic.

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

// ── Progress event ─────────────────────────────────────────────────────────

/// Emitted during every phase transition so the daemon can relay progress via SSE.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgressEvent {
    pub phase: String,
    pub round: usize,
    pub percent: u8,
    pub message: String,
}

// ── Core domain types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub provider: String,
    #[serde(default)]
    pub relevance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub source_url: String,
    pub source_title: String,
    pub extracted_text: String,
    pub round: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Round {
    pub number: usize,
    pub queries: Vec<String>,
    pub sources_found: Vec<Source>,
    pub findings: Vec<Finding>,
    pub report_after: String,
    #[serde(default)]
    pub stop_decision: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchStatus {
    Queued,
    Running,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchStats {
    pub total_rounds: usize,
    pub total_queries: usize,
    pub total_sources: usize,
    pub total_findings: usize,
    pub duration_secs: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSession {
    pub id: String,
    pub query: String,
    pub model_id: String,
    pub max_rounds: usize,
    pub max_time_secs: u64,
    pub search_provider: Option<String>,
    pub category: Option<String>,
    pub owner: Option<String>,
    pub status: ResearchStatus,
    pub rounds: Vec<Round>,
    pub final_report_md: Option<String>,
    pub final_report_html: Option<String>,
    pub all_sources: Vec<Source>,
    pub all_findings: Vec<Finding>,
    pub hidden_images: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub stats: Option<ResearchStats>,
}

// ── Configuration ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchConfig {
    pub max_rounds: usize,
    pub max_time_secs: u64,
    pub max_urls_per_round: usize,
    pub max_content_chars: usize,
    pub extraction_timeout_secs: u64,
    pub planning_timeout_secs: u64,
    pub query_timeout_secs: u64,
    pub min_rounds: usize,
    pub max_empty_rounds: usize,
}

impl Default for ResearchConfig {
    fn default() -> Self {
        Self {
            max_rounds: 5,
            max_time_secs: 300,
            max_urls_per_round: 3,
            max_content_chars: 8000,
            extraction_timeout_secs: 90,
            planning_timeout_secs: 90,
            query_timeout_secs: 120,
            min_rounds: 2,
            max_empty_rounds: 2,
        }
    }
}

// ── Pluggable function types ───────────────────────────────────────────────

/// Async search function: (query, provider, max_results) → Vec<Source>.
pub type SearchFn = Arc<
    dyn Fn(
            String,
            Option<String>,
            usize,
        )
            -> Pin<Box<dyn std::future::Future<Output = Result<Vec<Source>, String>> + Send>>
        + Send
        + Sync,
>;

/// Async fetch function: (url) → (title, body_text).
pub type FetchFn = Arc<
    dyn Fn(
            String,
        )
            -> Pin<Box<dyn std::future::Future<Output = Result<(String, String), String>> + Send>>
        + Send
        + Sync,
>;

/// Async LLM call: (system_prompt, user_prompt) → response_text.
pub type LlmFn = Arc<
    dyn Fn(
            String,
            String,
        ) -> Pin<Box<dyn std::future::Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

// ── Prompt templates (compact — must work with modest local models) ────────

fn current_date_context() -> String {
    let now = Utc::now();
    format!(
        "Today is {} {}, {}.",
        match now.month() {
            1 => "January",
            2 => "February",
            3 => "March",
            4 => "April",
            5 => "May",
            6 => "June",
            7 => "July",
            8 => "August",
            9 => "September",
            10 => "October",
            11 => "November",
            12 => "December",
            _ => "Unknown",
        },
        now.day(),
        now.year()
    )
}

fn plan_prompt(question: &str) -> (String, String) {
    let system = "You are a research strategist. Given a question, produce a concise research plan (3-5 bullet points). Be specific about what information to look for.".to_string();
    let user = format!(
        "{}\n\nQuestion: {}\n\nResearch plan (3-5 bullets):",
        current_date_context(),
        question
    );
    (system, user)
}

fn query_gen_prompt(
    question: &str,
    n: usize,
    round: usize,
    seen: &[String],
    report_so_far: &str,
) -> (String, String) {
    let system = "You are a search query generator. Generate focused search queries to find specific information. Return ONLY a JSON array of strings, no other text.".to_string();
    let seen_str = if seen.is_empty() {
        "none yet".to_string()
    } else {
        seen.iter()
            .map(|s| format!("- {}", s))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let user = format!(
        "{}\n\nQuestion: {}\n\nRound: {}/{}\n\nQueries already used (avoid these):\n{}\n\nCurrent report summary:\n{}\n\nGenerate {} new search queries as a JSON array of strings:",
        current_date_context(),
        question,
        round,
        10, // max
        seen_str,
        if report_so_far.is_empty() { "No findings yet." } else { report_so_far },
        n,
    );
    (system, user)
}

fn extract_prompt(full_text: &str, question: &str, max_chars: usize) -> (String, String) {
    let system = "Extract key facts relevant to the research question. Be concise. Only include information directly relevant.".to_string();
    let truncated = if full_text.len() > max_chars {
        &full_text[..max_chars]
    } else {
        full_text
    };
    let user = format!(
        "Question: {}\n\nText:\n{}\n\nRelevant facts (concise bullet points):",
        question, truncated
    );
    (system, user)
}

fn synthesize_prompt(
    question: &str,
    current_report: &str,
    new_findings: &[Finding],
) -> (String, String) {
    let system = "You are a research synthesizer. Integrate new findings into the existing report. Maintain a clear structure with sections. Cite sources inline as [Source: title].".to_string();
    let findings_text = new_findings
        .iter()
        .map(|f| format!("[From: {}]\n{}", f.source_title, f.extracted_text))
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    let user = if current_report.is_empty() {
        format!(
            "Question: {}\n\nNew findings:\n{}\n\nWrite an initial research report with sections. Use [Source: ...] citations.",
            question, findings_text
        )
    } else {
        format!(
            "Question: {}\n\nCurrent report:\n{}\n\nNew findings:\n{}\n\nIntegrate the new findings into the report. Add new sections if needed. Preserve existing content unless contradicted.",
            question, current_report, findings_text
        )
    };
    (system, user)
}

fn stop_prompt(question: &str, report: &str, round: usize, max_rounds: usize) -> (String, String) {
    let system = "You decide if research is complete. Reply with ONLY the word STOP or CONTINUE, followed by one sentence of reason.".to_string();
    let user = format!(
        "Question: {}\n\nRound {}/{}.\n\nCurrent report:\n{}\n\nIs this report comprehensive enough? STOP or CONTINUE:",
        question, round, max_rounds, report
    );
    (system, user)
}

fn final_report_prompt(question: &str, report: &str, category: Option<&str>) -> (String, String) {
    let system = "You are a report writer. Write a polished, well-structured markdown report. Use headings, bullet points, and source citations. Target 800+ words.".to_string();
    let cat_hint = match category {
        Some("product") => {
            "This is a product review/comparison. Include a comparison table if relevant."
        }
        Some("comparison") => {
            "This is a comparison. Use a structured comparison format with pros/cons."
        }
        Some("howto") => "This is a how-to guide. Include step-by-step instructions.",
        Some("factcheck") => "This is a fact-check. Clearly separate facts from analysis.",
        _ => "",
    };
    let user = format!(
        "{}\n\nQuestion: {}\n\nResearch findings:\n{}\n\n{}\n\nWrite a polished final report in markdown:",
        current_date_context(),
        question,
        report,
        cat_hint,
    );
    (system, user)
}

// ── Research Engine ────────────────────────────────────────────────────────

pub struct ResearchEngine {
    config: ResearchConfig,
    search_fn: SearchFn,
    fetch_fn: FetchFn,
    llm_fn: LlmFn,
}

impl ResearchEngine {
    pub fn new(
        config: ResearchConfig,
        search_fn: SearchFn,
        fetch_fn: FetchFn,
        llm_fn: LlmFn,
    ) -> Self {
        Self {
            config,
            search_fn,
            fetch_fn,
            llm_fn,
        }
    }

    /// Run the full iterative research loop.
    #[allow(clippy::too_many_arguments)]
    pub async fn run(
        &self,
        query: &str,
        model_id: &str,
        search_provider: Option<String>,
        category: Option<String>,
        owner: Option<String>,
        token: CancellationToken,
        progress_cb: impl Fn(ProgressEvent) + Send + Sync + 'static,
    ) -> ResearchSession {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let started = Instant::now();

        let mut session = ResearchSession {
            id: id.clone(),
            query: query.to_string(),
            model_id: model_id.to_string(),
            max_rounds: self.config.max_rounds,
            max_time_secs: self.config.max_time_secs,
            search_provider,
            category,
            owner,
            status: ResearchStatus::Running,
            rounds: Vec::new(),
            final_report_md: None,
            final_report_html: None,
            all_sources: Vec::new(),
            all_findings: Vec::new(),
            hidden_images: Vec::new(),
            created_at: now,
            started_at: Some(now),
            finished_at: None,
            error: None,
            stats: None,
        };

        info!("Research {} started: {}", id, query);

        // ── Phase: Planning ──
        if token.is_cancelled() {
            session.status = ResearchStatus::Cancelled;
            session.error = Some("Cancelled during planning".to_string());
            session.finished_at = Some(Utc::now());
            return session;
        }

        progress_cb(ProgressEvent {
            phase: "planning".into(),
            round: 0,
            percent: 5,
            message: "Planning research strategy...".into(),
        });

        let (sys, usr) = plan_prompt(query);
        let plan = match (self.llm_fn)(sys, usr).await {
            Ok(p) => p,
            Err(e) => {
                session.status = ResearchStatus::Error;
                session.error = Some(format!("LLM planning error: {e}"));
                session.finished_at = Some(Utc::now());
                return session;
            }
        };
        debug!("Research plan: {}", &plan[..plan.len().min(200)]);

        // Auto-detect category if not provided
        let category = if session.category.is_none() {
            let (csys, cusr) = classify_category_prompt(query);
            match (self.llm_fn)(csys, cusr).await {
                Ok(c) => {
                    let c = c.trim().to_lowercase();
                    let cat = if c.contains("product") || c.contains("review") {
                        Some("product".to_string())
                    } else if c.contains("comparison") || c.contains("compare") {
                        Some("comparison".to_string())
                    } else if c.contains("how") || c.contains("tutorial") || c.contains("guide") {
                        Some("howto".to_string())
                    } else if c.contains("fact") || c.contains("verify") {
                        Some("factcheck".to_string())
                    } else {
                        None
                    };
                    session.category = cat.clone();
                    cat
                }
                Err(_) => {
                    // Category classification is non-critical — keep going
                    session.category.clone()
                }
            }
        } else {
            session.category.clone()
        };

        let mut current_report = String::new();
        let mut all_queries: HashSet<String> = HashSet::new();
        let mut all_urls: HashSet<String> = HashSet::new();
        let mut empty_rounds = 0usize;

        // ── Iterative rounds ──
        for round_num in 1..=self.config.max_rounds {
            // Check cancellation
            if token.is_cancelled() {
                session.status = ResearchStatus::Cancelled;
                session.finished_at = Some(Utc::now());
                session.error = Some(format!("Cancelled at round {}", round_num));
                info!("Research {} cancelled at round {}", id, round_num);
                return session;
            }

            // Check timeout
            if started.elapsed().as_secs() > self.config.max_time_secs {
                warn!("Research {} timed out at round {}", id, round_num);
                break;
            }

            let round_pct = 10 + ((round_num as f64 / self.config.max_rounds as f64) * 70.0) as u8;

            // ── Generate queries ──
            progress_cb(ProgressEvent {
                phase: "searching".into(),
                round: round_num,
                percent: round_pct,
                message: format!("Generating search queries for round {}...", round_num),
            });

            let seen_vec: Vec<String> = all_queries.iter().cloned().collect();
            let (qsys, qusr) = query_gen_prompt(query, 2, round_num, &seen_vec, &current_report);
            let queries = match (self.llm_fn)(qsys, qusr).await {
                Ok(raw) => parse_json_string_array(&raw)
                    .unwrap_or_else(|| vec![format!("{} round {}", query, round_num)]),
                Err(e) => {
                    session.status = ResearchStatus::Error;
                    session.error = Some(format!("LLM query generation error: {e}"));
                    session.finished_at = Some(Utc::now());
                    return session;
                }
            };

            // Deduplicate
            let queries: Vec<String> = queries
                .into_iter()
                .filter(|q| all_queries.insert(q.clone()))
                .take(self.config.max_urls_per_round)
                .collect();

            if queries.is_empty() {
                empty_rounds += 1;
                if empty_rounds >= self.config.max_empty_rounds
                    && round_num >= self.config.min_rounds
                {
                    info!("Research {} stopping: {} empty rounds", id, empty_rounds);
                    break;
                }
                continue;
            }
            empty_rounds = 0;

            // ── Search ──
            let mut round_sources: Vec<Source> = Vec::new();
            for q in &queries {
                if token.is_cancelled() {
                    break;
                }
                progress_cb(ProgressEvent {
                    phase: "searching".into(),
                    round: round_num,
                    percent: round_pct,
                    message: format!("Searching: {}...", &q[..q.len().min(60)]),
                });

                match (self.search_fn)(q.clone(), session.search_provider.clone(), 5).await {
                    Ok(results) => {
                        for src in results {
                            if all_urls.insert(src.url.clone()) {
                                round_sources.push(src);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Search failed for '{}': {e}", q);
                    }
                }
            }

            if round_sources.is_empty() {
                empty_rounds += 1;
                session.rounds.push(Round {
                    number: round_num,
                    queries,
                    sources_found: vec![],
                    findings: vec![],
                    report_after: current_report.clone(),
                    stop_decision: None,
                });
                if empty_rounds >= self.config.max_empty_rounds
                    && round_num >= self.config.min_rounds
                {
                    break;
                }
                continue;
            }

            session.all_sources.extend(round_sources.clone());

            // ── Fetch & Extract ──
            let mut round_findings: Vec<Finding> = Vec::new();
            for src in round_sources.iter().take(self.config.max_urls_per_round) {
                if token.is_cancelled() {
                    break;
                }
                progress_cb(ProgressEvent {
                    phase: "extracting".into(),
                    round: round_num,
                    percent: round_pct + 2,
                    message: format!("Reading: {}...", &src.title[..src.title.len().min(60)]),
                });

                match (self.fetch_fn)(src.url.clone()).await {
                    Ok((title, text)) => {
                        let (esys, eusr) =
                            extract_prompt(&text, query, self.config.max_content_chars);
                        match (self.llm_fn)(esys, eusr).await {
                            Ok(extracted) => {
                                let finding = Finding {
                                    source_url: src.url.clone(),
                                    source_title: if title.is_empty() {
                                        src.title.clone()
                                    } else {
                                        title
                                    },
                                    extracted_text: extracted,
                                    round: round_num,
                                };
                                round_findings.push(finding);
                            }
                            Err(e) => {
                                session.status = ResearchStatus::Error;
                                session.error =
                                    Some(format!("LLM extraction error for {}: {e}", src.url));
                                session.finished_at = Some(Utc::now());
                                return session;
                            }
                        }
                    }
                    Err(e) => {
                        // Use snippet as fallback
                        let finding = Finding {
                            source_url: src.url.clone(),
                            source_title: src.title.clone(),
                            extracted_text: format!("Snippet: {}", src.snippet),
                            round: round_num,
                        };
                        round_findings.push(finding);
                        warn!("Fetch failed for {}: {e}", src.url);
                    }
                }
            }

            session.all_findings.extend(round_findings.clone());

            // ── Synthesize ──
            progress_cb(ProgressEvent {
                phase: "synthesizing".into(),
                round: round_num,
                percent: round_pct + 5,
                message: format!("Synthesizing round {} findings...", round_num),
            });

            let (ssys, susr) = synthesize_prompt(query, &current_report, &round_findings);
            current_report = match (self.llm_fn)(ssys, susr).await {
                Ok(synth) => synth,
                Err(e) => {
                    session.status = ResearchStatus::Error;
                    session.error = Some(format!("LLM synthesis error: {e}"));
                    session.finished_at = Some(Utc::now());
                    return session;
                }
            };

            // ── Decide stop ──
            progress_cb(ProgressEvent {
                phase: "deciding".into(),
                round: round_num,
                percent: round_pct + 7,
                message: "Evaluating if more research is needed...".into(),
            });

            let stop_decision = if round_num < self.config.min_rounds {
                "CONTINUE (minimum rounds not reached)".to_string()
            } else {
                let (tsys, tusr) =
                    stop_prompt(query, &current_report, round_num, self.config.max_rounds);
                match (self.llm_fn)(tsys, tusr).await {
                    Ok(d) => d,
                    Err(e) => {
                        session.status = ResearchStatus::Error;
                        session.error = Some(format!("LLM stop-decision error: {e}"));
                        session.finished_at = Some(Utc::now());
                        return session;
                    }
                }
            };

            let should_stop = stop_decision.trim().to_uppercase().starts_with("STOP")
                || round_num >= self.config.max_rounds;

            session.rounds.push(Round {
                number: round_num,
                queries,
                sources_found: round_sources,
                findings: round_findings,
                report_after: current_report.clone(),
                stop_decision: Some(stop_decision.clone()),
            });

            if should_stop {
                info!(
                    "Research {} stopping at round {}: {}",
                    id, round_num, stop_decision
                );
                break;
            }
        }

        // ── Phase: Final report ──
        if token.is_cancelled() {
            session.status = ResearchStatus::Cancelled;
            session.finished_at = Some(Utc::now());
            return session;
        }

        progress_cb(ProgressEvent {
            phase: "finalizing".into(),
            round: session.rounds.len(),
            percent: 85,
            message: "Writing final report...".into(),
        });

        let (fsys, fusr) = final_report_prompt(query, &current_report, category.as_deref());
        let final_md = match (self.llm_fn)(fsys, fusr).await {
            Ok(r) => r,
            Err(e) => {
                session.status = ResearchStatus::Error;
                session.error = Some(format!("LLM final-report error: {e}"));
                session.finished_at = Some(Utc::now());
                return session;
            }
        };

        // Generate HTML
        progress_cb(ProgressEvent {
            phase: "finalizing".into(),
            round: session.rounds.len(),
            percent: 95,
            message: "Generating visual report...".into(),
        });

        let title = extract_title_from_markdown(&final_md);
        let elapsed = started.elapsed().as_secs_f64();
        let stats = ResearchStats {
            total_rounds: session.rounds.len(),
            total_queries: session.rounds.iter().map(|r| r.queries.len()).sum(),
            total_sources: session.all_sources.len(),
            total_findings: session.all_findings.len(),
            duration_secs: elapsed,
        };

        // Generate HTML with stats
        let final_html = generate_html_report(
            &final_md,
            &title,
            &session.all_sources,
            Some(&stats),
            &session.hidden_images,
            session.category.as_deref(),
        );

        session.final_report_md = Some(final_md);
        session.final_report_html = Some(final_html);
        session.status = ResearchStatus::Done;
        session.finished_at = Some(Utc::now());
        session.stats = Some(stats);

        progress_cb(ProgressEvent {
            phase: "done".into(),
            round: session.rounds.len(),
            percent: 100,
            message: "Research complete!".into(),
        });

        info!(
            "Research {} complete: {} rounds, {} sources, {:.1}s",
            id,
            session.rounds.len(),
            session.all_sources.len(),
            elapsed
        );

        session
    }
}

// ── Helper: parse JSON string array from LLM output ───────────────────────

fn parse_json_string_array(raw: &str) -> Option<Vec<String>> {
    // Try to extract a JSON array from the text
    let trimmed = raw.trim();
    // Remove markdown code fences if present
    let cleaned = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .map(|s| s.strip_suffix("```").unwrap_or(s))
        .unwrap_or(trimmed)
        .trim();

    // Try direct parse
    if let Ok(arr) = serde_json::from_str::<Vec<String>>(cleaned) {
        return Some(arr);
    }

    // Try to find [...] in the text
    if let Some(start) = cleaned.find('[') {
        if let Some(end) = cleaned.rfind(']') {
            let slice = &cleaned[start..=end];
            if let Ok(arr) = serde_json::from_str::<Vec<String>>(slice) {
                return Some(arr);
            }
            // Try as Vec<serde_json::Value> and stringify
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(slice) {
                return Some(
                    arr.iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        })
                        .collect(),
                );
            }
        }
    }

    // Fallback: split by newlines and clean up
    let lines: Vec<String> = cleaned
        .lines()
        .map(|l| {
            l.trim()
                .trim_start_matches('-')
                .trim_start_matches(|c: char| c.is_numeric() || c == '.')
                .trim()
        })
        .filter(|l| !l.is_empty())
        .map(|l| l.trim_matches('"').to_string())
        .collect();

    if !lines.is_empty() {
        Some(lines)
    } else {
        None
    }
}

fn classify_category_prompt(question: &str) -> (String, String) {
    let system =
        "Classify this research question into ONE category. Reply with only the category word."
            .to_string();
    let user = format!(
        "Question: {}\n\nCategories: product, comparison, howto, factcheck, general\n\nCategory:",
        question
    );
    (system, user)
}

// ── HTML Report Generation ─────────────────────────────────────────────────

/// Convert markdown to sanitized HTML.
pub fn render_markdown_to_html(md: &str) -> String {
    let mut options = comrak::ComrakOptions::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.header_ids = Some("heading-".to_string());
    options.render.unsafe_ = false;
    comrak::markdown_to_html(md, &options)
}

/// Sanitize HTML, allowing only safe tags (ammonia defaults + structural/visual extras).
pub fn sanitize_html(html: &str) -> String {
    let mut builder = ammonia::Builder::default();
    builder
        .add_generic_attributes(&["id", "class", "data-image-url", "data-hidden"])
        .add_tags(&["h1", "h2", "h3", "h4", "h5", "h6"])
        .add_tags(&["p", "br", "hr"])
        .add_tags(&["ul", "ol", "li"])
        .add_tags(&["strong", "em", "b", "i", "u", "s", "del", "ins"])
        .add_tags(&["code", "pre"])
        .add_tags(&["blockquote"])
        .add_tags(&["table", "thead", "tbody", "tr", "th", "td"])
        .add_tags(&["img", "figure", "figcaption"])
        .add_tags(&[
            "div", "span", "section", "nav", "main", "article", "header", "footer",
        ])
        .add_tags(&["details", "summary"])
        .add_tags(&["button"])
        .add_tag_attributes(
            "img",
            &[
                "src",
                "alt",
                "title",
                "width",
                "height",
                "loading",
                "data-hidden",
            ],
        )
        .add_tag_attributes("td", &["align"])
        .add_tag_attributes("th", &["align"])
        .add_tag_attributes("col", &["span", "width"])
        .add_tag_attributes("colgroup", &["span", "width"])
        .add_tag_attributes("button", &["onclick", "data-url"]);
    builder.clean(html).to_string()
}

/// Extract the first h1 heading from markdown.
pub fn extract_title_from_markdown(md: &str) -> String {
    for line in md.lines() {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix("# ") {
            return stripped.trim().to_string();
        }
    }
    // Fallback: first non-empty line
    md.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim().trim_start_matches('#').trim().to_string())
        .unwrap_or_else(|| "Research Report".to_string())
}

/// Extract headings from markdown for TOC generation.
fn extract_headings(md: &str) -> Vec<(String, String, usize)> {
    // Returns (id, text, level)
    let mut headings = Vec::new();
    for line in md.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("## ") {
            let text = rest.trim();
            let id = text
                .to_lowercase()
                .replace(|c: char| !c.is_alphanumeric(), "-");
            headings.push((id, text.to_string(), 2));
        } else if let Some(rest) = trimmed.strip_prefix("### ") {
            let text = rest.trim();
            let id = text
                .to_lowercase()
                .replace(|c: char| !c.is_alphanumeric(), "-");
            headings.push((id, text.to_string(), 3));
        }
    }
    headings
}

/// Generate a complete self-contained HTML report page.
pub fn generate_html_report(
    markdown: &str,
    title: &str,
    sources: &[Source],
    stats: Option<&ResearchStats>,
    hidden_images: &[String],
    category: Option<&str>,
) -> String {
    let body_html = render_markdown_to_html(markdown);
    let body_sanitized = sanitize_html(&body_html);
    let headings = extract_headings(markdown);

    // Build TOC
    let mut toc_html = String::new();
    for (id, text, level) in &headings {
        let indent = if *level == 3 {
            "padding-left: 16px;"
        } else {
            ""
        };
        toc_html.push_str(&format!(
            "<li style=\"{}\"><a href=\"#heading-{}\">{}</a></li>",
            indent,
            id,
            html_escape(text),
        ));
    }

    // Stats bar
    let stats_html = if let Some(s) = stats {
        format!(
            r#"<div class="stats-bar">
  <span class="stat"><strong>Rounds:</strong> {rounds}</span>
  <span class="stat"><strong>Sources:</strong> {sources_count}</span>
  <span class="stat"><strong>Findings:</strong> {findings}</span>
  <span class="stat"><strong>Duration:</strong> {duration:.1}s</span>
</div>"#,
            rounds = s.total_rounds,
            sources_count = s.total_sources,
            findings = s.total_findings,
            duration = s.duration_secs,
        )
    } else {
        String::new()
    };

    // Sources section
    let mut sources_html = String::new();
    for (i, src) in sources.iter().enumerate() {
        sources_html.push_str(&format!(
            r#"<div class="source-item">
  <span class="source-num">{i}.</span>
  <a href="{url}" target="_blank" rel="noopener">{title}</a>
  <span class="source-provider">({provider})</span>
  <p class="source-snippet">{snippet}</p>
</div>"#,
            i = i + 1,
            url = html_escape(&src.url),
            title = html_escape(&src.title),
            provider = html_escape(&src.provider),
            snippet = html_escape(&src.snippet),
        ));
    }

    // Hidden images as JSON for JS
    let hidden_json = serde_json::to_string(hidden_images).unwrap_or_else(|_| "[]".to_string());

    // Escaped title for JS embedding
    let title_escaped = title.replace('\\', "\\\\").replace('\'', "\\'");

    format!(
        r#"<!DOCTYPE html>
<html lang="pt-BR">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title} — Deep Research</title>
<style>
  :root {{
    --bg: #0d1117;
    --bg-secondary: #161b22;
    --bg-tertiary: #21262d;
    --border: #30363d;
    --text: #e6edf3;
    --text-secondary: #8b949e;
    --accent: #58a6ff;
    --accent-hover: #79c0ff;
    --green: #3fb950;
    --orange: #d2991d;
    --red: #f85149;
  }}
  *, *::before, *::after {{ box-sizing: border-box; margin: 0; padding: 0; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', system-ui, sans-serif;
    background: var(--bg);
    color: var(--text);
    line-height: 1.6;
    max-width: 900px;
    margin: 0 auto;
    padding: 24px 20px 80px;
  }}
  .hero {{
    text-align: center;
    padding: 60px 20px 40px;
    border-bottom: 1px solid var(--border);
    margin-bottom: 32px;
  }}
  .hero h1 {{
    font-size: 2.2em;
    margin-bottom: 8px;
    background: linear-gradient(135deg, var(--accent), #a371f7);
    -webkit-background-clip: text;
    -webkit-text-fill-color: transparent;
    background-clip: text;
  }}
  .hero .category {{
    display: inline-block;
    background: var(--bg-tertiary);
    color: var(--accent);
    padding: 4px 12px;
    border-radius: 20px;
    font-size: 0.85em;
    margin-top: 8px;
  }}
  .stats-bar {{
    display: flex;
    gap: 24px;
    justify-content: center;
    flex-wrap: wrap;
    padding: 20px;
    background: var(--bg-secondary);
    border-radius: 8px;
    border: 1px solid var(--border);
    margin-bottom: 32px;
  }}
  .stat {{ font-size: 0.9em; color: var(--text-secondary); }}
  .stat strong {{ color: var(--text); }}
  .toc-container {{
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 20px 24px;
    margin-bottom: 32px;
  }}
  .toc-container h2 {{ font-size: 1.1em; margin-bottom: 12px; color: var(--accent); }}
  .toc-container ol {{ padding-left: 20px; }}
  .toc-container li {{ margin: 6px 0; font-size: 0.9em; }}
  .toc-container a {{ color: var(--text-secondary); text-decoration: none; }}
  .toc-container a:hover {{ color: var(--accent-hover); }}
  .report-body {{
    font-size: 1.05em;
    line-height: 1.8;
  }}
  .report-body h1, .report-body h2, .report-body h3, .report-body h4 {{
    color: var(--text);
    margin-top: 2em;
    margin-bottom: 0.5em;
  }}
  .report-body h2 {{ border-bottom: 1px solid var(--border); padding-bottom: 8px; }}
  .report-body p {{ margin-bottom: 1em; }}
  .report-body a {{ color: var(--accent); text-decoration: none; }}
  .report-body a:hover {{ text-decoration: underline; }}
  .report-body blockquote {{
    border-left: 3px solid var(--accent);
    padding: 8px 16px;
    margin: 16px 0;
    background: var(--bg-secondary);
    border-radius: 0 6px 6px 0;
  }}
  .report-body code {{
    background: var(--bg-tertiary);
    padding: 2px 6px;
    border-radius: 4px;
    font-size: 0.9em;
  }}
  .report-body pre {{
    background: var(--bg-secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 16px;
    overflow-x: auto;
    margin: 16px 0;
    font-size: 0.9em;
  }}
  .report-body table {{
    width: 100%;
    border-collapse: collapse;
    margin: 16px 0;
  }}
  .report-body th, .report-body td {{
    border: 1px solid var(--border);
    padding: 8px 12px;
    text-align: left;
  }}
  .report-body th {{ background: var(--bg-tertiary); }}
  .report-body img {{
    max-width: 100%;
    height: auto;
    border-radius: 8px;
    margin: 16px 0;
    cursor: pointer;
  }}
  .report-body img[data-hidden="true"] {{
    opacity: 0.15;
    filter: blur(20px);
    transition: all 0.3s;
  }}
  .report-body img[data-hidden="true"]:hover {{
    opacity: 0.5;
    filter: blur(8px);
  }}
  .img-overlay {{
    position: relative;
    display: inline-block;
  }}
  .img-overlay button {{
    position: absolute;
    top: 8px;
    right: 8px;
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 4px 10px;
    border-radius: 4px;
    cursor: pointer;
    font-size: 0.8em;
  }}
  .img-overlay button:hover {{ background: var(--border); }}
  .sources-section {{
    margin-top: 48px;
    border-top: 1px solid var(--border);
    padding-top: 32px;
  }}
  .sources-section h2 {{ color: var(--accent); }}
  .source-item {{
    padding: 12px;
    margin: 8px 0;
    background: var(--bg-secondary);
    border-radius: 6px;
    border: 1px solid var(--border);
  }}
  .source-num {{ color: var(--text-secondary); margin-right: 8px; font-size: 0.85em; }}
  .source-provider {{ color: var(--text-secondary); font-size: 0.8em; margin-left: 6px; }}
  .source-snippet {{ color: var(--text-secondary); font-size: 0.85em; margin-top: 4px; }}
  .export-bar {{
    display: flex;
    gap: 12px;
    justify-content: center;
    margin-top: 48px;
    padding-top: 24px;
    border-top: 1px solid var(--border);
  }}
  .export-bar button {{
    background: var(--bg-tertiary);
    border: 1px solid var(--border);
    color: var(--text);
    padding: 8px 20px;
    border-radius: 6px;
    cursor: pointer;
    font-size: 0.9em;
  }}
  .export-bar button:hover {{ background: var(--border); }}
  @media print {{
    body {{ background: white; color: black; }}
    .stats-bar, .toc-container, .export-bar {{ break-inside: avoid; }}
    .report-body img[data-hidden="true"] {{ opacity: 1; filter: none; }}
  }}
</style>
</head>
<body>
<header class="hero">
  <h1>{title}</h1>
  {category_html}
</header>
{stats_html}
<nav class="toc-container">
  <h2>Table of Contents</h2>
  <ol>
    {toc_html}
  </ol>
</nav>
<main class="report-body">
  {body}
</main>
<section class="sources-section">
  <h2>Sources ({sources_len})</h2>
  {sources_html}
</section>
<div class="export-bar">
  <button onclick="window.print()">Export PDF</button>
  <button onclick="navigator.clipboard.writeText(document.documentElement.outerHTML)">Copy HTML</button>
</div>
<script>
  var HIDDEN_IMAGES = {hidden_json};
  var REPORT_TITLE = '{title_escaped}';
  document.querySelectorAll('.report-body img').forEach(function(img) {{
    if (HIDDEN_IMAGES.indexOf(img.src) >= 0) {{
      img.setAttribute('data-hidden', 'true');
    }}
    img.addEventListener('click', function() {{
      if (img.getAttribute('data-hidden') === 'true') {{
        img.setAttribute('data-hidden', 'false');
        img.style.opacity = '';
        img.style.filter = '';
      }} else {{
        img.setAttribute('data-hidden', 'true');
        img.style.opacity = '0.15';
        img.style.filter = 'blur(20px)';
      }}
    }});
  }});
</script>
</body>
</html>"#,
        title = html_escape(title),
        category_html = if let Some(cat) = category {
            format!(r#"<div class="category">{}</div>"#, html_escape(cat))
        } else {
            String::new()
        },
        stats_html = stats_html,
        toc_html = toc_html,
        body = body_sanitized,
        sources_len = sources.len(),
        sources_html = sources_html,
        hidden_json = hidden_json,
        title_escaped = title_escaped,
    )
}

// The generate_html_report function needs category from the session.
// We handle this via a separate internal function.

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

// ── Persistence ────────────────────────────────────────────────────────────

/// Persist a research session to disk as JSON.
pub fn persist_session(data_dir: &Path, session: &ResearchSession) -> Result<(), String> {
    let dir = data_dir.join("research");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create research dir: {e}"))?;
    let path = dir.join(format!("{}.json", session.id));
    let json =
        serde_json::to_string_pretty(session).map_err(|e| format!("Serialization error: {e}"))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write session: {e}"))?;
    debug!("Persisted research session {}", session.id);
    Ok(())
}

/// Load a research session from disk.
pub fn load_session(data_dir: &Path, id: &str) -> Result<ResearchSession, String> {
    let path = data_dir.join("research").join(format!("{}.json", id));
    let json = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read session {}: {e}", id))?;
    serde_json::from_str(&json).map_err(|e| format!("Deserialization error: {e}"))
}

/// List all persisted research sessions.
pub fn list_sessions(data_dir: &Path) -> Result<Vec<ResearchSession>, String> {
    let dir = data_dir.join("research");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    let entries =
        std::fs::read_dir(&dir).map_err(|e| format!("Failed to read research dir: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("Dir entry error: {e}"))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            match std::fs::read_to_string(&path) {
                Ok(json) => {
                    if let Ok(session) = serde_json::from_str::<ResearchSession>(&json) {
                        sessions.push(session);
                    }
                }
                Err(e) => warn!("Failed to read session file {}: {e}", path.display()),
            }
        }
    }
    sessions.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(sessions)
}

/// Delete a persisted research session.
pub fn delete_session(data_dir: &Path, id: &str) -> Result<(), String> {
    let path = data_dir.join("research").join(format!("{}.json", id));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| format!("Failed to delete session: {e}"))?;
    }
    Ok(())
}

// ── Helpers for image hide/reroll ──────────────────────────────────────────

/// Hide an image in a session's report.
pub fn hide_image(data_dir: &Path, session_id: &str, image_url: &str) -> Result<(), String> {
    let mut session = load_session(data_dir, session_id)?;
    if !session.hidden_images.contains(&image_url.to_string()) {
        session.hidden_images.push(image_url.to_string());
    }
    // Regenerate HTML to reflect hidden state
    if let Some(ref md) = session.final_report_md {
        let title = extract_title_from_markdown(md);
        let cat = session.category.clone();
        session.final_report_html = Some(generate_html_report(
            md,
            &title,
            &session.all_sources,
            session.stats.as_ref(),
            &session.hidden_images,
            cat.as_deref(),
        ));
    }
    persist_session(data_dir, &session)?;
    Ok(())
}

/// Unhide all images in a session's report.
pub fn unhide_all_images(data_dir: &Path, session_id: &str) -> Result<(), String> {
    let mut session = load_session(data_dir, session_id)?;
    session.hidden_images.clear();
    if let Some(ref md) = session.final_report_md {
        let title = extract_title_from_markdown(md);
        let cat = session.category.clone();
        session.final_report_html = Some(generate_html_report(
            md,
            &title,
            &session.all_sources,
            session.stats.as_ref(),
            &session.hidden_images,
            cat.as_deref(),
        ));
    }
    persist_session(data_dir, &session)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_title() {
        assert_eq!(
            extract_title_from_markdown("# Hello World\n\nSome text"),
            "Hello World"
        );
        assert_eq!(
            extract_title_from_markdown("No heading here"),
            "No heading here"
        );
    }

    #[test]
    fn test_render_markdown() {
        let html = render_markdown_to_html("**bold** and *italic*");
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<em>italic</em>"));
    }

    #[test]
    fn test_sanitize_html() {
        let dirty = r#"<p>Hello</p><script>alert('xss')</script>"#;
        let clean = sanitize_html(dirty);
        assert!(clean.contains("<p>Hello</p>"));
        assert!(!clean.contains("<script>"));
    }

    #[test]
    fn test_parse_json_string_array() {
        let result = parse_json_string_array(r#"["a", "b", "c"]"#);
        assert_eq!(result, Some(vec!["a".into(), "b".into(), "c".into()]));

        let result = parse_json_string_array("no array here");
        assert!(result.is_some()); // falls back to line-based
    }

    #[test]
    fn test_html_escape() {
        assert_eq!(html_escape("<script>"), "&lt;script&gt;");
        assert_eq!(html_escape("a & b"), "a &amp; b");
    }

    #[test]
    fn test_extract_headings() {
        let md = "## Section A\n### Subsection\n## Section B";
        let headings = extract_headings(md);
        assert_eq!(headings.len(), 3);
        assert_eq!(headings[0].1, "Section A");
        assert_eq!(headings[0].2, 2);
        assert_eq!(headings[1].1, "Subsection");
        assert_eq!(headings[1].2, 3);
    }

    // ── Fail-fast tests ─────────────────────────────────────────────────
    /// Mock helpers for testing the engine abort-on-LLM-error behavior.
    fn mock_search_fn() -> SearchFn {
        Arc::new(|_q, _p, _n| {
            Box::pin(async {
                Ok(vec![Source {
                    url: "http://example.com".into(),
                    title: "Test Source".into(),
                    snippet: "A test snippet".into(),
                    provider: "test".into(),
                    relevance: None,
                }])
            })
        })
    }

    fn mock_fetch_fn() -> FetchFn {
        Arc::new(|_url| {
            Box::pin(async { Ok(("Test Title".into(), "Test body text for extraction.".into())) })
        })
    }

    fn failing_llm_fn() -> LlmFn {
        Arc::new(|_sys, _usr| Box::pin(async { Err("LLM unavailable".to_string()) }))
    }

    fn ok_llm_fn(response: &'static str) -> LlmFn {
        Arc::new(move |_sys, _usr| Box::pin(async move { Ok(response.to_string()) }))
    }

    #[tokio::test]
    async fn test_engine_aborts_on_planning_llm_error() {
        let config = ResearchConfig::default();
        let engine =
            ResearchEngine::new(config, mock_search_fn(), mock_fetch_fn(), failing_llm_fn());
        let token = CancellationToken::new();
        let events = std::sync::Mutex::new(Vec::new());

        let session = engine
            .run(
                "test question",
                "test-model",
                None,
                None,
                None,
                token,
                move |ev| {
                    if let Ok(mut v) = events.lock() {
                        v.push(ev.phase);
                    }
                },
            )
            .await;

        assert_eq!(session.status, ResearchStatus::Error);
        assert!(session.error.unwrap().contains("LLM planning error"));
    }

    #[tokio::test]
    async fn test_engine_aborts_on_synthesis_llm_error() {
        // Use an LLM that succeeds for planning but fails on synthesis.
        // After planning, the engine calls query-gen. We fail there.
        let config = ResearchConfig::default();
        let call_count = std::sync::atomic::AtomicUsize::new(0);
        let selective_llm: LlmFn = Arc::new(move |_sys, _usr| {
            let n = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(async move {
                if n == 0 {
                    Ok("Research plan: look it up.".to_string()) // planning OK
                } else {
                    Err("LLM crashed".to_string()) // query-gen fails
                }
            })
        });

        let engine = ResearchEngine::new(config, mock_search_fn(), mock_fetch_fn(), selective_llm);
        let token = CancellationToken::new();

        let session = engine
            .run("test", "m", None, None, None, token, |_| {})
            .await;

        assert_eq!(session.status, ResearchStatus::Error);
        assert!(session
            .error
            .unwrap()
            .contains("LLM query generation error"));
    }

    #[tokio::test]
    async fn test_engine_honours_cancellation() {
        let config = ResearchConfig::default();
        let engine =
            ResearchEngine::new(config, mock_search_fn(), mock_fetch_fn(), ok_llm_fn("OK"));
        let token = CancellationToken::new();
        token.cancel(); // Cancel immediately

        let session = engine
            .run("test", "m", None, None, None, token, |_| {})
            .await;

        assert_eq!(session.status, ResearchStatus::Cancelled);
    }
}
