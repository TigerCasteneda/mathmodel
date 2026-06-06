 Claude-Code Feature Integration into Modeler AI

 Context

 The Modeler AI app (Tauri v2 + Next.js) has a working AI coding chat with 12 tools, SSE streaming, and dual workspace mode. The
 claude-code/ directory (not part of this project) reveals production-hardened patterns for tool orchestration, permission management,
 context compaction, and search. This plan ports those patterns into the Tauri app while preserving the existing architecture — no rewrites,
 only targeted upgrades.

 Goal: Desktop Claude Code experience with local file explorer access, including: concurrent tool execution, enhanced tool search, accurate
 token counting, multi-tier context compaction, layered permission rules, thinking/token events, stop generation, and full session
 persistence.

 ---
 Phase 1: Dead Code Removal (5 min)

 Action: Delete src-tauri/src/ai/tools.rs (355 lines).

 Why: Superseded by runtime.rs. Zero references anywhere — not declared in mod.rs, not imported in lib.rs. Confirmed by grep.

 Risk: None.

 ---
 Phase 2: Tool System Upgrade

 2A: Enrich ToolCatalogEntry (runtime.rs)

 Add 4 fields to the struct:
 - search_hint: &'static str — short hint for tool_search results
 - aliases: &'static [&'static str] — e.g., ["Bash"] for execute_command
 - is_concurrency_safe: bool — can run in parallel
 - is_read_only: bool — does not mutate workspace

 Populate for all 12 entries. Conservative rules:
 - Read-only + concurrency-safe: file_read, read_file, list_files, search_files, web_search, fetch_url, tool_search
 - Serial-only: file_write, write_file, file_edit, save_reference, execute_command
 - Special: start_background_task (concurrency-safe but not read-only)

 2B: Upgrade tool_search Scoring (runtime.rs)

 Replace current scoring in ToolSearchExecutor::execute with claude-code's algorithm:
 - Exact name match: 10 pts, partial: 5 pts, alias: 5 pts, search_hint: 4 pts, keyword: 2 pts, description: 2 pts
 - CamelCase tokenization (split by uppercase)
 - select:tool_name syntax for direct selection (comma-separated multi-select)
 - Required terms with + prefix
 - Dedup: shorter name wins on tie; canonical form over alias

 2C: Concurrent Tool Execution (chat.rs + new executor.rs)

 In the tool loop (chat.rs ~lines 308-355), replace sequential for-loop:
 1. Partition tool calls by is_concurrency_safe
 2. Run safe tools concurrently via FuturesUnordered
 3. Run unsafe tools sequentially (they may depend on prior writes)
 4. Emit results in original tool call order

 Add futures crate to Cargo.toml if not already present.

 ---
 Phase 3: Context Management Overhaul

 3A: Accurate Token Counting (chat.rs)

 Replace estimate_tokens() chars/1.3 heuristic with tiktoken-rs cl100k_base tokenizer. Keep chars/1.3 as fallback. Gate behind #[cfg]
 feature for Windows compatibility.

 3B: API Round Grouping (chat.rs)

 Add ConversationRound struct to group user→assistant→tool_results sequences. Build rounds from the flat message list.

 3C: Three-Tier Compaction (new compaction.rs)

 1. Time-based: Evict tool results from rounds older than 60 min (keep last 3 rounds intact)
 2. Session memory: After 5+ rounds, insert synthetic system message summarizing rounds 1 through N-3
 3. LLM summary: Full collapse when tier 1+2 insufficient; summarize all but last 2 rounds via one extra API call

 ---
 Phase 4: Permission Rules System

 4A: Rule Data Structures (new permissions.rs)

 String-based rules: "Bash(git reset *)" format with wildcard matching.
 - PermissionConfig with deny_list, ask_list, allow_list, mode
 - DenialTracker for circuit breaker
 - Persist as permissions.json in Tauri app data dir

 4B: Decision Pipeline

 Layered evaluation: DenyList → AskList → ToolCheck → Mode Default → AllowList.
 Pattern matching: "Bash(git *)" matches "Bash(git status)".

 4C: Denial Tracking

 Circuit breaker: 3 consecutive or 10 total denials → lock. Reset on allow/ask.

 4D: Interactive Permission Prompts

 When decision is Ask, emit chat:permission_request event. Frontend shows dialog. Response via invoke("resolve_permission") → oneshot
 channel in backend. 30-second timeout, defaults to Deny.

 ---
 Phase 5: Frontend Data Flow

 5A: Emit Thinking Events (chat.rs + chat-panel.tsx)

 Parse reasoning_content from SSE delta chunks. Emit chat:thinking events. Frontend renders in existing ThinkingStrip component.
 Message.thinking field already exists but is never populated.

 5B: Emit Token Usage (chat.rs + chat-panel.tsx)

 Extract usage from final SSE chunk. Emit chat:token_usage event. Frontend displays in chat footer.

 5C: Stop Generation Button (chat.rs + chat-panel.tsx)

 New stop_generation Tauri command + StopFlags managed state (Mutex<HashSet<String>>). Streaming loop checks flag each chunk. Frontend
 button visible during streaming.

 ---
 Phase 6: Session Persistence Upgrade

 6A: Extend SessionMessage (session.rs)

 Add tool_calls: Option<Vec<PersistedToolCall>> and tool_call_id: Option<String> to SessionMessage. All new fields are Option with
 #[serde(default)] for backward compat.

 6B: Update history() (session.rs)

 Return actual tool_calls and tool_call_id instead of hardcoded None.

 6C: Auto-Generate Session Titles (session.rs)

 When session name is still "New Chat" and first user message arrives, truncate to 50 chars as title.

 ---
 Phase 7: Wiring (mod.rs + lib.rs + Cargo.toml)

 - mod.rs: Add pub mod compaction;, pub mod executor;, pub mod permissions;
 - lib.rs: Register new commands (stop_generation, permission CRUD, resolve_permission_request), new managed states (PermissionGate,
 StopFlags)
 - Cargo.toml: Add tiktoken-rs, futures, tokio (verify sync feature)

 ---
 Dependency Graph

 Phase 1 ──> Phase 2A ──> Phase 2B
               │
               └──> Phase 2C
               │
 Phase 3A ──> Phase 3C
 Phase 3B ──> Phase 3C
               │
 Phase 4A ──> Phase 4B ──> Phase 4C ──> Phase 4D
               │
 Phase 5A/B/C (independent, can run parallel to Phase 4)
               │
 Phase 6A ──> Phase 6B
 Phase 6C (independent)
               │
 Phase 7 (after all above)

 Recommended order: 1 → 2A → 2B/2C → 3A/3B → 3C → 4A/4B/4C/4D → 5A/5B/5C → 6A/6B/6C → 7

 Phase 5 can run in parallel with Phase 4.

 ---
 Critical Files

 ┌───────────────────────────────────────┬───────────────────────────────────────────────────────────────────────────────────┐
 │                 File                  │                                      Changes                                      │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/ai/runtime.rs           │ Enrich ToolCatalogEntry, upgrade search scoring, add permission gate              │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/ai/chat.rs              │ Concurrent exec, token counting, thinking/token events, stop flag, round grouping │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/ai/session.rs           │ Extend SessionMessage, fix history(), auto-title                                  │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/ai/permissions.rs (NEW) │ Rule engine, denial tracker, interactive prompt channel                           │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/ai/compaction.rs (NEW)  │ Three-tier compaction, round grouping, LLM summarization                          │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/ai/executor.rs (NEW)    │ Concurrent tool execution with FuturesUnordered                                   │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/ai/mod.rs               │ Declare new modules                                                               │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/src/lib.rs                  │ Register new commands and managed state                                           │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ src-tauri/Cargo.toml                  │ Add tiktoken-rs, futures                                                          │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ components/chat/chat-panel.tsx        │ ThinkingStrip render, token display, stop button, permission dialog               │
 ├───────────────────────────────────────┼───────────────────────────────────────────────────────────────────────────────────┤
 │ lib/tauri-api.ts                      │ Add onChatThinking, onChatTokenUsage, onPermissionRequest listeners               │
 └───────────────────────────────────────┴───────────────────────────────────────────────────────────────────────────────────┘

 ---
 Risk Assessment

 ┌──────────────────────────────────────────────────┬──────────┬────────────────────────────────────────────────────────────────────────┐
 │                       Risk                       │ Severity │                               Mitigation                               │
 ├──────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────┤
 │ tiktoken-rs fails on Windows native build        │ Medium   │ Keep chars/1.3 fallback; gate behind feature flag                      │
 ├──────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────┤
 │ Concurrent tool exec causes file lock races on   │ Medium   │ Conservative allowlist — only read/search tools run in parallel        │
 │ Windows                                          │          │                                                                        │
 ├──────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────┤
 │ Permission prompt channel timeout (frontend      │ Low      │ 30s timeout → default Deny                                             │
 │ unresponsive)                                    │          │                                                                        │
 ├──────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────┤
 │ Old session JSON breaks on new fields            │ Low      │ All new fields are Option with #[serde(default)]                       │
 ├──────────────────────────────────────────────────┼──────────┼────────────────────────────────────────────────────────────────────────┤
 │ SSE chunk format differs across model providers  │ Medium   │ Graceful parse — handle missing fields, both reasoning_content and     │
 │                                                  │          │ standard formats                                                       │
 └──────────────────────────────────────────────────┴──────────┴────────────────────────────────────────────────────────────────────────┘

 ---
 Verification

 1. Build: cargo build in src-tauri/ — must compile with new modules and dependencies
 2. Unit tests: cargo test — all existing tests pass
 3. Manual smoke test:
   - Launch app, open chat, send "List files in the workspace" — verify tool_search returns results with new scoring
   - Send "Read file1.txt and file2.txt" — verify both reads execute concurrently (check timing in logs)
   - Send 15+ messages to trigger compaction — verify context stays within limits
   - Change permission mode to AcceptEdit, send a write command — verify permission dialog appears
   - Click Stop during generation — verify streaming stops
   - Close and reopen app — verify session restores with tool calls intact
 4. Frontend: npm run dev — no TypeScript errors, new event listeners work