use crate::ai::adaptor::Adaptor;
use crate::ai::model::*;
use crate::error::AppError;
use async_trait::async_trait;

pub struct TavilyAdaptor;

#[async_trait]
impl Adaptor for TavilyAdaptor {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        base_url.trim_end_matches('/').to_string()
    }

    fn build_headers(&self, _api_key: &str) -> Vec<(String, String)> {
        vec![("Content-Type".into(), "application/json".into())]
    }

    fn convert_request(
        &self,
        req: &ChatCompletionRequest,
        _model: &str,
    ) -> Result<serde_json::Value, AppError> {
        let query = req
            .messages
            .last()
            .map(|m| m.content.clone())
            .unwrap_or_default();

        Ok(serde_json::json!({
            "api_key": "",
            "query": query,
            "max_results": 5,
        }))
    }

    async fn parse_response(&self, body: &str) -> Result<(serde_json::Value, Usage), AppError> {
        let v: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| AppError::Internal(format!("tavily parse error: {}", e)))?;

        let results_text = v["results"]
            .as_array()
            .map(|results| {
                results
                    .iter()
                    .enumerate()
                    .map(|(i, r)| {
                        format!(
                            "{}. **{}**\n   {}\n   {}",
                            i + 1,
                            r["title"].as_str().unwrap_or(""),
                            r["url"].as_str().unwrap_or(""),
                            r["content"].as_str().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n")
            })
            .unwrap_or_default();

        let usage = Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        };

        let openai_resp = serde_json::json!({
            "id": format!("search-{}", uuid::Uuid::new_v4()),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": "tavily-search",
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": results_text },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 0,
                "completion_tokens": 0,
                "total_tokens": 0,
            }
        });

        Ok((openai_resp, usage))
    }

    fn provider_name(&self) -> &str {
        "tavily"
    }
}
