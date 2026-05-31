pub mod anthropic;
pub mod openai;

use crate::ai::model::*;
use crate::error::AppError;
use async_trait::async_trait;

#[async_trait]
pub trait Adaptor: Send + Sync {
    /// Build the upstream request URL
    fn build_url(&self, base_url: &str, model: &str) -> String;

    /// Build request headers
    fn build_headers(&self, api_key: &str) -> Vec<(String, String)>;

    /// Convert OpenAI-format request to provider-specific JSON body
    fn convert_request(
        &self,
        req: &ChatCompletionRequest,
        model: &str,
    ) -> Result<serde_json::Value, AppError>;

    /// Parse provider response into OpenAI-format JSON + Usage
    async fn parse_response(&self, body: &str) -> Result<(serde_json::Value, Usage), AppError>;

    /// Provider name for logging
    fn provider_name(&self) -> &str;
}
