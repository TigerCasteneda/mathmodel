use super::config::{AiConfig, AiConfigState};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize)]
pub struct SearchResultsEvent {
    pub query: String,
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchStreamEvent {
    pub seq: u64,
    pub content: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchQuestionsEvent {
    pub questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchErrorEvent {
    pub message: String,
}

async fn search_tavily(query: &str, api_key: &str) -> anyhow::Result<Vec<SearchResultItem>> {
    let response = reqwest::Client::new()
        .post("https://api.tavily.com/search")
        .json(&json!({
            "api_key": api_key,
            "query": query,
            "max_results": 10,
            "search_depth": "advanced",
        }))
        .send()
        .await?
        .json::<Value>()
        .await?;

    let results = response["results"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|r| SearchResultItem {
                    title: r["title"].as_str().unwrap_or("").to_string(),
                    url: r["url"].as_str().unwrap_or("").to_string(),
                    content: r["content"].as_str().unwrap_or("").to_string(),
                    score: r["score"].as_f64().unwrap_or(0.0),
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(results)
}

fn build_synthesis_prompt(query: &str, results: &[SearchResultItem]) -> String {
    let mut context = String::new();
    for (i, result) in results.iter().enumerate() {
        context.push_str(&format!(
            "[{}] {}\n    URL: {}\n    {}\n\n",
            i + 1,
            result.title,
            result.url,
            result.content
        ));
    }

    format!(
        "You are an AI research assistant. Answer the user's question using ONLY the provided search results.\n\
         Follow these rules strictly:\n\
         1. Base your answer on the provided search results — do not fabricate information\n\
         2. Cite EVERY factual claim with the source number in brackets: [1], [2], etc.\n\
         3. If search results conflict, note the disagreement\n\
         4. If the results don't fully answer the question, state this clearly\n\
         5. Use markdown formatting: headers, lists, bold for key points\n\
         6. Write in a clear, informative style — be concise but thorough\n\
         7. End with a short summary\n\n\
         Search results:\n{context}\
         User question: {query}"
    )
}

#[tauri::command]
pub async fn ai_search(
    query: String,
    app: AppHandle,
    config_state: State<'_, AiConfigState>,
) -> Result<(), String> {
    let config = config_state.get()?;

    let api_key = config
        .tavily_api_key
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "Tavily API key is not configured. Set it in Settings.".to_string())?;

    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Err("Query is empty.".to_string());
    }

    // 1. Search Tavily
    let results = search_tavily(trimmed, api_key)
        .await
        .map_err(|e| format!("Tavily search failed: {e}"))?;

    let _ = app.emit(
        "search:results",
        SearchResultsEvent {
            query: trimmed.to_string(),
            results: results.clone(),
        },
    );

    if results.is_empty() {
        let _ = app.emit(
            "search:stream",
            SearchStreamEvent {
                seq: 0,
                content: "No search results found. Try a different query.".to_string(),
                done: true,
            },
        );
        return Ok(());
    }

    // 2. Build prompt and stream DeepSeek V4
    let system_prompt = build_synthesis_prompt(trimmed, &results);

    let client = claude_code_rs::ApiClient::new(
        config.to_claude_settings(std::env::current_dir().unwrap_or_default()),
    );

    let messages = vec![
        claude_code_rs::ChatMessage::system(system_prompt),
        claude_code_rs::ChatMessage::user(trimmed.to_string()),
    ];

    let response = client
        .chat_stream(messages, None)
        .await
        .map_err(|e| format!("AI request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let msg = format!("AI error ({status}): {body}");
        let _ = app.emit("search:error", SearchErrorEvent { message: msg.clone() });
        return Err(msg);
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut stream_seq = 0u64;
    let mut full_answer = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| format!("Stream error: {e}"))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(idx) = buffer.find('\n') {
            let line = buffer[..idx].trim_end_matches('\r').to_string();
            buffer = buffer[idx + 1..].to_string();

            if let Some(content) = parse_search_sse_line(&line) {
                full_answer.push_str(&content);
                stream_seq += 1;
                let _ = app.emit(
                    "search:stream",
                    SearchStreamEvent {
                        seq: stream_seq,
                        content,
                        done: false,
                    },
                );
            }
        }
    }

    // Final done event
    let _ = app.emit(
        "search:stream",
        SearchStreamEvent {
            seq: stream_seq + 1,
            content: String::new(),
            done: true,
        },
    );

    // 3. Generate related questions (async, non-blocking)
    let question_client = claude_code_rs::ApiClient::new(
        config.to_claude_settings(std::env::current_dir().unwrap_or_default()),
    );
    let questions = generate_related_questions(&question_client, trimmed, &full_answer).await;
    let _ = app.emit(
        "search:questions",
        SearchQuestionsEvent { questions },
    );

    Ok(())
}

fn parse_search_sse_line(line: &str) -> Option<String> {
    let data = line.strip_prefix("data: ")?;
    if data == "[DONE]" {
        return None;
    }
    let chunk: Value = serde_json::from_str(data).ok()?;
    chunk["choices"][0]["delta"]["content"]
        .as_str()
        .map(ToOwned::to_owned)
}

async fn generate_related_questions(
    client: &claude_code_rs::ApiClient,
    query: &str,
    answer: &str,
) -> Vec<String> {
    let prompt = format!(
        "Based on this question and answer, suggest 3-5 related questions the user might ask next.\n\
         Return ONLY a JSON array of strings, no other text.\n\n\
         Question: {query}\n\n\
         Answer (summary): {}\n\n\
         Related questions (JSON array):",
        &answer[..answer.len().min(2000)]
    );

    let messages = vec![claude_code_rs::ChatMessage::user(prompt)];

    let result = match client.chat(messages, None).await {
        Ok(response) => response,
        Err(_) => return vec![],
    };

    let text = result
        .choices
        .first()
        .and_then(|c| c.message.content.as_deref())
        .unwrap_or("");
    // Try to extract JSON array from the response
    let trimmed = text.trim();
    let json_text = if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            &trimmed[start..=end]
        } else {
            return vec![];
        }
    } else {
        return vec![];
    };

    serde_json::from_str::<Vec<String>>(json_text).unwrap_or_default()
}
