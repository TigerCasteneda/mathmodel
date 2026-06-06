# Compaction Foundations Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a dedicated `compaction.rs` module that performs local context compaction before chat requests: evict stale tool output while preserving recent rounds, then collapse older rounds into a synthetic session-memory system message.

**Architecture:** Move round-aware context handling out of `chat.rs` into a reusable compaction module that operates on timestamped chat messages. `chat.rs` keeps tokenizer-backed token estimation and passes timestamped session/runtime messages into the compactor before each API round trip.

**Tech Stack:** Rust, Tauri v2, `claude_code_rs::api::ChatMessage`, existing session store timestamps, `cargo test`, `cargo check`, `npx.cmd tsc --noEmit`.

---

### Task 1: Add Compaction Module Tests

**Files:**
- Add: `src-tauri/src/ai/compaction.rs`
- Modify: `src-tauri/src/ai/mod.rs`

- [ ] **Step 1: Write failing compaction tests**

Add tests for:

```rust
#[test]
fn evicts_stale_tool_results_outside_recent_rounds() {
    let now = 10_000;
    let messages = vec![
        context_assistant_with_tools("call_1", "tool_search", now - 4_000),
        context_tool("call_1", r#"{"success":true,"results":["old"]}"#, now - 4_000),
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
        message.role == "assistant"
            && message
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
    assert!(compacted.iter().any(|message| {
        message.content.as_deref() == Some("round-6 user")
    }));
    assert!(!compacted.iter().any(|message| {
        message.content.as_deref() == Some("round-1 user")
    }));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run from `src-tauri`:

```powershell
cargo test evicts_stale_tool_results_outside_recent_rounds
cargo test inserts_session_memory_summary_after_five_rounds
```

Expected: FAIL because the compaction module and APIs do not exist yet.

### Task 2: Implement Timestamped Compaction

**Files:**
- Add: `src-tauri/src/ai/compaction.rs`
- Modify: `src-tauri/src/ai/chat.rs`
- Modify: `src-tauri/src/ai/session.rs`
- Modify: `src-tauri/src/ai/mod.rs`

- [ ] **Step 1: Add timestamped message model**

Introduce a lightweight runtime type:

```rust
pub struct ContextMessage {
    pub message: ChatMessage,
    pub timestamp: i64,
}
```

Add a `session.history_with_timestamps()` helper so chat can rebuild context with persisted timestamps.

- [ ] **Step 2: Implement compaction pipeline**

In `compaction.rs`, add:
- round grouping for `ContextMessage`
- stale tool eviction for rounds older than 60 minutes, excluding the newest 3 rounds
- synthetic assistant placeholder for stripped tool activity
- synthetic system session-memory summary when there are 5 or more rounds
- round-aware token trimming using the existing estimator callback

- [ ] **Step 3: Integrate with chat.rs**

Update `ai_chat()` and context trimming flow to:
- load timestamped history from the session store
- append in-memory assistant/tool turns with current timestamps
- call `compact_context(...)` before `chat_stream(...)`

Keep tokenizer-backed counting in `chat.rs`; pass it into compaction as a callback.

- [ ] **Step 4: Re-run compaction tests**

Expected: PASS.

### Task 3: Verification and Commit

**Files:**
- No additional files unless cleanup is needed

- [ ] **Step 1: Run focused Rust tests**

```powershell
cargo test evicts_stale_tool_results_outside_recent_rounds
cargo test inserts_session_memory_summary_after_five_rounds
```

- [ ] **Step 2: Run broader verification**

```powershell
cargo test
cargo check
npx.cmd tsc --noEmit
```

- [ ] **Step 3: Inspect diff and commit**

Ensure unrelated files are untouched, then commit the part as a standalone changeset.
