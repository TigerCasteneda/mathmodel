use serde::{Deserialize, Serialize};

/// Matches research_items after the Phase 9 research migrations.
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
    pub cloud_file_id: Option<String>,
    pub methodology: String,
    pub key_parameters: String,
    pub ai_relevance: String,
    pub raw_json: String,
    pub created_at: i64,
    pub updated_at: i64,
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
    pub summary: Option<String>,
    pub authors: Option<String>,
    pub publish_year: Option<i32>,
    pub keywords: Option<String>,
    pub methodology: Option<String>,
    pub key_parameters: Option<String>,
    pub ai_relevance: Option<String>,
    pub relevance_score: Option<f64>,
    pub bibtex: Option<String>,
    pub raw_json: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct SaveItemsResponse {
    pub saved: i32,
    pub items: Vec<ResearchItem>,
    pub files_created: i32,
    pub warnings: Vec<String>,
    /// Mirror metadata for any host agent running in Host Local mode. The
    /// server is still authoritative; clients use this to write a one-way
    /// local copy into `work_dir/references/`. Empty for guest-only callers.
    pub mirrors: Vec<ResearchFileMirror>,
}

/// Per-item mirror payload returned alongside the canonical save response.
/// Allows a Host Local agent to write a byte-identical copy of the cloud-side
/// `.md` (and `.bib` when present) to its local workspace without re-running
/// the AI extraction or the slug derivation.
#[derive(Debug, Serialize)]
pub struct ResearchFileMirror {
    /// ID of the cloud `files` row holding the canonical markdown body.
    pub cloud_file_id: String,
    /// Server-computed filename for the `.md`, e.g. `bayesian_sir-7f3a8c12.md`.
    pub file_name: String,
    /// Exact body written into `crdt_docs` for the `.md` file.
    pub body_md: String,
    /// Server-computed filename for the `.bib`, present when bibtex was extracted.
    pub bib_file_name: Option<String>,
    /// Exact body written into `crdt_docs` for the `.bib` file.
    pub body_bib: Option<String>,
    /// Title as supplied by the client; used for the local manifest entry.
    pub title: String,
    /// Original URL; used for the local manifest entry.
    pub url: String,
}

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

fn default_sort() -> String {
    "created_at".into()
}

fn default_order() -> String {
    "desc".into()
}

fn default_limit() -> i32 {
    50
}

#[derive(Debug, Deserialize)]
pub struct UpdateItemRequest {
    pub notes: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ItemPathParam {
    pub item_id: String,
}
