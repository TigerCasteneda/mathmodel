# Phase 7a: Search Module Redesign — Design Spec

**Date**: 2026-05-31
**Status**: approved
**Scope**: Replace Tavily with Morphic + SearXNG, build research pipeline, wire up frontend

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
2. **Save flow**: Frontend selected items → `POST /research/items` → write SQLite → send `create_file` to Agent via agent_bridge → Agent creates `references/*.md` → file_watcher detects → sync back to DB via CRDT
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

```sql
ALTER TABLE research_items ADD COLUMN category TEXT DEFAULT 'literature';
ALTER TABLE research_items ADD COLUMN authors TEXT DEFAULT '';
ALTER TABLE research_items ADD COLUMN publish_year INTEGER;
ALTER TABLE research_items ADD COLUMN keywords TEXT DEFAULT '';
ALTER TABLE research_items ADD COLUMN relevance_score REAL DEFAULT 0.0;
ALTER TABLE research_items ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
```

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

### Agent-side handling (`ws_client.rs`)

1. Receive `CreateFile` message variant
2. Create file (including parent directories) under `current_work_dir`
3. `file_watcher` auto-detects → pushes `FileChange` → server → CRDT sync → all collaborators

### Agent status awareness on frontend

- Use existing `agent_status` WebSocket messages (`"ready"` | `"disconnected"`)
- `status === "ready"` → Save button normal
- `status === "disconnected"` → Save button shows warning: "Agent not connected — files will only be saved to cloud"
  Button still enabled (knowledge base saved regardless)

---

## 7. Frontend Design

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

## 9. Scope Boundaries

### In scope (Phase 7a)
- Morphic module (client + model)
- Research module (model + handlers + references template)
- Migration 005
- Remove Tavily
- Frontend MainWorkspace refactor (SearchTab + ResearchLibraryTab)
- Agent `create_file` message handling
- Agent status hook for save button

### Out of scope (future)
- AI Chat panel (user explicitly rejected)
- BibTeX export
- Dataset import to compute environment
- Formula → LaTeX citation insertion
- Competition paper method comparison table
- Streaming search responses (use blocking HTTP for now)
- Tabbit integration (user said keep for later)
- Morphic chat history (we don't use it, it just exists in their stack)
