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
