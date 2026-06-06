# Session Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist chat tool-call turns end-to-end so restored sessions keep assistant tool calls, tool results, and first-message auto-titles.

**Architecture:** Extend `SessionMessage` to mirror `claude_code_rs::api::ChatMessage` closely enough for lossless round-tripping. Update `chat.rs` to persist the actual tool-turn structure instead of flattening it, and update the frontend restore path to rebuild existing `toolCalls` UI state from persisted assistant/tool messages.

**Tech Stack:** Rust, Tauri v2, `claude_code_rs::api::ChatMessage`, Next.js/TypeScript, existing `cargo test`, `cargo check`, `npx.cmd tsc --noEmit`.

---

### Task 1: Extend SessionMessage and Preserve History Structure

**Files:**
- Modify: `src-tauri/src/ai/session.rs`

- [ ] **Step 1: Write failing session round-trip tests**

Add tests:

```rust
#[test]
fn history_preserves_tool_calls_and_tool_call_id() {
    let store = test_store();
    let conversation_id = "tool-history";

    store
        .push_chat_message(
            conversation_id,
            claude_code_rs::api::ChatMessage::assistant_with_tools(vec![
                claude_code_rs::api::ToolCall {
                    id: "call_1".to_string(),
                    r#type: "function".to_string(),
                    function: claude_code_rs::api::ToolCallFunction {
                        name: "web_search".to_string(),
                        arguments: r#"{"query":"sir"}"#.to_string(),
                    },
                },
            ]),
        )
        .unwrap();
    store
        .push_chat_message(
            conversation_id,
            claude_code_rs::api::ChatMessage::tool("call_1", r#"{"success":true}"#),
        )
        .unwrap();

    let history = store.history(conversation_id).unwrap();

    assert_eq!(history.len(), 2);
    assert_eq!(history[0].tool_calls.as_ref().map(|calls| calls.len()), Some(1));
    assert_eq!(history[1].tool_call_id.as_deref(), Some("call_1"));
}

#[test]
fn first_user_message_sets_session_title() {
    let store = test_store();
    let conversation_id = "title";

    store
        .push_user(
            conversation_id,
            "Build a traffic prediction baseline with graph neural networks".to_string(),
        )
        .unwrap();

    let session = store.load(conversation_id).unwrap();
    assert_eq!(session.name, "Build a traffic prediction baseline with graph ");
}

#[test]
fn legacy_string_content_session_still_loads() {
    let store = test_store();
    let session_path = store.sessions_dir_for_tests().join("legacy.json");
    std::fs::write(
        &session_path,
        r#"{"id":"legacy","name":"New Chat","created_at":1,"updated_at":1,"messages":[{"role":"assistant","content":"hello","timestamp":1}]}"#,
    )
    .unwrap();

    let history = store.history("legacy").unwrap();

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].content.as_deref(), Some("hello"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```powershell
cargo test history_preserves_tool_calls_and_tool_call_id
cargo test first_user_message_sets_session_title
cargo test legacy_string_content_session_still_loads
```

from `src-tauri`.

Expected: FAIL because `SessionMessage` has no tool fields, `push_chat_message()` does not exist, and auto-title is not implemented.

- [ ] **Step 3: Extend persisted session model**

Change `SessionMessage` to:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    #[serde(default)]
    pub content: Option<String>,
    pub timestamp: i64,
    #[serde(default)]
    pub tool_calls: Option<Vec<claude_code_rs::api::ToolCall>>,
    #[serde(default)]
    pub tool_call_id: Option<String>,
}
```

Keep all new fields `#[serde(default)]` for backward compatibility.

- [ ] **Step 4: Add generic push helper and auto-title**

Add:

```rust
pub fn push_chat_message(
    &self,
    conversation_id: &str,
    message: claude_code_rs::api::ChatMessage,
) -> Result<(), String>
```

Implement `push_user()` and `push_assistant()` on top of it. When session name is `"New Chat"` and a user message with non-empty text arrives, truncate the title to 50 characters.

- [ ] **Step 5: Return real history fields**

Update `history()` to map:

```rust
content: m.content.clone(),
tool_calls: m.tool_calls.clone(),
tool_call_id: m.tool_call_id.clone(),
```

and add small test-only helpers as needed for temp session directories.

- [ ] **Step 6: Run session tests**

Run:

```powershell
cargo test history_preserves_tool_calls_and_tool_call_id
cargo test first_user_message_sets_session_title
cargo test legacy_string_content_session_still_loads
```

Expected: PASS.

### Task 2: Persist Tool Turns From chat.rs

**Files:**
- Modify: `src-tauri/src/ai/chat.rs`

- [ ] **Step 1: Write failing chat persistence test**

Add a focused unit test for a helper that decides which chat messages are persisted:

```rust
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
    assert_eq!(persisted[0].tool_calls.as_ref().map(|calls| calls.len()), Some(1));
    assert_eq!(persisted[1].tool_call_id.as_deref(), Some("call_1"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test tool_turn_persistence_shape_matches_runtime_history
```

Expected: FAIL because helper does not exist.

- [ ] **Step 3: Implement persistence helper**

Add in `chat.rs`:

```rust
fn build_persisted_tool_turn_messages(
    tool_calls: &[claude_code_rs::api::ToolCall],
    tool_results: &[(String, String)],
) -> Vec<claude_code_rs::api::ChatMessage>
```

Return:
- `ChatMessage::assistant_with_tools(tool_calls.to_vec())`
- one `ChatMessage::tool(id, output)` per result in order

- [ ] **Step 4: Persist real tool turns in ai_chat()**

In the `tool_calls` branch:
- remove `sessions.push_assistant(&conversation_id, assistant_text)?;`
- collect executor results into a `Vec<(String, String)>`
- persist `build_persisted_tool_turn_messages(...)` through `sessions.push_chat_message(...)`

Keep the normal completion branch persisting the final assistant text response.

- [ ] **Step 5: Run chat persistence test**

Run:

```powershell
cargo test tool_turn_persistence_shape_matches_runtime_history
```

Expected: PASS.

### Task 3: Restore Persisted Tool Calls in Frontend

**Files:**
- Modify: `lib/tauri-api.ts`
- Modify: `components/chat/chat-panel.tsx`

- [ ] **Step 1: Extend session types**

Update `lib/tauri-api.ts`:

```ts
export interface SessionToolCallFunction {
  name: string
  arguments: string
}

export interface SessionToolCall {
  id: string
  type: string
  function: SessionToolCallFunction
}

export interface SessionMessage {
  role: string
  content?: string | null
  timestamp: number
  tool_calls?: SessionToolCall[] | null
  tool_call_id?: string | null
}
```

- [ ] **Step 2: Add restore helper in chat panel**

Create a small helper in `chat-panel.tsx`:

```ts
function restoreMessages(sessionMessages: SessionMessage[]): Message[] { ... }
```

Rules:
- `assistant` + `tool_calls` becomes an assistant `Message` with `toolCalls` entries and `content: ""`.
- Matching persisted `tool` messages update those `toolCalls` entries by `tool_call_id`.
- Plain `assistant/user/system` messages restore as normal.
- Tool status mirrors backend logic: `"Error..."` or JSON `{ success: false }` => `"error"`, otherwise `"success"`.

- [ ] **Step 3: Use helper during session load**

Replace:

```ts
const restored: Message[] = (session.messages || []).map(...)
```

with:

```ts
const restored = restoreMessages(session.messages || [])
```

- [ ] **Step 4: Run frontend typecheck**

Run:

```powershell
npx.cmd tsc --noEmit
```

Expected: PASS.

### Task 4: Final Verification and Commit

**Files:**
- Verify all touched files.

- [ ] **Step 1: Run Rust verification**

Run:

```powershell
cargo check
cargo test
```

from `src-tauri`.

Expected: both exit 0.

- [ ] **Step 2: Run frontend typecheck**

Run:

```powershell
npx.cmd tsc --noEmit
```

from repo root.

Expected: exit 0.

- [ ] **Step 3: Review diff**

Run:

```powershell
git diff -- src-tauri/src/ai/session.rs src-tauri/src/ai/chat.rs lib/tauri-api.ts components/chat/chat-panel.tsx docs/superpowers/plans/2026-06-06-session-persistence-plan.md
```

Expected: only Part 3 session persistence changes.

- [ ] **Step 4: Commit**

Run:

```powershell
git add docs/superpowers/plans/2026-06-06-session-persistence-plan.md src-tauri/src/ai/session.rs src-tauri/src/ai/chat.rs lib/tauri-api.ts components/chat/chat-panel.tsx
git commit -m "Persist tool calls in chat sessions"
```

Expected: commit created.
