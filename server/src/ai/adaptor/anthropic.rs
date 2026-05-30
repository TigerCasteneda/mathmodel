use crate::ai::adaptor::Adaptor;
use crate::ai::model::*;
use crate::error::AppError;
use async_trait::async_trait;

pub struct AnthropicAdaptor;

#[async_trait]
impl Adaptor for AnthropicAdaptor {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        format!("{}/messages", base)
    }

    fn build_headers(&self, api_key: &str) -> Vec<(String, String)> {
        vec![
            ("x-api-key".into(), api_key.to_string()),
            ("anthropic-version".into(), "2023-06-01".into()),
            ("Content-Type".into(), "application/json".into()),
        ]
    }

    fn convert_request(
        &self,
        req: &ChatCompletionRequest,
        model: &str,
    ) -> Result<serde_json::Value, AppError> {
        let system = req
            .messages
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.clone())
            .collect::<Vec<_>>()
            .join("\n");

        let messages: Vec<serde_json::Value> = req
            .messages
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| {
                serde_json::json!({
                    "role": if m.role == "assistant" { "assistant" } else { "user" },
                    "content": m.content
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": model,
            "max_tokens": req.max_tokens.unwrap_or(1024),
            "messages": messages,
        });

        if !system.is_empty() {
            body["system"] = serde_json::Value::String(system);
        }

        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }

        Ok(body)
    }

    async fn parse_response(&self, body: &str) -> Result<(serde_json::Value, Usage), AppError> {
        let v: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| AppError::Internal(format!("anthropic parse error: {}", e)))?;

        let usage = v
            .get("usage")
            .map(|u| Usage {
                prompt_tokens: u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0) as i32,
                completion_tokens: u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0)
                    as i32,
                total_tokens: 0,
            })
            .unwrap_or(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            });

        let content_text = v["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("");

        let openai_resp = serde_json::json!({
            "id": v["id"],
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": v["model"],
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": content_text },
                "finish_reason": v.get("stop_reason").and_then(|s| s.as_str()).unwrap_or("stop")
            }],
            "usage": {
                "prompt_tokens": usage.prompt_tokens,
                "completion_tokens": usage.completion_tokens,
                "total_tokens": usage.prompt_tokens + usage.completion_tokens,
            }
        });

        Ok((openai_resp, usage))
    }

    fn provider_name(&self) -> &str {
        "anthropic"
    }
}
