use claude_code_rs::api::ChatMessage;
use std::collections::BTreeSet;

const STALE_TOOL_RESULT_SECONDS: i64 = 60 * 60;
const RECENT_ROUNDS_TO_KEEP: usize = 3;
const SESSION_MEMORY_TRIGGER_ROUNDS: usize = 5;
const SESSION_MEMORY_MAX_CHARS: usize = 1_200;

#[derive(Debug, Clone)]
pub struct ContextMessage {
    pub message: ChatMessage,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct ConversationRound {
    pub messages: Vec<ContextMessage>,
}

impl ConversationRound {
    fn token_estimate(&self, estimate_tokens: &dyn Fn(&ChatMessage) -> usize) -> usize {
        self.messages
            .iter()
            .map(|message| estimate_tokens(&message.message))
            .sum()
    }

    fn starts_with_role(&self, role: &str) -> bool {
        self.messages
            .first()
            .map(|message| message.message.role == role)
            .unwrap_or(false)
    }

    fn newest_timestamp(&self) -> i64 {
        self.messages
            .iter()
            .map(|message| message.timestamp)
            .max()
            .unwrap_or_default()
    }
}

pub fn build_conversation_rounds(messages: &[ContextMessage]) -> Vec<ConversationRound> {
    let mut rounds = Vec::new();
    let mut current = Vec::new();

    for message in messages {
        let should_split = !current.is_empty()
            && match message.message.role.as_str() {
                "user" => true,
                "assistant" => current
                    .iter()
                    .all(|existing: &ContextMessage| existing.message.role == "user"),
                _ => false,
            };

        if should_split {
            rounds.push(ConversationRound {
                messages: std::mem::take(&mut current),
            });
        }

        current.push(message.clone());
    }

    if !current.is_empty() {
        rounds.push(ConversationRound { messages: current });
    }

    rounds
}

pub fn compact_context(
    messages: &[ContextMessage],
    now: i64,
    max_tokens: usize,
    estimate_tokens: &dyn Fn(&ChatMessage) -> usize,
) -> Vec<ChatMessage> {
    let rounds = build_conversation_rounds(messages);
    let rounds = evict_stale_tool_results(rounds, now);
    let rounds = insert_session_memory(rounds);
    trim_rounds_to_budget(rounds, max_tokens, estimate_tokens)
        .into_iter()
        .map(|message| message.message)
        .collect()
}

fn evict_stale_tool_results(rounds: Vec<ConversationRound>, now: i64) -> Vec<ConversationRound> {
    let preserve_from = rounds.len().saturating_sub(RECENT_ROUNDS_TO_KEEP);
    rounds
        .into_iter()
        .enumerate()
        .filter_map(|(index, round)| {
            if index >= preserve_from
                || round.newest_timestamp() > now.saturating_sub(STALE_TOOL_RESULT_SECONDS)
            {
                return Some(round);
            }

            let mut compacted_messages = Vec::new();
            let mut removed_tool_activity = false;

            for entry in &round.messages {
                if entry.message.role == "tool" {
                    removed_tool_activity = true;
                    continue;
                }

                if let Some(tool_calls) = entry.message.tool_calls.as_ref() {
                    removed_tool_activity = true;
                    if let Some(content) = entry.message.content.as_deref() {
                        let trimmed = content.trim();
                        if !trimmed.is_empty() {
                            compacted_messages.push(ContextMessage {
                                message: ChatMessage::assistant(trimmed.to_string()),
                                timestamp: entry.timestamp,
                            });
                        }
                    }

                    if !tool_calls.is_empty() {
                        compacted_messages.push(ContextMessage {
                            message: ChatMessage::assistant(format!(
                                "Earlier tool activity used {}. Raw tool results were omitted during context compaction.",
                                join_tool_names(tool_calls)
                            )),
                            timestamp: entry.timestamp,
                        });
                    }
                    continue;
                }

                compacted_messages.push(entry.clone());
            }

            if !removed_tool_activity {
                return Some(round);
            }

            if compacted_messages.is_empty() {
                None
            } else {
                Some(ConversationRound {
                    messages: compacted_messages,
                })
            }
        })
        .collect()
}

fn insert_session_memory(rounds: Vec<ConversationRound>) -> Vec<ConversationRound> {
    if rounds.len() < SESSION_MEMORY_TRIGGER_ROUNDS {
        return rounds;
    }

    let split_index = rounds.len().saturating_sub(RECENT_ROUNDS_TO_KEEP);
    let older = &rounds[..split_index];
    let recent = &rounds[split_index..];
    if older.is_empty() {
        return rounds;
    }

    let newest_older_timestamp = older
        .iter()
        .map(ConversationRound::newest_timestamp)
        .max()
        .unwrap_or_default();

    let mut output = vec![ConversationRound {
        messages: vec![ContextMessage {
            message: ChatMessage::system(build_session_memory_summary(older)),
            timestamp: newest_older_timestamp,
        }],
    }];
    output.extend(recent.iter().cloned());
    output
}

fn trim_rounds_to_budget(
    rounds: Vec<ConversationRound>,
    max_tokens: usize,
    estimate_tokens: &dyn Fn(&ChatMessage) -> usize,
) -> Vec<ContextMessage> {
    let mut budget = max_tokens;
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
            .map(|round| round.token_estimate(estimate_tokens))
            .sum();
        if candidate_tokens <= budget {
            budget = budget.saturating_sub(candidate_tokens);
            kept_rounds.push(candidate_rounds);
        } else if kept_rounds.is_empty()
            && rounds[current_index].token_estimate(estimate_tokens) <= budget
            && !rounds[current_index].starts_with_role("assistant")
        {
            budget = budget.saturating_sub(rounds[current_index].token_estimate(estimate_tokens));
            kept_rounds.push(vec![rounds[current_index].clone()]);
        } else {
            break;
        }

        index -= 1;
    }

    kept_rounds.reverse();
    kept_rounds
        .into_iter()
        .flatten()
        .flat_map(|round| round.messages)
        .collect()
}

fn build_session_memory_summary(rounds: &[ConversationRound]) -> String {
    let mut lines = vec!["Session memory summary of earlier conversation:".to_string()];
    for (index, round) in rounds.iter().enumerate() {
        let user = first_content(round, "user").unwrap_or_else(|| "(no user prompt)".to_string());
        let assistant =
            first_content(round, "assistant").unwrap_or_else(|| "(tool-only round)".to_string());
        lines.push(format!(
            "{}. User: {} | Assistant: {}",
            index + 1,
            truncate_line(&user, 120),
            truncate_line(&assistant, 120)
        ));
    }

    truncate_line(&lines.join("\n"), SESSION_MEMORY_MAX_CHARS)
}

fn first_content(round: &ConversationRound, role: &str) -> Option<String> {
    round
        .messages
        .iter()
        .find(|message| message.message.role == role)
        .and_then(|message| message.message.content.clone())
}

fn truncate_line(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut truncated = value.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

fn join_tool_names(tool_calls: &[claude_code_rs::api::ToolCall]) -> String {
    let mut names = BTreeSet::new();
    for tool_call in tool_calls {
        if !tool_call.function.name.trim().is_empty() {
            names.insert(tool_call.function.name.trim().to_string());
        }
    }

    if names.is_empty() {
        "previous tools".to_string()
    } else {
        names.into_iter().collect::<Vec<_>>().join(", ")
    }
}

#[cfg(test)]
mod tests {
    fn estimate_stub(message: &claude_code_rs::api::ChatMessage) -> usize {
        message.content.as_deref().map(str::len).unwrap_or_default()
            + message
                .tool_calls
                .as_ref()
                .map(|tool_calls| tool_calls.len() * 16)
                .unwrap_or_default()
    }

    fn context_user(content: &str, timestamp: i64) -> super::ContextMessage {
        super::ContextMessage {
            message: claude_code_rs::api::ChatMessage::user(content),
            timestamp,
        }
    }

    fn context_assistant(content: &str, timestamp: i64) -> super::ContextMessage {
        super::ContextMessage {
            message: claude_code_rs::api::ChatMessage::assistant(content),
            timestamp,
        }
    }

    fn context_assistant_with_tools(
        call_id: &str,
        tool_name: &str,
        timestamp: i64,
    ) -> super::ContextMessage {
        super::ContextMessage {
            message: claude_code_rs::api::ChatMessage::assistant_with_tools(vec![
                claude_code_rs::api::ToolCall {
                    id: call_id.to_string(),
                    r#type: "function".to_string(),
                    function: claude_code_rs::api::ToolCallFunction {
                        name: tool_name.to_string(),
                        arguments: r#"{"query":"older context"}"#.to_string(),
                    },
                },
            ]),
            timestamp,
        }
    }

    fn context_tool(tool_call_id: &str, content: &str, timestamp: i64) -> super::ContextMessage {
        super::ContextMessage {
            message: claude_code_rs::api::ChatMessage::tool(tool_call_id, content),
            timestamp,
        }
    }

    #[test]
    fn evicts_stale_tool_results_outside_recent_rounds() {
        let now = 10_000;
        let messages = vec![
            context_assistant_with_tools("call_1", "tool_search", now - 4_000),
            context_tool(
                "call_1",
                r#"{"success":true,"results":["old"]}"#,
                now - 4_000,
            ),
            context_user("recent-1", now - 30),
            context_assistant("recent-1-answer", now - 29),
            context_user("recent-2", now - 20),
            context_assistant("recent-2-answer", now - 19),
            context_user("recent-3", now - 10),
            context_assistant("recent-3-answer", now - 9),
        ];

        let compacted = super::compact_context(&messages, now, 96_000, &estimate_stub);

        assert!(!compacted.iter().any(|message| message.role == "tool"));
        assert!(compacted.iter().any(|message| {
            message
                .content
                .as_deref()
                .unwrap_or_default()
                .contains("tool_search")
        }));
    }

    #[test]
    fn inserts_session_memory_summary_after_five_rounds() {
        let now = 20_000;
        let messages = vec![
            context_user("round-1 user", now - 500),
            context_assistant("round-1 answer", now - 499),
            context_user("round-2 user", now - 400),
            context_assistant("round-2 answer", now - 399),
            context_user("round-3 user", now - 300),
            context_assistant("round-3 answer", now - 299),
            context_user("round-4 user", now - 200),
            context_assistant("round-4 answer", now - 199),
            context_user("round-5 user", now - 100),
            context_assistant("round-5 answer", now - 99),
            context_user("round-6 user", now - 50),
            context_assistant("round-6 answer", now - 49),
        ];

        let compacted = super::compact_context(&messages, now, 96_000, &estimate_stub);

        assert!(compacted.iter().any(|message| {
            message.role == "system"
                && message
                    .content
                    .as_deref()
                    .unwrap_or_default()
                    .contains("Session memory")
        }));
        assert!(compacted
            .iter()
            .any(|message| message.content.as_deref() == Some("round-6 user")));
        assert!(!compacted
            .iter()
            .any(|message| message.content.as_deref() == Some("round-1 user")));
    }
}
