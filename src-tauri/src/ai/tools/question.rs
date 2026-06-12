use async_trait::async_trait;
use claude_code_rs::mcp::ToolExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{oneshot, Mutex};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionItem {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    #[serde(rename = "multiSelect", default)]
    pub multi_select: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuestionEvent {
    pub request_id: String,
    pub conversation_id: String,
    pub questions: Vec<QuestionItem>,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QuestionAnswer {
    pub request_id: String,
    pub answers: Value, // { "question text": { "answer": "selected option", ... } }
}

#[derive(Clone)]
pub struct QuestionStore {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<String>>>>,
}

impl QuestionStore {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn wait_for_answer(&self, request_id: String) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        {
            let mut guard = self.pending.lock().await;
            guard.insert(request_id, tx);
        }
        rx.await.ok()
    }

    pub async fn resolve(&self, request_id: &str, answers: &str) -> bool {
        let mut guard = self.pending.lock().await;
        if let Some(tx) = guard.remove(request_id) {
            tx.send(answers.to_string()).is_ok()
        } else {
            false
        }
    }
}

pub struct AskUserQuestionExecutor {
    pub question_store: QuestionStore,
    pub app_handle: AppHandle,
    pub conversation_id: String,
}

#[async_trait]
impl ToolExecutor for AskUserQuestionExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let questions = input["questions"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("questions array is required"))?;

        if questions.is_empty() {
            return Err(anyhow::anyhow!("at least one question is required"));
        }

        let validated: Vec<QuestionItem> = questions
            .iter()
            .enumerate()
            .map(|(i, q)| {
                let question = q["question"].as_str().unwrap_or("").to_string();
                let header = q["header"].as_str().unwrap_or("Question").to_string();
                let multi_select = q["multiSelect"].as_bool().unwrap_or(false);
                let options: Vec<QuestionOption> = q["options"]
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .map(|o| QuestionOption {
                                label: o["label"].as_str().unwrap_or("").to_string(),
                                description: o["description"]
                                    .as_str()
                                    .unwrap_or("")
                                    .to_string(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                if question.is_empty() {
                    anyhow::bail!("question[{i}]: question text is required");
                }
                if options.len() < 2 {
                    anyhow::bail!("question[{i}]: at least 2 options required");
                }

                Ok(QuestionItem {
                    question,
                    header,
                    options,
                    multi_select,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;

        let request_id = uuid::Uuid::new_v4().to_string();
        let expires_at_ms = chrono::Utc::now().timestamp_millis() + 120_000; // 2 min timeout

        let event = QuestionEvent {
            request_id: request_id.clone(),
            conversation_id: self.conversation_id.clone(),
            questions: validated,
            expires_at_ms,
        };

        self.app_handle
            .emit("chat:question", &event)
            .map_err(|e| anyhow::anyhow!("failed to emit question event: {e}"))?;

        // Wait for answer or timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(120),
            self.question_store.wait_for_answer(request_id.clone()),
        )
        .await;

        match result {
            Ok(Some(answer)) => {
                let parsed: Value =
                    serde_json::from_str(&answer).unwrap_or(json!({ "answer": answer }));
                Ok(parsed)
            }
            Ok(None) => Ok(json!({ "status": "cancelled" })),
            Err(_) => {
                // Timeout - clean up
                let mut guard = self.question_store.pending.lock().await;
                guard.remove(&request_id);
                Ok(json!({ "status": "timeout" }))
            }
        }
    }
}
