use async_trait::async_trait;
use claude_code_rs::mcp::ToolExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

pub use claude_code_rs::api::ChatMessage;
use super::config::AiConfig;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentType {
    Architect,
    Planner,
    CodeReviewer,
    SecurityReviewer,
    Explorer,
    GeneralPurpose,
}

impl AgentType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "architect" => Some(Self::Architect),
            "planner" => Some(Self::Planner),
            "code_reviewer" | "reviewer" => Some(Self::CodeReviewer),
            "security_reviewer" | "security" => Some(Self::SecurityReviewer),
            "explorer" | "explore" => Some(Self::Explorer),
            "general" | "general_purpose" => Some(Self::GeneralPurpose),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentDefinition {
    pub agent_type: AgentType,
    pub name: String,
    pub description: String,
    pub tools: Vec<String>,
}

fn builtin_agents() -> Vec<AgentDefinition> {
    vec![
        AgentDefinition {
            agent_type: AgentType::Architect,
            name: "Architect".into(),
            description: "Software architecture specialist for system design and technical decisions.".into(),
            tools: vec!["file_read".into(), "read_file".into(), "list_files".into(), "search_files".into(), "web_search".into()],
        },
        AgentDefinition {
            agent_type: AgentType::Planner,
            name: "Planner".into(),
            description: "Implementation planning specialist for breaking down complex features.".into(),
            tools: vec!["file_read".into(), "list_files".into(), "search_files".into(), "web_search".into()],
        },
        AgentDefinition {
            agent_type: AgentType::CodeReviewer,
            name: "Code Reviewer".into(),
            description: "Code review specialist for quality, security, and maintainability.".into(),
            tools: vec!["file_read".into(), "search_files".into(), "list_files".into()],
        },
        AgentDefinition {
            agent_type: AgentType::SecurityReviewer,
            name: "Security Reviewer".into(),
            description: "Security vulnerability detection and remediation specialist.".into(),
            tools: vec!["file_read".into(), "search_files".into(), "web_search".into()],
        },
        AgentDefinition {
            agent_type: AgentType::Explorer,
            name: "Explorer".into(),
            description: "Codebase exploration specialist for broad fan-out searches.".into(),
            tools: vec!["file_read".into(), "list_files".into(), "search_files".into(), "web_search".into(), "fetch_url".into()],
        },
        AgentDefinition {
            agent_type: AgentType::GeneralPurpose,
            name: "General Purpose".into(),
            description: "Catch-all agent for any task that doesn't fit a specialized agent.".into(),
            tools: vec!["file_read".into(), "file_write".into(), "file_edit".into(), "list_files".into(), "search_files".into(), "git_operations".into()],
        },
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentSession {
    pub id: String,
    pub agent_type: String,
    pub status: String, // "running" | "completed" | "failed"
    pub prompt: String,
    pub result: Option<String>,
}

#[derive(Clone)]
pub struct AgentOrchestrator {
    definitions: Vec<AgentDefinition>,
    sessions: Arc<RwLock<HashMap<String, AgentSession>>>,
    app_handle: AppHandle,
    work_dir: std::path::PathBuf,
}

impl AgentOrchestrator {
    pub fn new(
        app_handle: AppHandle,
        work_dir: std::path::PathBuf,
    ) -> Self {
        Self {
            definitions: builtin_agents(),
            sessions: Arc::new(RwLock::new(HashMap::new())),
            app_handle,
            work_dir,
        }
    }

    pub fn list_agents(&self) -> Vec<AgentDefinition> {
        self.definitions.clone()
    }

    pub async fn spawn(
        &self,
        agent_type_str: &str,
        prompt: &str,
        config: AiConfig,
    ) -> anyhow::Result<String> {
        let agent_type = AgentType::from_str(agent_type_str)
            .ok_or_else(|| anyhow::anyhow!("unknown agent type: {agent_type_str}"))?;
        let definition = self.definitions
            .iter()
            .find(|d| d.agent_type == agent_type)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("agent definition not found"))?;

        let session_id = uuid::Uuid::new_v4().to_string();
        let session = AgentSession {
            id: session_id.clone(),
            agent_type: agent_type_str.to_string(),
            status: "running".into(),
            prompt: prompt.to_string(),
            result: None,
        };

        {
            let mut sessions = self.sessions.write().await;
            sessions.insert(session_id.clone(), session.clone());
        }

        // Emit agent start event
        let _ = self.app_handle.emit("chat:agent_start", &session);

        // Run agent as background task
        let sessions = self.sessions.clone();
        let app_handle = self.app_handle.clone();
        let sid = session_id.clone();
        let p = prompt.to_string();
        let def = definition.clone();
        let wd = self.work_dir.clone();

        tokio::spawn(async move {
            let result = run_agent_task(config, def, p, wd, app_handle, sid.clone()).await;
            let mut sessions = sessions.write().await;
            if let Some(s) = sessions.get_mut(&sid) {
                s.status = if result.is_ok() { String::from("completed") } else { String::from("failed") };
                s.result = result.ok();
            }
        });

        Ok(session_id)
    }

    pub async fn status(&self, session_id: &str) -> Option<AgentSession> {
        self.sessions.read().await.get(session_id).cloned()
    }

    pub async fn list_sessions(&self) -> Vec<AgentSession> {
        self.sessions.read().await.values().cloned().collect()
    }
}

async fn run_agent_task(
    config: AiConfig,
    agent: AgentDefinition,
    prompt: String,
    work_dir: std::path::PathBuf,
    app_handle: AppHandle,
    session_id: String,
) -> anyhow::Result<String> {
    let client = claude_code_rs::ApiClient::new(config.to_claude_settings(work_dir));
    let system = format!(
        "You are a specialized {}. {}\n\nYou have access to these tools: {}. Return only the final result. Do NOT ask the user questions.",
        agent.name,
        agent.description,
        agent.tools.join(", ")
    );

    let messages = vec![
        claude_code_rs::api::ChatMessage::system(&system),
        claude_code_rs::api::ChatMessage::user(&prompt),
    ];

    let tool_defs: Vec<claude_code_rs::api::ToolDefinition> = agent.tools.iter().map(|t| {
        claude_code_rs::api::ToolDefinition::new(t.clone(), String::new(), json!({}))
    }).collect();

    match client.chat(messages, Some(tool_defs)).await {
        Ok(response) => {
            let content = response.choices.first().and_then(|c| c.message.content.clone()).unwrap_or_default();
            let _ = app_handle.emit("chat:agent_complete", &serde_json::json!({
                "session_id": session_id,
                "result": content
            }));
            Ok(content)
        }
        Err(e) => {
            let _ = app_handle.emit("chat:agent_error", &serde_json::json!({
                "session_id": session_id,
                "error": e.to_string()
            }));
            Err(anyhow::anyhow!("agent failed: {e}"))
        }
    }
}

// ── Tool Executors for agent management ──

pub struct AgentSpawnExecutor {
    pub orchestrator: Arc<AgentOrchestrator>,
    pub config: AiConfig,
}

#[async_trait]
impl ToolExecutor for AgentSpawnExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let agent_type = input["agent_type"].as_str()
            .ok_or_else(|| anyhow::anyhow!("agent_type required"))?;
        let prompt = input["prompt"].as_str()
            .ok_or_else(|| anyhow::anyhow!("prompt required"))?;
        let session_id = self.orchestrator.spawn(agent_type, prompt, self.config.clone()).await?;
        Ok(json!({ "session_id": session_id, "status": "running" }))
    }
}

pub struct AgentListExecutor {
    pub orchestrator: Arc<AgentOrchestrator>,
}

#[async_trait]
impl ToolExecutor for AgentListExecutor {
    async fn execute(&self, _input: Value) -> anyhow::Result<Value> {
        let agents = self.orchestrator.list_agents();
        Ok(serde_json::to_value(&agents)?)
    }
}

pub struct AgentStatusExecutor {
    pub orchestrator: Arc<AgentOrchestrator>,
}

#[async_trait]
impl ToolExecutor for AgentStatusExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let session_id = input["session_id"].as_str()
            .ok_or_else(|| anyhow::anyhow!("session_id required"))?;
        match self.orchestrator.status(session_id).await {
            Some(s) => Ok(serde_json::to_value(&s)?),
            None => Ok(json!({ "error": "session not found" })),
        }
    }
}
