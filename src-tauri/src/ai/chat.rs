use super::config::{AiConfig, AiConfigState, AiConfigStatus};
use super::session::ChatSessionStore;
use super::tools::modeler_tool_definitions;
use crate::agent::state::AgentState;
use claude_code_rs::api::StreamChunk;
use claude_code_rs::{ApiClient, ChatMessage};
use futures::StreamExt;
use serde::Serialize;
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

    let mut messages = vec![ChatMessage::system(system_prompt())];
    messages.extend(sessions.history(&conversation_id)?);

    let client = ApiClient::new(config.to_claude_settings(work_dir));
    let response = client
        .chat_stream(messages, Some(modeler_tool_definitions()))
        .await
        .map_err(|e| {
            let message = format!("API request failed: {e}");
            emit_chat_error(&app, &conversation_id, &message);
            message
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let message = format!("API error ({status}): {body}");
        emit_chat_error(&app, &conversation_id, &message);
        return Err(message);
    }

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut assistant = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| {
            let message = format!("Stream error: {e}");
            emit_chat_error(&app, &conversation_id, &message);
            message
        })?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(idx) = buffer.find('\n') {
            let line = buffer[..idx].trim_end_matches('\r').to_string();
            buffer = buffer[idx + 1..].to_string();

            match parse_sse_line(&line) {
                StreamLine::Content(content) => {
                    assistant.push_str(&content);
                    emit_stream(&app, &conversation_id, content, false);
                }
                StreamLine::Done => {
                    sessions.push_assistant(&conversation_id, assistant)?;
                    emit_stream(&app, &conversation_id, String::new(), true);
                    return Ok(());
                }
                StreamLine::Ignore => {}
            }
        }
    }

    sessions.push_assistant(&conversation_id, assistant)?;
    emit_stream(&app, &conversation_id, String::new(), true);
    Ok(())
}

fn system_prompt() -> String {
    "You are Modeler AI, a mathematical modeling assistant embedded in a collaborative platform for MCM/ICM competition teams. Provide mathematical reasoning, cite sources when available, and save valuable findings to the Research Library.".to_string()
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

#[derive(Debug, PartialEq)]
enum StreamLine {
    Content(String),
    Done,
    Ignore,
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

    chunk
        .choices
        .first()
        .and_then(|choice| choice.delta.content.clone())
        .map(StreamLine::Content)
        .unwrap_or(StreamLine::Ignore)
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
}
