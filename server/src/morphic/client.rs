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
