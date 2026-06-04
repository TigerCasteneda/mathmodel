# Phase 9: Tauri Native AI — Design Spec

**Date**: 2026-06-04
**Status**: approved
**Scope**: Replace Agent/PTY bridge with embedded claude_code_rs, rebuild UI layout, add AI-driven research pipeline, remove AI relay station

---

## 1. Architecture Overview

```
┌─ Tauri Desktop App ──────────────────────────────────────────────────────┐
│                                                                          │
│  ┌─ WebView (Next.js static export) ─────────────────────────────────┐  │
│  │  ActivityBar │ Sidebar │ Tabbed Main Area                          │  │
│  │  Icons       │ Files   │ ┌─ model.py ──┬─ 💬 Chat ────────────┐   │  │
│  │  📁 Explorer │ / Chat  │ │ Monaco      │ Message bubbles     │   │  │
│  │  🔍 Research │ / Lib   │ │ Editor      │ Tool call cards     │   │  │
│  │  💬 Chat     │         │ │             │ Input area          │   │  │
│  │  ⚙️ Settings │         │ └─────────────┴─────────────────────┘   │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│       │ invoke() / listen() events                                      │
│       ▼                                                                 │
│  ┌─ Tauri Rust Backend ─────────────────────────────────────────────┐  │
│  │                                                                    │  │
│  │  ┌─ claude_code_rs (library) ──────────────────────────────────┐  │  │
│  │  │  ApiClient  → user's own API Key → LLM provider             │  │  │
│  │  │  ToolRegistry                                               │  │  │
│  │  │    ├ web_search (SearXNG)                                   │  │  │
│  │  │    ├ fetch_url (Firecrawl)                                  │  │  │
│  │  │    ├ context7_docs (library docs)                           │  │  │
│  │  │    ├ file_read / file_write / file_edit                     │  │  │
│  │  │    ├ save_reference (→ research_items + local .md)          │  │  │
│  │  │    └ execute_code (Docker compute)                          │  │  │
│  │  └────────────────────────────────────────────────────────────┘  │  │
│  │                                                                    │  │
│  │  ┌─ Agent Services ────────────────────────────────────────────┐  │  │
│  │  │  file_watcher  → notify-based file monitoring                │  │  │
│  │  │  commands      → Tauri invoke handlers (list/read/create)    │  │  │
│  │  │  events        → Tauri emit (file_change, file_tree)         │  │  │
│  │  │  state         → AgentState (work_dir, api_key, etc.)        │  │  │
│  │  └────────────────────────────────────────────────────────────┘  │  │
│  │                                                                    │  │
│  │  ┌─ Host Mode (optional) ───────────────────────────────────────┐  │  │
│  │  │  Embedded Axum Server (modeler_server)                       │  │  │
│  │  │  Listen: 0.0.0.0:{port} (LAN accessible)                    │  │  │
│  │  │  SQLite at {work_dir}/.modeler-data/modeler.db               │  │  │
│  │  └────────────────────────────────────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────┘
         │ SearXNG :8080          │ Firecrawl API          │ LLM API
         ▼                        ▼                        ▼
   ┌──────────┐           ┌──────────────┐          ┌──────────┐
   │ SearXNG  │           │  Firecrawl    │          │ Anthropic│
   │ (Docker) │           │  (cloud API)  │          │ OpenAI   │
   └──────────┘           └──────────────┘          │ etc.     │
                                                    └──────────┘
```

### Key Data Flows

1. **Chat flow**: WebView input → `invoke('chat', {message})` → Tauri Rust → claude_code_rs → API → stream chunks → `emit('chat:stream')` → WebView renders
2. **Tool execution**: LLM returns tool_call → Tauri Rust executes locally (file ops, search, reference save) → result sent back to LLM → final response streamed
3. **File sync**: Host opens local folder → file_watcher detects changes → pushes to server → CRDT sync → teammates' file trees update
4. **Research save**: AI calls save_reference → Host writes `references/{slug}.md` → watcher → server; teammates call `/research/items` → server writes files table
5. **Collaboration**: Host starts server → `0.0.0.0:{port}` → teammates connect via LAN IP + JWT

---

## 2. Module Changes

### 2.1 Deleted

| Path | Reason |
|------|--------|
| `agent/` (entire directory) | Replaced by Tauri + claude_code_rs |
| `server/src/ai/` (entire module) | AI relay no longer needed; each user brings own API key |
| `server/src/morphic/` (entire module) | Morphic middleware removed; SearXNG called directly |
| `server/migrations/002_ai.sql` | ai_usage_logs, channels tables no longer needed |
| `server/src/agent_bridge/` (entire module) | Agent WebSocket bridge retired |
| `src-tauri/src/agent/pty.rs` | PTY shell spawn retired |
| `src-tauri/src/agent/state.rs` | Simplified — remove pty_tx, add claude_code_rs state |
| `components/dashboard/code-canvas.tsx` | Refactored into new layout (see Section 4) |

### 2.2 Added

| Path | Purpose |
|------|---------|
| `src-tauri/src/ai/` | Tauri-side AI module |
| `src-tauri/src/ai/mod.rs` | Re-exports |
| `src-tauri/src/ai/chat.rs` | Chat session management: stream_handler, tool_executor |
| `src-tauri/src/ai/tools.rs` | Custom tool definitions: web_search, fetch_url, context7, save_reference, execute_code |
| `src-tauri/src/ai/session.rs` | Conversation history per project, message storage |
| `src-tauri/src/ai/config.rs` | Per-user API key management (in-memory, from WebView) |
| `components/layout/` | New UI layout: ActivityBar, Sidebar, MainArea |
| `components/chat/` | Chat UI: ChatPanel, MessageBubble, ToolCallCard, ThinkingPanel, InputArea |
| `components/editor/` | Monaco wrapper (from code-canvas), PDF viewer |
| `components/research/` | Research panel (from main-workspace refactor) |
| `server/src/search/` | SearXNG + Firecrawl HTTP clients (thin adaptors) |

### 2.3 Modified

| Path | Change |
|------|--------|
| `server/src/main.rs` | Remove ai, morphic, agent_bridge modules; add search module |
| `server/src/lib.rs` | Remove ai, morphic, agent_bridge re-exports; add search |
| `server/src/error.rs` | Remove ServiceUnavailable variant (no longer needed) |
| `server/src/research/handlers.rs` | Remove search endpoint (now in Tauri); simplify to CRUD only |
| `src-tauri/Cargo.toml` | Remove portable-pty; add claude_code_rs dep |
| `src-tauri/src/lib.rs` | Replace AgentState with AI state; remove PTY commands; add chat commands |
| `src-tauri/src/agent/commands.rs` | Remove pty_spawn/write/resize/kill; add ai_chat, save_reference, open_folder |
| `src-tauri/src/agent/events.rs` | Add chat:stream, chat:thinking, chat:tool_call events |

---

## 3. UI Layout

### 3.1 Overall Structure

```
┌── Activity Bar (48px) ──┬── Sidebar (260px, resizable) ──┬── Main Area ───────────────┐
│                          │                                │                            │
│  📁 Explorer             │  ┌─ File Tree ─────────────┐  │  ┌─ Tab Bar ─────────────┐ │
│  🔍 Research             │  │ model.py                │  │  │ model.py × │ 💬 Chat  │ │
│  💬 Chat                 │  │ references/             │  │  ├────────────────────────┤ │
│  ⚙️ Settings             │  │  └ bayesian-sir.md      │  │  │                        │ │
│                          │  │ paper.tex               │  │  │  Monaco Editor         │ │
│                          │  │ data/                   │  │  │  OR                    │ │
│                          │  └─────────────────────────┘  │  │  Chat Panel            │ │
│                          │                                │  │  (tab-switched)        │ │
│                          │  (alt: Chat conversations     │  │                        │ │
│                          │   or Research Library         │  │                        │ │
│                          │   depending on active icon)   │  │                        │ │
│                          │                                │  │                        │ │
├──────────────────────────┴────────────────────────────────┴────────────────────────────┤
├── Status Bar (28px) ──────────────────────────────────────────────────────────────────┤
│  ● Connected  │  Python 3.11  │  main  │  Ln 42, Col 15  │  UTF-8  │  Host: 🟢       │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

### 3.2 Activity Bar Icons

| Icon | Label | Active Sidebar Content |
|------|-------|----------------------|
| 📁 | Explorer | File tree (local or cloud, depending on role) |
| 🔍 | Research | Research Library (saved references) |
| 💬 | Chat | Chat conversation list + new chat button |
| ⚙️ | Settings | API Key, server URL, theme, work dir |

### 3.3 Tab Types

- **Code tabs**: Monaco editor for `.py/.js/.ts/.tsx/.json/.geojson/.html/.md/.tex/.txt`
- **PDF tab**: `react-pdf` viewer for `.pdf` files
- **Chat tab**: Message bubbles + tool call cards + input area (Claude Orange theme)
- **Research tab**: (optional inline) saved reference detail view

### 3.4 Color Scheme (Claude Orange Dark)

```css
/* From claude-code-rust/src/gui/theme.rs */
--bg-darkest:     #0D0D0D;   /* deepest background */
--bg-darker:      #121212;   
--bg-dark:        #1A1A1A;   /* panels */
--bg-surface:     #232323;   /* cards, inputs */
--bg-elevated:    #2D2D2D;   /* hover states */
--border:         #373737;   /* default borders */
--border-light:   #464646;   /* active borders */
--text-primary:   #E8E8E8;   /* body text */
--text-secondary: #B4B4B4;   /* subtitles */
--text-muted:     #787878;   /* hints */
--accent:         #D4A574;   /* Claude Orange */
--accent-dark:    #B4875A;   /* pressed state */
--accent-light:   #EBC396;   /* hover highlight */
--code-bg:        #1E1E1E;   /* code blocks */
--success:        #4CAF50;
--warning:        #FF9800;
--error:          #F44336;
--info:           #64B5F6;
```

### 3.5 Chat UI Components

**Message bubble (user)**:
- Background: Claude Orange (`#D4A574`)
- Text: White
- Border-radius: 16px
- Right-aligned
- Avatar: user initials circle (grey, 32px)

**Message bubble (assistant)**:
- Background: `--bg-surface`
- Text: `--text-primary`
- Left-aligned with 🦀 avatar
- Thinking section: collapsible (▶/▼ toggle), muted italics text
- Code blocks: `--code-bg` background, orange inline code
- Tool call cards: colored left border + icon + status indicator

**Input area**:
- Background: `--bg-surface`
- Border-radius: 16px
- Multiline textarea with placeholder "Message Modeler AI..."
- Shift+Enter for newline, Enter to send
- Send button: orange circle with ➤ icon

**Welcome banner** (first load):
- 🦀 logo + "Modeler AI" title
- Capability cards (Search, Code, Reference, Compute)

---

## 4. AI Integration

### 4.1 claude_code_rs Library Integration

The `claude_code_rs` crate is added as a dependency to `src-tauri/Cargo.toml`:

```toml
[dependencies]
claude_code_rs = { path = "../claude-code-rust" }
```

Key used components:
- `claude_code_rs::api::ApiClient` — HTTP client for chat completions
- `claude_code_rs::api::ChatMessage` — message types
- `claude_code_rs::tools::Tool` — tool trait
- `claude_code_rs::tools::ToolRegistry` — tool registration
- `claude_code_rs::tools::ToolOutput` — tool execution results

### 4.2 Custom Tools for Modeler AI

Each tool implements the `Tool` trait from claude_code_rs.

```rust
// src-tauri/src/ai/tools.rs

pub struct WebSearchTool;      // SearXNG search
pub struct FetchUrlTool;       // Firecrawl content extraction
pub struct Context7Tool;       // Library documentation lookup
pub struct SaveReferenceTool;  // Save to research_items + local .md
pub struct ExecuteCodeTool;    // Docker compute execution
pub struct ReadFileTool;       // Read workspace file
pub struct WriteFileTool;      // Write/overwrite workspace file
pub struct EditFileTool;       // Targeted string replacement in file
pub struct ListFilesTool;      // List project files
```

**WebSearchTool**:
```
Input:  { query: string, max_results?: number }
Output: { results: [{ title, url, snippet }] }
Calls:  GET http://localhost:8080/search?q={query}&format=json&categories=general
```

**FetchUrlTool**:
```
Input:  { url: string }
Output: { title, content: markdown, source_url }
Calls:  Firecrawl API POST /v1/scrape
```

**SaveReferenceTool**:
```
Input:  { title, url, summary, category, methodology?, parameters? }
Output: { item_id, file_path }
Action:
  - If Host: write references/{slug}.md to local workspace → watcher → server
  - If Guest: POST /research/items → server writes files table
  - Always: insert into research_items table
```

### 4.3 Chat Session Flow

```
1. User types message in Chat tab
2. WebView → invoke('ai_chat', { message, conversation_id })
3. Tauri Rust:
   a. Load conversation history from session store
   b. Build messages array with system prompt (context: file tree, research items, project info)
   c. Call ApiClient::chat_stream(messages, tools)
   d. Stream chunks → emit('chat:stream', { content, done })
   e. If tool_call detected:
      - emit('chat:tool_call', { name, input, status: 'running' })
      - Execute tool locally
      - emit('chat:tool_call', { name, status: 'success', output })
      - Send tool result back to LLM
      - Continue streaming
4. WebView renders:
   - chat:stream → append text to assistant bubble with blinking cursor
   - chat:tool_call → insert tool call card
   - final message saved to session
```

### 4.4 System Prompt

```
You are Modeler AI, a mathematical modeling assistant embedded in a collaborative
platform for MCM/ICM competition teams.

Current project context:
- Files: {list of key files in project}
- Research Library: {count} saved references covering {topics}
- Language: Python (numpy, scipy, sympy, matplotlib, pandas available)

You can:
- Search the web for papers, datasets, and methods
- Fetch full content from URLs
- Look up library documentation (numpy, scipy, sympy, etc.)
- Read, write, and edit files in the project
- Save useful references to the Research Library
- Execute Python code in the compute environment

Always provide mathematical reasoning, cite sources, and save valuable findings.
```

---

## 5. Collaboration Model

### 5.1 Roles

| Role | Has Local Folder | File Source | AI Saves To | Starts Server |
|------|-----------------|-------------|-------------|---------------|
| **Host** | Yes | Local disk → watcher → server | Local `references/` → watcher → server | Yes |
| **Guest** | No | Server files table (CRDT sync) | `POST /research/items` → server | No |

### 5.2 Connection Flow

```
Host:
  1. Launch app
  2. Open local project folder (Tauri file dialog)
  3. App auto-starts embedded server on port 3001
  4. Status bar shows LAN IP: "Host: 192.168.1.5:3001"

Guest:
  1. Launch app
  2. Settings → Server URL → enter Host's IP (192.168.1.5:3001)
  3. Login with account credentials
  4. Connected — file tree loads from server, no local folder needed
```

### 5.3 Permissions

- **All authenticated users**: read/write files, save research items, use AI chat
- **Server auth**: JWT-based, same as current Phase 1-7 authentication
- **API Keys**: each user configures their own in Settings; keys stored in-memory only (Tauri Rust state), never persisted to disk

---

## 6. Reference Management

### 6.1 Save Flow

**Host path**:
```
AI decides to save a reference
  ↓
SaveReferenceTool.execute()
  ↓
1. Write references/{slug}.md to local workspace
2. file_watcher detects → pushes FileChange → server → CRDT sync
3. INSERT into research_items table (via server /research/items)
  ↓
All teammates see:
  - File tree: references/bayesian-sir-estimation.md
  - Research Library: new entry with title, summary, category
```

**Guest path**:
```
AI decides to save a reference
  ↓
SaveReferenceTool.execute()
  ↓
1. POST /research/items → server
2. Server writes:
   - research_items row
   - files table entry (cloud .md file)
   - crdt_docs entry with markdown content
  ↓
All teammates see:
  - File tree: references/bayesian-sir-estimation.md
  - Research Library: new entry with title, summary, category
```

### 6.2 Reference File Template

```markdown
# {{title}}
- **URL**: {{url}}
- **Category**: {{category_label}}
- **Authors**: {{authors}}
- **Year**: {{publish_year}}
- **Keywords**: {{keywords}}
- **Saved**: {{date}}

## AI Summary
{{ai_generated_summary}}

## Methodology
{{extracted_methodology}}

## Key Parameters
{{extracted_parameters}}

## Relevance to Project
{{ai_generated_relevance}}

## Notes
<!-- Add your notes here -->
```

### 6.3 Research Items Table

**Existing** (from Phase 7a 005 migration):
- `id`, `project_id`, `created_by`, `source`, `category`, `url`, `title`, `summary`, `authors`, `publish_year`, `keywords`, `notes`, `relevance_score`, `cloud_file_id`, `raw_json`, `created_at`, `updated_at`

**New columns** (Phase 9, 006 migration):
- `methodology TEXT DEFAULT ''` — extracted methodology tags
- `key_parameters TEXT DEFAULT ''` — extracted parameter values
- `ai_relevance TEXT DEFAULT ''` — AI-generated relevance analysis

Research items CRUD endpoints (`/research/items`) remain unchanged from Phase 7a.

---

## 7. Search Pipeline

### 7.1 Architecture

```
claude_code_rs (LLM decides to search)
  ↓ tool_call: web_search(query)
Tauri Rust: WebSearchTool.execute()
  ↓
GET http://localhost:8080/search?q={query}&format=json&engines=google,bing,wikipedia
  ↓ SearXNG (Docker, localhost:8080)
Returns: { results: [{title, url, content(snippet)}] }
  ↓ LLM reads snippets, decides which URLs to open
tool_call: fetch_url(url)
  ↓
Tauri Rust: FetchUrlTool.execute()
  ↓
POST https://api.firecrawl.dev/v1/scrape { url, formats: ["markdown"] }
  ↓
Returns: { markdown: "...", title: "...", ... }
  ↓ LLM reads full content, decides to save or ignore
tool_call: save_reference(...)
```

### 7.2 SearXNG Deployment

Morphic's `docker-compose.yaml` already provisions SearXNG. For Modeler AI:
- SearXNG runs on `localhost:8080`
- Config file at `morphic/searxng-settings.yml`
- Engines: `google,bing,duckduckgo,wikipedia`
- Format: JSON

### 7.3 Firecrawl Integration

- API endpoint: `https://api.firecrawl.dev/v1/scrape`
- Requires API key: stored per-user (same as LLM API key)
- Returns clean markdown from any URL
- Handles PDFs, JS-rendered pages, academic paywalls

### 7.4 Context7 Integration

- API: `https://context7.com/api/v0/resolve`
- Purpose: Look up latest library docs (numpy, scipy, sympy, matplotlib, pandas)
- Triggered by: AI detecting code asking for API usage help

---

## 8. Build and Distribution

### 8.1 Development

```bash
npm run dev          # Next.js on :3000 (HMR)
cargo tauri dev      # Tauri loads from :3000, Rust HMR via cargo watch
```

### 8.2 Production Build

```bash
npm run build        # Next.js static export → out/
cargo tauri build    # Tauri builds .exe, bundles out/ as WebView resources
```

### 8.3 Windows Requirements

- Windows 11 (WebView2 pre-installed)
- NSIS installer generated by Tauri bundler
- Single `.exe` output: `modeler-ai_0.1.0_x64-setup.exe`

### 8.4 What Users Need

| Requirement | Host | Guest |
|-------------|------|-------|
| Modeler AI .exe | Install | Install |
| API Key (Anthropic/OpenAI/etc.) | Enter in Settings | Enter in Settings |
| Firecrawl API Key | Enter in Settings | Enter in Settings |
| SearXNG Docker | Host only (auto-started) | Not needed |
| Local project folder | Open via file dialog | Not needed |

---

## 9. What Gets Removed

| Component | Path(s) | Reason |
|-----------|---------|--------|
| Agent CLI | `agent/` (entire directory) | Replaced by Tauri + claude_code_rs |
| AI Relay Station | `server/src/ai/` (entire module) | Users bring own API keys |
| Morphic Client | `server/src/morphic/` (entire module) | SearXNG called directly |
| Agent Bridge | `server/src/agent_bridge/` (entire module) | No more Agent WS protocol |
| AI Usage Logs | `server/migrations/002_ai.sql` | No more centralized AI relay |
| PTY Manager | `src-tauri/src/agent/pty.rs` | No shell bridging |
| PTY Commands | `src-tauri/src/agent/commands.rs` (pty_spawn/write/resize/kill) | Retired |
| Tabbit | `agent/src/tabbit.rs` | Not ported; can be added later via Tauri plugin |
| xterm.js | Frontend terminal integration | Replaced by Chat UI |
| Morphic Stack | `docker-compose.yaml` (Morphic's PG+Redis+Morphic containers) | Only SearXNG needed |

---

## 10. New Migration

### `server/migrations/006_research_v3.sql`

```sql
-- 006: Extend research_items with AI-generated analysis columns
-- Executed via ensure_column() in db.rs (idempotent)

-- methodology: extracted methodology tags (comma-separated: "MCMC, Bayesian, ODE")
-- ALTER TABLE research_items ADD COLUMN methodology TEXT DEFAULT '';

-- key_parameters: extracted parameter values as JSON
-- ALTER TABLE research_items ADD COLUMN key_parameters TEXT DEFAULT '';

-- ai_relevance: AI-generated analysis of relevance to current project
-- ALTER TABLE research_items ADD COLUMN ai_relevance TEXT DEFAULT '';
```

---

## 11. Implementation Phases

### Phase 9a: Cleanup
- Delete `agent/`, `server/src/ai/`, `server/src/morphic/`, `server/src/agent_bridge/`
- Remove `server/migrations/002_ai.sql`
- Remove PTY code from `src-tauri/src/agent/`
- Update `server/src/main.rs` and `server/src/lib.rs` module declarations
- Verify server compiles

### Phase 9b: Tauri AI Core
- Add `claude_code_rs` as dependency
- Create `src-tauri/src/ai/` module (chat, tools, session, config)
- Implement custom tools (web_search, fetch_url, context7, save_reference)
- Wire chat command + streaming events
- Wire per-user API key management

### Phase 9c: SearXNG + Firecrawl Integration
- Create `server/src/search/` module (SearXNG client)
- Add Firecrawl HTTP client to Tauri tools
- Verify end-to-end search pipeline

### Phase 9d: New UI Layout
- Create `components/layout/` (ActivityBar, Sidebar, MainArea)
- Create `components/chat/` (ChatPanel, MessageBubble, ToolCallCard, InputArea)
- Refactor `components/editor/` from code-canvas.tsx
- Add PDF viewer component
- Apply Claude Orange color scheme to Tailwind config
- Wire Tauri invoke/listen for AI chat

### Phase 9e: Reference Save Flow
- Implement dual-path SaveReferenceTool (Host local + Guest server)
- Add 006 research migration
- Update Research Library UI to show AI-generated fields

### Phase 9f: Host Mode + Collaboration
- Server-side: embed Axum server, bind to `0.0.0.0`
- Frontend: connection settings UI, host status display
- File sync via existing CRDT infrastructure

### Phase 9g: Build + Installer
- Resolve Defender exclusion
- `npm run build` + `cargo tauri build`
- NSIS installer generation
- Test on clean Windows

### Phase 9h: Polish
- Replace Claude Orange with custom Modeler accent (optional)
- Welcome banner
- PDF viewer refinements
- Context7 integration
