# Token Counting and API Round Grouping Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the coarse token heuristic in `chat.rs` with an accurate tokenizer-backed estimator when available, and add API-round grouping so context trimming preserves whole user/assistant/tool sequences.

**Architecture:** Keep the current flat `Vec<ChatMessage>` runtime history, but derive `ConversationRound` views for trimming. Token counting remains local to `chat.rs`, with a feature-gated `tiktoken-rs` path and a fallback heuristic for unsupported builds.

**Tech Stack:** Rust, Tauri v2, `claude_code_rs::api::ChatMessage`, `tiktoken-rs`, `cargo test`, `cargo check`.

---

### Task 1: Accurate Token Counting

**Files:**
- Modify: `src-tauri/src/ai/chat.rs`
- Modify: `src-tauri/Cargo.toml`

- [ ] **Step 1: Write failing tokenizer-backed tests**

Add focused tests for:

```rust
#[cfg(feature = "accurate-tokenizer")]
#[test]
fn uses_cl100k_tokenizer_when_feature_enabled() {
    assert_eq!(super::estimate_tokens("hello world"), 2);
}

#[test]
fn token_estimator_counts_tool_call_payloads() {
    let msg = claude_code_rs::api::ChatMessage::assistant_with_tools(vec![
        claude_code_rs::api::ToolCall {
            id: "call_1".to_string(),
            r#type: "function".to_string(),
            function: claude_code_rs::api::ToolCallFunction {
                name: "web_search".to_string(),
                arguments: r#"{"query":"city traffic prediction"}"#.to_string(),
            },
        },
    ]);

    assert!(super::message_token_estimate(&msg) > 20);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri`:

```powershell
cargo test uses_cl100k_tokenizer_when_feature_enabled
cargo test token_estimator_counts_tool_call_payloads
```

Expected: FAIL because the current estimator is the old chars/1.3 heuristic and there is no `message_token_estimate()` helper.

- [ ] **Step 3: Add feature-gated tokenizer implementation**

In `Cargo.toml`, add:
- optional `tiktoken-rs`
- feature `accurate-tokenizer`
- keep fallback heuristic when the feature is disabled

In `chat.rs`, add:
- `token_encoder()` helper under `#[cfg(feature = "accurate-tokenizer")]`
- tokenizer-backed `estimate_tokens()`
- fallback `estimate_tokens()` for builds without the feature
- `message_token_estimate()` so tool calls and tool results are counted consistently

- [ ] **Step 4: Re-run token tests**

Expected: PASS.

### Task 2: API Round Grouping and Round-Preserving Trimming

**Files:**
- Modify: `src-tauri/src/ai/chat.rs`

- [ ] **Step 1: Write failing round-grouping tests**

Add tests for:

```rust
#[test]
fn groups_messages_into_api_rounds() {
    let messages = vec![
        claude_code_rs::api::ChatMessage::user("Find relevant files"),
        claude_code_rs::api::ChatMessage::assistant_with_tools(vec![tool_call("call_1", "tool_search")]),
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
        messages.push(claude_code_rs::api::ChatMessage::user(format!("user message {index} {}", "x".repeat(2000))));
        messages.push(claude_code_rs::api::ChatMessage::assistant(format!("assistant reply {index} {}", "y".repeat(2000))));
    }

    let trimmed = super::trim_context(messages);

    assert_eq!(trimmed.first().map(|m| m.role.as_str()), Some("system"));
    assert_eq!(trimmed.len() % 2, 1);
    assert_eq!(trimmed.get(1).map(|m| m.role.as_str()), Some("user"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test groups_messages_into_api_rounds
cargo test trim_context_preserves_recent_round_boundaries
```

Expected: FAIL because there is no round builder and trimming currently works on individual messages.

- [ ] **Step 3: Implement round grouping and trimming**

Add:
- `ConversationRound` struct
- `build_conversation_rounds(messages: &[ChatMessage]) -> Vec<ConversationRound>`
- updated `trim_context()` that budgets by whole rounds instead of individual flat messages

Boundary rule:
- start a new round whenever a new assistant message begins after prior accumulated messages
- keep tool-call assistant messages and their tool results in the same round
- preserve the system prompt separately

- [ ] **Step 4: Re-run round tests**

Expected: PASS.

### Task 3: Verification

**Files:**
- No additional files unless cleanup is needed

- [ ] **Step 1: Run focused Rust tests**

```powershell
cargo test uses_cl100k_tokenizer_when_feature_enabled
cargo test token_estimator_counts_tool_call_payloads
cargo test groups_messages_into_api_rounds
cargo test trim_context_preserves_recent_round_boundaries
```

- [ ] **Step 2: Run broader verification**

```powershell
cargo test
cargo check
npx.cmd tsc --noEmit
```

- [ ] **Step 3: Inspect diff for unrelated churn**

Ensure formatting-only changes in unrelated files are not included before commit.
