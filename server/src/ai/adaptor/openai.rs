use crate::ai::adaptor::Adaptor;
use crate::ai::model::*;
use crate::error::AppError;
use async_trait::async_trait;

pub struct OpenAIAdaptor {
    pub custom_provider: Option<String>,
}

#[async_trait]
impl Adaptor for OpenAIAdaptor {
    fn build_url(&self, base_url: &str, _model: &str) -> String {
        let base = base_url.trim_end_matches('/');
        format!("{}/chat/completions", base)
    }

    fn build_headers(&self, api_key: &str) -> Vec<(String, String)> {
        vec![
            ("Authorization".into(), format!("Bearer {}", api_key)),
            ("Content-Type".into(), "application/json".into()),
        ]
    }

    fn convert_request(
        &self,
        req: &ChatCompletionRequest,
        model: &str,
    ) -> Result<serde_json::Value, AppError> {
        let mut body = serde_json::json!({
            "model": model,
            "messages": req.messages.iter().map(|m| {
                serde_json::json!({"role": m.role, "content": m.content})
            }).collect::<Vec<_>>(),
        });

        if let Some(t) = req.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(mt) = req.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }
        if let Some(tp) = req.top_p {
            body["top_p"] = serde_json::json!(tp);
        }
        if let Some(n) = req.n {
            body["n"] = serde_json::json!(n);
        }
        if let Some(ref s) = req.stop {
            body["stop"] = serde_json::json!(s);
        }

        Ok(body)
    }

    async fn parse_response(&self, body: &str) -> Result<(serde_json::Value, Usage), AppError> {
        let v: serde_json::Value = serde_json::from_str(body)
            .map_err(|e| AppError::Internal(format!("openai parse error: {}", e)))?;

        let usage = v
            .get("usage")
            .map(|u| Usage {
                prompt_tokens: u["prompt_tokens"].as_i64().unwrap_or(0) as i32,
                completion_tokens: u["completion_tokens"].as_i64().unwrap_or(0) as i32,
                total_tokens: u["total_tokens"].as_i64().unwrap_or(0) as i32,
            })
            .unwrap_or(Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            });

        Ok((v, usage))
    }

    fn provider_name(&self) -> &str {
        self.custom_provider.as_deref().unwrap_or("openai")
    }
}
