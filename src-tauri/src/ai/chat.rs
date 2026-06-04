use super::config::{AiConfig, AiConfigState, AiConfigStatus};
use super::session::ChatSessionStore;
use super::tools::{execute_tool, modeler_tool_definitions};
use crate::agent::state::AgentState;
use claude_code_rs::{ApiClient, ChatMessage};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize)]
pub struct ChatStreamEvent {
    pub conversation_id: String,
    pub content: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatErrorEvent {
    pub conversation_id: String,
    pub message: String,
}

/// Approximate max tokens to send per request. Model-dependent;
/// DeepSeek supports up to ~128K but we conservatively trim at 96K to
/// leave room for the model response + tool call overhead.
const MAX_CONTEXT_TOKENS: usize = 96_000;

/// Rough token estimate: ~1.3 chars per token for English + code.
fn estimate_tokens(text: &str) -> usize {
    (text.len() as f64 / 1.3) as usize
}

/// Trim `messages` to fit within `MAX_CONTEXT_TOKENS`, preserving the
/// system prompt (always first) and recent conversation history.
fn trim_context(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let system = messages.first().filter(|m| m.role == "system").cloned();
    let rest: Vec<_> = if system.is_some() {
        messages.split_off(1)
    } else {
        messages
    };

    let system_tokens = system.as_ref().map(|s| s.content.as_deref().map(estimate_tokens).unwrap_or(0)).unwrap_or(0);
    let mut budget = MAX_CONTEXT_TOKENS.saturating_sub(system_tokens);

    // Keep recent messages; drop oldest non-system messages first
    let mut kept = Vec::new();
    for msg in rest.into_iter().rev() {
        let tokens = msg.content.as_deref().map(estimate_tokens).unwrap_or(0)
            + msg.tool_calls.as_ref().map(|tc| tc.len() * 20).unwrap_or(0);
        if tokens <= budget {
            budget = budget.saturating_sub(tokens);
            kept.push(msg);
        } else {
            break; // stop — older messages would exceed budget
        }
    }
    kept.reverse();

    if let Some(sys) = system {
        let mut out = vec![sys];
        out.extend(kept);
        out
    } else {
        kept
    }
}

#[tauri::command]
pub fn set_ai_config(config: AiConfig, state: State<'_, AiConfigState>) -> Result<(), String> {
    state.set(config)
}

#[tauri::command]
pub fn get_ai_config_status(state: State<'_, AiConfigState>) -> Result<AiConfigStatus, String> {
    Ok(state.get()?.into())
}

#[tauri::command]
pub async fn ai_chat(
    message: String,
    conversation_id: Option<String>,
    app: AppHandle,
    agent_state: State<'_, AgentState>,
    config_state: State<'_, AiConfigState>,
    sessions: State<'_, ChatSessionStore>,
) -> Result<(), String> {
    let conversation_id = conversation_id.unwrap_or_else(|| "default".to_string());
    let config = config_state.get()?;
    if config
        .api_key
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        emit_chat_error(&app, &conversation_id, "API key is not configured.");
        return Err("API key is not configured".to_string());
    }

    sessions.push_user(&conversation_id, message.clone())?;
    let work_dir = agent_state
        .work_dir
        .lock()
        .map_err(|e| e.to_string())?
        .clone();

    let client = ApiClient::new(config.to_claude_settings(work_dir));
    let tools = modeler_tool_definitions();

    let mut messages = vec![ChatMessage::system(system_prompt())];
    messages.extend(sessions.history(&conversation_id)?);

    loop {
        let trimmed = trim_context(messages.clone());
        let response = client
            .chat_stream(trimmed, Some(tools.clone()))
            .await
            .map_err(|e| {
                let msg = format!("API request failed: {e}");
                emit_chat_error(&app, &conversation_id, &msg);
                msg
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let msg = format!("API error ({status}): {body}");
            emit_chat_error(&app, &conversation_id, &msg);
            return Err(msg);
        }

        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut assistant_text = String::new();

        // Accumulated tool calls: index → {id, name, arguments}
        let mut tool_call_buf: HashMap<i64, ToolCallAccum> = HashMap::new();
        let mut finish_reason: Option<String> = None;

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.map_err(|e| {
                let msg = format!("Stream error: {e}");
                emit_chat_error(&app, &conversation_id, &msg);
                msg
            })?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(idx) = buffer.find('\n') {
                let line = buffer[..idx].trim_end_matches('\r').to_string();
                buffer = buffer[idx + 1..].to_string();

                match parse_sse_line(&line) {
                    StreamLine::Content(content) => {
                        assistant_text.push_str(&content);
                        emit_stream(&app, &conversation_id, content, false);
                    }
                    StreamLine::ToolCall(tc) => {
                        let entry = tool_call_buf.entry(tc.index).or_insert_with(|| ToolCallAccum {
                            id: String::new(),
                            name: String::new(),
                            arguments: String::new(),
                        });
                        if let Some(id) = tc.id { entry.id = id; }
                        if let Some(name) = tc.name { entry.name = name; }
                        if let Some(args) = tc.arguments { entry.arguments.push_str(&args); }
                    }
                    StreamLine::Finish(reason) => {
                        finish_reason = Some(reason);
                    }
                    StreamLine::Done => {
                        finish_reason = Some("stop".to_string());
                    }
                    StreamLine::Ignore => {}
                }
            }
        }

        // Handle tool calls
        if finish_reason.as_deref() == Some("tool_calls") && !tool_call_buf.is_empty() {
            // Build assistant message with tool_calls
            let tool_calls: Vec<claude_code_rs::api::ToolCall> = tool_call_buf
                .values()
                .map(|tc| claude_code_rs::api::ToolCall {
                    id: tc.id.clone(),
                    r#type: "function".to_string(),
                    function: claude_code_rs::api::ToolCallFunction {
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    },
                })
                .collect();

            messages.push(ChatMessage::assistant_with_tools(tool_calls.clone()));

            // Execute each tool and add results
            let mut tool_count = 0;
            for tc in &tool_calls {
                let args: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                    .unwrap_or(serde_json::json!({}));
                let result = execute_tool(&tc.function.name, &args, &agent_state).await;
                emit_stream(&app, &conversation_id, format!("\n\n🔧 **{}**\n{result}\n", tc.function.name), false);
                messages.push(ChatMessage::tool(&tc.id, result));
                tool_count += 1;
            }

            sessions.push_assistant(&conversation_id, format!("{assistant_text}\n\n[Executed {tool_count} tool(s)]"))?;

            // Continue loop — send tool results back to LLM (no hard limit)
            continue;
        }

        // Normal completion (no tool calls, or finish_reason is "stop")
        sessions.push_assistant(&conversation_id, assistant_text)?;
        emit_stream(&app, &conversation_id, String::new(), true);
        return Ok(());
    }
}

fn system_prompt() -> String {
    "You are Modeler AI, a mathematical modeling assistant embedded in a collaborative platform for MCM/ICM competition teams. You can search the web, fetch page content, read/write files, and save references. Provide mathematical reasoning, cite sources, and save valuable findings to the Research Library using the save_reference tool.".to_string()
}

fn emit_stream(app: &AppHandle, conversation_id: &str, content: String, done: bool) {
    let _ = app.emit(
        "chat:stream",
        ChatStreamEvent {
            conversation_id: conversation_id.to_string(),
            content,
            done,
        },
    );
}

fn emit_chat_error(app: &AppHandle, conversation_id: &str, message: &str) {
    let _ = app.emit(
        "chat:error",
        ChatErrorEvent {
            conversation_id: conversation_id.to_string(),
            message: message.to_string(),
        },
    );
}

#[derive(Debug, Default)]
struct ToolCallAccum {
    id: String,
    name: String,
    arguments: String,
}

#[derive(Debug, PartialEq)]
enum StreamLine {
    Content(String),
    ToolCall(ToolCallChunk),
    Finish(String),
    Done,
    Ignore,
}

#[derive(Debug, PartialEq)]
struct ToolCallChunk {
    index: i64,
    id: Option<String>,
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

    let Ok(chunk) = serde_json::from_str::<ExtendedStreamChunk>(data) else {
        return StreamLine::Ignore;
    };

    if let Some(choice) = chunk.choices.first() {
        // Check finish_reason
        if let Some(ref reason) = choice.finish_reason {
            return StreamLine::Finish(reason.clone());
        }

        // Check text content
        if let Some(ref content) = choice.delta.content {
            return StreamLine::Content(content.clone());
        }

        // Check tool calls
        if let Some(ref tcs) = choice.delta.tool_calls {
            for tc in tcs {
                let chunk = ToolCallChunk {
                    index: tc.index,
                    id: tc.id.clone(),
                    name: tc.function.as_ref().and_then(|f| f.name.clone()),
                    arguments: tc.function.as_ref().and_then(|f| f.arguments.clone()),
                };
                return StreamLine::ToolCall(chunk);
            }
        }
    }

    StreamLine::Ignore
}

/// Extended stream chunk with tool_call support (not in upstream claude_code_rs).
#[derive(Debug, Deserialize)]
struct ExtendedStreamChunk {
    choices: Vec<ExtendedChoice>,
}

#[derive(Debug, Deserialize)]
struct ExtendedChoice {
    delta: ExtendedDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ExtendedDelta {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ExtendedToolCall>>,
}

#[derive(Debug, Deserialize)]
struct ExtendedToolCall {
    index: i64,
    #[serde(default)]
    id: Option<String>,
    function: Option<ExtendedFunction>,
}

#[derive(Debug, Deserialize)]
struct ExtendedFunction {
    name: Option<String>,
    arguments: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{parse_sse_line, StreamLine};

    #[test]
    fn parses_openai_compatible_stream_content() {
        let line = r#"data: {"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#;
        assert_eq!(parse_sse_line(line), StreamLine::Content("hello".to_string()));
    }

    #[test]
    fn parses_done_marker() {
        assert_eq!(parse_sse_line("data: [DONE]"), StreamLine::Done);
    }

    #[test]
    fn parses_tool_call_delta() {
        let line = r#"data: {"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"web_search","arguments":"{\"query\":\"SIR\"}"}}]},"finish_reason":null}]}"#;
        match parse_sse_line(line) {
            StreamLine::ToolCall(tc) => {
                assert_eq!(tc.index, 0);
                assert_eq!(tc.id.as_deref(), Some("call_1"));
                assert_eq!(tc.name.as_deref(), Some("web_search"));
                assert_eq!(tc.arguments.as_deref(), Some("{\"query\":\"SIR\"}"));
            }
            other => panic!("expected ToolCall, got {other:?}"),
        }
    }
}
