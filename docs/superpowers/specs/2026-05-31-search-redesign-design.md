# Phase 7a: Search Module Redesign — Design Spec

**Date**: 2026-05-31
**Status**: approved (revised per review)
**Scope**: Replace Tavily with Morphic + SearXNG, build research pipeline, wire up frontend
**Implementation order**: 7a-0 → 7a-1 → 7a-2 → 7a-3 → 7a-4 → 7a-5

---

## 1. Architecture Overview

```
┌─ Browser ──────────────────────────────────────────────────────┐
│  MainWorkspace                                                 │
│  ┌─ Search Tab ─────────────┐  ┌─ Research Library Tab ──────┐ │
│  │ query + type selector    │  │ list / filter / delete       │ │
│  │ results + AI summary     │  │ view details / edit notes    │ │
│  │ [Save Selected] → Agent? │  │                              │ │
│  └──────────────────────────┘  └──────────────────────────────┘ │
└────────────────────────────────────────────────────────────────┘
         │ HTTP                          │ WebSocket (existing)
         ▼                              ▼
┌─ Rust Axum :3001 ──────────────────────────────────────────────┐
│                                                                 │
│  /research/search                   agent_bridge/              │
│       │                                  │                     │
│       ▼                                  │  create_file msg    │
│  ┌─ morphic/ ──HTTP──▶ Morphic :3002     │  → Agent           │
│  │  client.rs          (Next.js)         ▼                     │
│  │  model.rs           /api/chat      ┌─ Agent (local) ──┐    │
│  └──────────────────── /api/adv-search│  create .md file   │    │
│                        │              │  file_watcher →    │    │
│  /research/items CRUD  │              │  sync back to DB   │    │
│       │                │              └───────────────────┘    │
│       ▼                ▼                                       │
│  ┌─ SQLite ───────────────────────────────────────────────┐   │
│  │  research_items (extended)    files + file_blobs        │   │
│  │  research_context_pages                                 │   │
│  └─────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘

┌─ Docker: morphic-stack ────────────────────────────────────────┐
│  morphic:3002  ← Next.js                                       │
│  searxng:8080  ← Meta search engine                            │
│  postgres:5432 ← Morphic chat history (required, unused by us)  │
│  redis:6379    ← Search cache                                  │
└─────────────────────────────────────────────────────────────────┘
```

### Key data flows

1. **Search flow**: Frontend → `/research/search` → `morphic::client` → Morphic `/api/chat` (AI summary) + `/api/advanced-search` (structured results) → merged response to frontend
2. **Save flow**: Frontend selected items → `POST /research/items` → write SQLite (research_items + files/file_blobs for cloud copy) → optionally send `create_file` to Agent → Agent creates local `references/*.md` as additive sync. Cloud .md file always written server-side via existing file API; Agent local file is best-effort.
3. **Browse flow**: Research Library Tab → `GET /research/items?project_id=` → display by type/time → edit notes / delete

---

## 2. Module Structure

```
server/src/
├── morphic/                    # NEW: Morphic HTTP client module
│   ├── mod.rs
│   ├── client.rs               # chat() + advanced_search() HTTP calls
│   └── model.rs                # MorphicResponse, SearchResult, etc.
├── research/                   # NEW: Research pipeline module
│   ├── mod.rs
│   ├── model.rs                # ResearchItem, Create/Update requests
│   ├── handlers.rs             # /research/search, /research/items CRUD
│   └── references.rs           # .md template rendering + Agent file creation
├── ai/                         # Keep: LLM relay (unchanged)
│   └── adaptor/
│       └── tavily.rs           # DELETE
├── main.rs                     # Add morphic + research modules, mount routes
└── migrations/
    └── 005_research_v2.sql     # NEW: extend research_items table
```

Design rationale:
- `morphic/` focuses solely on HTTP communication with the Morphic service — independently testable, replaceable
- `research/` focuses on business logic: search → store → generate files — doesn't care which search backend
- `ai/` stays clean, only handles LLM relay
- Tavily adaptor removed

---

## 3. Data Model

### Migration `005_research_v2.sql`

**Idempotency strategy**: The current migration runner (`server/src/db.rs:22`) runs all SQL files on every startup. To make ALTER TABLE safe, add an `ensure_column(table, column, type_sql)` helper in `db.rs` that uses `PRAGMA table_info` to check column existence before each ALTER:

```rust
// db.rs — new helper
async fn ensure_column(pool: &SqlitePool, table: &str, column: &str, type_sql: &str) -> Result<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2"
    )
    .bind(table).bind(column)
    .fetch_one(pool).await?;
    if count == 0 {
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {type_sql}");
        sqlx::query(&sql).execute(pool).await?;
    }
    Ok(())
}
```

This way 004 (original table) already created the table, Tabbit inserts still work, and 005 idempotently extends it. Each ALTER is guarded by PRAGMA check:

```sql
-- Executed via ensure_column(), safe to run multiple times:
-- ALTER TABLE research_items ADD COLUMN category TEXT DEFAULT 'literature';
-- ALTER TABLE research_items ADD COLUMN authors TEXT DEFAULT '';
-- ALTER TABLE research_items ADD COLUMN publish_year INTEGER;
-- ALTER TABLE research_items ADD COLUMN keywords TEXT DEFAULT '';
-- ALTER TABLE research_items ADD COLUMN relevance_score REAL DEFAULT 0.0;
-- ALTER TABLE research_items ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
```

**Compatibility with Tabbit**: 005 only ADDs columns with defaults; existing `INSERT` statements in `agent_bridge/handlers.rs:298` are unaffected. Tabbit writes to `(id, project_id, created_by, source, url, title, summary, notes, raw_json, created_at)` — all unchanged, new columns get their default values for existing rows.

### Complete `research_items` table

| Column | Type | Description |
|--------|------|-------------|
| id | TEXT PK | UUID |
| project_id | TEXT FK | Project reference |
| created_by | TEXT FK | User who saved |
| source | TEXT | `'morphic'` |
| category | TEXT | `literature` / `dataset` / `code` / `formula` / `competition` |
| url | TEXT | Original source URL |
| title | TEXT | Item title |
| summary | TEXT | AI-generated summary |
| authors | TEXT | Comma-separated authors |
| publish_year | INTEGER | Year of publication |
| keywords | TEXT | Comma-separated keywords |
| notes | TEXT | User-editable notes |
| relevance_score | REAL | Relevance from Morphic |
| raw_json | TEXT | Full raw response JSON |
| created_at | INTEGER | Unix timestamp |
| updated_at | INTEGER | Unix timestamp |

### `research_context_pages` (unchanged)

Stores crawled page content for each research item.

### Rust structs (`server/src/research/model.rs`)

```rust
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct ResearchItem {
    pub id: String,
    pub project_id: String,
    pub created_by: String,
    pub source: String,
    pub category: String,
    pub url: String,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: Option<String>,
    pub notes: Option<String>,
    pub relevance_score: f64,
    pub raw_json: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub project_id: String,
    pub query: String,
    pub category: String,
    pub max_results: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub ai_summary: String,
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Serialize)]
pub struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub content: String,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: Option<String>,
    pub relevance_score: f64,
}

#[derive(Debug, Deserialize)]
pub struct SaveItemsRequest {
    pub project_id: String,
    pub items: Vec<SaveItemInput>,
}

#[derive(Debug, Deserialize)]
pub struct SaveItemInput {
    pub title: String,
    pub url: String,
    pub content: String,
    pub category: String,
    pub summary: String,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: Option<String>,
    pub relevance_score: f64,
    pub raw_json: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct SaveItemsResponse {
    pub saved: i32,
    pub items: Vec<ResearchItem>,
    pub files_created: i32,
}

#[derive(Debug, Deserialize)]
pub struct UpdateItemRequest {
    pub notes: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ListItemsQuery {
    pub project_id: String,
    pub category: Option<String>,
    pub sort: Option<String>,
    pub order: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}
```

### `references/*.md` template

```markdown
# {{title}}
- **URL**: {{url}}
- **Category**: {{category_label}}
- **Authors**: {{authors}}
- **Year**: {{publish_year}}
- **Keywords**: {{keywords}}
- **Saved**: {{date}}

## Abstract
{{summary}}

## Notes
<!-- Add your notes here -->
```

---

## 4. API Design

All endpoints under `/research`, require Bearer token + project membership.

### `POST /research/search`

Search with full Morphic pipeline.

```
Request:  { project_id, query, category, max_results? }
Response: { ai_summary: string, results: SearchResultItem[] }
```

Backend flow:
1. Call `morphic::client::chat(query, category)` → Morphic `/api/chat` → AI summary text
2. Call `morphic::client::advanced_search(query, max_results)` → Morphic `/api/advanced-search` → structured results
3. Merge and return

### `POST /research/items`

Save selected search results.

```
Request:  { project_id, items: SaveItemInput[] }
Response: { saved: int, items: ResearchItem[], files_created: int }
```

Backend flow:
1. For each item: insert into `research_items` + `research_context_pages`
2. Generate `.md` content from template
3. Send `create_file` message to Agent via agent_bridge
4. If Agent not connected: `files_created = 0`, items still saved to DB

### `GET /research/items?project_id={id}&category=&sort=created_at&order=desc&limit=50&offset=0`

List saved items for a project.

### `GET /research/items/{id}`

Single item detail.

### `PATCH /research/items/{id}`

```
Request:  { notes?: string, category?: string }
```
Only notes and category are editable.

### `DELETE /research/items/{id}`

Hard delete item + associated context_pages. Agent-side `.md` file must be deleted manually.

### Morphic HTTP config

```env
# .env (server)
MORPHIC_BASE_URL=http://localhost:3002
MORPHIC_TIMEOUT_SECS=30
```

---

## 5. Morphic Client Module

### `server/src/morphic/model.rs`

Types matching Morphic API responses.

### `server/src/morphic/client.rs`

```rust
pub struct MorphicClient {
    base_url: String,
    http: reqwest::Client,
}

impl MorphicClient {
    /// Call /api/chat — returns AI research summary
    pub async fn chat(&self, query: &str, category: &str) -> Result<String, AppError>;

    /// Call /api/advanced-search — returns structured search results with crawling
    pub async fn advanced_search(&self, query: &str, max_results: i32) -> Result<Vec<SearchResultItem>, AppError>;
}
```

Error handling:
- Connection timeout (5s) → `503 "Search engine is not available"`
- Morphic non-200 → transparent error passthrough
- `/api/chat` succeeds but `/api/advanced-search` fails → AI summary returned, empty results array
- Morphic completely unavailable → error returned, other features unaffected

---

## 6. Agent WebSocket Integration

### New message type

```
Server → Agent:
{ "type": "create_file", "path": "references/optimal-control-sir.md", "content": "# ..." }
```

Added as `CreateFile { path: String, content: String }` variant to `AgentMessage` enum in both server bridge and agent `ws_client.rs`.

### Agent-side handling (`ws_client.rs`)

Path safety validation (reject before any filesystem operation):
1. Reject absolute paths (`path.starts_with('/')` or `path.starts_with('\\')` or has `C:` prefix on Windows)
2. Reject `..` path traversal: `path.contains("..")` → error
3. Resolve under `current_work_dir`: `work_dir.join(sanitized_path)`, canonicalize and verify it's still under work_dir
4. Auto-create parent directories: `fs::create_dir_all(parent)`
5. If file already exists: skip creation, log warning (don't overwrite user work)

```rust
fn validate_and_resolve(work_dir: &Path, relative_path: &str) -> Result<PathBuf, String> {
    if relative_path.contains("..") { return Err("path traversal rejected".into()); }
    if relative_path.starts_with('/') || relative_path.starts_with('\\') { return Err("absolute path rejected".into()); }
    let resolved = work_dir.join(relative_path);
    // Canonicalize work_dir and verify resolved starts with it
    let canon_work = work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf());
    let canon_resolved = resolved.canonicalize().unwrap_or(resolved.clone());
    if !canon_resolved.starts_with(&canon_work) { return Err("path escapes workspace".into()); }
    Ok(resolved)
}
```

### Agent status field consistency

Server-side `handle_frontend` sends either `"connected"` or `"disconnected"` (line 182).
**Fix**: Change server to send `"ready"` (when agent connected) or `"disconnected"`.
Frontend hook `useAgentStatus` only recognizes `"ready"` | `"disconnected"`.

### File creation: dual-write approach

When saving research items, the server does TWO things:
1. **Primary (always)**: Write `.md` file to cloud file tree via existing file API (`files` + `file_blobs` tables). This guarantees the file appears in the web editor for all collaborators regardless of Agent state.
2. **Secondary (best-effort)**: Send `create_file` to Agent to write a local copy. If Agent is disconnected, skip silently — the cloud copy already exists.

This avoids the optimistic "file_watcher syncs back" path as the primary mechanism.

---

## 7. Frontend Design

### Prerequisite: projectId passthrough

Currently `app/projects/[id]/page.tsx:21` renders `<MainWorkspace />` without `projectId`. All `/research/*` APIs require `project_id`. Fix: pass `projectId={id}` from page to MainWorkspace.

```tsx
// app/projects/[id]/page.tsx
<MainWorkspace projectId={id} />
```

### Component tree

```
MainWorkspace (refactored)
├── SearchTab
│   ├── SearchBar          ← input + type selector dropdown + search button
│   ├── AIAnalysis         ← Markdown-rendered AI summary
│   ├── ResultList         ← search result cards
│   │   └── ResultCard     ← title / URL / snippet / checkbox
│   └── SaveBar            ← sticky bottom bar: "Save X Selected"
│
└── ResearchLibraryTab
    ├── FilterBar          ← type filter + sort
    ├── ItemList           ← saved items
    │   └── ItemCard       ← title / type badge / date / actions
    └── ItemDetail         ← expand/modal: detail view + editable notes + delete
```

### Search flow

1. User enters query + selects type → clicks Search
2. Loading skeleton shown (AI analysis area + result list area)
3. Response received → AI analysis renders Markdown, results render as cards
4. User selects results → SaveBar shows "Save X Selected"
5. Click SaveBar:
   - Frontend calls `POST /research/items`
   - Success: toast "Saved X items"
   - If `files_created < saved`: extra toast "Agent not connected — files not created locally"
6. Selection cleared, results remain (can continue selecting)

### Research Library browsing

1. Switch to Library Tab → `GET /research/items?project_id=X`
2. FilterBar: type dropdown multi-select + sort by time
3. Card list: title, colored type badge, date, notes preview
4. Click card → expand/modal with detail, editable notes field
5. Delete: confirm dialog → `DELETE /research/items/{id}`

### Type selector options

```
📄 Literature    → literature
📊 Dataset       → dataset
🧮 Code          → code
📐 Formula       → formula
🏆 Competition   → competition
```

### Agent status hook

```typescript
// hooks/use-agent-status.ts
// Listens to agent_status WS messages
// Returns { status: "ready" | "disconnected" | "connecting" }
```

---

## 8. What Gets Deleted

- `server/src/ai/adaptor/tavily.rs` — removed
- `server/src/ai/adaptor/mod.rs` — remove tavily module declaration
- `server/src/ai/handlers.rs` — remove `/ai/search` endpoint (line 24, 231-252)
- `server/src/ai/model.rs` — remove `SearchRequest`, `SearchResponse`, `SearchResult` (lines 137-155)
- `channel_type::TAVILY = 99` — can keep the constant or remove; no-op without the adaptor
- `server/migrations/004_research.sql` — superseded by 005 (the table still exists, just altered)

---

## 9. Implementation Phases

Phases are ordered to keep each step independently testable and avoid breaking existing functionality.

### 7a-0: Migration Foundation
- Add `ensure_column()` helper in `db.rs` using `PRAGMA table_info` for idempotent ALTER TABLE
- Run 005 extension columns on `research_items` (category, authors, publish_year, keywords, relevance_score, updated_at)
- Verify Tabbit inserts still work after migration
- No frontend or API changes

### 7a-1: Research Backend CRUD
- Create `server/src/research/` module (model, handlers, mod)
- `POST /research/items` — save items (writes research_items + files/file_blobs for cloud .md)
- `GET /research/items?project_id=` — list with category/sort/pagination
- `GET /research/items/{id}` — detail
- `PATCH /research/items/{id}` — edit notes/category
- `DELETE /research/items/{id}` — hard delete + context_pages
- Tavily and `/ai/search` remain untouched

### 7a-2: Morphic Client
- Create `server/src/morphic/` module (client, model)
- `POST /research/search` — calls Morphic `/api/chat` + `/api/advanced-search`, merges results
- Error paths: Morphic unavailable returns 503, partial failure returns AI summary with empty results
- Config: `MORPHIC_BASE_URL` env var
- Still keep Tavily running

### 7a-3: Frontend Search + Library
- Pass `projectId` from `page.tsx` to `MainWorkspace`
- Refactor MainWorkspace into SearchTab + ResearchLibraryTab
- SearchTab: type selector, real `/research/search` API call, result cards with checkboxes, SaveBar
- ResearchLibraryTab: filter bar, item list, detail view, edit notes, delete
- Agent status hook for SaveBar warning
- Replace all mock data

### 7a-4: Agent create_file
- Add `CreateFile` variant to `AgentMessage` enum (both server and agent sides)
- Agent path safety: reject absolute/`..` paths, validate under work_dir, create parents, skip existing
- Server-side dual-write on save: primary cloud copy via file API, secondary Agent local copy
- Handle Agent disconnected: skip Agent copy silently, cloud copy already exists

### 7a-5: Remove Tavily
- Wait until `/research/search` and frontend are verified stable
- Delete `server/src/ai/adaptor/tavily.rs`
- Remove `/ai/search` endpoint from `ai/handlers.rs`
- Clean up `SearchRequest`/`SearchResponse`/`SearchResult` from `ai/model.rs`
- Remove tavily from `ai/adaptor/mod.rs`

### Out of scope (future)
- AI Chat panel (user explicitly rejected)
- BibTeX export
- Dataset import to compute environment
- Formula → LaTeX citation insertion
- Competition paper method comparison table
- Streaming search responses (use blocking HTTP for now)
- Tabbit integration (user said keep for later)
- Morphic chat history (we don't use it, it just exists in their stack)
