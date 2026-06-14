use super::config::{AiConfig, AiConfigState};
use claude_code_rs::{ApiClient, ChatMessage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use tauri::State;

const PROVIDER_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResearchSearchKind {
    Auto,
    Web,
    Paper,
    Dataset,
    Code,
    Docs,
}

/// Which scraper backs a research search. Docs always use Context7 regardless;
/// this only chooses the provider for web/paper/dataset/code kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResearchScraper {
    #[default]
    Firecrawl,
    Tavily,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSearchItem {
    pub title: String,
    pub url: String,
    pub content: String,
    pub provider: String,
    pub source: String,
    pub category: String,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: Option<String>,
    pub relevance_score: f64,
    pub raw_json: Value,
    pub planned_kind: Option<ResearchSearchKind>,
    pub planned_query: Option<String>,
    pub reason: Option<String>,
    pub rank: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchSearchResponse {
    pub query: String,
    pub kind: ResearchSearchKind,
    pub results: Vec<ResearchSearchItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchExtractSaveRequest {
    pub project_id: String,
    pub results: Vec<ResearchSearchItem>,
    pub kind: ResearchSearchKind,
    pub auth_token: Option<String>,
    pub server_base: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchExtractedItem {
    pub title: String,
    pub url: String,
    pub content: String,
    pub category: String,
    pub summary: String,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: String,
    pub methodology: String,
    pub key_parameters: String,
    pub ai_relevance: String,
    pub relevance_score: f64,
    pub bibtex: String,
    pub raw_json: Value,
}

#[derive(Debug, Deserialize)]
struct FirecrawlSearchResponse {
    data: Option<Value>,
    results: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct Context7SearchResponse {
    results: Option<Vec<Value>>,
}

#[derive(Debug, Clone)]
struct SearchTask {
    kind: ResearchSearchKind,
    query: String,
    reason: String,
    expected_category: String,
}

#[derive(Debug, Clone)]
struct AcademicSourceProfile {
    include_domains: Vec<&'static str>,
    exclude_domains: Vec<&'static str>,
}

#[derive(Debug)]
struct ProviderSearchResult {
    items: Vec<ResearchSearchItem>,
    warning: Option<String>,
}

#[derive(Debug)]
enum ProviderTaskOutcome {
    Success {
        results: Vec<ResearchSearchItem>,
        warning: Option<String>,
    },
    Failure {
        query: String,
        error: String,
    },
}

#[derive(Debug)]
struct CollectedProviderResults {
    results: Vec<ResearchSearchItem>,
    warning: Option<String>,
}

#[tauri::command]
pub async fn research_search_native(
    query: String,
    kind: ResearchSearchKind,
    max_results: Option<u64>,
    scraper: Option<ResearchScraper>,
    config_state: State<'_, AiConfigState>,
) -> Result<ResearchSearchResponse, String> {
    let config = config_state.get()?;
    let scraper = scraper.unwrap_or_default();
    let limit = max_results.unwrap_or(8).clamp(1, 20);
    let mut warning = None;
    let tasks = if matches!(&kind, ResearchSearchKind::Auto) {
        match plan_search_tasks(&config, &query).await {
            Ok(tasks) => {
                if tasks.iter().any(|task| task.reason.contains("fallback")) {
                    warning = Some("AI planning failed; using a web search fallback.".to_string());
                }
                tasks
            }
            Err(error) => {
                warning = Some(format!(
                    "AI planning failed; using a web search fallback. {error}"
                ));
                manual_search_tasks(&ResearchSearchKind::Web, &query)
            }
        }
    } else {
        manual_search_tasks(&kind, &query)
    };

    let per_task_limit = per_task_limit(limit, tasks.len());
    let mut outcomes = Vec::new();
    for task in &tasks {
        let task_limit = if matches!(&kind, ResearchSearchKind::Auto) {
            per_task_limit
        } else {
            limit
        };
        let outcome = match match &task.kind {
            ResearchSearchKind::Docs => search_context7(&config, &task.query, task_limit).await,
            ResearchSearchKind::Auto => unreachable!("auto is never a provider task"),
            _ => match scraper {
                ResearchScraper::Tavily => {
                    search_tavily_for_research(&config, &task.query, &task.kind, task_limit).await
                }
                ResearchScraper::Firecrawl => {
                    search_firecrawl(&config, &task.query, &task.kind, task_limit).await
                }
            },
        } {
            Ok(provider_result) => ProviderTaskOutcome::Success {
                results: provider_result.items,
                warning: provider_result.warning,
            },
            Err(error) => ProviderTaskOutcome::Failure {
                query: task.query.clone(),
                error: error.to_string(),
            },
        };
        outcomes.push(outcome);
    }

    let collected = collect_provider_task_outcomes(&tasks, outcomes, limit)
        .map_err(|error| error.to_string())?;
    warning = append_warning(warning, collected.warning);
    let merged = collected.results;
    let results = if merged.is_empty() {
        merged
    } else {
        match rank_search_results(&config, &query, merged.clone()).await {
            Ok(results) => results,
            Err(error) => {
                warning = Some(match warning {
                    Some(existing) => {
                        format!("{existing} AI ranking failed; showing provider order. {error}")
                    }
                    None => format!("AI ranking failed; showing provider order. {error}"),
                });
                merged
            }
        }
    };

    Ok(ResearchSearchResponse {
        query,
        kind,
        results,
        warning,
    })
}

#[tauri::command]
pub async fn research_analyze_url(
    url: String,
    config_state: State<'_, AiConfigState>,
) -> Result<ResearchSearchItem, String> {
    let trimmed = url.trim();
    if trimmed.is_empty() {
        return Err("URL is empty.".to_string());
    }

    let config = config_state.get()?;
    let mut item = analyze_url_hint(trimmed).map_err(|error| error.to_string())?;
    match fetch_url_preview(trimmed).await {
        Ok(preview) => {
            if let Some(title) = preview.title.filter(|title| !title.trim().is_empty()) {
                item.title = title;
            }
            if !preview.content.trim().is_empty() {
                item.content = preview.content;
            }
            if let Some(content_type) = preview.content_type {
                item.raw_json["content_type"] = json!(content_type);
            }
        }
        Err(error) => {
            item.raw_json["fetch_warning"] = json!(error.to_string());
        }
    }

    if config
        .api_key
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        if let Ok(enriched) = enrich_analyzed_url_with_ai(&config, trimmed, &item).await {
            apply_url_ai_enrichment(&mut item, &enriched);
        }
    }

    Ok(item)
}

#[derive(Debug)]
struct UrlPreview {
    title: Option<String>,
    content: String,
    content_type: Option<String>,
}

async fn fetch_url_preview(url: &str) -> anyhow::Result<UrlPreview> {
    let response = reqwest::Client::builder()
        .timeout(PROVIDER_HTTP_TIMEOUT)
        .build()?
        .get(url)
        .send()
        .await?;
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    if !status.is_success() {
        anyhow::bail!("URL fetch failed ({status})");
    }
    let bytes = response.bytes().await?;
    let is_pdf = content_type
        .as_deref()
        .is_some_and(|value| value.to_ascii_lowercase().contains("pdf"))
        || url.to_ascii_lowercase().ends_with(".pdf");
    if is_pdf {
        return Ok(UrlPreview {
            title: None,
            content: format!("PDF document available at {url}."),
            content_type,
        });
    }
    let max_len = bytes.len().min(60_000);
    let text = String::from_utf8_lossy(&bytes[..max_len]).to_string();
    let title = html_title(&text);
    Ok(UrlPreview {
        title,
        content: html_to_text_snippet(&text),
        content_type,
    })
}

async fn enrich_analyzed_url_with_ai(
    config: &AiConfig,
    url: &str,
    item: &ResearchSearchItem,
) -> anyhow::Result<Value> {
    let prompt = format!(
        "Extract research metadata from this user-provided source. Return only JSON with optional keys: title, authors, publish_year, keywords, category, summary.\n\
         category must be one of literature, dataset, code, formula, competition.\n\n\
         URL: {url}\nTitle: {}\nContent:\n{}",
        item.title,
        truncate(&item.content, 5000)
    );
    let content = call_ai_text(
        config,
        "You extract concise metadata for mathematical modeling research sources.",
        &prompt,
    )
    .await?;
    Ok(parse_json_object(&content))
}

fn apply_url_ai_enrichment(item: &mut ResearchSearchItem, value: &Value) {
    if let Some(title) = string_field(value, "title") {
        item.title = title;
    }
    if let Some(authors) = string_field(value, "authors") {
        item.authors = Some(authors);
    }
    if let Some(year) = value.get("publish_year").and_then(|value| value.as_i64()) {
        item.publish_year = Some(year as i32);
    }
    if let Some(keywords) = string_field(value, "keywords") {
        item.keywords = Some(keywords);
    }
    if let Some(category) = string_field(value, "category") {
        item.category = category;
    }
    if let Some(summary) = string_field(value, "summary") {
        item.content = summary;
    }
    item.raw_json["ai_url_metadata"] = value.clone();
}

fn append_warning(current: Option<String>, next: Option<String>) -> Option<String> {
    match (current, next) {
        (Some(current), Some(next)) => Some(format!("{current} {next}")),
        (Some(current), None) => Some(current),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}

fn academic_source_profile(kind: &ResearchSearchKind) -> AcademicSourceProfile {
    let exclude_domains = vec![
        "youtube.com",
        "youtu.be",
        "wikipedia.org",
        "baike.baidu.com",
        "bilibili.com",
        "zhihu.com",
        "csdn.net",
        "medium.com",
    ];
    let include_domains = match kind {
        ResearchSearchKind::Code => vec!["github.com", "gitee.com", "gitlab.com"],
        ResearchSearchKind::Dataset => vec![
            "kaggle.com",
            "zenodo.org",
            "figshare.com",
            "data.gov",
            "worldbank.org",
            "nasa.gov",
            "noaa.gov",
            ".edu",
            ".gov",
        ],
        ResearchSearchKind::Docs => vec!["context7.com"],
        _ => vec![
            "arxiv.org",
            "doi.org",
            "semanticscholar.org",
            "openalex.org",
            "crossref.org",
            "pubmed.ncbi.nlm.nih.gov",
            "ieeexplore.ieee.org",
            "dl.acm.org",
            "springer.com",
            "sciencedirect.com",
            "nature.com",
            "wiley.com",
            ".edu",
            ".gov",
        ],
    };

    AcademicSourceProfile {
        include_domains,
        exclude_domains,
    }
}

fn host_from_url(url: &str) -> Option<String> {
    reqwest::Url::parse(url)
        .ok()
        .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
}

fn domain_matches(host: &str, pattern: &str) -> bool {
    let pattern = pattern.to_ascii_lowercase();
    if pattern.starts_with('.') {
        return host.ends_with(&pattern);
    }
    host == pattern || host.ends_with(&format!(".{pattern}"))
}

fn is_allowed_academic_source(url: &str, kind: &ResearchSearchKind) -> bool {
    let Some(host) = host_from_url(url) else {
        return false;
    };
    let profile = academic_source_profile(kind);
    if profile
        .exclude_domains
        .iter()
        .any(|domain| domain_matches(&host, domain))
    {
        return false;
    }
    profile
        .include_domains
        .iter()
        .any(|domain| domain_matches(&host, domain))
}

fn manual_search_tasks(kind: &ResearchSearchKind, query: &str) -> Vec<SearchTask> {
    let task_kind = if matches!(kind, ResearchSearchKind::Auto) {
        ResearchSearchKind::Web
    } else {
        kind.clone()
    };
    vec![SearchTask {
        expected_category: category_for_kind(&task_kind).to_string(),
        kind: task_kind,
        query: query.to_string(),
        reason: "manual search override".to_string(),
    }]
}

async fn plan_search_tasks(config: &AiConfig, query: &str) -> anyhow::Result<Vec<SearchTask>> {
    let prompt = format!(
        "Plan a research search for this modeling query. Return only JSON with a tasks array. \
         Each task must have kind, query, reason, expected_category. \
         kind must be one of web, paper, dataset, code, docs. \
         Use at most 4 tasks and do not invent providers. \
         Strictly prefer academic, official, dataset, and code repository sources. \
         Do not plan encyclopedia, video, forum, blog, or SEO content as fallback.\n\nQuery: {query}"
    );
    let content = call_ai_text(
        config,
        "You are a research search planner for mathematical modeling projects.",
        &prompt,
    )
    .await?;
    let tasks = parse_ai_search_plan(&content, query);
    if tasks.is_empty() {
        anyhow::bail!("AI planner returned no valid tasks");
    }
    Ok(tasks)
}

async fn rank_search_results(
    config: &AiConfig,
    query: &str,
    results: Vec<ResearchSearchItem>,
) -> anyhow::Result<Vec<ResearchSearchItem>> {
    let candidates = results
        .iter()
        .enumerate()
        .map(|(index, item)| {
            json!({
                "index": index,
                "title": item.title,
                "url": item.url,
                "provider": item.provider,
                "planned_kind": item.planned_kind,
                "category": item.category,
                "content": truncate(&item.content, 700),
            })
        })
        .collect::<Vec<_>>();
    let prompt = format!(
        "Rank these research search results for the user query. Return only JSON with a results array. \
         Each ranked item must include url, title, rank, reason, category, relevance_score. \
         rank is 1 for most relevant. category should be one of literature, dataset, code, formula, competition.\n\n\
         Query: {query}\nResults:\n{}",
        serde_json::to_string(&candidates)?
    );
    let content = call_ai_text(
        config,
        "You are a precise research result ranking assistant.",
        &prompt,
    )
    .await?;
    apply_ai_ranking(results, &content)
}

async fn call_ai_text(config: &AiConfig, system: &str, prompt: &str) -> anyhow::Result<String> {
    if config
        .api_key
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        anyhow::bail!("API key is not configured.");
    }
    let client =
        ApiClient::new(config.to_claude_settings(std::env::current_dir().unwrap_or_default()));
    let response = client
        .chat(
            vec![ChatMessage::system(system), ChatMessage::user(prompt)],
            None,
        )
        .await?;
    Ok(response
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .unwrap_or_default())
}

fn per_task_limit(limit: u64, task_count: usize) -> u64 {
    if task_count <= 1 {
        return limit;
    }
    let task_count = task_count as u64;
    ((limit + task_count - 1) / task_count).clamp(2, 6)
}

fn parse_ai_search_plan(text: &str, fallback_query: &str) -> Vec<SearchTask> {
    let value = parse_json_value(text);
    let tasks = value
        .get("tasks")
        .and_then(|field| field.as_array())
        .cloned()
        .or_else(|| value.as_array().cloned())
        .unwrap_or_default();
    let mut parsed = Vec::new();
    for task in tasks {
        let Some(kind) = task
            .get("kind")
            .and_then(|value| value.as_str())
            .and_then(research_kind_from_task)
        else {
            continue;
        };
        let query = string_field(&task, "query").unwrap_or_else(|| fallback_query.to_string());
        let reason =
            string_field(&task, "reason").unwrap_or_else(|| "AI planned search task".to_string());
        let expected_category = string_field(&task, "expected_category")
            .unwrap_or_else(|| category_for_kind(&kind).to_string());
        parsed.push(SearchTask {
            kind,
            query,
            reason,
            expected_category,
        });
        if parsed.len() >= 4 {
            break;
        }
    }
    if parsed.is_empty() {
        manual_search_tasks(&ResearchSearchKind::Web, fallback_query)
            .into_iter()
            .map(|mut task| {
                task.reason = "AI planning fallback web search".to_string();
                task
            })
            .collect()
    } else {
        parsed
    }
}

fn research_kind_from_task(value: &str) -> Option<ResearchSearchKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "web" => Some(ResearchSearchKind::Web),
        "paper" => Some(ResearchSearchKind::Paper),
        "dataset" => Some(ResearchSearchKind::Dataset),
        "code" => Some(ResearchSearchKind::Code),
        "docs" => Some(ResearchSearchKind::Docs),
        _ => None,
    }
}

fn merge_task_results(
    tasks: &[SearchTask],
    result_sets: Vec<Vec<ResearchSearchItem>>,
    limit: usize,
) -> Vec<ResearchSearchItem> {
    let mut seen = std::collections::HashSet::new();
    let mut merged = Vec::new();
    for (task, results) in tasks.iter().zip(result_sets) {
        for mut item in results {
            let keys = dedupe_keys(&item);
            if keys.is_empty() || keys.iter().any(|key| seen.contains(key)) {
                continue;
            }
            seen.extend(keys);
            item.category = task.expected_category.clone();
            item.planned_kind = Some(task.kind.clone());
            item.planned_query = Some(task.query.clone());
            item.reason = Some(task.reason.clone());
            merged.push(item);
            if merged.len() >= limit {
                return merged;
            }
        }
    }
    merged
}

fn collect_provider_task_outcomes(
    tasks: &[SearchTask],
    outcomes: Vec<ProviderTaskOutcome>,
    limit: u64,
) -> anyhow::Result<CollectedProviderResults> {
    let mut successful_tasks = Vec::new();
    let mut result_sets = Vec::new();
    let mut warnings = Vec::new();
    let mut successes = 0usize;

    for (task, outcome) in tasks.iter().zip(outcomes) {
        match outcome {
            ProviderTaskOutcome::Success { results, warning } => {
                successes += 1;
                successful_tasks.push(task.clone());
                result_sets.push(results);
                if let Some(warning) = warning {
                    warnings.push(format!("Task \"{}\" warning: {warning}", task.query));
                }
            }
            ProviderTaskOutcome::Failure { query, error } => {
                warnings.push(format!("Task \"{query}\" failed: {error}"));
            }
        }
    }

    if successes == 0 && !warnings.is_empty() {
        anyhow::bail!(warnings.join(" "));
    }

    Ok(CollectedProviderResults {
        results: merge_task_results(&successful_tasks, result_sets, limit as usize),
        warning: if warnings.is_empty() {
            None
        } else {
            Some(warnings.join(" "))
        },
    })
}

fn dedupe_keys(item: &ResearchSearchItem) -> Vec<String> {
    let mut keys = Vec::new();
    let url = item.url.trim().to_ascii_lowercase();
    if !url.is_empty() {
        keys.push(format!("url:{url}"));
    }
    let title = item.title.trim().to_ascii_lowercase();
    if !title.is_empty() {
        keys.push(format!("title:{title}"));
    }
    keys
}

fn apply_ai_ranking(
    mut results: Vec<ResearchSearchItem>,
    ranking_text: &str,
) -> anyhow::Result<Vec<ResearchSearchItem>> {
    let value = parse_json_value(ranking_text);
    let rankings = value
        .get("results")
        .and_then(|field| field.as_array())
        .cloned()
        .or_else(|| value.as_array().cloned())
        .unwrap_or_default();
    if rankings.is_empty() {
        anyhow::bail!("AI ranking returned no results");
    }

    for ranking in rankings {
        let url = string_field(&ranking, "url");
        let title = string_field(&ranking, "title");
        let rank = ranking.get("rank").and_then(|value| value.as_u64());
        let reason = string_field(&ranking, "reason")
            .or_else(|| string_field(&ranking, "relevance"))
            .or_else(|| string_field(&ranking, "label"));
        let category = string_field(&ranking, "category");
        let relevance_score = ranking
            .get("relevance_score")
            .and_then(|value| value.as_f64());
        if let Some(item) = results
            .iter_mut()
            .find(|item| ranking_matches(item, &url, &title))
        {
            if let Some(rank) = rank {
                item.rank = Some(rank);
            }
            if let Some(reason) = reason {
                item.reason = Some(reason);
            }
            if let Some(category) = category {
                item.category = category;
            }
            if let Some(relevance_score) = relevance_score {
                item.relevance_score = relevance_score;
            }
            item.raw_json["ai_ranking"] = ranking;
        }
    }

    results.sort_by(|a, b| {
        a.rank
            .unwrap_or(u64::MAX)
            .cmp(&b.rank.unwrap_or(u64::MAX))
            .then_with(|| b.relevance_score.total_cmp(&a.relevance_score))
    });
    Ok(results)
}

fn ranking_matches(
    item: &ResearchSearchItem,
    url: &Option<String>,
    title: &Option<String>,
) -> bool {
    if let Some(url) = url {
        if !url.trim().is_empty() && item.url.eq_ignore_ascii_case(url.trim()) {
            return true;
        }
    }
    if let Some(title) = title {
        if !title.trim().is_empty() && item.title.eq_ignore_ascii_case(title.trim()) {
            return true;
        }
    }
    false
}

#[tauri::command]
pub async fn research_extract_and_save(
    request: ResearchExtractSaveRequest,
    config_state: State<'_, AiConfigState>,
) -> Result<Value, String> {
    let config = config_state.get()?;
    if config
        .api_key
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err("API key is not configured.".to_string());
    }
    let auth_token = request
        .auth_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Auth token is required to save research items.".to_string())?;
    let server_base = request
        .server_base
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "Server base URL is required to save research items.".to_string())?;

    let client =
        ApiClient::new(config.to_claude_settings(std::env::current_dir().unwrap_or_default()));
    let mut items = Vec::new();
    for result in &request.results {
        items.push(extract_item(&client, result, &request.kind).await?);
    }

    let response = reqwest::Client::new()
        .post(format!(
            "{}/research/items",
            server_base.trim_end_matches('/')
        ))
        .bearer_auth(auth_token)
        .json(&json!({
            "project_id": request.project_id,
            "items": items,
        }))
        .send()
        .await
        .map_err(|error| format!("Save request failed: {error}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("Save response failed: {error}"))?;
    if !status.is_success() {
        return Err(format!("Research save failed ({status}): {body}"));
    }
    serde_json::from_str(&body).map_err(|error| format!("Invalid save response: {error}"))
}

async fn search_firecrawl(
    config: &AiConfig,
    query: &str,
    kind: &ResearchSearchKind,
    limit: u64,
) -> anyhow::Result<ProviderSearchResult> {
    let api_key = config
        .firecrawl_api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Firecrawl API key is not configured."))?;
    let primary_body =
        firecrawl_search_request(api_key, firecrawl_search_body(query, kind, limit, false)).await;
    let (body, warning) = match primary_body {
        Ok(body) => (body, None),
        Err(primary_error) => {
            let fallback_body = firecrawl_search_request(
                api_key,
                firecrawl_search_body(query, kind, limit, true),
            )
            .await
            .map_err(|fallback_error| {
                anyhow::anyhow!(
                    "Firecrawl search failed. Primary format: {primary_error}. Legacy format: {fallback_error}"
                )
            })?;
            (
                fallback_body,
                Some(format!(
                    "Firecrawl primary search format failed; used legacy format. {primary_error}"
                )),
            )
        }
    };
    let parsed: FirecrawlSearchResponse = serde_json::from_str(&body)?;
    let values = firecrawl_result_values(parsed);
    Ok(ProviderSearchResult {
        items: values
            .into_iter()
            .map(|value| firecrawl_item(value, kind))
            .filter(|item| is_allowed_academic_source(&item.url, kind))
            .take(limit as usize)
            .collect(),
        warning,
    })
}

/// Tavily-backed research provider. Tavily returns general web results, so we
/// apply the same academic source allow/deny profile that Firecrawl uses to
/// keep the result quality consistent across scrapers.
async fn search_tavily_for_research(
    config: &AiConfig,
    query: &str,
    kind: &ResearchSearchKind,
    limit: u64,
) -> anyhow::Result<ProviderSearchResult> {
    let api_key = config
        .tavily_api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Tavily API key is not configured."))?;

    let response = reqwest::Client::builder()
        .timeout(PROVIDER_HTTP_TIMEOUT)
        .build()?
        .post("https://api.tavily.com/search")
        .json(&json!({
            "api_key": api_key,
            "query": query,
            // Request extra results because the academic filter discards many.
            "max_results": (limit * 3).clamp(10, 20),
            "search_depth": "advanced",
        }))
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("Tavily search failed ({status}): {body}");
    }
    let parsed: Value = serde_json::from_str(&body)?;
    let values = parsed["results"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    Ok(ProviderSearchResult {
        items: values
            .into_iter()
            .map(|value| tavily_item(value, kind))
            .filter(|item| is_allowed_academic_source(&item.url, kind))
            .take(limit as usize)
            .collect(),
        warning: None,
    })
}

fn tavily_item(value: Value, kind: &ResearchSearchKind) -> ResearchSearchItem {
    let title = string_field(&value, "title").unwrap_or_else(|| "Untitled".to_string());
    let url = string_field(&value, "url").unwrap_or_default();
    let content = string_field(&value, "content")
        .or_else(|| string_field(&value, "raw_content"))
        .unwrap_or_default();
    ResearchSearchItem {
        title,
        url,
        content,
        provider: "tavily".to_string(),
        source: "tavily_search".to_string(),
        category: category_for_kind(kind).to_string(),
        authors: None,
        publish_year: None,
        keywords: None,
        relevance_score: value["score"].as_f64().unwrap_or(0.0),
        raw_json: value,
        planned_kind: None,
        planned_query: None,
        reason: None,
        rank: None,
    }
}

fn firecrawl_search_body(
    query: &str,
    kind: &ResearchSearchKind,
    limit: u64,
    legacy_formats: bool,
) -> Value {
    let profile = academic_source_profile(kind);
    let formats = if legacy_formats {
        json!(["markdown"])
    } else {
        json!([{ "type": "markdown" }])
    };
    let mut body = json!({
        "query": query,
        "limit": limit,
        "scrapeOptions": {
            "formats": formats,
            "onlyMainContent": true
        }
    });

    // Firecrawl's native category targeting is what actually surfaces academic
    // / arxiv results — far more reliable than a domain allow-list.
    if let Some(categories) = firecrawl_categories(kind) {
        body["categories"] = categories;
    }

    // Firecrawl REJECTS a request that specifies BOTH includeDomains and
    // excludeDomains, and rejects bare TLD suffixes like ".edu". Send at most
    // one sanitized list: a tight allow-list for repo/dataset kinds, otherwise
    // a junk-site deny-list. (Local post-filtering still applies the full
    // academic profile, including the ".edu"/".gov" suffixes the API can't take.)
    match kind {
        ResearchSearchKind::Code | ResearchSearchKind::Dataset => {
            let include = sanitize_firecrawl_domains(&profile.include_domains);
            if !include.is_empty() {
                body["includeDomains"] = json!(include);
            }
        }
        _ => {
            let exclude = sanitize_firecrawl_domains(&profile.exclude_domains);
            if !exclude.is_empty() {
                body["excludeDomains"] = json!(exclude);
            }
        }
    }

    body
}

/// Firecrawl `categories` targeting for kinds that map to a built-in source.
/// Paper → research + pdf is the academic-paper (arxiv) path.
fn firecrawl_categories(kind: &ResearchSearchKind) -> Option<Value> {
    match kind {
        ResearchSearchKind::Paper => Some(json!(["research", "pdf"])),
        _ => None,
    }
}

/// Keep only entries Firecrawl's `searchDomainSchema` accepts: a valid bare
/// hostname with no protocol/path and no leading dot. Drops suffix patterns
/// like ".edu"/".gov" that are only meaningful to our local domain matcher.
fn sanitize_firecrawl_domains(domains: &[&str]) -> Vec<String> {
    domains
        .iter()
        .map(|domain| domain.trim())
        .filter(|domain| !domain.is_empty() && !domain.starts_with('.') && domain.contains('.'))
        .map(ToOwned::to_owned)
        .collect()
}

async fn firecrawl_search_request(api_key: &str, body: Value) -> anyhow::Result<String> {
    let response = reqwest::Client::builder()
        .timeout(PROVIDER_HTTP_TIMEOUT)
        .build()?
        .post("https://api.firecrawl.dev/v2/search")
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("Firecrawl search failed ({status}): {text}");
    }
    Ok(text)
}

fn firecrawl_result_values(parsed: FirecrawlSearchResponse) -> Vec<Value> {
    if let Some(results) = parsed.results {
        return results;
    }
    let Some(data) = parsed.data else {
        return Vec::new();
    };
    if let Some(items) = data.as_array() {
        return items.clone();
    }
    if let Some(web) = data.get("web").and_then(|value| value.as_array()) {
        return web.clone();
    }
    let mut out = Vec::new();
    if let Some(object) = data.as_object() {
        for value in object.values() {
            if let Some(items) = value.as_array() {
                out.extend(items.iter().cloned());
            }
        }
    }
    out
}

fn firecrawl_item(value: Value, kind: &ResearchSearchKind) -> ResearchSearchItem {
    let metadata = value.get("metadata").unwrap_or(&Value::Null);
    let title = string_field(&value, "title")
        .or_else(|| string_field(metadata, "title"))
        .unwrap_or_else(|| "Untitled".to_string());
    let url = string_field(&value, "url")
        .or_else(|| string_field(metadata, "sourceURL"))
        .or_else(|| string_field(metadata, "url"))
        .unwrap_or_default();
    let content = string_field(&value, "markdown")
        .or_else(|| string_field(&value["content"], "markdown"))
        .or_else(|| string_field(&value["data"], "markdown"))
        .or_else(|| string_field(&value, "content"))
        .or_else(|| string_field(&value, "description"))
        .or_else(|| string_field(&value, "snippet"))
        .unwrap_or_default();
    ResearchSearchItem {
        title,
        url,
        content,
        provider: "firecrawl".to_string(),
        source: "firecrawl_search".to_string(),
        category: category_for_kind(kind).to_string(),
        authors: None,
        publish_year: None,
        keywords: None,
        relevance_score: value["score"].as_f64().unwrap_or(0.0),
        raw_json: value,
        planned_kind: None,
        planned_query: None,
        reason: None,
        rank: None,
    }
}

async fn search_context7(
    config: &AiConfig,
    query: &str,
    limit: u64,
) -> anyhow::Result<ProviderSearchResult> {
    let limit_string = limit.to_string();
    let primary_body = context7_get(
        config,
        "https://context7.com/api/v2/libs/search",
        &[
            ("libraryName", query),
            ("query", query),
            ("limit", &limit_string),
        ],
    )
    .await;
    let (body, warning) = match primary_body {
        Ok(body) => (body, None),
        Err(primary_error) => {
            let fallback_body = context7_get(
                config,
                "https://context7.com/api/v1/search",
                &[
                    ("libraryName", query),
                    ("query", query),
                    ("limit", &limit_string),
                ],
            )
            .await
            .map_err(|fallback_error| {
                anyhow::anyhow!("Context7 search failed. v2: {primary_error}. v1: {fallback_error}")
            })?;
            (
                fallback_body,
                Some(format!(
                    "Context7 v2 search failed; used v1 fallback. {primary_error}"
                )),
            )
        }
    };
    let parsed: Context7SearchResponse = serde_json::from_str(&body).or_else(|_| {
        Ok::<_, serde_json::Error>(Context7SearchResponse {
            results: serde_json::from_str::<Vec<Value>>(&body).ok(),
        })
    })?;
    let results = parsed.results.unwrap_or_default();
    let mut items = Vec::new();
    for library in results.into_iter().take(limit as usize) {
        let library_id = string_field(&library, "id")
            .or_else(|| string_field(&library, "libraryId"))
            .or_else(|| string_field(&library, "library_id"))
            .unwrap_or_default();
        let title = string_field(&library, "title")
            .or_else(|| string_field(&library, "name"))
            .unwrap_or_else(|| library_id.clone());
        let docs = fetch_context7_docs(config, &library_id, query)
            .await
            .unwrap_or_default();
        items.push(ResearchSearchItem {
            title,
            url: if library_id.is_empty() {
                "https://context7.com".to_string()
            } else {
                format!("https://context7.com/{library_id}")
            },
            content: docs,
            provider: "context7".to_string(),
            source: "context7_docs".to_string(),
            category: "code".to_string(),
            authors: None,
            publish_year: None,
            keywords: Some(query.to_string()),
            relevance_score: library["score"].as_f64().unwrap_or(0.0),
            raw_json: library,
            planned_kind: None,
            planned_query: None,
            reason: None,
            rank: None,
        });
    }
    Ok(ProviderSearchResult { items, warning })
}

async fn fetch_context7_docs(
    config: &AiConfig,
    library_id: &str,
    topic: &str,
) -> anyhow::Result<String> {
    if library_id.trim().is_empty() {
        return Ok(String::new());
    }
    let body = match context7_get(
        config,
        "https://context7.com/api/v2/context",
        &[
            ("libraryId", library_id),
            ("query", topic),
            ("tokens", "5000"),
            ("type", "json"),
        ],
    )
    .await
    {
        Ok(body) => body,
        Err(_) => {
            let path = library_id.trim_start_matches('/');
            context7_get(
                config,
                &format!("https://context7.com/api/v1/{path}"),
                &[("query", topic), ("tokens", "5000")],
            )
            .await?
        }
    };
    Ok(context7_body_to_markdown(&body))
}

async fn context7_get(
    config: &AiConfig,
    url: &str,
    query: &[(&str, &str)],
) -> anyhow::Result<String> {
    let mut request = reqwest::Client::builder()
        .timeout(PROVIDER_HTTP_TIMEOUT)
        .build()?
        .get(url)
        .query(query);
    if let Some(key) = config
        .context7_api_key
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        request = request.bearer_auth(key);
    }
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("Context7 request failed ({status}): {body}");
    }
    Ok(body)
}

fn context7_body_to_markdown(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return body.to_string();
    };
    let mut chunks = Vec::new();
    if let Some(snippets) = value["codeSnippets"].as_array() {
        for snippet in snippets {
            if let Some(title) = snippet["codeTitle"].as_str() {
                chunks.push(format!("## {title}"));
            }
            if let Some(list) = snippet["codeList"].as_array() {
                for code in list {
                    if let Some(text) = code["code"].as_str() {
                        chunks.push(format!("```text\n{text}\n```"));
                    }
                }
            }
        }
    }
    if let Some(snippets) = value["infoSnippets"].as_array() {
        for snippet in snippets {
            if let Some(content) = snippet["content"].as_str() {
                chunks.push(content.to_string());
            }
        }
    }
    if chunks.is_empty() {
        body.to_string()
    } else {
        chunks.join("\n\n")
    }
}

fn analyze_url_hint(url: &str) -> anyhow::Result<ResearchSearchItem> {
    let parsed = reqwest::Url::parse(url)?;
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let path = parsed.path().trim_matches('/');
    let mut category = "literature";
    let mut provider = "user_url";
    let mut title = title_from_url_path(path).unwrap_or_else(|| host.clone());
    let mut raw_json = json!({
        "source_profile": "user_url",
        "analyzed_url": url,
    });

    if host.ends_with("github.com") {
        category = "code";
        provider = "github";
        title = repo_title_from_path(path).unwrap_or(title);
    } else if host.ends_with("gitee.com") {
        category = "code";
        provider = "gitee";
        title = repo_title_from_path(path).unwrap_or(title);
    } else if host.ends_with("gitlab.com") {
        category = "code";
        provider = "gitlab";
        title = repo_title_from_path(path).unwrap_or(title);
    } else if host.ends_with("kaggle.com")
        || host.ends_with("zenodo.org")
        || host.ends_with("figshare.com")
        || host.ends_with("data.gov")
    {
        category = "dataset";
        provider = "dataset_url";
    } else if host.ends_with("arxiv.org") {
        provider = "arxiv";
        if let Some(pdf_url) = arxiv_pdf_url(&parsed) {
            raw_json["pdf_url"] = json!(pdf_url);
        }
    } else if host.ends_with("doi.org") {
        provider = "doi";
    }

    if parsed.path().to_ascii_lowercase().ends_with(".pdf") {
        provider = if provider == "user_url" {
            "pdf_url"
        } else {
            provider
        };
        raw_json["pdf_url"] = json!(url);
        raw_json["attachment_filename"] = json!(format!("{title}.pdf"));
    }

    Ok(ResearchSearchItem {
        title,
        url: url.to_string(),
        content: format!("User-provided research source: {url}"),
        provider: provider.to_string(),
        source: "user_url".to_string(),
        category: category.to_string(),
        authors: None,
        publish_year: None,
        keywords: None,
        relevance_score: 1.0,
        raw_json,
        planned_kind: None,
        planned_query: None,
        reason: Some("User-provided URL analysis".to_string()),
        rank: None,
    })
}

fn arxiv_pdf_url(url: &reqwest::Url) -> Option<String> {
    let mut segments = url.path_segments()?;
    match segments.next()? {
        "abs" => segments
            .next()
            .map(|id| format!("https://arxiv.org/pdf/{id}.pdf")),
        "pdf" => Some(url.to_string()),
        _ => None,
    }
}

fn repo_title_from_path(path: &str) -> Option<String> {
    let parts = path
        .split('/')
        .filter(|part| !part.trim().is_empty())
        .take(2)
        .collect::<Vec<_>>();
    if parts.len() == 2 {
        Some(format!("{}/{}", parts[0], parts[1]))
    } else {
        None
    }
}

fn title_from_url_path(path: &str) -> Option<String> {
    path.rsplit('/')
        .find(|part| !part.trim().is_empty())
        .map(|part| {
            part.trim_end_matches(".pdf")
                .replace(['-', '_'], " ")
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let start_close = lower[start..].find('>')? + start + 1;
    let end = lower[start_close..].find("</title>")? + start_close;
    Some(html[start_close..end].trim().to_string()).filter(|value| !value.is_empty())
}

fn html_to_text_snippet(html: &str) -> String {
    let mut text = String::with_capacity(html.len().min(10_000));
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
        if text.len() >= 10_000 {
            break;
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

async fn extract_item(
    client: &ApiClient,
    item: &ResearchSearchItem,
    kind: &ResearchSearchKind,
) -> Result<ResearchExtractedItem, String> {
    let prompt = format!(
        "Extract mathematical modeling research notes from this source. Return only compact JSON with keys: summary, authors, publish_year, keywords, methodology, key_parameters, ai_relevance, bibtex.\n\
         - keywords must be a comma-separated string.\n\
         - methodology should list useful methods, assumptions, variables, formulas, datasets, and limitations.\n\
         - key_parameters should list variables/formulas/data requirements.\n\
         - ai_relevance should explain which modeling tasks this source helps.\n\
         - bibtex should be a single valid BibTeX entry; use misc if unsure.\n\n\
         Kind: {:?}\nTitle: {}\nURL: {}\nContent:\n{}",
        kind,
        item.title,
        item.url,
        truncate(&item.content, 12_000)
    );
    let response = client
        .chat(
            vec![
                ChatMessage::system("You are a research extraction assistant for MCM/ICM mathematical modeling projects."),
                ChatMessage::user(prompt),
            ],
            None,
        )
        .await
        .map_err(|error| format!("AI extraction failed: {error}"))?;
    let content = response
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .unwrap_or_default();
    let extracted = parse_json_object(&content);

    let summary =
        string_field(&extracted, "summary").unwrap_or_else(|| truncate(&item.content, 600));
    let keywords = string_field(&extracted, "keywords")
        .or_else(|| item.keywords.clone())
        .unwrap_or_default();
    let methodology = string_field(&extracted, "methodology").unwrap_or_default();
    let key_parameters = string_field(&extracted, "key_parameters").unwrap_or_default();
    let ai_relevance = string_field(&extracted, "ai_relevance").unwrap_or_default();
    let bibtex = string_field(&extracted, "bibtex").unwrap_or_else(|| fallback_bibtex(item));
    let mut raw_json = item.raw_json.clone();
    raw_json["provider"] = json!(item.provider);
    raw_json["extraction"] = extracted;
    raw_json["bibtex"] = json!(bibtex);

    Ok(ResearchExtractedItem {
        title: item.title.clone(),
        url: item.url.clone(),
        content: item.content.clone(),
        category: item.category.clone(),
        summary,
        authors: string_field(&raw_json["extraction"], "authors").or_else(|| item.authors.clone()),
        publish_year: raw_json["extraction"]["publish_year"]
            .as_i64()
            .map(|value| value as i32)
            .or(item.publish_year),
        keywords,
        methodology,
        key_parameters,
        ai_relevance,
        relevance_score: item.relevance_score,
        bibtex,
        raw_json,
    })
}

fn category_for_kind(kind: &ResearchSearchKind) -> &'static str {
    match kind {
        ResearchSearchKind::Dataset => "dataset",
        ResearchSearchKind::Code | ResearchSearchKind::Docs => "code",
        _ => "literature",
    }
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|field| field.as_str())
        .map(str::trim)
        .filter(|field| !field.is_empty())
        .map(ToOwned::to_owned)
}

fn truncate(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        text.to_string()
    } else {
        format!("{}...", &text[..limit])
    }
}

fn parse_json_object(text: &str) -> Value {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return value;
    }
    let unfenced = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        return value;
    }
    let Some(start) = unfenced.find('{') else {
        return json!({});
    };
    let Some(end) = unfenced.rfind('}') else {
        return json!({});
    };
    serde_json::from_str(&unfenced[start..=end]).unwrap_or_else(|_| json!({}))
}

fn parse_json_value(text: &str) -> Value {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return value;
    }
    let unfenced = trimmed
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(value) = serde_json::from_str::<Value>(unfenced) {
        return value;
    }
    let object_start = unfenced.find('{');
    let object_end = unfenced.rfind('}');
    if let (Some(start), Some(end)) = (object_start, object_end) {
        if let Ok(value) = serde_json::from_str::<Value>(&unfenced[start..=end]) {
            return value;
        }
    }
    let array_start = unfenced.find('[');
    let array_end = unfenced.rfind(']');
    if let (Some(start), Some(end)) = (array_start, array_end) {
        if let Ok(value) = serde_json::from_str::<Value>(&unfenced[start..=end]) {
            return value;
        }
    }
    json!({})
}

fn fallback_bibtex(item: &ResearchSearchItem) -> String {
    let key = item
        .title
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(32)
        .collect::<String>();
    format!(
        "@misc{{{},\n  title = {{{}}},\n  url = {{{}}}\n}}",
        if key.is_empty() { "reference" } else { &key },
        item.title.replace('{', "").replace('}', ""),
        item.url
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(title: &str, url: &str, kind: ResearchSearchKind) -> ResearchSearchItem {
        ResearchSearchItem {
            title: title.to_string(),
            url: url.to_string(),
            content: format!("{title} content"),
            provider: "firecrawl".to_string(),
            source: "firecrawl_search".to_string(),
            category: category_for_kind(&kind).to_string(),
            authors: None,
            publish_year: None,
            keywords: None,
            relevance_score: 0.1,
            raw_json: json!({ "title": title, "url": url }),
            planned_kind: Some(kind),
            planned_query: Some(format!("{title} query")),
            reason: Some("provider reason".to_string()),
            rank: None,
        }
    }

    #[test]
    fn parses_ai_plan_with_only_allowed_kinds_and_four_tasks() {
        let parsed = parse_ai_search_plan(
            r#"
            {
              "tasks": [
                {"kind":"paper","query":"traffic congestion prediction graph neural networks","reason":"find models","expected_category":"literature"},
                {"kind":"dataset","query":"urban traffic speed dataset","reason":"find benchmark data","expected_category":"dataset"},
                {"kind":"code","query":"traffic forecasting GNN github","reason":"find implementation","expected_category":"code"},
                {"kind":"docs","query":"scipy optimize linprog","reason":"find solver docs","expected_category":"code"},
                {"kind":"video","query":"ignore me","reason":"bad provider","expected_category":"video"}
              ]
            }
            "#,
            "fallback query",
        );

        assert_eq!(parsed.len(), 4);
        assert!(matches!(parsed[0].kind, ResearchSearchKind::Paper));
        assert!(matches!(parsed[1].kind, ResearchSearchKind::Dataset));
        assert!(matches!(parsed[2].kind, ResearchSearchKind::Code));
        assert!(matches!(parsed[3].kind, ResearchSearchKind::Docs));
        assert_eq!(
            parsed[0].query,
            "traffic congestion prediction graph neural networks"
        );
        assert_eq!(parsed[1].expected_category, "dataset");
    }

    #[test]
    fn invalid_ai_plan_falls_back_to_web_task() {
        let parsed = parse_ai_search_plan("not json", "city traffic data");

        assert_eq!(parsed.len(), 1);
        assert!(matches!(parsed[0].kind, ResearchSearchKind::Web));
        assert_eq!(parsed[0].query, "city traffic data");
        assert!(parsed[0].reason.contains("fallback"));
    }

    #[test]
    fn manual_override_builds_single_task_without_expansion() {
        let tasks = manual_search_tasks(&ResearchSearchKind::Dataset, "traffic dataset");

        assert_eq!(tasks.len(), 1);
        assert!(matches!(tasks[0].kind, ResearchSearchKind::Dataset));
        assert_eq!(tasks[0].query, "traffic dataset");
        assert_eq!(tasks[0].expected_category, "dataset");
    }

    #[test]
    fn merge_dedupes_by_url_then_title_and_keeps_planning_metadata() {
        let tasks = vec![
            SearchTask {
                kind: ResearchSearchKind::Paper,
                query: "traffic gnn".to_string(),
                reason: "models".to_string(),
                expected_category: "literature".to_string(),
            },
            SearchTask {
                kind: ResearchSearchKind::Dataset,
                query: "traffic datasets".to_string(),
                reason: "data".to_string(),
                expected_category: "dataset".to_string(),
            },
        ];
        let result_sets = vec![
            vec![
                item(
                    "Traffic Forecasting",
                    "https://example.com/paper",
                    ResearchSearchKind::Paper,
                ),
                item(
                    "Duplicate URL",
                    "https://example.com/shared",
                    ResearchSearchKind::Paper,
                ),
            ],
            vec![
                item(
                    "Duplicate URL",
                    "https://example.com/shared",
                    ResearchSearchKind::Dataset,
                ),
                item("Traffic Forecasting", "", ResearchSearchKind::Dataset),
                item(
                    "Metro Dataset",
                    "https://example.com/dataset",
                    ResearchSearchKind::Dataset,
                ),
            ],
        ];

        let merged = merge_task_results(&tasks, result_sets, 3);

        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].url, "https://example.com/paper");
        assert_eq!(merged[1].url, "https://example.com/shared");
        assert_eq!(merged[2].url, "https://example.com/dataset");
        assert!(matches!(
            merged[2].planned_kind,
            Some(ResearchSearchKind::Dataset)
        ));
        assert_eq!(merged[2].planned_query.as_deref(), Some("traffic datasets"));
        assert_eq!(merged[2].reason.as_deref(), Some("data"));
    }

    #[test]
    fn applies_ai_ranking_reason_category_and_rank() {
        let results = vec![
            item("Low", "https://example.com/low", ResearchSearchKind::Web),
            item(
                "High",
                "https://example.com/high",
                ResearchSearchKind::Dataset,
            ),
        ];

        let ranked = apply_ai_ranking(
            results,
            r#"
            {
              "results": [
                {"url":"https://example.com/high","title":"High","rank":1,"reason":"best dataset match","category":"dataset","relevance_score":0.95},
                {"url":"https://example.com/low","title":"Low","rank":2,"reason":"background only","category":"literature","relevance_score":0.4}
              ]
            }
            "#,
        )
        .expect("ranking should parse");

        assert_eq!(ranked[0].title, "High");
        assert_eq!(ranked[0].rank, Some(1));
        assert_eq!(ranked[0].reason.as_deref(), Some("best dataset match"));
        assert_eq!(ranked[0].category, "dataset");
        assert_eq!(ranked[0].relevance_score, 0.95);
    }

    #[test]
    fn provider_task_outcomes_keep_successes_and_collect_warnings() {
        let tasks = vec![
            SearchTask {
                kind: ResearchSearchKind::Web,
                query: "traffic models".to_string(),
                reason: "models".to_string(),
                expected_category: "literature".to_string(),
            },
            SearchTask {
                kind: ResearchSearchKind::Dataset,
                query: "traffic dataset".to_string(),
                reason: "data".to_string(),
                expected_category: "dataset".to_string(),
            },
        ];
        let outcomes = vec![
            ProviderTaskOutcome::Success {
                results: vec![item(
                    "Traffic Model",
                    "https://example.com/model",
                    ResearchSearchKind::Web,
                )],
                warning: None,
            },
            ProviderTaskOutcome::Failure {
                query: "traffic dataset".to_string(),
                error: "Firecrawl rate limited".to_string(),
            },
        ];

        let collected = collect_provider_task_outcomes(&tasks, outcomes, 8).unwrap();

        assert_eq!(collected.results.len(), 1);
        assert_eq!(collected.results[0].title, "Traffic Model");
        assert!(collected
            .warning
            .unwrap()
            .contains("Firecrawl rate limited"));
    }

    #[test]
    fn provider_task_outcomes_error_when_every_task_fails() {
        let tasks = vec![SearchTask {
            kind: ResearchSearchKind::Web,
            query: "traffic models".to_string(),
            reason: "models".to_string(),
            expected_category: "literature".to_string(),
        }];
        let outcomes = vec![ProviderTaskOutcome::Failure {
            query: "traffic models".to_string(),
            error: "Firecrawl unavailable".to_string(),
        }];

        let error = collect_provider_task_outcomes(&tasks, outcomes, 8).unwrap_err();

        assert!(error.to_string().contains("Firecrawl unavailable"));
    }

    #[test]
    fn academic_source_profile_for_code_includes_github_gitee_and_excludes_video_sites() {
        let profile = academic_source_profile(&ResearchSearchKind::Code);

        assert!(profile.include_domains.contains(&"github.com"));
        assert!(profile.include_domains.contains(&"gitee.com"));
        assert!(profile.exclude_domains.contains(&"youtube.com"));
        assert!(profile.exclude_domains.contains(&"wikipedia.org"));
    }

    #[test]
    fn academic_source_filter_rejects_low_quality_domains() {
        assert!(is_allowed_academic_source(
            "https://arxiv.org/abs/2401.00001",
            &ResearchSearchKind::Paper
        ));
        assert!(is_allowed_academic_source(
            "https://gitee.com/model/repo",
            &ResearchSearchKind::Code
        ));
        assert!(!is_allowed_academic_source(
            "https://www.youtube.com/watch?v=abc",
            &ResearchSearchKind::Paper
        ));
        assert!(!is_allowed_academic_source(
            "https://en.wikipedia.org/wiki/Traffic_flow",
            &ResearchSearchKind::Web
        ));
    }

    #[test]
    fn firecrawl_search_body_never_sends_both_domain_lists() {
        // Firecrawl rejects a request carrying BOTH includeDomains and
        // excludeDomains. Paper/Web use a deny-list; Code/Dataset an allow-list.
        let paper = firecrawl_search_body("traffic gnn", &ResearchSearchKind::Paper, 8, false);
        assert!(paper.get("includeDomains").is_none());
        let paper_exclude = paper["excludeDomains"].as_array().unwrap();
        assert!(paper_exclude.iter().any(|value| value == "youtube.com"));
        // Paper uses Firecrawl's native academic category targeting.
        let categories = paper["categories"].as_array().unwrap();
        assert!(categories.iter().any(|value| value == "research"));

        let code = firecrawl_search_body("traffic gnn github", &ResearchSearchKind::Code, 8, false);
        assert!(code.get("excludeDomains").is_none());
        let code_include = code["includeDomains"].as_array().unwrap();
        assert!(code_include.iter().any(|value| value == "github.com"));
    }

    #[test]
    fn sanitize_firecrawl_domains_drops_bare_tld_suffixes() {
        // ".edu"/".gov" are valid for our local matcher but rejected by
        // Firecrawl's hostname schema — they must be stripped before sending.
        let cleaned = sanitize_firecrawl_domains(&["arxiv.org", ".edu", ".gov", "doi.org"]);
        assert_eq!(cleaned, vec!["arxiv.org".to_string(), "doi.org".to_string()]);
    }

    #[test]
    fn tavily_item_maps_fields_and_tags_provider() {
        let value = json!({
            "title": "Traffic GNN",
            "url": "https://arxiv.org/abs/2401.00001",
            "content": "abstract text",
            "score": 0.87
        });
        let item = tavily_item(value, &ResearchSearchKind::Paper);
        assert_eq!(item.title, "Traffic GNN");
        assert_eq!(item.url, "https://arxiv.org/abs/2401.00001");
        assert_eq!(item.content, "abstract text");
        assert_eq!(item.provider, "tavily");
        assert_eq!(item.source, "tavily_search");
        assert_eq!(item.category, "literature");
        assert_eq!(item.relevance_score, 0.87);
    }

    #[test]
    fn url_analysis_hint_recognizes_pdf_arxiv_github_and_gitee() {
        let pdf = analyze_url_hint("https://example.edu/paper.pdf").unwrap();
        let arxiv = analyze_url_hint("https://arxiv.org/abs/2401.00001").unwrap();
        let github = analyze_url_hint("https://github.com/org/repo").unwrap();
        let gitee = analyze_url_hint("https://gitee.com/org/repo").unwrap();

        assert_eq!(pdf.category, "literature");
        assert_eq!(pdf.raw_json["pdf_url"], "https://example.edu/paper.pdf");
        assert_eq!(
            arxiv.raw_json["pdf_url"],
            "https://arxiv.org/pdf/2401.00001.pdf"
        );
        assert_eq!(github.category, "code");
        assert_eq!(gitee.category, "code");
        assert_eq!(gitee.provider, "gitee");
    }
}
