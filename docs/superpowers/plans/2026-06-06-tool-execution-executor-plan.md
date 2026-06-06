# Tool Execution Executor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the sequential chat tool-call loop with a small executor that preserves original tool-call order while running concurrency-safe tool batches in parallel.

**Architecture:** Add `src-tauri/src/ai/executor.rs` as a focused module for parsing tool calls, batching them by safety metadata, and executing batches. `chat.rs` remains responsible for UI events and conversation state, while executor returns structured results in model-visible order.

**Tech Stack:** Rust, Tauri v2, `claude_code_rs::api::ToolCall`, existing `futures` dependency, `cargo test`, `cargo check`, `npx.cmd tsc --noEmit`.

---

### Task 1: Create Executor Request Model and Batching

**Files:**
- Create: `src-tauri/src/ai/executor.rs`
- Modify: `src-tauri/src/ai/mod.rs`

- [ ] **Step 1: Write failing batching tests**

Create `executor.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::{build_execution_batches, ToolExecutionBatch, ToolExecutionRequest};
    use serde_json::json;

    fn request(index: usize, name: &str) -> ToolExecutionRequest {
        ToolExecutionRequest {
            index,
            id: format!("call_{index}"),
            name: name.to_string(),
            arguments: json!({ "index": index }),
        }
    }

    #[test]
    fn batches_consecutive_safe_tools_and_keeps_unsafe_serial() {
        let requests = vec![
            request(0, "file_read"),
            request(1, "search_files"),
            request(2, "file_write"),
            request(3, "fetch_url"),
            request(4, "list_files"),
        ];

        let batches = build_execution_batches(&requests);

        assert_eq!(batches.len(), 3);
        assert!(matches!(&batches[0], ToolExecutionBatch::Concurrent(items) if items.len() == 2));
        assert!(matches!(&batches[1], ToolExecutionBatch::Serial(item) if item.name == "file_write"));
        assert!(matches!(&batches[2], ToolExecutionBatch::Concurrent(items) if items.len() == 2));
    }

    #[test]
    fn unknown_tools_fail_closed_as_serial() {
        let requests = vec![request(0, "definitely_missing_tool")];
        let batches = build_execution_batches(&requests);

        assert!(matches!(&batches[0], ToolExecutionBatch::Serial(item) if item.name == "definitely_missing_tool"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test executor::
```

from `src-tauri`.

Expected: FAIL because executor module is not declared and batching code does not exist.

- [ ] **Step 3: Implement request and batch model**

Add:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionRequest {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecutionBatch {
    Concurrent(Vec<ToolExecutionRequest>),
    Serial(ToolExecutionRequest),
}
```

Implement `build_execution_batches()` using `crate::ai::runtime::is_tool_concurrency_safe()`:
- Consecutive safe tools become one `Concurrent` batch.
- Unsafe and unknown tools become individual `Serial` batches.
- Request order is never changed.

- [ ] **Step 4: Declare module**

Add to `src-tauri/src/ai/mod.rs`:

```rust
pub mod executor;
```

- [ ] **Step 5: Run batching tests**

Run:

```powershell
cargo test executor::
```

Expected: PASS for batching tests.

### Task 2: Parse Tool Calls in Stable Model Order

**Files:**
- Modify: `src-tauri/src/ai/executor.rs`
- Modify: `src-tauri/src/ai/chat.rs`

- [ ] **Step 1: Write failing parse-order test**

Add:

```rust
use claude_code_rs::api::{ToolCall, ToolCallFunction};

fn tool_call(id: &str, name: &str, args: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        r#type: "function".to_string(),
        function: ToolCallFunction {
            name: name.to_string(),
            arguments: args.to_string(),
        },
    }
}

#[test]
fn builds_requests_in_tool_call_order_and_parses_arguments() {
    let calls = vec![
        tool_call("call_b", "file_read", r#"{ "path": "b.txt" }"#),
        tool_call("call_a", "file_read", r#"{ "path": "a.txt" }"#),
    ];

    let requests = super::build_execution_requests(&calls);

    assert_eq!(requests[0].index, 0);
    assert_eq!(requests[0].id, "call_b");
    assert_eq!(requests[0].arguments["path"], "b.txt");
    assert_eq!(requests[1].index, 1);
    assert_eq!(requests[1].id, "call_a");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test builds_requests_in_tool_call_order_and_parses_arguments
```

Expected: FAIL because `build_execution_requests()` does not exist.

- [ ] **Step 3: Implement request parsing**

Add:

```rust
pub fn build_execution_requests(tool_calls: &[ToolCall]) -> Vec<ToolExecutionRequest>
```

Parse `function.arguments` with `serde_json::from_str()`, falling back to `{}` on invalid JSON. Use slice order as `index`.

- [ ] **Step 4: Sort accumulated chat tool calls by SSE index**

In `chat.rs`, replace `tool_call_buf.values()` collection with key-sorted collection:

```rust
let mut accumulated = tool_call_buf.into_iter().collect::<Vec<_>>();
accumulated.sort_by_key(|(index, _)| *index);
let tool_calls = accumulated.into_iter().map(|(_, tc)| ...).collect::<Vec<_>>();
```

- [ ] **Step 5: Run parse-order test**

Run:

```powershell
cargo test builds_requests_in_tool_call_order_and_parses_arguments
```

Expected: PASS.

### Task 3: Execute Safe Batches Concurrently and Preserve Results

**Files:**
- Modify: `src-tauri/src/ai/executor.rs`

- [ ] **Step 1: Write failing async executor test**

Add:

```rust
#[tokio::test]
async fn executes_safe_batch_concurrently_but_returns_original_order() {
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let requests = vec![request(0, "file_read"), request(1, "search_files")];
    let started = Arc::new(Mutex::new(Vec::new()));
    let started_for_runner = started.clone();
    let begin = Instant::now();

    let results = super::execute_requests_with(requests, move |request| {
        let started_for_runner = started_for_runner.clone();
        async move {
            started_for_runner.lock().unwrap().push(request.index);
            if request.index == 0 {
                tokio::time::sleep(Duration::from_millis(80)).await;
            } else {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            format!("done_{}", request.index)
        }
    })
    .await;

    assert!(begin.elapsed() < Duration::from_millis(140));
    assert_eq!(started.lock().unwrap().as_slice(), &[0, 1]);
    assert_eq!(results.iter().map(|r| r.output.as_str()).collect::<Vec<_>>(), vec!["done_0", "done_1"]);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```powershell
cargo test executes_safe_batch_concurrently_but_returns_original_order
```

Expected: FAIL because async execution helpers do not exist.

- [ ] **Step 3: Implement execution result and generic runner**

Add:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ToolExecutionResult {
    pub index: usize,
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
    pub output: String,
}
```

Implement:

```rust
pub async fn execute_requests_with<F, Fut>(
    requests: Vec<ToolExecutionRequest>,
    runner: F,
) -> Vec<ToolExecutionResult>
where
    F: Fn(ToolExecutionRequest) -> Fut,
    Fut: std::future::Future<Output = String>,
```

Use `futures::future::join_all()` for `Concurrent` batches, await `Serial` one-by-one, and sort final results by `index`.

- [ ] **Step 4: Run async executor test**

Run:

```powershell
cargo test executes_safe_batch_concurrently_but_returns_original_order
```

Expected: PASS.

### Task 4: Wire Executor Into Chat Loop

**Files:**
- Modify: `src-tauri/src/ai/executor.rs`
- Modify: `src-tauri/src/ai/chat.rs`

- [ ] **Step 1: Implement runtime-backed executor**

Add:

```rust
pub async fn execute_tool_calls(
    runtime: &ModelerAiRuntime,
    tool_calls: &[ToolCall],
) -> Vec<ToolExecutionResult>
```

It should:
- Build requests with `build_execution_requests()`.
- Execute with `execute_requests_with()`.
- Call `runtime.execute_tool(&request.name, request.arguments.clone()).await`.
- Use `Unknown tool: <name>` when runtime returns `None`.

- [ ] **Step 2: Replace sequential loop in chat**

In `chat.rs`, import:

```rust
use super::executor::execute_tool_calls;
```

Replace the `for tc in &tool_calls` sequential execution with:
- Build requests via `build_execution_requests(&tool_calls)` only for running event emission, or emit running events from returned request data.
- Emit `"running"` for each request before executor call.
- Await `execute_tool_calls(&runtime, &tool_calls)`.
- For each result in returned order, compute status, emit final tool event, push `ChatMessage::tool(&result.id, result.output)`.

- [ ] **Step 3: Run chat and executor tests**

Run:

```powershell
cargo test executor:: ai::chat::tests::done_marker_does_not_override_tool_calls_finish_reason
```

Expected: PASS.

### Task 5: Verification and Commit

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
git diff -- src-tauri/src/ai/executor.rs src-tauri/src/ai/chat.rs src-tauri/src/ai/mod.rs docs/superpowers/plans/2026-06-06-tool-execution-executor-plan.md
```

Expected: only Part 2 executor changes.

- [ ] **Step 4: Commit**

Run:

```powershell
git add docs/superpowers/plans/2026-06-06-tool-execution-executor-plan.md src-tauri/src/ai/executor.rs src-tauri/src/ai/chat.rs src-tauri/src/ai/mod.rs
git commit -m "Add concurrent tool execution executor"
```

Expected: commit created.
