# Phase 7a: Search Module Redesign — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Tavily with Morphic + SearXNG search pipeline, build research knowledge base with CRUD API, wire up frontend MainWorkspace with real search and library tabs, add Agent `create_file` for local reference files.

**Architecture:** New `morphic/` module for HTTP client to Morphic advanced-search API, new `research/` module for search+saving CRUD, frontend MainWorkspace refactored into SearchTab + ResearchLibraryTab with `projectId` passthrough. Migration uses `PRAGMA table_info` idempotent column addition. Agent gets `CreateFile` message with path-safety validation. Tavily removed last after everything is stable.

**Tech Stack:** Rust Axum + SQLx + Yrs (server), Next.js 16 + React 19 + TypeScript + Tailwind + shadcn/ui (frontend), Rust tokio-tungstenite + notify (agent)

**Spec:** `docs/superpowers/specs/2026-05-31-search-redesign-design.md`

---

## File Structure

```
CREATE:
  server/src/morphic/mod.rs
  server/src/morphic/client.rs
  server/src/morphic/model.rs
  server/src/research/mod.rs
  server/src/research/model.rs
  server/src/research/handlers.rs
  server/src/research/references.rs
  server/migrations/005_research_v2.sql
  hooks/use-agent-status.ts

MODIFY:
  server/src/db.rs                          — add ensure_column(), run 005 migration
  server/src/main.rs                        — add morphic/research modules, mount routes
  server/src/agent_bridge/registry.rs       — fix status "connected" → "ready"
  server/src/agent_bridge/handlers.rs       — fix status, add create_file routing
  agent/src/ws_client.rs                    — add CreateFile variant + handler
  components/dashboard/main-workspace.tsx   — full refactor
  app/projects/[id]/page.tsx               — pass projectId to MainWorkspace
  server/src/ai/adaptor/mod.rs             — remove tavily (7a-5)
  server/src/ai/handlers.rs                — remove /ai/search (7a-5)
  server/src/ai/model.rs                   — remove SearchRequest/Response/Result (7a-5)

DELETE:
  server/src/ai/adaptor/tavily.rs          — (7a-5)
```

---

### Task 1: Migration Foundation — `ensure_column` helper

**Files:**
- Modify: `server/src/db.rs:1-35`

- [ ] **Step 1: Add `ensure_column` helper to `db.rs`**

Replace the contents of `server/src/db.rs`:

```rust
use sqlx::sqlite::SqlitePool;
use std::path::Path;

pub async fn init_pool(database_url: &str) -> SqlitePool {
    if database_url.starts_with("sqlite:") {
        let path = database_url.strip_prefix("sqlite:").unwrap();
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).ok();
        }
    }

    let pool = SqlitePool::connect(database_url)
        .await
        .expect("Failed to connect to database");

    run_migrations(&pool).await;

    pool
}

async fn run_migrations(pool: &SqlitePool) {
    run_specific_migration(pool, include_str!("../migrations/001_initial.sql")).await;
    run_specific_migration(pool, include_str!("../migrations/002_ai.sql")).await;
    run_specific_migration(pool, include_str!("../migrations/003_history.sql")).await;
    run_specific_migration(pool, include_str!("../migrations/004_research.sql")).await;
    run_005_migration(pool).await;
}

pub async fn run_specific_migration(pool: &SqlitePool, sql: &str) {
    for statement in sql.split(';').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        sqlx::query(statement)
            .execute(pool)
            .await
            .expect("Failed to run migration");
    }
}

/// Idempotent column addition: checks PRAGMA table_info before ALTER TABLE.
pub async fn ensure_column(
    pool: &SqlitePool,
    table: &str,
    column: &str,
    type_sql: &str,
) -> Result<(), sqlx::Error> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info(?1) WHERE name = ?2"
    )
    .bind(table)
    .bind(column)
    .fetch_one(pool)
    .await?;

    if count == 0 {
        let sql = format!("ALTER TABLE {table} ADD COLUMN {column} {type_sql}");
        sqlx::query(&sql).execute(pool).await?;
        tracing::info!("Migration: added column {table}.{column} {type_sql}");
    }

    Ok(())
}

async fn run_005_migration(pool: &SqlitePool) {
    ensure_column(pool, "research_items", "category", "TEXT DEFAULT 'literature'")
        .await
        .expect("005: category");
    ensure_column(pool, "research_items", "authors", "TEXT DEFAULT ''")
        .await
        .expect("005: authors");
    ensure_column(pool, "research_items", "publish_year", "INTEGER")
        .await
        .expect("005: publish_year");
    ensure_column(pool, "research_items", "keywords", "TEXT DEFAULT ''")
        .await
        .expect("005: keywords");
    ensure_column(pool, "research_items", "relevance_score", "REAL DEFAULT 0.0")
        .await
        .expect("005: relevance_score");
    ensure_column(pool, "research_items", "updated_at", "INTEGER NOT NULL DEFAULT 0")
        .await
        .expect("005: updated_at");
}
```

- [ ] **Step 2: Create the SQL migration file (for documentation/reference only)**

Create `server/migrations/005_research_v2.sql`:

```sql
-- 005: Extend research_items for Morphic search
-- Executed via ensure_column() in db.rs — idempotent, safe for repeated runs.
-- Columns are added only if they don't already exist (PRAGMA table_info check).

-- ALTER TABLE research_items ADD COLUMN category TEXT DEFAULT 'literature';
-- ALTER TABLE research_items ADD COLUMN authors TEXT DEFAULT '';
-- ALTER TABLE research_items ADD COLUMN publish_year INTEGER;
-- ALTER TABLE research_items ADD COLUMN keywords TEXT DEFAULT '';
-- ALTER TABLE research_items ADD COLUMN relevance_score REAL DEFAULT 0.0;
-- ALTER TABLE research_items ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
```

- [ ] **Step 3: Build and verify migration runs**

Run: `cd server && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles successfully.

- [ ] **Step 4: Commit**

```bash
git add server/src/db.rs server/migrations/005_research_v2.sql
git commit -m "feat: idempotent ensure_column helper + 005 research extension"
```

---

### Task 2: Research Module — model.rs

**Files:**
- Create: `server/src/research/mod.rs`
- Create: `server/src/research/model.rs`

- [ ] **Step 1: Create `server/src/research/mod.rs`**

```rust
pub mod handlers;
pub mod model;
pub mod references;
```

- [ ] **Step 2: Create `server/src/research/model.rs`**

```rust
use serde::{Deserialize, Serialize};

/// Matches research_items table (after 005 migration).
/// Existing Tabbit columns plus new Morphic search columns.
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

// ── Search ──

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub project_id: String,
    pub query: String,
    pub category: String,
    #[serde(default = "default_max_results")]
    pub max_results: i32,
}

fn default_max_results() -> i32 { 20 }

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SearchResultItem {
    pub title: String,
    pub url: String,
    pub content: String,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: Option<String>,
    pub relevance_score: f64,
}

// ── Save ──

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
    pub summary: Option<String>,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: Option<String>,
    pub relevance_score: Option<f64>,
    pub raw_json: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SaveItemsResponse {
    pub saved: i32,
    pub items: Vec<ResearchItem>,
    pub files_created: i32,
}

// ── List / Detail / Update ──

#[derive(Debug, Deserialize)]
pub struct ListItemsQuery {
    pub project_id: String,
    pub category: Option<String>,
    #[serde(default = "default_sort")]
    pub sort: String,
    #[serde(default = "default_order")]
    pub order: String,
    #[serde(default = "default_limit")]
    pub limit: i32,
    #[serde(default)]
    pub offset: i32,
}

fn default_sort() -> String { "created_at".into() }
fn default_order() -> String { "desc".into() }
fn default_limit() -> i32 { 50 }

#[derive(Debug, Deserialize)]
pub struct UpdateItemRequest {
    pub notes: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ItemPathParam {
    pub item_id: String,
}
```

- [ ] **Step 3: Commit**

```bash
git add server/src/research/mod.rs server/src/research/model.rs
git commit -m "feat: research module model types"
```

---

### Task 3: Research Module — helpers (file creation via CRDT path)

**Files:**
- Create: `server/src/research/references.rs`

- [ ] **Step 1: Create `server/src/research/references.rs`**

This file handles:
1. Generating markdown content from a search result
2. Creating a cloud file in the existing `files` + `crdt_docs` path
3. Broadcasting a `create_file` message to the Agent (best-effort)

```rust
use crate::agent_bridge::registry::AgentRegistry;
use crate::error::AppError;
use chrono::Utc;
use std::sync::Arc;
use uuid::Uuid;

use super::model::SaveItemInput;

/// Generate a references/<slug>.md file body from a search result.
pub fn render_md(input: &SaveItemInput) -> String {
    let category_label = category_label(&input.category);
    let summary = input.summary.as_deref().unwrap_or("");
    let authors = input.authors.as_deref().unwrap_or("");
    let publish_year = input
        .publish_year
        .map(|y| y.to_string())
        .unwrap_or_default();
    let keywords = input.keywords.as_deref().unwrap_or("");
    let date = Utc::now().format("%Y-%m-%d");

    format!(
        "# {title}\n\
         - **URL**: {url}\n\
         - **Category**: {category}\n\
         - **Authors**: {authors}\n\
         - **Year**: {year}\n\
         - **Keywords**: {keywords}\n\
         - **Saved**: {date}\n\n\
         ## Abstract\n\
         {summary}\n\n\
         ## Notes\n\
         <!-- Add your notes here -->\n",
        title = input.title,
        url = input.url,
        category = category_label,
        authors = authors,
        year = publish_year,
        keywords = keywords,
        date = date,
        summary = summary,
    )
}

fn category_label(cat: &str) -> &str {
    match cat {
        "literature" => "📄 Literature",
        "dataset" => "📊 Dataset",
        "code" => "🧮 Code",
        "formula" => "📐 Formula",
        "competition" => "🏆 Competition",
        _ => "📄 Literature",
    }
}

/// Derive a filesystem-safe slug from a title.
pub fn title_to_slug(title: &str) -> String {
    let slug = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    // Truncate to reasonable length
    if slug.len() > 64 { slug[..64].to_string() } else { slug }
}

/// Create a cloud file entry using existing project file + CRDT storage path.
///
/// Returns the created file's UUID.
pub async fn create_cloud_md_file(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    filename: &str,
    md_content: &str,
) -> Result<String, AppError> {
    let file_id = Uuid::new_v4().to_string();
    let now = Utc::now().timestamp();

    // 1. Insert into files table (zone=research)
    sqlx::query(
        "INSERT INTO files (id, project_id, parent_id, name, type, zone, created_at, updated_at)
         VALUES (?, ?, NULL, ?, 'file', 'research', ?, ?)"
    )
    .bind(&file_id)
    .bind(project_id)
    .bind(filename)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    // 2. Encode the markdown as a Yrs CRDT update and store in crdt_docs
    let ydoc = yrs::Doc::new();
    let text = ydoc.get_or_insert_text("content");
    {
        let mut txn = ydoc.transact_mut();
        text.insert(&mut txn, 0, md_content);
    }
    let state = {
        let txn = ydoc.transact();
        txn.encode_state_as_update_v1(&yrs::StateVector::default())
    };

    sqlx::query(
        "INSERT INTO crdt_docs (file_id, ydoc_state, updated_at) VALUES (?, ?, ?)"
    )
    .bind(&file_id)
    .bind(&state)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(file_id)
}

/// Send create_file to Agent (best-effort, errors are logged not returned).
pub async fn notify_agent_create_file(
    agent_registry: &Arc<AgentRegistry>,
    project_id: &str,
    relative_path: &str,
    content: &str,
) -> i32 {
    let Some(bridge) = {
        let projects = agent_registry.projects.read().await;
        projects.get(project_id).cloned()
    } else {
        tracing::info!("No agent bridge for project {project_id}, skipping local file creation");
        return 0;
    };

    let msg = serde_json::json!({
        "type": "create_file",
        "path": relative_path,
        "content": content,
    });

    match bridge.send_to_agent(msg).await {
        Ok(()) => {
            tracing::info!("Sent create_file to agent: {relative_path}");
            1
        }
        Err(()) => {
            tracing::info!("Agent not connected for project {project_id}, skipping local file");
            0
        }
    }
}
```

- [ ] **Step 2: Commit**

```bash
git add server/src/research/references.rs
git commit -m "feat: research references — md template + cloud file creation via CRDT"
```

---

### Task 4: Research Module — handlers.rs (CRUD)

**Files:**
- Create: `server/src/research/handlers.rs`

- [ ] **Step 1: Create `server/src/research/handlers.rs`**

```rust
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, patch, post},
    Json, Router,
};
use chrono::Utc;
use uuid::Uuid;

use super::model::*;
use super::references;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/research/items", get(list_items).post(save_items))
        .route(
            "/research/items/{item_id}",
            get(get_item).patch(update_item).delete(delete_item),
        )
}

// ── Helpers ──

async fn verify_membership(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    user_id: &str,
) -> Result<(), AppError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM project_members WHERE project_id = ? AND user_id = ?)",
    )
    .bind(project_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    if exists == 0 {
        Err(AppError::Forbidden("not a member of this project".into()))
    } else {
        Ok(())
    }
}

fn validate_category(cat: &str) -> Result<&str, AppError> {
    match cat {
        "literature" | "dataset" | "code" | "formula" | "competition" => Ok(cat),
        _ => Err(AppError::BadRequest(format!("invalid category: {cat}"))),
    }
}

// ── POST /research/items — save search results ──

async fn save_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SaveItemsRequest>,
) -> Result<Json<SaveItemsResponse>, AppError> {
    verify_membership(&state.pool, &req.project_id, &auth.user_id).await?;

    let mut saved = Vec::new();
    let mut files_created = 0i32;
    let now = Utc::now().timestamp();

    for input in &req.items {
        let cat = validate_category(&input.category)?;
        let item_id = Uuid::new_v4().to_string();
        let summary = input.summary.clone().unwrap_or_default();
        let authors = input.authors.clone().unwrap_or_default();
        let keywords = input.keywords.clone().unwrap_or_default();
        let relevance = input.relevance_score.unwrap_or(0.0);
        let raw_json = input
            .raw_json
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "{}".to_string());

        sqlx::query(
            "INSERT INTO research_items
             (id, project_id, created_by, source, category, url, title, summary,
              authors, publish_year, keywords, notes, relevance_score, raw_json,
              created_at, updated_at)
             VALUES (?, ?, ?, 'morphic', ?, ?, ?, ?, ?, ?, ?, '', ?, ?, ?, ?)",
        )
        .bind(&item_id)
        .bind(&req.project_id)
        .bind(&auth.user_id)
        .bind(cat)
        .bind(&input.url)
        .bind(&input.title)
        .bind(&summary)
        .bind(&authors)
        .bind(input.publish_year)
        .bind(&keywords)
        .bind(relevance)
        .bind(&raw_json)
        .bind(now)
        .bind(now)
        .execute(&state.pool)
        .await?;

        // Generate cloud .md file via existing CRDT path
        let slug = references::title_to_slug(&input.title);
        let filename = format!("references/{slug}.md");
        let md_content = references::render_md(input);

        match references::create_cloud_md_file(
            &state.pool,
            &req.project_id,
            &filename,
            &md_content,
        )
        .await
        {
            Ok(_file_id) => {
                // Best-effort: also send to Agent for local copy
                files_created += references::notify_agent_create_file(
                    &state.agent_registry,
                    &req.project_id,
                    &filename,
                    &md_content,
                )
                .await;
            }
            Err(err) => {
                tracing::warn!("Failed to create cloud md file: {err:?}");
            }
        }

        let item: ResearchItem = sqlx::query_as(
            "SELECT * FROM research_items WHERE id = ?",
        )
        .bind(&item_id)
        .fetch_one(&state.pool)
        .await?;

        saved.push(item);
    }

    Ok(Json(SaveItemsResponse {
        saved: saved.len() as i32,
        items: saved,
        files_created,
    }))
}

// ── GET /research/items — list ──

async fn list_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(q): Query<ListItemsQuery>,
) -> Result<Json<Vec<ResearchItem>>, AppError> {
    verify_membership(&state.pool, &q.project_id, &auth.user_id).await?;

    let sort_col = match q.sort.as_str() {
        "created_at" | "updated_at" | "title" | "category" | "relevance_score" => q.sort.as_str(),
        _ => "created_at",
    };
    let order = if q.order == "asc" { "ASC" } else { "DESC" };

    let items: Vec<ResearchItem> = if let Some(ref cat) = q.category {
        sqlx::query_as(&format!(
            "SELECT * FROM research_items WHERE project_id = ? AND category = ? ORDER BY {sort_col} {order} LIMIT ? OFFSET ?"
        ))
        .bind(&q.project_id)
        .bind(cat)
        .bind(q.limit)
        .bind(q.offset)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as(&format!(
            "SELECT * FROM research_items WHERE project_id = ? ORDER BY {sort_col} {order} LIMIT ? OFFSET ?"
        ))
        .bind(&q.project_id)
        .bind(q.limit)
        .bind(q.offset)
        .fetch_all(&state.pool)
        .await?
    };

    Ok(Json(items))
}

// ── GET /research/items/{item_id} — detail ──

async fn get_item(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(params): Path<ItemPathParam>,
) -> Result<Json<ResearchItem>, AppError> {
    let item: ResearchItem = sqlx::query_as(
        "SELECT * FROM research_items WHERE id = ?",
    )
    .bind(&params.item_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("research item not found".into()))?;

    Ok(Json(item))
}

// ── PATCH /research/items/{item_id} — update notes/category ──

async fn update_item(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(params): Path<ItemPathParam>,
    Json(req): Json<UpdateItemRequest>,
) -> Result<Json<ResearchItem>, AppError> {
    let now = Utc::now().timestamp();

    if let Some(ref cat) = req.category {
        validate_category(cat)?;
        sqlx::query("UPDATE research_items SET category = ?, updated_at = ? WHERE id = ?")
            .bind(cat)
            .bind(now)
            .bind(&params.item_id)
            .execute(&state.pool)
            .await?;
    }

    if let Some(ref notes) = req.notes {
        sqlx::query("UPDATE research_items SET notes = ?, updated_at = ? WHERE id = ?")
            .bind(notes)
            .bind(now)
            .bind(&params.item_id)
            .execute(&state.pool)
            .await?;
    }

    let item: ResearchItem = sqlx::query_as(
        "SELECT * FROM research_items WHERE id = ?",
    )
    .bind(&params.item_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| AppError::NotFound("research item not found".into()))?;

    Ok(Json(item))
}

// ── DELETE /research/items/{item_id} ──

async fn delete_item(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(params): Path<ItemPathParam>,
) -> Result<Json<serde_json::Value>, AppError> {
    // Delete context pages first (FK)
    sqlx::query("DELETE FROM research_context_pages WHERE item_id = ?")
        .bind(&params.item_id)
        .execute(&state.pool)
        .await?;

    let result = sqlx::query("DELETE FROM research_items WHERE id = ?")
        .bind(&params.item_id)
        .execute(&state.pool)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("research item not found".into()));
    }

    Ok(Json(serde_json::json!({ "deleted": true })))
}
```

- [ ] **Step 2: Commit**

```bash
git add server/src/research/handlers.rs
git commit -m "feat: research CRUD handlers — save/list/get/update/delete"
```

---

### Task 5: Register Research Module in main.rs

**Files:**
- Modify: `server/src/main.rs:1-66`

- [ ] **Step 1: Add research module and mount routes**

Edit `server/src/main.rs`:

```rust
mod agent_bridge;
mod ai;
mod auth;
mod compute;
mod config;
mod db;
mod error;
mod file;
mod history;
mod morphic;       // ADD (placeholder until Task 6)
mod project;
mod research;      // ADD
mod sync;

use axum::routing::get;
use axum::Router;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let cfg = config::Config::from_env();
    let pool = db::init_pool(&cfg.database_url).await;

    let app_state = AppState {
        pool,
        config: cfg.clone(),
        room_registry: Arc::new(sync::room::RoomRegistry::new()),
        agent_registry: Arc::new(agent_bridge::registry::AgentRegistry::new()),
    };

    let app = Router::new()
        .nest("/auth", auth::handlers::routes())
        .nest("/projects", project::handlers::routes())
        .merge(file::handlers::routes())
        .route("/sync", get(sync::handlers::ws_handler))
        .route("/agent", get(agent_bridge::handlers::agent_ws_handler))
        .merge(ai::handlers::routes())
        .merge(compute::handlers::routes())
        .merge(history::handlers::routes())
        .merge(research::handlers::routes())    // ADD
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", cfg.port)).await?;
    tracing::info!("Server running on port {}", cfg.port);
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::SqlitePool,
    pub config: config::Config,
    pub room_registry: Arc<sync::room::RoomRegistry>,
    pub agent_registry: Arc<agent_bridge::registry::AgentRegistry>,
}
```

Note: `mod morphic;` compiles against a placeholder. Create a minimal placeholder now:

Create `server/src/morphic/mod.rs`:

```rust
pub mod client;
pub mod model;
```

Create `server/src/morphic/model.rs` (placeholder):

```rust
// Placeholder — filled in by Task 6
```

Create `server/src/morphic/client.rs` (placeholder):

```rust
// Placeholder — filled in by Task 6
```

- [ ] **Step 2: Build and verify**

Run: `cd server && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles. Research routes are mounted.

- [ ] **Step 3: Commit**

```bash
git add server/src/main.rs server/src/morphic/
git commit -m "feat: mount research routes + morphic placeholder"
```

---

### Task 6: Fix Agent Status — "connected" → "ready"

**Files:**
- Modify: `server/src/agent_bridge/registry.rs:24-26`
- Modify: `server/src/agent_bridge/handlers.rs:182-186`

- [ ] **Step 1: Fix `registry.rs` — set_agent broadcasts "ready"**

In `server/src/agent_bridge/registry.rs`, change line 25:

```rust
// Before:
"status": "connected"

// After:
"status": "ready"
```

- [ ] **Step 2: Fix `handlers.rs` — initial status sends "ready"**

In `server/src/agent_bridge/handlers.rs`, change lines 182-186:

```rust
// Before:
let status = if bridge.has_agent().await {
    "connected"
} else {
    "disconnected"
};

// After:
let status = if bridge.has_agent().await {
    "ready"
} else {
    "disconnected"
};
```

- [ ] **Step 3: Build and verify**

Run: `cd server && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles.

- [ ] **Step 4: Commit**

```bash
git add server/src/agent_bridge/registry.rs server/src/agent_bridge/handlers.rs
git commit -m "fix: agent status 'connected' → 'ready' for consistency"
```

---

### Task 7: Morphic Client Module

**Files:**
- Replace: `server/src/morphic/model.rs` (placeholder → real)
- Replace: `server/src/morphic/client.rs` (placeholder → real)

- [ ] **Step 1: Write `server/src/morphic/model.rs`**

```rust
use serde::Deserialize;

/// Response from Morphic POST /api/advanced-search
#[derive(Debug, Deserialize)]
pub struct AdvancedSearchResponse {
    pub query: String,
    #[serde(default)]
    pub results: Vec<MorphicResult>,
    #[serde(default)]
    pub number_of_results: i32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MorphicResult {
    pub title: String,
    pub url: String,
    pub content: String,
}
```

- [ ] **Step 2: Write `server/src/morphic/client.rs`**

```rust
use crate::error::AppError;
use super::model::*;

pub struct MorphicClient {
    base_url: String,
    http: reqwest::Client,
}

impl MorphicClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("reqwest client"),
        }
    }

    pub fn from_env() -> Self {
        let base_url = std::env::var("MORPHIC_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:3000".to_string());
        Self::new(base_url)
    }

    /// POST /api/advanced-search — structured search with crawling + relevance scoring
    pub async fn advanced_search(
        &self,
        query: &str,
        max_results: i32,
    ) -> Result<AdvancedSearchResponse, AppError> {
        let url = format!("{}/api/advanced-search", self.base_url);

        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "query": query,
                "maxResults": max_results,
                "searchDepth": "advanced",
            }))
            .send()
            .await
            .map_err(|e| {
                tracing::warn!("Morphic connection error: {e}");
                AppError::Internal("Search engine is not available".into())
            })?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            tracing::warn!("Morphic error {status}: {body}");
            return Err(AppError::Internal(format!(
                "Search engine returned {status}"
            )));
        }

        let result: AdvancedSearchResponse = resp.json().await.map_err(|e| {
            tracing::warn!("Morphic parse error: {e}");
            AppError::Internal("Failed to parse search results".into())
        })?;

        Ok(result)
    }
}
```

- [ ] **Step 3: Build and verify**

Run: `cd server && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles. `MorphicClient` is callable.

- [ ] **Step 4: Commit**

```bash
git add server/src/morphic/model.rs server/src/morphic/client.rs
git commit -m "feat: Morphic client — advanced-search HTTP integration"
```

---

### Task 8: Research Search Endpoint (`POST /research/search`)

**Files:**
- Modify: `server/src/research/handlers.rs` — add `/research/search` route

- [ ] **Step 1: Add search endpoint to handlers.rs**

Add to the `routes()` function in `server/src/research/handlers.rs`:

```rust
// Change:
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/research/items", get(list_items).post(save_items))
        .route(
            "/research/items/{item_id}",
            get(get_item).patch(update_item).delete(delete_item),
        )
}

// To:
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/research/search", post(search))   // ADD
        .route("/research/items", get(list_items).post(save_items))
        .route(
            "/research/items/{item_id}",
            get(get_item).patch(update_item).delete(delete_item),
        )
}
```

Add the `search` handler function at the top of the file (after the `validate_category` helper):

```rust
use crate::morphic::client::MorphicClient;

// ── POST /research/search ──

async fn search(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<SearchRequest>,
) -> Result<Json<SearchResponse>, AppError> {
    verify_membership(&state.pool, &req.project_id, &auth.user_id).await?;
    validate_category(&req.category)?;

    let client = MorphicClient::from_env();

    let morphic_resp = client
        .advanced_search(&req.query, req.max_results)
        .await
        .unwrap_or_else(|_| {
            // Return empty results on failure — don't break the frontend
            AdvancedSearchResponse {
                query: req.query.clone(),
                results: vec![],
                number_of_results: 0,
            }
        });

    let results: Vec<SearchResultItem> = morphic_resp
        .results
        .into_iter()
        .map(|r| SearchResultItem {
            title: r.title,
            url: r.url,
            content: r.content,
            authors: None,
            publish_year: None,
            keywords: None,
            relevance_score: 0.0,
        })
        .collect();

    Ok(Json(SearchResponse {
        query: morphic_resp.query,
        results,
    }))
}
```

Also add the import at the top of the file:

```rust
use super::model::*;
use super::references;
use crate::auth::middleware::AuthUser;
use crate::error::AppError;
use crate::morphic::client::MorphicClient;      // ADD
use crate::morphic::model::AdvancedSearchResponse; // ADD
use crate::AppState;
```

- [ ] **Step 2: Build and verify**

Run: `cd server && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles.

- [ ] **Step 3: Commit**

```bash
git add server/src/research/handlers.rs
git commit -m "feat: POST /research/search — Morphic advanced-search endpoint"
```

---

### Task 9: Frontend — projectId Passthrough + API Client

**Files:**
- Modify: `app/projects/[id]/page.tsx:9-28`
- Modify: `lib/api.ts` — add research API functions

- [ ] **Step 1: Pass projectId to MainWorkspace**

In `app/projects/[id]/page.tsx`, change:

```tsx
// Before (line 21):
<MainWorkspace />

// After:
<MainWorkspace projectId={id as string} />
```

- [ ] **Step 2: Add research API functions to `lib/api.ts`**

Add at the end of `lib/api.ts`:

```typescript
// ── Research / Search ──

export interface SearchResultItem {
  title: string
  url: string
  content: string
  authors?: string
  publish_year?: number
  keywords?: string
  relevance_score: number
}

export interface SearchResponse {
  query: string
  results: SearchResultItem[]
}

export interface SaveItemInput {
  title: string
  url: string
  content: string
  category: string
  summary?: string
  authors?: string
  publish_year?: number
  keywords?: string
  relevance_score?: number
  raw_json?: Record<string, unknown>
}

export interface ResearchItem {
  id: string
  project_id: string
  created_by: string
  source: string
  category: string
  url: string
  title?: string
  summary?: string
  authors?: string
  publish_year?: number
  keywords?: string
  notes?: string
  relevance_score: number
  raw_json: string
  created_at: number
  updated_at: number
}

export interface SaveItemsResponse {
  saved: number
  items: ResearchItem[]
  files_created: number
}

export async function researchSearch(
  projectId: string,
  query: string,
  category: string,
  maxResults = 20
): Promise<SearchResponse> {
  return apiFetch<SearchResponse>("/research/search", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ project_id: projectId, query, category, max_results: maxResults }),
  })
}

export async function saveResearchItems(
  projectId: string,
  items: SaveItemInput[]
): Promise<SaveItemsResponse> {
  return apiFetch<SaveItemsResponse>("/research/items", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ project_id: projectId, items }),
  })
}

export async function listResearchItems(
  projectId: string,
  category?: string,
  sort = "created_at",
  order = "desc",
  limit = 50,
  offset = 0
): Promise<ResearchItem[]> {
  const params = new URLSearchParams({ project_id: projectId, sort, order, limit: String(limit), offset: String(offset) })
  if (category) params.set("category", category)
  return apiFetch<ResearchItem[]>(`/research/items?${params.toString()}`)
}

export async function getResearchItem(itemId: string): Promise<ResearchItem> {
  return apiFetch<ResearchItem>(`/research/items/${itemId}`)
}

export async function updateResearchItem(
  itemId: string,
  data: { notes?: string; category?: string }
): Promise<ResearchItem> {
  return apiFetch<ResearchItem>(`/research/items/${itemId}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(data),
  })
}

export async function deleteResearchItem(itemId: string): Promise<{ deleted: boolean }> {
  return apiFetch<{ deleted: boolean }>(`/research/items/${itemId}`, {
    method: "DELETE",
  })
}
```

- [ ] **Step 3: Commit**

```bash
git add app/projects/\[id\]/page.tsx lib/api.ts
git commit -m "feat: projectId passthrough + research API client functions"
```

---

### Task 10: Frontend — Agent Status Hook

**Files:**
- Create: `hooks/use-agent-status.ts`

- [ ] **Step 1: Create `hooks/use-agent-status.ts`**

```typescript
"use client"

import { useState, useEffect } from "react"

export type AgentStatus = "ready" | "disconnected" | "connecting"

export function useAgentStatus(projectId: string): AgentStatus {
  const [status, setStatus] = useState<AgentStatus>("connecting")

  useEffect(() => {
    const token = typeof window !== "undefined"
      ? localStorage.getItem("auth_token")
      : null
    if (!token) {
      setStatus("disconnected")
      return
    }

    const wsUrl = `${process.env.NEXT_PUBLIC_WS_URL || "ws://localhost:3001"}/agent?token=${encodeURIComponent(token)}&project_id=${encodeURIComponent(projectId)}&role=frontend`
    const ws = new WebSocket(wsUrl)

    ws.onopen = () => {
      setStatus("connecting") // wait for first agent_status message
    }

    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data)
        if (msg.type === "agent_status") {
          setStatus(msg.status === "ready" ? "ready" : "disconnected")
        }
      } catch { /* ignore parse errors */ }
    }

    ws.onclose = () => {
      setStatus("disconnected")
    }

    ws.onerror = () => {
      setStatus("disconnected")
    }

    return () => {
      ws.close()
    }
  }, [projectId])

  return status
}
```

- [ ] **Step 2: Commit**

```bash
git add hooks/use-agent-status.ts
git commit -m "feat: useAgentStatus hook for save button state"
```

---

### Task 11: Frontend — MainWorkspace Refactor (SearchTab)

**Files:**
- Rewrite: `components/dashboard/main-workspace.tsx`

- [ ] **Step 1: Rewrite `components/dashboard/main-workspace.tsx`**

```tsx
"use client"

import { useState } from "react"
import { Search, Sparkles, BookOpen, Loader2 } from "lucide-react"
import { Input } from "@/components/ui/input"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Skeleton } from "@/components/ui/skeleton"
import { cn } from "@/lib/utils"
import { useAgentStatus } from "@/hooks/use-agent-status"
import {
  researchSearch,
  saveResearchItems,
  listResearchItems,
  updateResearchItem,
  deleteResearchItem,
  type SearchResultItem,
  type ResearchItem,
  type SaveItemInput,
} from "@/lib/api"

const SEARCH_TYPES = [
  { value: "literature", label: "📄 Literature" },
  { value: "dataset", label: "📊 Dataset" },
  { value: "code", label: "🧮 Code" },
  { value: "formula", label: "📐 Formula" },
  { value: "competition", label: "🏆 Competition" },
]

const TYPE_BADGES: Record<string, string> = {
  literature: "bg-blue-500/10 text-blue-400",
  dataset: "bg-green-500/10 text-green-400",
  code: "bg-purple-500/10 text-purple-400",
  formula: "bg-amber-500/10 text-amber-400",
  competition: "bg-red-500/10 text-red-400",
}

const TYPE_LABELS: Record<string, string> = {
  literature: "📄 Literature",
  dataset: "📊 Dataset",
  code: "🧮 Code",
  formula: "📐 Formula",
  competition: "🏆 Competition",
}

// ── Search Result Card ──

function ResultCard({
  result,
  selected,
  onToggle,
}: {
  result: SearchResultItem
  selected: boolean
  onToggle: () => void
}) {
  return (
    <div
      className={cn(
        "relative p-4 rounded-lg border transition-all cursor-pointer group",
        "hover:border-primary/40",
        selected
          ? "border-primary/50 bg-primary/5"
          : "border-border bg-card"
      )}
      onClick={onToggle}
    >
      <div className="absolute top-3 right-3">
        <Checkbox checked={selected} onCheckedChange={onToggle} />
      </div>
      <div className="pr-8">
        <h4 className="font-medium text-sm text-foreground line-clamp-2 mb-1">
          {result.title}
        </h4>
        <a
          href={result.url}
          target="_blank"
          rel="noopener noreferrer"
          className="text-xs text-primary/60 hover:text-primary truncate block mb-2"
          onClick={(e) => e.stopPropagation()}
        >
          {result.url}
        </a>
        <p className="text-xs text-muted-foreground line-clamp-3">
          {result.content}
        </p>
      </div>
    </div>
  )
}

// ── Saved Item Card ──

function ItemCard({
  item,
  onDelete,
}: {
  item: ResearchItem
  onDelete: (id: string) => void
}) {
  const [expanded, setExpanded] = useState(false)
  const [notes, setNotes] = useState(item.notes || "")
  const [saving, setSaving] = useState(false)

  const handleSaveNotes = async () => {
    setSaving(true)
    try {
      await updateResearchItem(item.id, { notes })
    } catch (err) {
      console.error("Failed to save notes:", err)
    }
    setSaving(false)
  }

  return (
    <div className="p-4 rounded-lg border border-border bg-card">
      <div className="flex items-start justify-between">
        <div
          className="flex-1 cursor-pointer"
          onClick={() => setExpanded(!expanded)}
        >
          <div className="flex items-center gap-2 mb-1">
            <span
              className={cn(
                "text-xs px-2 py-0.5 rounded-full",
                TYPE_BADGES[item.category] || TYPE_BADGES.literature
              )}
            >
              {TYPE_LABELS[item.category] || item.category}
            </span>
          </div>
          <h4 className="font-medium text-sm text-foreground line-clamp-1">
            {item.title || "Untitled"}
          </h4>
          <p className="text-xs text-muted-foreground mt-1">
            {item.url}
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon"
          className="h-7 w-7 text-muted-foreground hover:text-destructive"
          onClick={() => onDelete(item.id)}
        >
          ✕
        </Button>
      </div>

      {expanded && (
        <div className="mt-3 pt-3 border-t border-border space-y-3">
          {item.summary && (
            <div>
              <span className="text-xs font-medium text-muted-foreground">
                Summary
              </span>
              <p className="text-xs text-muted-foreground mt-1">
                {item.summary}
              </p>
            </div>
          )}
          <div>
            <span className="text-xs font-medium text-muted-foreground">
              Notes
            </span>
            <textarea
              value={notes}
              onChange={(e) => setNotes(e.target.value)}
              className="w-full mt-1 p-2 text-xs bg-muted rounded border border-border text-foreground resize-none h-20"
              placeholder="Add your notes..."
            />
            <Button
              size="sm"
              variant="outline"
              className="mt-2 h-7 text-xs"
              onClick={handleSaveNotes}
              disabled={saving}
            >
              {saving ? "Saving..." : "Save Notes"}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}

// ── Main Component ──

export function MainWorkspace({ projectId }: { projectId: string }) {
  const agentStatus = useAgentStatus(projectId)

  // Search state
  const [activeTab, setActiveTab] = useState<"search" | "library">("search")
  const [query, setQuery] = useState("")
  const [category, setCategory] = useState("literature")
  const [results, setResults] = useState<SearchResultItem[]>([])
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [isSearching, setIsSearching] = useState(false)
  const [hasSearched, setHasSearched] = useState(false)
  const [isSaving, setIsSaving] = useState(false)

  // Library state
  const [items, setItems] = useState<ResearchItem[]>([])
  const [libraryLoaded, setLibraryLoaded] = useState(false)

  const loadLibrary = async () => {
    try {
      const data = await listResearchItems(projectId)
      setItems(data)
      setLibraryLoaded(true)
    } catch (err) {
      console.error("Failed to load research items:", err)
    }
  }

  const handleSearch = async () => {
    if (!query.trim()) return
    setIsSearching(true)
    setHasSearched(false)
    setSelected(new Set())
    try {
      const resp = await researchSearch(projectId, query, category)
      setResults(resp.results)
    } catch (err) {
      console.error("Search failed:", err)
      setResults([])
    }
    setIsSearching(false)
    setHasSearched(true)
  }

  const toggleResult = (index: number) => {
    const next = new Set(selected)
    if (next.has(index)) next.delete(index)
    else next.add(index)
    setSelected(next)
  }

  const handleSave = async () => {
    const toSave: SaveItemInput[] = []
    selected.forEach((i) => {
      const r = results[i]
      if (r) {
        toSave.push({
          title: r.title,
          url: r.url,
          content: r.content,
          category,
          summary: r.content.slice(0, 500),
          relevance_score: r.relevance_score,
          raw_json: r as unknown as Record<string, unknown>,
        })
      }
    })
    if (toSave.length === 0) return

    setIsSaving(true)
    try {
      const resp = await saveResearchItems(projectId, toSave)
      alert(`Saved ${resp.saved} item(s).${resp.files_created < resp.saved ? " Agent not connected — local files not created." : ""}`)
      setSelected(new Set())
    } catch (err) {
      console.error("Save failed:", err)
      alert("Failed to save items.")
    }
    setIsSaving(false)
  }

  const handleDelete = async (itemId: string) => {
    if (!confirm("Delete this research item?")) return
    try {
      await deleteResearchItem(itemId)
      setItems((prev) => prev.filter((it) => it.id !== itemId))
    } catch (err) {
      console.error("Delete failed:", err)
    }
  }

  const selectedCount = selected.size

  return (
    <div className="flex flex-col h-full bg-background">
      {/* Tab Switcher */}
      <div className="flex border-b border-border">
        <button
          className={cn(
            "flex-1 py-3 text-sm font-medium flex items-center justify-center gap-2 transition-colors",
            activeTab === "search"
              ? "text-foreground border-b-2 border-primary"
              : "text-muted-foreground hover:text-foreground"
          )}
          onClick={() => setActiveTab("search")}
        >
          <Search className="w-4 h-4" />
          Search
        </button>
        <button
          className={cn(
            "flex-1 py-3 text-sm font-medium flex items-center justify-center gap-2 transition-colors",
            activeTab === "library"
              ? "text-foreground border-b-2 border-primary"
              : "text-muted-foreground hover:text-foreground"
          )}
          onClick={() => {
            setActiveTab("library")
            if (!libraryLoaded) loadLibrary()
          }}
        >
          <BookOpen className="w-4 h-4" />
          Research Library
        </button>
      </div>

      {/* ── Search Tab ── */}
      {activeTab === "search" && (
        <>
          {/* Search Bar */}
          <div className="p-4 border-b border-border">
            <div className="flex gap-2 mb-2">
              <select
                value={category}
                onChange={(e) => setCategory(e.target.value)}
                className="px-3 py-2 text-sm bg-muted border border-border rounded-lg text-foreground"
              >
                {SEARCH_TYPES.map((t) => (
                  <option key={t.value} value={t.value}>
                    {t.label}
                  </option>
                ))}
              </select>
              <div className="flex-1 relative">
                <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground" />
                <Input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && handleSearch()}
                  placeholder="Search for mathematical models, methods, papers..."
                  className="pl-10 pr-4 py-5 text-sm bg-input border-border rounded-xl"
                />
              </div>
              <Button
                onClick={handleSearch}
                disabled={isSearching || !query.trim()}
                className="bg-primary text-primary-foreground hover:bg-primary/90"
              >
                {isSearching ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : (
                  <Sparkles className="w-4 h-4" />
                )}
              </Button>
            </div>
          </div>

          {/* Results */}
          <ScrollArea className="flex-1">
            <div className="p-4 max-w-4xl mx-auto">
              {isSearching ? (
                <div className="space-y-4">
                  {[1, 2, 3, 4].map((i) => (
                    <div key={i} className="p-4 rounded-lg border border-border">
                      <Skeleton className="h-4 w-3/4 bg-muted mb-2" />
                      <Skeleton className="h-3 w-1/2 bg-muted mb-3" />
                      <Skeleton className="h-3 w-full bg-muted" />
                    </div>
                  ))}
                </div>
              ) : hasSearched && results.length === 0 ? (
                <div className="text-center py-20 text-muted-foreground">
                  <p>No results found. Try a different query.</p>
                </div>
              ) : hasSearched ? (
                <div className="grid grid-cols-1 gap-3">
                  {results.map((r, i) => (
                    <ResultCard
                      key={i}
                      result={r}
                      selected={selected.has(i)}
                      onToggle={() => toggleResult(i)}
                    />
                  ))}
                </div>
              ) : (
                <div className="flex flex-col items-center justify-center py-20 text-center">
                  <div className="w-16 h-16 rounded-2xl bg-primary/10 flex items-center justify-center mb-4">
                    <Sparkles className="w-8 h-8 text-primary" />
                  </div>
                  <h3 className="text-lg font-medium text-foreground mb-2">
                    Start Your Research
                  </h3>
                  <p className="text-sm text-muted-foreground max-w-sm">
                    Search for mathematical models, datasets, code examples, formulas, and competition papers.
                  </p>
                </div>
              )}
            </div>
          </ScrollArea>

          {/* Save Bar */}
          {selectedCount > 0 && (
            <div className="sticky bottom-0 p-4 border-t border-border bg-background/80 backdrop-blur-sm">
              <div className="flex items-center justify-between max-w-4xl mx-auto">
                <span className="text-sm text-muted-foreground">
                  {selectedCount} selected
                  {agentStatus !== "ready" && (
                    <span className="text-amber-400 ml-2">
                      ⚠ Agent offline — cloud save only
                    </span>
                  )}
                </span>
                <Button
                  onClick={handleSave}
                  disabled={isSaving}
                  className="bg-primary text-primary-foreground hover:bg-primary/90"
                >
                  {isSaving ? (
                    <Loader2 className="w-4 h-4 mr-2 animate-spin" />
                  ) : (
                    <Sparkles className="w-4 h-4 mr-2" />
                  )}
                  Save {selectedCount} Item{selectedCount > 1 ? "s" : ""}
                </Button>
              </div>
            </div>
          )}
        </>
      )}

      {/* ── Research Library Tab ── */}
      {activeTab === "library" && (
        <ScrollArea className="flex-1">
          <div className="p-4 max-w-4xl mx-auto">
            {items.length === 0 ? (
              <div className="text-center py-20 text-muted-foreground">
                <BookOpen className="w-12 h-12 mx-auto mb-4 opacity-30" />
                <p>No saved research items yet.</p>
                <p className="text-sm mt-1">
                  Switch to the Search tab to find and save references.
                </p>
              </div>
            ) : (
              <div className="space-y-3">
                {/* Filter */}
                <div className="flex gap-2 mb-4">
                  <select
                    className="px-3 py-1.5 text-xs bg-muted border border-border rounded-lg text-foreground"
                    onChange={(e) => {
                      if (!e.target.value) {
                        loadLibrary()
                      } else {
                        listResearchItems(projectId, e.target.value).then(setItems).catch(console.error)
                      }
                    }}
                  >
                    <option value="">All Types</option>
                    {SEARCH_TYPES.map((t) => (
                      <option key={t.value} value={t.value}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                  <Button
                    variant="ghost"
                    size="sm"
                    className="text-xs"
                    onClick={loadLibrary}
                  >
                    Refresh
                  </Button>
                </div>
                {items.map((item) => (
                  <ItemCard
                    key={item.id}
                    item={item}
                    onDelete={handleDelete}
                  />
                ))}
              </div>
            )}
          </div>
        </ScrollArea>
      )}
    </div>
  )
}
```

- [ ] **Step 2: Commit**

```bash
git add components/dashboard/main-workspace.tsx
git commit -m "feat: MainWorkspace refactor — SearchTab + ResearchLibraryTab with real API"
```

---

### Task 12: Agent — CreateFile Message + Path Safety

**Files:**
- Modify: `agent/src/ws_client.rs` — add `CreateFile` variant + handler
- Modify: `server/src/agent_bridge/handlers.rs` — route `create_file` to agent

- [ ] **Step 1: Add `CreateFile` to AgentMessage enum in `agent/src/ws_client.rs`**

In `agent/src/ws_client.rs`, add the variant to the enum (around line 44):

```rust
#[serde(rename = "create_file")]
CreateFile { path: String, content: String },
```

- [ ] **Step 2: Add path safety validation function**

Add before the `run` function in `agent/src/ws_client.rs`:

```rust
fn validate_create_path(work_dir: &std::path::Path, relative_path: &str) -> Result<std::path::PathBuf, String> {
    // Reject absolute paths
    if relative_path.starts_with('/') || relative_path.starts_with('\\') {
        return Err("absolute path rejected".into());
    }
    // Reject path traversal
    if relative_path.contains("..") {
        return Err("path traversal rejected".into());
    }
    // Resolve under work_dir
    let resolved = work_dir.join(relative_path);
    // Canonicalize work_dir
    let canon_work = work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf());
    // For the resolved path, we can only canonicalize if the parent exists
    // So just verify the resolved path starts with canon_work
    if let Ok(canon) = resolved.canonicalize() {
        if !canon.starts_with(&canon_work) {
            return Err("path escapes workspace".into());
        }
    } else {
        // Path doesn't exist yet — check that the string prefix matches
        if !resolved.to_string_lossy().starts_with(canon_work.to_string_lossy().as_ref()) {
            return Err("path escapes workspace".into());
        }
    }
    Ok(resolved)
}
```

- [ ] **Step 3: Add handler in the message loop**

Add a new match arm in the server message handler (after the `AgentMessage::NewFolder` arm, around line 152):

```rust
AgentMessage::CreateFile { path, content } => {
    match validate_create_path(&current_work_dir, &path) {
        Ok(resolved) => {
            if let Some(parent) = resolved.parent() {
                if let Err(err) = std::fs::create_dir_all(parent) {
                                    let _ = outbound_tx.send(AgentMessage::AgentError {
                                        message: format!("failed to create parent dir: {err:#}"),
                                    });
                                    continue;
                                }
                            }
                            if resolved.exists() {
                                tracing::warn!("create_file: path already exists, skipping: {}", path);
                                continue;
                            }
                            if let Err(err) = std::fs::write(&resolved, &content) {
                                let _ = outbound_tx.send(AgentMessage::AgentError {
                                    message: format!("failed to write file: {err:#}"),
                                });
                            } else {
                                tracing::info!("create_file: wrote {}", path);
                            }
                        }
                        Err(err) => {
                            let _ = outbound_tx.send(AgentMessage::AgentError {
                                message: format!("create_file rejected: {err}"),
                            });
                        }
                    }
                }
```

- [ ] **Step 4: Add `create_file` to the server-side routing**

In `server/src/agent_bridge/handlers.rs`, in `handle_frontend` (the `msg_type` match around line 206), add `"create_file"` to the list of messages forwarded to the agent — but actually, `create_file` goes *from server to agent*, so it needs to be in `handle_agent` received from the agent bridge... 

Wait, re-reading the flow: the server research handler sends `create_file` to the agent via `bridge.send_to_agent()`. This uses the outbound channel directly — no routing needed in the agent WebSocket handler. The agent receives it as a server message and handles it. So the server-side `agent_bridge/handlers.rs` doesn't need changes for the routing.

The `handle_frontend` function already forwards `"terminal_input" | "terminal_resize" | "claude_command" | "open_file" | "list_files" | "change_work_dir" | "new_file" | "new_folder"` to the agent. We do NOT add `"create_file"` here because frontends never send `create_file` — only the server does.

So no server-side routing changes needed beyond what's already done in the research handler.

- [ ] **Step 5: Build agent**

Run: `cd agent && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles.

- [ ] **Step 6: Commit**

```bash
git add agent/src/ws_client.rs
git commit -m "feat: Agent CreateFile message — path validation + file creation"
```

---

### Task 13: Integration — Build & Verify Full Stack

- [ ] **Step 1: Build server**

Run: `cd server && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles without error.

- [ ] **Step 2: Build agent**

Run: `cd agent && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles without error.

- [ ] **Step 3: Build frontend**

Run: `cd D:/5_user/mathmodel && npx next build 2>&1`
Expected: Build succeeds (may have warnings but no errors).

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "chore: full stack build verification for Phase 7a-0 through 7a-4"
```

---

### Task 14: Remove Tavily (Phase 7a-5)

**Files:**
- Delete: `server/src/ai/adaptor/tavily.rs`
- Modify: `server/src/ai/adaptor/mod.rs`
- Modify: `server/src/ai/handlers.rs`
- Modify: `server/src/ai/model.rs`

- [ ] **Step 1: Delete `server/src/ai/adaptor/tavily.rs`**

```bash
rm server/src/ai/adaptor/tavily.rs
```

- [ ] **Step 2: Update `server/src/ai/adaptor/mod.rs`**

Remove the tavily module declaration and the constant match branch:

```rust
// Before:
pub mod anthropic;
pub mod openai;
pub mod tavily;

// ...

fn get_adaptor(ctype: i32, channel_name: &str) -> Box<dyn Adaptor> {
    match ctype {
        channel_type::ANTHROPIC => Box::new(AnthropicAdaptor),
        channel_type::TAVILY => Box::new(TavilyAdaptor),
        _ => Box::new(OpenAIAdaptor { ... }),
    }
}

// After:
pub mod anthropic;
pub mod openai;

// ...

fn get_adaptor(ctype: i32, channel_name: &str) -> Box<dyn Adaptor> {
    match ctype {
        channel_type::ANTHROPIC => Box::new(AnthropicAdaptor),
        _ => Box::new(OpenAIAdaptor {
            custom_provider: Some(channel_name.to_string()),
        }),
    }
}
```

Also remove the `use crate::ai::adaptor::tavily::TavilyAdaptor;` import if present.

- [ ] **Step 3: Remove `/ai/search` from `server/src/ai/handlers.rs`**

In the `routes()` function, remove the `.route("/ai/search", post(search))` line (line 24).

Remove the `search` handler function (lines 231-252).

Remove unused imports: `use crate::ai::adaptor::tavily::TavilyAdaptor;`

- [ ] **Step 4: Remove `SearchRequest`/`SearchResponse`/`SearchResult` from `server/src/ai/model.rs`**

Delete lines 136-155 (the three search-related structs).

- [ ] **Step 5: Build and verify**

Run: `cd server && export PATH="$HOME/.cargo/bin:$PATH" && cargo build 2>&1`
Expected: Compiles. No references to tavily or `/ai/search`.

- [ ] **Step 6: Commit**

```bash
git add server/src/ai/
git commit -m "feat: remove Tavily adaptor and /ai/search endpoint (replaced by /research/search)"
```

---

## Completion Checklist

- [ ] `ensure_column` runs idempotently, 005 columns exist after restart
- [ ] Tabbit `insert_research_item` still works (has all old columns, new columns get defaults)
- [ ] `POST /research/search` returns results when Morphic is running, 503 when not
- [ ] `POST /research/items` saves to DB, creates cloud .md file with CRDT state, attempts Agent local file
- [ ] `GET/PATCH/DELETE /research/items` CRUD works with auth
- [ ] Agent status broadcasts `"ready"` / `"disconnected"`
- [ ] Frontend SearchTab calls real API, displays results, saves items
- [ ] Frontend ResearchLibraryTab lists/filters/edits/deletes items
- [ ] Agent `create_file` rejects `..`, absolute paths, escapes workspace
- [ ] Agent `create_file` creates parent directories, skips existing files
- [ ] Tavily code completely removed, no compilation errors
- [ ] Unit tests (if any existing) pass
