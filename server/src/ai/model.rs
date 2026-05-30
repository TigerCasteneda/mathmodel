use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub channel_type: i32,
    pub base_url: String,
    pub api_key: String,
    pub models: String,
    pub model_mapping: String,
    pub weight: i32,
    pub status: i32,
    pub config: String,
    pub created_at: i64,
    pub updated_at: i64,
}

impl Channel {
    pub fn parsed_models(&self) -> Vec<String> {
        self.models
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn supports_model(&self, model: &str) -> bool {
        self.models.trim() == "*" || self.parsed_models().iter().any(|m| m == model)
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    pub name: String,
    pub channel_type: i32,
    pub base_url: String,
    pub api_key: String,
    pub models: String,
    pub model_mapping: Option<String>,
    pub weight: Option<i32>,
    pub config: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateChannelRequest {
    pub name: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub models: Option<String>,
    pub model_mapping: Option<String>,
    pub weight: Option<i32>,
    pub status: Option<i32>,
    pub config: Option<String>,
}

pub mod channel_type {
    pub const OPENAI_COMPATIBLE: i32 = 1;
    pub const ANTHROPIC: i32 = 18;
    pub const TAVILY: i32 = 99;
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub project_id: String,
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: Option<f64>,
    pub max_tokens: Option<i32>,
    pub stream: Option<bool>,
    pub top_p: Option<f64>,
    pub n: Option<i32>,
    pub stop: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Choice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct AiUsageLog {
    pub id: String,
    pub user_id: String,
    pub project_id: String,
    pub channel_id: Option<String>,
    pub model: String,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
    pub status: String,
    pub error_message: Option<String>,
    pub duration_ms: i32,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

#[derive(Debug, Serialize)]
pub struct ModelListResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    pub project_id: String,
    pub query: String,
    pub max_results: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct SearchResponse {
    pub query: String,
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    pub score: f64,
}
