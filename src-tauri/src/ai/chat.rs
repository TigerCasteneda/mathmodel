use super::compaction::{self, ContextMessage};
use super::config::{AiConfig, AiConfigState, AiConfigStatus};
use super::executor::{build_execution_requests, execute_tool_calls};
use super::runtime::{ModelerAiRuntime, PermissionMode};
use super::session::ChatSessionStore;
use super::workspace::WorkspaceContext;
use crate::agent::state::AgentState;
use claude_code_rs::ChatMessage;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, State};

#[derive(Debug, Clone, Serialize)]
pub struct ChatStreamEvent {
    pub conversation_id: String,
    pub seq: u64,
    pub content: String,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatThinkingEvent {
    pub conversation_id: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatToolCallEvent {
    pub conversation_id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub output: String,
    pub status: String, // "running" | "success" | "error"
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatTokenUsage {
    pub conversation_id: String,
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
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

fn rough_token_estimate(text: &str) -> usize {
    (text.len() as f64 / 1.3) as usize
}

#[cfg(feature = "accurate-tokenizer")]
fn token_encoder() -> Option<&'static tiktoken_rs::CoreBPE> {
    static TOKEN_ENCODER: OnceLock<Option<tiktoken_rs::CoreBPE>> = OnceLock::new();
    TOKEN_ENCODER
        .get_or_init(|| tiktoken_rs::cl100k_base().ok())
        .as_ref()
}

fn estimate_tokens(text: &str) -> usize {
    #[cfg(feature = "accurate-tokenizer")]
    {
        if let Some(encoder) = token_encoder() {
            return encoder.encode_ordinary(text).len();
        }
    }

    rough_token_estimate(text)
}

fn message_token_estimate(message: &ChatMessage) -> usize {
    serde_json::to_string(message)
        .map(|value| estimate_tokens(&value))
        .unwrap_or_else(|_| {
            let content_tokens = message
                .content
                .as_deref()
                .map(estimate_tokens)
                .unwrap_or_default();
            let tool_tokens = message
                .tool_calls
                .as_ref()
                .map(|tool_calls| {
                    tool_calls
                        .iter()
                        .map(|tool_call| {
                            estimate_tokens(&tool_call.id)
                                + estimate_tokens(&tool_call.function.name)
                                + estimate_tokens(&tool_call.function.arguments)
                        })
                        .sum::<usize>()
                })
                .unwrap_or_default();
            let tool_call_id_tokens = message
                .tool_call_id
                .as_deref()
                .map(estimate_tokens)
                .unwrap_or_default();
            content_tokens + tool_tokens + tool_call_id_tokens
        })
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct ConversationRound {
    messages: Vec<ChatMessage>,
}

#[cfg(test)]
impl ConversationRound {
    fn token_estimate(&self) -> usize {
        self.messages.iter().map(message_token_estimate).sum()
    }

    fn starts_with_role(&self, role: &str) -> bool {
        self.messages
            .first()
            .map(|message| message.role == role)
            .unwrap_or(false)
    }
}

#[cfg(test)]
fn build_conversation_rounds(messages: &[ChatMessage]) -> Vec<ConversationRound> {
    compaction::build_conversation_rounds(
        &messages
            .iter()
            .cloned()
            .map(|message| ContextMessage {
                message,
                timestamp: 0,
            })
            .collect::<Vec<_>>(),
    )
    .into_iter()
    .map(|round| ConversationRound {
        messages: round
            .messages
            .into_iter()
            .map(|message| message.message)
            .collect(),
    })
    .collect()
}

fn format_file_tree(item: &crate::agent::file_watcher::FileTreeItem, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let node_str = if item.node_type == "folder" {
        format!("{indent}{}/", item.name)
    } else {
        format!("{indent}{}", item.name)
    };
    let children = item
        .children
        .as_ref()
        .map(|c| {
            c.iter()
                .map(|ch| format_file_tree(ch, depth + 1))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if children.is_empty() {
        node_str
    } else {
        format!("{node_str}\n{children}")
    }
}

/// Trim `messages` to fit within `MAX_CONTEXT_TOKENS`, preserving the
/// system prompt (always first) and recent conversation history.
#[cfg(test)]
fn trim_context(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    let system = messages.first().filter(|m| m.role == "system").cloned();
    let rest: Vec<_> = if system.is_some() {
        messages.split_off(1)
    } else {
        messages
    };

    let system_tokens = system
        .as_ref()
        .map(|s| s.content.as_deref().map(estimate_tokens).unwrap_or(0))
        .unwrap_or(0);
    let mut budget = MAX_CONTEXT_TOKENS.saturating_sub(system_tokens);
    let rounds = build_conversation_rounds(&rest);

    // Keep recent rounds and prefer complete user->assistant bundles.
    let mut kept_rounds = Vec::new();
    let mut index = rounds.len();
    while index > 0 {
        let current_index = index - 1;
        let mut candidate_rounds = vec![rounds[current_index].clone()];
        if candidate_rounds[0].starts_with_role("assistant")
            && current_index > 0
            && rounds[current_index - 1].starts_with_role("user")
        {
            candidate_rounds.insert(0, rounds[current_index - 1].clone());
            index -= 1;
        }

        let candidate_tokens: usize = candidate_rounds
            .iter()
            .map(ConversationRound::token_estimate)
            .sum();
        if candidate_tokens <= budget {
            budget = budget.saturating_sub(candidate_tokens);
            kept_rounds.push(candidate_rounds);
        } else if kept_rounds.is_empty()
            && rounds[current_index].token_estimate() <= budget
            && !rounds[current_index].starts_with_role("assistant")
        {
            budget = budget.saturating_sub(rounds[current_index].token_estimate());
            kept_rounds.push(vec![rounds[current_index].clone()]);
        } else {
            break; // stop — older messages would exceed budget
        }
        index -= 1;
    }
    kept_rounds.reverse();
    let kept = kept_rounds
        .into_iter()
        .flatten()
        .flat_map(|round| round.messages)
        .collect::<Vec<_>>();

    if let Some(sys) = system {
        let mut out = vec![sys];
        out.extend(kept);
        out
    } else {
        kept
    }
}

fn compact_timestamped_context(messages: &[ContextMessage]) -> Vec<ChatMessage> {
    let system = messages
        .first()
        .filter(|message| message.message.role == "system")
        .cloned();
    let rest = if system.is_some() {
        messages[1..].to_vec()
    } else {
        messages.to_vec()
    };

    let system_tokens = system
        .as_ref()
        .map(|message| {
            message
                .message
                .content
                .as_deref()
                .map(estimate_tokens)
                .unwrap_or(0)
        })
        .unwrap_or(0);
    let kept = compaction::compact_context(
        &rest,
        chrono::Utc::now().timestamp(),
        MAX_CONTEXT_TOKENS.saturating_sub(system_tokens),
        &message_token_estimate,
    );

    if let Some(system_message) = system {
        let mut out = vec![system_message.message];
        out.extend(kept);
        out
    } else {
        kept
    }
}

#[tauri::command]
pub fn set_ai_config(config: AiConfig, state: State<'_, AiConfigState>) -> Result<(), String> {
    let current = state.get().unwrap_or_default();
    state.set(AiConfig {
        api_key: config.api_key.or(current.api_key),
        base_url: if config.base_url.trim().is_empty() {
            current.base_url
        } else {
            config.base_url
        },
        model: if config.model.trim().is_empty() {
            current.model
        } else {
            config.model
        },
        firecrawl_api_key: config.firecrawl_api_key.or(current.firecrawl_api_key),
        context7_api_key: config.context7_api_key.or(current.context7_api_key),
        searxng_url: if config.searxng_url.trim().is_empty() {
            current.searxng_url
        } else {
            config.searxng_url
        },
    })
}

#[tauri::command]
pub fn get_ai_config_status(state: State<'_, AiConfigState>) -> Result<AiConfigStatus, String> {
    Ok(state.get()?.into())
}

#[tauri::command]
pub fn set_ai_model(
    model: String,
    state: State<'_, AiConfigState>,
) -> Result<AiConfigStatus, String> {
    let mut config = state.get()?;
    config.model = model;
    state.set(config.clone())?;
    Ok(config.into())
}

#[tauri::command]
pub async fn ai_chat(
    message: String,
    conversation_id: Option<String>,
    workspace_mode: Option<String>,
    permission_mode: Option<String>,
    project_id: Option<String>,
    auth_token: Option<String>,
    server_base: Option<String>,
    capabilities: Option<Vec<String>>,
    app: AppHandle,
    _agent_state: State<'_, AgentState>,
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
    let work_dir = _agent_state
        .work_dir
        .lock()
        .map_err(|e| e.to_string())?
        .clone();

    let workspace = WorkspaceContext::new(
        work_dir,
        workspace_mode,
        project_id,
        auth_token,
        server_base,
        capabilities,
    );
    let permission_mode = PermissionMode::from_option(permission_mode);
    let runtime = ModelerAiRuntime::new(
        config,
        workspace,
        app.clone(),
        conversation_id.clone(),
        permission_mode,
    )
    .await
    .map_err(|e| {
        let msg = format!("Workspace runtime failed: {e}");
        emit_chat_error(&app, &conversation_id, &msg);
        msg
    })?;
    let mut tools = runtime.tool_definitions().await;

    let tree_text = match runtime.workspace_tree().await {
        Ok(tree) => format_file_tree(&tree, 0),
        Err(_) => String::from("(file tree unavailable)"),
    };

    let mut context_messages = vec![ContextMessage {
        message: ChatMessage::system(system_prompt(
            runtime.workspace_label(),
            runtime.permission_label(),
            &tree_text,
        )),
        timestamp: chrono::Utc::now().timestamp(),
    }];
    context_messages.extend(sessions.history_with_timestamps(&conversation_id)?);
    let mut stream_seq = 0u64;

    loop {
        let trimmed = compact_timestamped_context(&context_messages);
        let response = runtime
            .client()
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
                        stream_seq += 1;
                        emit_stream(&app, &conversation_id, stream_seq, content, false);
                    }
                    StreamLine::ToolCall(tc) => {
                        let entry =
                            tool_call_buf
                                .entry(tc.index)
                                .or_insert_with(|| ToolCallAccum {
                                    id: String::new(),
                                    name: String::new(),
                                    arguments: String::new(),
                                });
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
                    StreamLine::Finish(reason) => {
                        finish_reason = Some(reason);
                    }
                    StreamLine::Done => {
                        mark_stream_done(&mut finish_reason);
                    }
                    StreamLine::Ignore => {}
                }
            }
        }

        // Handle tool calls
        if finish_reason.as_deref() == Some("tool_calls") && !tool_call_buf.is_empty() {
            // Build assistant message with tool_calls
            let mut accumulated = tool_call_buf.into_iter().collect::<Vec<_>>();
            accumulated.sort_by_key(|(index, _)| *index);
            let tool_calls: Vec<claude_code_rs::api::ToolCall> = accumulated
                .into_iter()
                .map(|(_, tc)| claude_code_rs::api::ToolCall {
                    id: tc.id.clone(),
                    r#type: "function".to_string(),
                    function: claude_code_rs::api::ToolCallFunction {
                        name: tc.name.clone(),
                        arguments: tc.arguments.clone(),
                    },
                })
                .collect();

            context_messages.push(ContextMessage {
                message: ChatMessage::assistant_with_tools(tool_calls.clone()),
                timestamp: chrono::Utc::now().timestamp(),
            });

            let execution_requests = build_execution_requests(&tool_calls);
            for request in &execution_requests {
                emit_tool(
                    &app,
                    &conversation_id,
                    &request.name,
                    &request.arguments,
                    "",
                    "running",
                );
            }

            let mut persisted_tool_results = Vec::new();
            // Execute concurrency-safe tools in parallel while preserving result order.
            for result in execute_tool_calls(&runtime, &tool_calls).await {
                let status = tool_result_status(&result.output);
                emit_tool(
                    &app,
                    &conversation_id,
                    &result.name,
                    &result.arguments,
                    &result.output,
                    status,
                );
                persisted_tool_results.push((result.id.clone(), result.output.clone()));
                context_messages.push(ContextMessage {
                    message: ChatMessage::tool(&result.id, result.output),
                    timestamp: chrono::Utc::now().timestamp(),
                });
            }

            for persisted in
                build_persisted_tool_turn_messages(&tool_calls, &persisted_tool_results)
            {
                sessions.push_chat_message(&conversation_id, persisted)?;
            }
            tools = runtime.tool_definitions().await;

            // Continue loop — send tool results back to LLM (no hard limit)
            continue;
        }

        // Normal completion (no tool calls, or finish_reason is "stop")
        sessions.push_assistant(&conversation_id, assistant_text)?;
        stream_seq += 1;
        emit_stream(&app, &conversation_id, stream_seq, String::new(), true);
        return Ok(());
    }
}

fn mark_stream_done(finish_reason: &mut Option<String>) {
    if finish_reason.is_none() {
        *finish_reason = Some("stop".to_string());
    }
}

fn tool_result_status(result: &str) -> &'static str {
    if result.starts_with("Error") {
        return "error";
    }
    match serde_json::from_str::<serde_json::Value>(result) {
        Ok(value) if value["success"].as_bool() == Some(false) => "error",
        _ => "success",
    }
}

fn build_persisted_tool_turn_messages(
    tool_calls: &[claude_code_rs::api::ToolCall],
    tool_results: &[(String, String)],
) -> Vec<claude_code_rs::api::ChatMessage> {
    let mut messages = Vec::with_capacity(tool_results.len() + 1);
    messages.push(ChatMessage::assistant_with_tools(tool_calls.to_vec()));
    messages.extend(
        tool_results
            .iter()
            .map(|(tool_call_id, output)| ChatMessage::tool(tool_call_id.clone(), output.clone())),
    );
    messages
}

fn system_prompt(workspace_label: &str, permission_label: &str, file_tree: &str) -> String {
    format!(
        "You are Modeler AI, a mathematical modeling assistant embedded in a collaborative platform for MCM/ICM competition teams.\n\
         You are running on the claude-code-rust runtime layer inside a Tauri desktop app.\n\
         Current workspace source: {workspace_label}.\n\
         Current permission mode: {permission_label}.\n\
         Core tools are always visible: tool_search, file_read/read_file, file_write/write_file when workspace permissions allow writes, web_search, and save_reference.\n\
         Deferred tools such as file_edit, list_files, execute_command, search_files, fetch_url, and start_background_task must be discovered with tool_search before use.\n\
         fetch_url uses a Jina Reader markdown fallback inside chat. Use the Research panel for Firecrawl web search and Context7 docs search.\n\
         In Guest Remote mode, execute_command is unavailable because teammates do not own the host shell.\n\
         Default mode is read/search only. Accept Edit allows file edits. Auto allows edits and low-risk commands. Bypass allows broader shell execution.\n\
         Provide mathematical reasoning and make concrete workspace changes when asked.\n\
         \n\
         ## Current project files\n\
         Use file_read(path) to inspect contents, file_write(path, content) to create/overwrite when allowed, and tool_search before targeted edits or shell commands.\n\
         {file_tree}"
    )
}

fn emit_stream(app: &AppHandle, conversation_id: &str, seq: u64, content: String, done: bool) {
    let _ = app.emit(
        "chat:stream",
        ChatStreamEvent {
            conversation_id: conversation_id.to_string(),
            seq,
            content,
            done,
        },
    );
}

fn emit_tool(
    app: &AppHandle,
    conversation_id: &str,
    name: &str,
    arguments: &serde_json::Value,
    output: &str,
    status: &str,
) {
    let _ = app.emit(
        "chat:tool_call",
        ChatToolCallEvent {
            conversation_id: conversation_id.to_string(),
            name: name.to_string(),
            arguments: arguments.clone(),
            output: output.to_string(),
            status: status.to_string(),
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

    fn tool_call(id: &str, name: &str) -> claude_code_rs::api::ToolCall {
        claude_code_rs::api::ToolCall {
            id: id.to_string(),
            r#type: "function".to_string(),
            function: claude_code_rs::api::ToolCallFunction {
                name: name.to_string(),
                arguments: r#"{"query":"city traffic prediction"}"#.to_string(),
            },
        }
    }

    #[test]
    fn parses_openai_compatible_stream_content() {
        let line = r#"data: {"id":"1","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"content":"hello"},"finish_reason":null}]}"#;
        assert_eq!(
            parse_sse_line(line),
            StreamLine::Content("hello".to_string())
        );
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

    #[test]
    fn done_marker_does_not_override_tool_calls_finish_reason() {
        let mut finish_reason = Some("tool_calls".to_string());
        super::mark_stream_done(&mut finish_reason);
        assert_eq!(finish_reason.as_deref(), Some("tool_calls"));
    }

    #[test]
    fn system_prompt_documents_deferred_tools() {
        let prompt = super::system_prompt("Host Local", "Auto", "model.py");

        assert!(prompt.contains("tool_search"));
        assert!(prompt.contains("Deferred tools"));
        assert!(prompt.contains("Research panel for Firecrawl web search and Context7 docs search"));
    }

    #[test]
    fn tool_turn_persistence_shape_matches_runtime_history() {
        let tool_calls = vec![claude_code_rs::api::ToolCall {
            id: "call_1".to_string(),
            r#type: "function".to_string(),
            function: claude_code_rs::api::ToolCallFunction {
                name: "web_search".to_string(),
                arguments: r#"{"query":"sir"}"#.to_string(),
            },
        }];
        let tool_results = vec![(
            "call_1".to_string(),
            r#"{"success":true,"results":[]}"#.to_string(),
        )];

        let persisted = super::build_persisted_tool_turn_messages(&tool_calls, &tool_results);

        assert_eq!(persisted.len(), 2);
        assert_eq!(
            persisted[0].tool_calls.as_ref().map(|calls| calls.len()),
            Some(1)
        );
        assert_eq!(persisted[1].tool_call_id.as_deref(), Some("call_1"));
    }

    #[cfg(feature = "accurate-tokenizer")]
    #[test]
    fn uses_cl100k_tokenizer_when_feature_enabled() {
        assert_eq!(super::estimate_tokens("hello world"), 2);
    }

    #[test]
    fn token_estimator_counts_tool_call_payloads() {
        let message = claude_code_rs::api::ChatMessage::assistant_with_tools(vec![tool_call(
            "call_1",
            "web_search",
        )]);

        assert!(super::message_token_estimate(&message) > 20);
    }

    #[test]
    fn groups_messages_into_api_rounds() {
        let messages = vec![
            claude_code_rs::api::ChatMessage::user("Find relevant files"),
            claude_code_rs::api::ChatMessage::assistant_with_tools(vec![tool_call(
                "call_1",
                "tool_search",
            )]),
            claude_code_rs::api::ChatMessage::tool("call_1", r#"{"success":true}"#),
            claude_code_rs::api::ChatMessage::assistant("I found the files."),
            claude_code_rs::api::ChatMessage::user("Read both"),
        ];

        let rounds = super::build_conversation_rounds(&messages);

        assert_eq!(rounds.len(), 3);
        assert_eq!(rounds[0].messages.len(), 1);
        assert_eq!(rounds[1].messages.len(), 3);
        assert_eq!(rounds[2].messages.len(), 1);
    }

    #[test]
    fn trim_context_preserves_recent_round_boundaries() {
        let mut messages = vec![claude_code_rs::api::ChatMessage::system("system")];
        for index in 0..40 {
            messages.push(claude_code_rs::api::ChatMessage::user(format!(
                "user message {index} {}",
                "x".repeat(2000)
            )));
            messages.push(claude_code_rs::api::ChatMessage::assistant(format!(
                "assistant reply {index} {}",
                "y".repeat(2000)
            )));
        }

        let trimmed = super::trim_context(messages);

        assert_eq!(
            trimmed.first().map(|message| message.role.as_str()),
            Some("system")
        );
        assert_eq!(trimmed.len() % 2, 1);
        assert_eq!(
            trimmed.get(1).map(|message| message.role.as_str()),
            Some("user")
        );
    }
}
