use crate::agent::state::AgentState;
use claude_code_rs::api::ToolDefinition;
use serde_json::{json, Value};
use tauri::{Emitter, State};

pub fn modeler_tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition::new(
            "web_search",
            "Search the web through the host SearXNG instance.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "max_results": { "type": "integer", "minimum": 1, "maximum": 20 }
                },
                "required": ["query"]
            }),
        ),
        ToolDefinition::new(
            "fetch_url",
            "Fetch and extract a web page as markdown through Jina Reader fallback.",
            json!({
                "type": "object",
                "properties": {
                    "url": { "type": "string" }
                },
                "required": ["url"]
            }),
        ),
        ToolDefinition::new(
            "save_reference",
            "Save a useful reference into the project's research library.",
            json!({
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "url": { "type": "string" },
                    "summary": { "type": "string" },
                    "category": { "type": "string", "enum": ["literature", "dataset", "code", "formula", "competition"] },
                    "methodology": { "type": "string" },
                    "key_parameters": { "type": "string" },
                    "ai_relevance": { "type": "string" }
                },
                "required": ["title", "url", "summary", "category"]
            }),
        ),
        ToolDefinition::new(
            "read_file",
            "Read a file from the current local workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }
                },
                "required": ["path"]
            }),
        ),
        ToolDefinition::new(
            "write_file",
            "Create a new file in the current local workspace.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        ),
    ]
}

/// Execute a tool call locally and return the result as a ChatMessage::tool.
pub async fn execute_tool(
    name: &str,
    arguments: &Value,
    agent_state: &State<'_, AgentState>,
) -> String {
    match name {
        "web_search" => execute_web_search(arguments).await,
        "fetch_url" => execute_fetch_url(arguments).await,
        "save_reference" => execute_save_reference(arguments, agent_state).await,
        "read_file" => execute_read_file(arguments, agent_state).await,
        "write_file" => execute_write_file(arguments, agent_state).await,
        _ => format!("Unknown tool: {name}"),
    }
}

async fn execute_web_search(arguments: &Value) -> String {
    let query = arguments["query"].as_str().unwrap_or("");
    let max_results = arguments["max_results"].as_u64().unwrap_or(10);

    if query.is_empty() {
        return "Error: query is required".to_string();
    }

    let url = format!(
        "http://localhost:8080/search?q={}&format=json&categories=general&pageno=1",
        urlencoding(query)
    );

    match reqwest::get(&url).await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(data) => {
                let results = data["results"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .take(max_results as usize)
                            .map(|r| {
                                json!({
                                    "title": r["title"].as_str().unwrap_or(""),
                                    "url": r["url"].as_str().unwrap_or(""),
                                    "snippet": r["content"].as_str().unwrap_or("")
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();

                if results.is_empty() {
                    "No results found.".to_string()
                } else {
                    format!(
                        "Search results for \"{query}\":\n{}",
                        results
                            .iter()
                            .enumerate()
                            .map(|(i, r)| format!(
                                "{}. **{}**\n   URL: {}\n   {}\n",
                                i + 1,
                                r["title"].as_str().unwrap_or(""),
                                r["url"].as_str().unwrap_or(""),
                                r["snippet"].as_str().unwrap_or("")
                            ))
                            .collect::<Vec<_>>()
                            .join("\n")
                    )
                }
            }
            Err(e) => format!("Failed to parse search results: {e}"),
        },
        Err(e) => format!("Search engine unavailable: {e}"),
    }
}

fn urlencoding(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char)
            }
            b' ' => result.push_str("%20"),
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}

async fn execute_fetch_url(arguments: &Value) -> String {
    let url = arguments["url"].as_str().unwrap_or("");
    if url.is_empty() {
        return "Error: url is required".to_string();
    }

    // Use Jina Reader as a free fallback (no API key needed)
    let reader_url = format!("https://r.jina.ai/{}", url);

    match reqwest::get(&reader_url).await {
        Ok(resp) => match resp.text().await {
            Ok(markdown) => {
                let truncated = if markdown.len() > 8000 {
                    format!(
                        "{}...\n\n(Content truncated at 8000 characters)",
                        &markdown[..8000]
                    )
                } else {
                    markdown
                };
                format!("Content from {url}:\n\n{truncated}")
            }
            Err(e) => format!("Failed to read page content: {e}"),
        },
        Err(e) => format!("Failed to fetch URL: {e}"),
    }
}

async fn execute_save_reference(arguments: &Value, agent_state: &State<'_, AgentState>) -> String {
    let title = arguments["title"].as_str().unwrap_or("");
    let url = arguments["url"].as_str().unwrap_or("");
    let summary = arguments["summary"].as_str().unwrap_or("");
    let category = arguments["category"].as_str().unwrap_or("literature");
    let methodology = arguments["methodology"].as_str().unwrap_or("");
    let key_parameters = arguments["key_parameters"].as_str().unwrap_or("");
    let ai_relevance = arguments["ai_relevance"].as_str().unwrap_or("");

    if title.is_empty() || summary.is_empty() {
        return "Error: title and summary are required to save a reference".to_string();
    }

    let slug = title_to_slug(title);
    let now = chrono::Utc::now().format("%Y-%m-%d");
    let md_content = format!(
        "# {title}\n\
         - **URL**: {url}\n\
         - **Category**: {category}\n\
         - **Methodology**: {methodology}\n\
         - **Key Parameters**: {key_parameters}\n\
         - **Saved**: {now}\n\n\
         ## AI Summary\n\
         {summary}\n\n\
         ## Relevance to Project\n\
         {ai_relevance}\n\n\
         ## Notes\n\
         <!-- Add your notes here -->\n",
        title = title,
        url = url,
        category = category,
        methodology = methodology,
        key_parameters = key_parameters,
        now = now,
        summary = summary,
        ai_relevance = ai_relevance,
    );

    let work_dir = match agent_state.work_dir.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return "Error: cannot access workspace".to_string(),
    };

    let ref_dir = work_dir.join("references");
    if let Err(e) = std::fs::create_dir_all(&ref_dir) {
        return format!("Error creating references directory: {e}");
    }

    let file_path = ref_dir.join(format!("{slug}.md"));
    match std::fs::write(&file_path, &md_content) {
        Ok(()) => {
            let _ = agent_state.app_handle.emit(
                "file-change",
                serde_json::json!({
                    "type": "file_change",
                    "path": format!("references/{slug}.md"),
                    "content": md_content
                }),
            );
            let _ = agent_state.app_handle.emit(
                "file-tree",
                serde_json::json!({
                    "type": "file_tree",
                    "tree": crate::agent::file_watcher::scan_tree(&work_dir).ok()
                }),
            );
            format!("Saved reference \"{title}\" to references/{slug}.md")
        }
        Err(e) => format!("Error writing reference file: {e}"),
    }
}

fn title_to_slug(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches('_');
    if slug.len() > 64 {
        slug[..64].to_string()
    } else {
        slug.to_string()
    }
}

async fn execute_read_file(arguments: &Value, agent_state: &State<'_, AgentState>) -> String {
    let path = arguments["path"].as_str().unwrap_or("");
    if path.is_empty() {
        return "Error: path is required".to_string();
    }

    let work_dir = match agent_state.work_dir.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return "Error: cannot access workspace".to_string(),
    };

    match crate::agent::file_watcher::read_workspace_file(&work_dir, path) {
        Ok(content) => format!("File: {path}\n\n{content}"),
        Err(e) => format!("Error reading file: {e:#}"),
    }
}

async fn execute_write_file(arguments: &Value, agent_state: &State<'_, AgentState>) -> String {
    let path = arguments["path"].as_str().unwrap_or("");
    let content = arguments["content"].as_str().unwrap_or("");
    if path.is_empty() {
        return "Error: path is required".to_string();
    }

    let work_dir = match agent_state.work_dir.lock() {
        Ok(guard) => guard.clone(),
        Err(_) => return "Error: cannot access workspace".to_string(),
    };

    let resolved = match crate::agent::commands::validate_and_resolve_path(&work_dir, path) {
        Ok(p) => p,
        Err(e) => return format!("Error: {e}"),
    };

    if let Some(parent) = resolved.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return format!("Error creating directory: {e}");
        }
    }

    match std::fs::write(&resolved, content) {
        Ok(()) => {
            let _ = agent_state.app_handle.emit(
                "file-change",
                serde_json::json!({
                    "type": "file_change",
                    "path": path,
                    "content": content
                }),
            );
            format!("File written: {path}")
        }
        Err(e) => format!("Error writing file: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::modeler_tool_definitions;

    #[test]
    fn exposes_phase9_tool_names() {
        let names = modeler_tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect::<Vec<_>>();

        assert!(names.contains(&"web_search".to_string()));
        assert!(names.contains(&"fetch_url".to_string()));
        assert!(names.contains(&"save_reference".to_string()));
        assert!(names.contains(&"read_file".to_string()));
    }
}
