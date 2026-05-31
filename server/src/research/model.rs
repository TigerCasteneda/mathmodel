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
    pub cloud_file_id: Option<String>,
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
