//! Agentic research command. Runs a streaming tool-loop where the LLM drives
//! multi-step research: it searches academic sources (via the Python sidecar),
//! optionally fetches full page content to go deeper, and streams a cited
//! synthesis. Mirrors the native tool-calling loop in `chat.rs`, but its tools
//! are research providers rather than the workspace toolset.

use super::config::{AiConfig, AiConfigState};
use super::research::{
    research_search_for_agent, ResearchSearchItem, ResearchSearchKind,
};
use super::sidecar::SidecarState;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};

const AGENT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
/// Hard cap on tool-loop turns to bound cost/latency for a single research run.
const MAX_TURNS: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct AgentThinkingEvent {
    pub request_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentToolEvent {
    pub request_id: String,
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub status: String, // "running" | "success" | "error"
    pub summary: String,
}

/// A search result surfaced to the UI, carrying the global citation index the
/// model is instructed to cite as `[n]`.
#[derive(Debug, Clone, Serialize)]
pub struct AgentResultsEvent {
    pub request_id: String,
    pub results: Vec<AgentSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSource {
    pub citation: usize,
    pub title: String,
    pub url: String,
    pub content: String,
    pub provider: String,
    pub category: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStreamEvent {
    pub request_id: String,
    pub seq: u64,
    pub content: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentErrorEvent {
    pub request_id: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentDoneEvent {
    pub request_id: String,
}

/// Internal accumulator for streamed tool-call deltas, keyed by stream index.
#[derive(Default)]
struct ToolCallAccum {
    id: String,
    name: String,
    arguments: String,
}

fn agent_tool_defs() -> Vec<claude_code_rs::api::ToolDefinition> {
    vec![
        claude_code_rs::api::ToolDefinition::new(
            "search_academic",
            "Search academic and code sources for papers, datasets, or code repositories. \
             Returns titles, URLs, and abstracts/descriptions. Use kind='paper' for research \
             literature (arXiv, Semantic Scholar, OpenAlex), kind='dataset' for datasets \
             (Zenodo, Kaggle, GitHub), kind='code' for code repositories (GitHub).",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query" },
                    "kind": {
                        "type": "string",
                        "enum": ["paper", "dataset", "code"],
                        "description": "What kind of source to search for"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Max results to return (1-15)",
                        "default": 8
                    }
                },
                "required": ["query", "kind"]
            }),
        ),
        claude_code_rs::api::ToolDefinition::new(
            "search_web",
            "Search the general web for background, news, or context not found in academic \
             sources. Returns titles, URLs, and snippets.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The web search query" }
                },
                "required": ["query"]
            }),
        ),
        claude_code_rs::api::ToolDefinition::new(
            "fetch_url",
            "Fetch the full text content of a specific URL to read it in depth. Use this on a \
             promising result from a prior search to extract details, methodology, or data \
             beyond the abstract.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string", "description": "The URL to fetch and read" }
                },
                "required": ["url"]
            }),
        ),
    ]
}

fn system_prompt() -> String {
    "You are an expert research assistant for mathematical modeling projects (MCM/ICM style). \
     Your job is to answer the user's research question by actively searching academic sources.\n\n\
     Process:\n\
     1. Break the question into search tasks. Call `search_academic` (and `search_web` when you \
     need general context) to gather sources. Prefer multiple targeted searches over one broad one.\n\
     2. When a result looks important, call `fetch_url` to read its full content before relying on it.\n\
     3. Once you have enough evidence, write a thorough, well-structured answer in markdown.\n\n\
     Citation rules:\n\
     - Every search result is given a number like [1], [2]. Cite EVERY factual claim with the \
     matching bracketed number(s), e.g. \"GNNs improve traffic forecasting [3][5]\".\n\
     - Only cite numbers that were actually returned to you. Never invent citations.\n\
     - Use headers, lists, and bold for structure. Be concise but complete. End with a short summary \
     and concrete suggestions for the modeling task.\n\n\
     Do not fabricate sources or data. If the searches don't answer the question, say so."
        .to_string()
}

// ── SSE stream parsing (OpenAI-compatible, with tool_calls) ──

#[derive(Debug)]
enum StreamLine {
    Content(String),
    Thinking(String),
    ToolCall(ToolCallChunk),
    Finish(String),
    Done,
    Ignore,
}

#[derive(Debug)]
struct ToolCallChunk {
    index: i64,
    id: Option<String>,
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    delta: StreamDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamDelta {
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCall>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCall {
    index: i64,
    #[serde(default)]
    id: Option<String>,
    function: Option<StreamFunction>,
}

#[derive(Debug, Deserialize)]
struct StreamFunction {
    name: Option<String>,
    arguments: Option<String>,
}

fn parse_sse_line(line: &str) -> StreamLine {
    let Some(data) = line.strip_prefix("data: ") else {
        return StreamLine::Ignore;
    };
    if data == "[DONE]" {
        return StreamLine::Done;
    }
    let Ok(chunk) = serde_json::from_str::<StreamChunk>(data) else {
        return StreamLine::Ignore;
    };
    let Some(choice) = chunk.choices.first() else {
        return StreamLine::Ignore;
    };
    if let Some(ref reason) = choice.finish_reason {
        return StreamLine::Finish(reason.clone());
    }
    if let Some(ref content) = choice.delta.reasoning_content {
        return StreamLine::Thinking(content.clone());
    }
    if let Some(ref content) = choice.delta.reasoning {
        return StreamLine::Thinking(content.clone());
    }
    if let Some(ref content) = choice.delta.content {
        return StreamLine::Content(content.clone());
    }
    if let Some(ref tcs) = choice.delta.tool_calls {
        if let Some(tc) = tcs.first() {
            return StreamLine::ToolCall(ToolCallChunk {
                index: tc.index,
                id: tc.id.clone(),
                name: tc.function.as_ref().and_then(|f| f.name.clone()),
                arguments: tc.function.as_ref().and_then(|f| f.arguments.clone()),
            });
        }
    }
    StreamLine::Ignore
}

/// Shared state threaded through the loop: accumulates sources with their
/// global citation indices so the same source keeps one number across turns.
struct CitationStore {
    sources: Vec<AgentSource>,
    seen: HashMap<String, usize>,
}

impl CitationStore {
    fn new() -> Self {
        Self {
            sources: Vec::new(),
            seen: HashMap::new(),
        }
    }

    /// Register results, assigning each a stable citation number. Returns the
    /// newly added sources (for UI emission) and the citation index per item.
    fn add(&mut self, items: &[ResearchSearchItem]) -> Vec<AgentSource> {
        let mut added = Vec::new();
        for item in items {
            let key = item.url.trim().to_ascii_lowercase();
            if key.is_empty() || self.seen.contains_key(&key) {
                continue;
            }
            let citation = self.sources.len() + 1;
            self.seen.insert(key, citation);
            let source = AgentSource {
                citation,
                title: item.title.clone(),
                url: item.url.clone(),
                content: truncate(&item.content, 600),
                provider: item.provider.clone(),
                category: item.category.clone(),
            };
            self.sources.push(source.clone());
            added.push(source);
        }
        added
    }

    fn citation_for(&self, url: &str) -> Option<usize> {
        self.seen.get(&url.trim().to_ascii_lowercase()).copied()
    }
}

fn truncate(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        text.to_string()
    } else {
        let truncated: String = text.chars().take(limit).collect();
        format!("{truncated}...")
    }
}

#[tauri::command]
pub async fn research_agent_run(
    query: String,
    request_id: String,
    scraper: Option<super::research::ResearchScraper>,
    app: AppHandle,
    config_state: State<'_, AiConfigState>,
    sidecar_state: State<'_, SidecarState>,
) -> Result<(), String> {
    let config = config_state.get()?;
    if config
        .api_key
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        let message = "AI API key is not configured. Set it in Settings.".to_string();
        emit_error(&app, &request_id, &message);
        return Err(message);
    }
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Query is empty.".to_string());
    }
    let scraper = scraper.unwrap_or_default();

    let run = AgentRun {
        app,
        request_id,
        config,
        scraper,
    };

    if let Err(error) = run.execute(trimmed, &sidecar_state).await {
        let message = error.to_string();
        emit_error(&run.app, &run.request_id, &message);
        return Err(message);
    }
    let _ = run.app.emit(
        "research_agent:done",
        AgentDoneEvent {
            request_id: run.request_id.clone(),
        },
    );
    Ok(())
}

struct AgentRun {
    app: AppHandle,
    request_id: String,
    config: AiConfig,
    scraper: super::research::ResearchScraper,
}

impl AgentRun {
    async fn execute(
        &self,
        query: &str,
        sidecar: &SidecarState,
    ) -> anyhow::Result<()> {
        let client = claude_code_rs::ApiClient::new(
            self.config
                .to_claude_settings(std::env::current_dir().unwrap_or_default()),
        );
        let tools = agent_tool_defs();
        let mut messages = vec![
            claude_code_rs::ChatMessage::system(system_prompt()),
            claude_code_rs::ChatMessage::user(query.to_string()),
        ];
        let mut citations = CitationStore::new();
        let mut stream_seq = 0u64;

        for _turn in 0..MAX_TURNS {
            let response = client
                .chat_stream(messages.clone(), Some(tools.clone()))
                .await?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                anyhow::bail!("AI error ({status}): {body}");
            }

            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let mut answer_chunk = String::new();
            let mut tool_buf: HashMap<i64, ToolCallAccum> = HashMap::new();
            let mut finish_reason: Option<String> = None;

            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(|e| anyhow::anyhow!("Stream error: {e}"))?;
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                while let Some(idx) = buffer.find('\n') {
                    let line = buffer[..idx].trim_end_matches('\r').to_string();
                    buffer = buffer[idx + 1..].to_string();
                    match parse_sse_line(&line) {
                        StreamLine::Content(content) => {
                            answer_chunk.push_str(&content);
                            stream_seq += 1;
                            let _ = self.app.emit(
                                "research_agent:stream",
                                AgentStreamEvent {
                                    request_id: self.request_id.clone(),
                                    seq: stream_seq,
                                    content,
                                    done: false,
                                },
                            );
                        }
                        StreamLine::Thinking(content) => {
                            let _ = self.app.emit(
                                "research_agent:thinking",
                                AgentThinkingEvent {
                                    request_id: self.request_id.clone(),
                                    content,
                                },
                            );
                        }
                        StreamLine::ToolCall(tc) => {
                            let entry = tool_buf.entry(tc.index).or_default();
                            if let Some(id) = tc.id {
                                entry.id = id;
                            }
                            if let Some(name) = tc.name {
                                entry.name = name;
                            }
                            if let Some(args) = tc.arguments {
                                entry.arguments.push_str(&args);
                            }
                        }
                        StreamLine::Finish(reason) => finish_reason = Some(reason),
                        StreamLine::Done => {
                            if finish_reason.is_none() {
                                finish_reason = Some("stop".to_string());
                            }
                        }
                        StreamLine::Ignore => {}
                    }
                }
            }

            // Tool-calling turn: execute tools and feed results back.
            if finish_reason.as_deref() == Some("tool_calls") && !tool_buf.is_empty() {
                let mut accumulated: Vec<(i64, ToolCallAccum)> = tool_buf.into_iter().collect();
                accumulated.sort_by_key(|(index, _)| *index);
                let tool_calls: Vec<claude_code_rs::api::ToolCall> = accumulated
                    .into_iter()
                    .map(|(_, tc)| claude_code_rs::api::ToolCall {
                        id: tc.id,
                        r#type: "function".to_string(),
                        function: claude_code_rs::api::ToolCallFunction {
                            name: tc.name,
                            arguments: tc.arguments,
                        },
                    })
                    .collect();

                messages.push(claude_code_rs::ChatMessage::assistant_with_tools(
                    tool_calls.clone(),
                ));

                for tc in &tool_calls {
                    let output = self
                        .run_tool(tc, sidecar, &mut citations)
                        .await;
                    messages.push(claude_code_rs::ChatMessage::tool(&tc.id, output));
                }
                continue;
            }

            // No tool calls -> this was the final answer turn.
            break;
        }

        // Final done marker for the answer stream.
        let _ = self.app.emit(
            "research_agent:stream",
            AgentStreamEvent {
                request_id: self.request_id.clone(),
                seq: stream_seq + 1,
                content: String::new(),
                done: true,
            },
        );
        Ok(())
    }

    /// Execute one tool call, emit running/result events, and return the text
    /// fed back to the model (results labeled with their citation numbers).
    async fn run_tool(
        &self,
        tc: &claude_code_rs::api::ToolCall,
        sidecar: &SidecarState,
        citations: &mut CitationStore,
    ) -> String {
        let args: Value = serde_json::from_str(&tc.function.arguments).unwrap_or_else(|_| json!({}));
        let _ = self.app.emit(
            "research_agent:tool",
            AgentToolEvent {
                request_id: self.request_id.clone(),
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: args.clone(),
                status: "running".to_string(),
                summary: String::new(),
            },
        );

        let result = match tc.function.name.as_str() {
            "search_academic" => self.tool_search_academic(&args, sidecar, citations).await,
            "search_web" => self.tool_search_web(&args, citations).await,
            "fetch_url" => self.tool_fetch_url(&args, citations).await,
            other => Err(format!("Unknown tool: {other}")),
        };

        match result {
            Ok((feedback, summary)) => {
                let _ = self.app.emit(
                    "research_agent:tool",
                    AgentToolEvent {
                        request_id: self.request_id.clone(),
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: args,
                        status: "success".to_string(),
                        summary,
                    },
                );
                feedback
            }
            Err(error) => {
                let _ = self.app.emit(
                    "research_agent:tool",
                    AgentToolEvent {
                        request_id: self.request_id.clone(),
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        arguments: args,
                        status: "error".to_string(),
                        summary: error.clone(),
                    },
                );
                json!({ "error": error }).to_string()
            }
        }
    }

    async fn tool_search_academic(
        &self,
        args: &Value,
        sidecar: &SidecarState,
        citations: &mut CitationStore,
    ) -> Result<(String, String), String> {
        let query = args["query"].as_str().unwrap_or("").trim();
        if query.is_empty() {
            return Err("search_academic requires a non-empty query".to_string());
        }
        let kind = match args["kind"].as_str().unwrap_or("paper") {
            "dataset" => ResearchSearchKind::Dataset,
            "code" => ResearchSearchKind::Code,
            _ => ResearchSearchKind::Paper,
        };
        let limit = args["limit"].as_u64().unwrap_or(8).clamp(1, 15);

        let items = research_search_for_agent(&self.config, sidecar, self.scraper, query, &kind, limit)
            .await
            .map_err(|e| e.to_string())?;
        Ok(self.ingest_results(query, items, citations))
    }

    async fn tool_search_web(
        &self,
        args: &Value,
        citations: &mut CitationStore,
    ) -> Result<(String, String), String> {
        let query = args["query"].as_str().unwrap_or("").trim();
        if query.is_empty() {
            return Err("search_web requires a non-empty query".to_string());
        }
        let items = research_search_for_agent(
            &self.config,
            // web search never uses the sidecar; pass through to scraper
            &SidecarState::new(std::path::PathBuf::new()),
            self.scraper,
            query,
            &ResearchSearchKind::Web,
            8,
        )
        .await
        .map_err(|e| e.to_string())?;
        Ok(self.ingest_results(query, items, citations))
    }

    async fn tool_fetch_url(
        &self,
        args: &Value,
        citations: &CitationStore,
    ) -> Result<(String, String), String> {
        let url = args["url"].as_str().unwrap_or("").trim();
        if url.is_empty() {
            return Err("fetch_url requires a url".to_string());
        }
        let body = fetch_url_text(url).await.map_err(|e| e.to_string())?;
        let citation_hint = citations
            .citation_for(url)
            .map(|n| format!(" (this is source [{n}])"))
            .unwrap_or_default();
        let feedback = json!({
            "url": url,
            "content": truncate(&body, 8000),
            "note": format!("Full content fetched{citation_hint}."),
        })
        .to_string();
        Ok((feedback, format!("Fetched {url}")))
    }

    /// Register results for citation, emit them to the UI, and build the
    /// model-facing feedback string with each result labeled by its number.
    fn ingest_results(
        &self,
        query: &str,
        items: Vec<ResearchSearchItem>,
        citations: &mut CitationStore,
    ) -> (String, String) {
        let added = citations.add(&items);
        if !added.is_empty() {
            let _ = self.app.emit(
                "research_agent:results",
                AgentResultsEvent {
                    request_id: self.request_id.clone(),
                    results: added,
                },
            );
        }

        // Build feedback referencing ALL matched results (new and previously seen)
        // so the model can cite stable numbers.
        let mut lines = Vec::new();
        for item in &items {
            if let Some(n) = citations.citation_for(&item.url) {
                lines.push(format!(
                    "[{n}] {}\n    {}\n    {}",
                    item.title,
                    item.url,
                    truncate(&item.content, 500)
                ));
            }
        }
        let summary = format!("{} results for \"{}\"", lines.len(), query);
        let feedback = if lines.is_empty() {
            format!("No results found for \"{query}\".")
        } else {
            format!("Search results for \"{query}\":\n\n{}", lines.join("\n\n"))
        };
        (feedback, summary)
    }
}

async fn fetch_url_text(url: &str) -> anyhow::Result<String> {
    let response = reqwest::Client::builder()
        .timeout(AGENT_HTTP_TIMEOUT)
        .build()?
        .get(url)
        .header(
            reqwest::header::USER_AGENT,
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
        )
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("URL fetch failed ({})", response.status());
    }
    let bytes = response.bytes().await?;
    let max = bytes.len().min(120_000);
    let text = String::from_utf8_lossy(&bytes[..max]);
    Ok(html_to_text(&text))
}

fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len().min(20_000));
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                text.push(' ');
            }
            _ if !in_tag => text.push(ch),
            _ => {}
        }
        if text.len() >= 20_000 {
            break;
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn emit_error(app: &AppHandle, request_id: &str, message: &str) {
    let _ = app.emit(
        "research_agent:error",
        AgentErrorEvent {
            request_id: request_id.to_string(),
            message: message.to_string(),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, url: &str) -> ResearchSearchItem {
        ResearchSearchItem {
            title: title.to_string(),
            url: url.to_string(),
            content: "abstract".to_string(),
            provider: "arxiv".to_string(),
            source: "sidecar_academic".to_string(),
            category: "literature".to_string(),
            authors: None,
            publish_year: None,
            keywords: None,
            relevance_score: 0.5,
            raw_json: json!({}),
            planned_kind: None,
            planned_query: None,
            reason: None,
            rank: None,
        }
    }

    #[test]
    fn citation_store_assigns_stable_increasing_numbers() {
        let mut store = CitationStore::new();
        let added = store.add(&[
            item("A", "https://example.com/a"),
            item("B", "https://example.com/b"),
        ]);
        assert_eq!(added.len(), 2);
        assert_eq!(added[0].citation, 1);
        assert_eq!(added[1].citation, 2);
        assert_eq!(store.citation_for("https://example.com/a"), Some(1));
    }

    #[test]
    fn citation_store_dedupes_by_url_and_keeps_first_number() {
        let mut store = CitationStore::new();
        store.add(&[item("A", "https://example.com/a")]);
        let added = store.add(&[
            item("A again", "https://example.com/a"),
            item("C", "https://example.com/c"),
        ]);
        // "a" already seen -> only "c" is newly added, taking number 2
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].citation, 2);
        assert_eq!(store.citation_for("https://example.com/a"), Some(1));
    }

    #[test]
    fn parses_tool_call_delta() {
        let line = r#"data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"search_academic","arguments":"{}"}}]},"finish_reason":null}]}"#;
        match parse_sse_line(line) {
            StreamLine::ToolCall(tc) => {
                assert_eq!(tc.id.as_deref(), Some("call_1"));
                assert_eq!(tc.name.as_deref(), Some("search_academic"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }

    #[test]
    fn parses_content_and_done() {
        assert!(matches!(
            parse_sse_line(r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#),
            StreamLine::Content(c) if c == "hi"
        ));
        assert!(matches!(parse_sse_line("data: [DONE]"), StreamLine::Done));
    }
}



