use async_trait::async_trait;
use claude_code_rs::mcp::ToolExecutor;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

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

    #[allow(dead_code)]
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
    let client = claude_code_rs::ApiClient::new(config.to_claude_settings(work_dir.clone()));
    let system = format!(
        "You are a specialized {}. {}\n\nReturn only your final analysis. Be thorough but concise. Do NOT ask questions — produce output.",
        agent.name, agent.description
    );

    let tool_defs = agent_tool_defs(&agent.tools);
    let mut messages = vec![
        claude_code_rs::api::ChatMessage::system(&system),
        claude_code_rs::api::ChatMessage::user(&prompt),
    ];

    // Tool loop: up to 4 turns
    for _turn in 0..4 {
        let response = client.chat(messages.clone(), Some(tool_defs.clone())).await
            .map_err(|e| anyhow::anyhow!("agent API error: {e}"))?;

        let choice = response.choices.first()
            .ok_or_else(|| anyhow::anyhow!("no response from agent"))?;

        let finish = choice.finish_reason.as_deref().unwrap_or("stop");

        if finish == "stop" {
            let content = choice.message.content.clone().unwrap_or_default();
            let _ = app_handle.emit("chat:agent_complete", &serde_json::json!({
                "session_id": &session_id, "result": &content
            }));
            return Ok(content);
        }

        // Tool calls: execute and feed results
        if let Some(ref tool_calls) = choice.message.tool_calls {
            if tool_calls.is_empty() {
                let content = choice.message.content.clone().unwrap_or_default();
                let _ = app_handle.emit("chat:agent_complete", &serde_json::json!({
                    "session_id": &session_id, "result": &content
                }));
                return Ok(content);
            }

            messages.push(choice.message.clone());

            for tc in tool_calls {
                let result = execute_agent_tool(&tc.function.name, &tc.function.arguments, &work_dir).await;
                messages.push(claude_code_rs::api::ChatMessage::tool(&tc.id, result));
            }
            continue;
        }

        let content = choice.message.content.clone().unwrap_or_default();
        let _ = app_handle.emit("chat:agent_complete", &serde_json::json!({
            "session_id": &session_id, "result": &content
        }));
        return Ok(content);
    }

    Err(anyhow::anyhow!("agent exceeded max tool-calling turns"))
}

fn agent_tool_defs(tools: &[String]) -> Vec<claude_code_rs::api::ToolDefinition> {
    tools.iter().map(|name| {
        let (desc, params) = agent_tool_schema(name);
        claude_code_rs::api::ToolDefinition::new(name.clone(), desc, params)
    }).collect()
}

fn agent_tool_schema(name: &str) -> (String, serde_json::Value) {
    use serde_json::json;
    match name {
        "file_read" | "read_file" => (
            "Read a file from the workspace".into(),
            json!({ "type": "object", "properties": { "path": { "type": "string" } }, "required": ["path"] }),
        ),
        "file_write" | "write_file" => (
            "Write content to a file".into(),
            json!({ "type": "object", "properties": { "path": { "type": "string" }, "content": { "type": "string" } }, "required": ["path", "content"] }),
        ),
        "file_edit" => (
            "Edit a file by replacing one exact string with another".into(),
            json!({ "type": "object", "properties": { "path": { "type": "string" }, "old_content": { "type": "string" }, "new_content": { "type": "string" } }, "required": ["path", "old_content", "new_content"] }),
        ),
        "list_files" => (
            "List workspace files".into(),
            json!({ "type": "object", "properties": {} }),
        ),
        "search_files" => (
            "Search for text pattern in workspace files".into(),
            json!({ "type": "object", "properties": { "pattern": { "type": "string" }, "path": { "type": "string" } }, "required": ["pattern"] }),
        ),
        "execute_command" => (
            "Run a shell command".into(),
            json!({ "type": "object", "properties": { "command": { "type": "string" }, "cwd": { "type": "string" } }, "required": ["command"] }),
        ),
        "git_operations" => (
            "Run git commands (status, log, diff, branch only for code reviewer agents)".into(),
            json!({ "type": "object", "properties": { "operation": { "type": "string" } }, "required": ["operation"] }),
        ),
        "web_search" => (
            "Search the web".into(),
            json!({ "type": "object", "properties": { "query": { "type": "string" } }, "required": ["query"] }),
        ),
        "fetch_url" => (
            "Fetch a URL as markdown".into(),
            json!({ "type": "object", "properties": { "url": { "type": "string" } }, "required": ["url"] }),
        ),
        _ => (format!("Execute {name}"), json!({ "type": "object", "properties": {} })),
    }
}

async fn execute_agent_tool(name: &str, arguments: &str, work_dir: &std::path::Path) -> String {
    let args: serde_json::Value = serde_json::from_str(arguments).unwrap_or_default();
    let skip = |e: std::io::Error| format!("Error: {e}");

    match name {
        "file_read" | "read_file" => {
            let path = args["path"].as_str().unwrap_or("");
            let full = work_dir.join(path);
            std::fs::read_to_string(&full).unwrap_or_else(skip)
        }
        "list_files" => list_files_recursive(work_dir, 3),
        "search_files" => {
            let pattern = args["pattern"].as_str().unwrap_or("");
            let path = args["path"].as_str().unwrap_or(".");
            search_workspace(work_dir, pattern, path).unwrap_or_else(skip)
        }
        "execute_command" => {
            let cmd = args["command"].as_str().unwrap_or("");
            let cwd = args["cwd"].as_str().unwrap_or(".");
            let output = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .current_dir(work_dir.join(cwd))
                .output().await;
            match output {
                Ok(o) => format!("{}\n{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
                Err(e) => format!("Command failed: {e}"),
            }
        }
        "git_operations" => {
            let op = args["operation"].as_str().unwrap_or("status");
            let output = tokio::process::Command::new("git")
                .args([op, "--no-pager"]).current_dir(work_dir)
                .output().await;
            match output {
                Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
                Err(e) => format!("git failed: {e}"),
            }
        }
        "web_search" => {
            format!("[web_search not available in agent mode] Use file_read/list_files instead.")
        }
        "fetch_url" => {
            let url = args["url"].as_str().unwrap_or("");
            match reqwest::get(url).await {
                Ok(resp) => resp.text().await.unwrap_or_else(|e| format!("read error: {e}")),
                Err(e) => format!("fetch error: {e}"),
            }
        }
        _ => format!("Unknown tool: {name}")
    }
}

fn list_files_recursive(dir: &std::path::Path, max_depth: usize) -> String {
    let mut out = String::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let relative = path.strip_prefix(dir).unwrap_or(&path);
            out.push_str(&format!("{}\n", relative.display()));
            if path.is_dir() && max_depth > 0 {
                out.push_str(&list_files_recursive(&path, max_depth - 1));
            }
        }
    }
    out
}

fn search_workspace(dir: &std::path::Path, pattern: &str, subpath: &str) -> Result<String, std::io::Error> {
    let mut out = String::new();
    let search_dir = dir.join(subpath);
    if search_dir.is_dir() {
        for entry in std::fs::read_dir(&search_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    for (i, line) in content.lines().enumerate() {
                        if line.contains(pattern) {
                            let rel = path.strip_prefix(dir).unwrap_or(&path);
                            out.push_str(&format!("{}:{}: {}\n", rel.display(), i + 1, line));
                            if out.len() > 4000 { return Ok(out); }
                        }
                    }
                }
            }
        }
    }
    Ok(out)
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
