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
