use async_trait::async_trait;
use claude_code_rs::api::ToolDefinition;
use claude_code_rs::mcp::{McpTool, ToolExecutor, ToolRegistry};
use claude_code_rs::ApiClient;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::RwLock;

use super::config::AiConfig;
use super::permissions::{PermissionDecision, PermissionStore};
use super::agent::{
    AgentListExecutor, AgentOrchestrator, AgentSpawnExecutor, AgentStatusExecutor,
};
use super::plan::{
    EnterPlanModeExecutor, ExitPlanModeExecutor, PlanService, PlanUpdateExecutor,
};
use super::skills::SkillRegistry;
use super::tasks::TaskStore;
use super::tools::git::GitExecutor;
use super::tools::question::{AskUserQuestionExecutor, QuestionStore};
use super::workspace::{
    build_workspace_provider, tool_optional_path_arg, tool_path_arg, WorkspaceContext,
    WorkspaceProvider,
};
use crate::agent::file_watcher::FileTreeItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionMode {
    Default,
    AcceptEdit,
    Auto,
    Bypass,
}

impl PermissionMode {
    pub fn from_option(value: Option<String>) -> Self {
        match value.as_deref() {
            Some("accept_edit") => Self::AcceptEdit,
            Some("auto") => Self::Auto,
            Some("bypass") => Self::Bypass,
            _ => Self::Default,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::AcceptEdit => "Accept Edit",
            Self::Auto => "Auto",
            Self::Bypass => "Bypass",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolExposure {
    Core,
    Deferred,
}

#[derive(Debug, Clone)]
struct ToolCatalogEntry {
    name: &'static str,
    description: &'static str,
    exposure: ToolExposure,
    keywords: &'static [&'static str],
    search_hint: &'static str,
    aliases: &'static [&'static str],
    is_concurrency_safe: bool,
    is_read_only: bool,
}

const TOOL_CATALOG: &[ToolCatalogEntry] = &[
    ToolCatalogEntry {
        name: "tool_search",
        description: "Find and enable optional tools by keyword or direct selection.",
        exposure: ToolExposure::Core,
        keywords: &["tool", "search", "discover", "enable"],
        search_hint: "Find deferred tools by name or capability.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "file_read",
        description: "Read a file from the current workspace.",
        exposure: ToolExposure::Core,
        keywords: &["read", "file", "inspect", "view"],
        search_hint: "Read workspace file contents.",
        aliases: &["Read"],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "read_file",
        description: "Alias for file_read.",
        exposure: ToolExposure::Core,
        keywords: &["read", "file", "alias"],
        search_hint: "Read workspace file contents.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "file_write",
        description: "Create or overwrite a file inside the current workspace.",
        exposure: ToolExposure::Core,
        keywords: &["write", "file", "create", "overwrite"],
        search_hint: "Create or overwrite a workspace file.",
        aliases: &["Write"],
        is_concurrency_safe: false,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "write_file",
        description: "Alias for file_write.",
        exposure: ToolExposure::Core,
        keywords: &["write", "file", "alias"],
        search_hint: "Create or overwrite a workspace file.",
        aliases: &[],
        is_concurrency_safe: false,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "web_search",
        description: "Search the web through the configured SearXNG host.",
        exposure: ToolExposure::Core,
        keywords: &["web", "search", "paper", "internet"],
        search_hint: "Search public web results.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "save_reference",
        description: "Save a useful reference into references/*.md in the workspace.",
        exposure: ToolExposure::Core,
        keywords: &["save", "reference", "paper", "research"],
        search_hint: "Save a research reference file.",
        aliases: &[],
        is_concurrency_safe: false,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "file_edit",
        description: "Edit a file by replacing one exact string with another.",
        exposure: ToolExposure::Deferred,
        keywords: &["edit", "replace", "patch", "file"],
        search_hint: "Edit a workspace file by exact string replacement.",
        aliases: &["Edit"],
        is_concurrency_safe: false,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "list_files",
        description: "List the current workspace file tree.",
        exposure: ToolExposure::Deferred,
        keywords: &["list", "tree", "files", "folder"],
        search_hint: "List workspace files and folders.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "execute_command",
        description: "Execute a shell command in Host Local mode.",
        exposure: ToolExposure::Deferred,
        keywords: &["shell", "bash", "command", "run", "test"],
        search_hint: "Run shell commands such as tests, build checks, and git status.",
        aliases: &["Bash", "Shell"],
        is_concurrency_safe: false,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "search_files",
        description: "Search for a text pattern in workspace files.",
        exposure: ToolExposure::Deferred,
        keywords: &["grep", "search", "pattern", "code"],
        search_hint: "Search workspace files for text patterns.",
        aliases: &["Grep"],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "fetch_url",
        description:
            "Fetch a URL as markdown through Jina Reader fallback. Use Research for Firecrawl search.",
        exposure: ToolExposure::Deferred,
        keywords: &["fetch", "url", "markdown", "webpage", "jina"],
        search_hint: "Fetch a webpage URL as markdown.",
        aliases: &["WebFetch"],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "start_background_task",
        description: "Start a background copilot task and report progress in the chat UI.",
        exposure: ToolExposure::Deferred,
        keywords: &["background", "subagent", "review", "research", "parallel"],
        search_hint: "Start a background research, review, modeling, or analysis task.",
        aliases: &["Task"],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "git_operations",
        description: "Execute git version control operations: status, add, commit, push, pull, log, diff, branch, checkout.",
        exposure: ToolExposure::Deferred,
        keywords: &["git", "commit", "push", "pull", "branch", "diff", "log", "version", "control"],
        search_hint: "Run git commands for version control in the workspace.",
        aliases: &["Git", "git"],
        is_concurrency_safe: false,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "task_create",
        description: "Create a new task in the task list to track progress and organize work.",
        exposure: ToolExposure::Core,
        keywords: &["task", "create", "todo", "track"],
        search_hint: "Create a task to track a piece of work.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "task_update",
        description: "Update the status, subject, or description of an existing task.",
        exposure: ToolExposure::Core,
        keywords: &["task", "update", "status", "progress"],
        search_hint: "Update task status, priority, or details.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "task_list",
        description: "List all tasks, optionally filtered by status or priority.",
        exposure: ToolExposure::Core,
        keywords: &["task", "list", "all", "status", "filter"],
        search_hint: "List current tasks.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "task_get",
        description: "Get full details of a specific task by ID.",
        exposure: ToolExposure::Core,
        keywords: &["task", "get", "detail", "view"],
        search_hint: "View details of a specific task.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "ask_user_question",
        description: "Ask the user a structured question with multiple-choice options when a decision is needed.",
        exposure: ToolExposure::Core,
        keywords: &["ask", "question", "user", "prompt", "choice", "decide"],
        search_hint: "Ask the user for input when a decision is needed.",
        aliases: &["AskUserQuestion"],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "agent_spawn",
        description: "Launch a specialized sub-agent for parallel work (architect, planner, reviewer, explorer, general).",
        exposure: ToolExposure::Core,
        keywords: &["agent", "spawn", "parallel", "subagent", "delegate"],
        search_hint: "Launch a sub-agent to handle a task in parallel.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "agent_list",
        description: "List available agent types and their capabilities.",
        exposure: ToolExposure::Core,
        keywords: &["agent", "list", "types", "available"],
        search_hint: "List available agent types.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "agent_status",
        description: "Check the status of a running agent session.",
        exposure: ToolExposure::Core,
        keywords: &["agent", "status", "session", "check"],
        search_hint: "Check agent session status.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: true,
    },
    ToolCatalogEntry {
        name: "enter_plan_mode",
        description: "Enter plan-only mode: restrict to read-only tools for exploring and designing before implementing.",
        exposure: ToolExposure::Core,
        keywords: &["plan", "design", "explore", "readonly", "mode"],
        search_hint: "Enter plan mode for read-only exploration and design.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "exit_plan_mode",
        description: "Exit plan mode and submit a structured plan for approval. Takes title and phases[] with steps.",
        exposure: ToolExposure::Core,
        keywords: &["plan", "submit", "exit", "approval"],
        search_hint: "Submit a plan for user approval before implementation.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "plan_update",
        description: "Update a plan phase status during plan execution (phase: title, status: pending|in_progress|completed|skipped).",
        exposure: ToolExposure::Core,
        keywords: &["plan", "update", "phase", "progress"],
        search_hint: "Update plan execution progress.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: false,
    },
    ToolCatalogEntry {
        name: "skill_invoke",
        description: "Invoke a registered skill by name (e.g. code-review, math-verify, model-fit, refactor, latex-compile).",
        exposure: ToolExposure::Core,
        keywords: &["skill", "invoke", "capability", "specialized"],
        search_hint: "Invoke a specialized skill for focused tasks.",
        aliases: &[],
        is_concurrency_safe: true,
        is_read_only: false,
    },
];

pub struct ModelerAiRuntime {
    client: ApiClient,
    registry: Arc<ToolRegistry>,
    workspace: Arc<dyn WorkspaceProvider>,
    workspace_label: &'static str,
    enabled_deferred_tools: Arc<RwLock<HashSet<String>>>,
    permission_mode: PermissionMode,
    permission_store: PermissionStore,
    #[allow(dead_code)]
    question_store: QuestionStore,
    app_handle: AppHandle,
    conversation_id: String,
    /// Authenticated user who owns this runtime's tool calls. Used to
    /// scope plan/hook state to the right account. The runtime is
    /// constructed per `ai_chat` invocation, where user_id is decoded
    /// from the Supabase JWT by the frontend.
    user_id: String,
}

impl ModelerAiRuntime {
    pub async fn new(
        config: AiConfig,
        context: WorkspaceContext,
        app_handle: AppHandle,
        conversation_id: String,
        user_id: String,
        permission_mode: PermissionMode,
        permission_store: PermissionStore,
        question_store: QuestionStore,
        agent: Arc<AgentOrchestrator>,
        plan: Arc<PlanService>,
        skills: Arc<SkillRegistry>,
    ) -> anyhow::Result<Self> {
        let client = ApiClient::new(config.to_claude_settings(context.work_dir.clone()));
        let workspace_label = context.label();
        let can_write = context.has_capability("files.write") && context.has_capability("ai.write");
        let work_dir = context.work_dir.clone();
        let workspace = build_workspace_provider(context, app_handle.clone())?;
        let registry = Arc::new(ToolRegistry::new());
        let enabled_deferred_tools = Arc::new(RwLock::new(HashSet::new()));

        let data_dir = app_handle
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."));
        register_workspace_tools(
            &registry,
            workspace.clone(),
            can_write,
            enabled_deferred_tools.clone(),
            app_handle.clone(),
            user_id.clone(),
            conversation_id.clone(),
            work_dir,
            data_dir,
            question_store.clone(),
        )
        .await;

        // Register agent tools
        registry
            .register(
                McpTool::new(
                    "agent_spawn",
                    "Launch a specialized sub-agent for parallel work. Use agent_list to see available types.",
                    json!({
                        "type": "object",
                        "properties": {
                            "agent_type": { "type": "string", "enum": ["architect", "planner", "code_reviewer", "security_reviewer", "explorer", "general"], "description": "Agent type" },
                            "prompt": { "type": "string", "description": "The task to delegate to the agent" }
                        },
                        "required": ["agent_type", "prompt"]
                    }),
                ),
                Arc::new(AgentSpawnExecutor {
                    orchestrator: agent.clone(),
                    config: config.clone(),
                }),
            )
            .await;

        registry
            .register(
                McpTool::new("agent_list", "List available agent types and their capabilities.", json!({ "type": "object", "properties": {} })),
                Arc::new(AgentListExecutor { orchestrator: agent.clone() }),
            )
            .await;

        registry
            .register(
                McpTool::new("agent_status", "Check the status of a running agent session.", json!({
                    "type": "object",
                    "properties": { "session_id": { "type": "string" } },
                    "required": ["session_id"]
                })),
                Arc::new(AgentStatusExecutor { orchestrator: agent }),
            )
            .await;

        // Register plan tools
        registry
            .register(
                McpTool::new("enter_plan_mode", "Enter plan-only mode: restrict to read-only tools for exploration before implementing.", json!({ "type": "object", "properties": {} })),
                Arc::new(EnterPlanModeExecutor {
                    plan_service: plan.clone(),
                    user_id: user_id.clone(),
                }),
            )
            .await;

        registry
            .register(
                McpTool::new("exit_plan_mode", "Exit plan mode and submit a structured plan with title and phases.", json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "phases": { "type": "array", "items": {
                            "type": "object",
                            "properties": {
                                "title": { "type": "string" },
                                "steps": { "type": "array", "items": { "type": "string" } }
                            },
                            "required": ["title"]
                        }}
                    },
                    "required": ["title", "phases"]
                })),
                Arc::new(ExitPlanModeExecutor {
                    plan_service: plan.clone(),
                    user_id: user_id.clone(),
                }),
            )
            .await;

        registry
            .register(
                McpTool::new("plan_update", "Update a plan phase status during execution.", json!({
                    "type": "object",
                    "properties": {
                        "phase": { "type": "string", "description": "Phase title" },
                        "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "skipped"] }
                    },
                    "required": ["phase", "status"]
                })),
                Arc::new(PlanUpdateExecutor {
                    plan_service: plan,
                    user_id: user_id.clone(),
                }),
            )
            .await;

        // Register skill tool
        registry
            .register(
                McpTool::new("skill_invoke", "Invoke a registered skill. Use skill names from the skill registry.", json!({
                    "type": "object",
                    "properties": {
                        "skill": { "type": "string", "description": "Skill name (e.g. code-review, math-verify, model-fit, refactor, latex-compile)" },
                        "input": { "type": "string", "description": "Task description for the skill" }
                    },
                    "required": ["skill", "input"]
                })),
                Arc::new(SkillInvokeExecutor { skills: skills.clone() }),
            )
            .await;

        Ok(Self {
            client,
            registry,
            workspace,
            workspace_label,
            enabled_deferred_tools,
            permission_mode,
            permission_store,
            question_store,
            app_handle,
            conversation_id,
            user_id,
        })
    }

    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    pub fn workspace_label(&self) -> &'static str {
        self.workspace_label
    }

    #[allow(dead_code)]
    pub fn question_store(&self) -> &QuestionStore {
        &self.question_store
    }

    pub fn permission_label(&self) -> &'static str {
        self.permission_mode.label()
    }

    pub async fn workspace_tree(&self) -> anyhow::Result<FileTreeItem> {
        self.workspace.list_files().await
    }

    pub async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        let enabled = self.enabled_deferred_tools.read().await.clone();
        let mut out = Vec::new();
        for tool in self.registry.list().await {
            if tool.name == "tool_search"
                || is_core_tool(&tool.name)
                || enabled.contains(&tool.name)
            {
                out.push(ToolDefinition::new(
                    tool.name,
                    tool.description,
                    tool.input_schema,
                ));
            }
        }
        out
    }

    pub async fn execute_tool(&self, name: &str, arguments: Value) -> Option<String> {
        if self.registry.get(name).await.is_none() {
            return None;
        }
        if is_deferred_tool(name) && !self.enabled_deferred_tools.read().await.contains(name) {
            return Some(
                json!({
                    "success": false,
                    "error": format!("{name} is a deferred tool. Use tool_search to enable it before calling it.")
                })
                .to_string(),
            );
        }

        let permission =
            match self
                .permission_store
                .evaluate_tool_call(&self.user_id, self.permission_mode, name, &arguments)
            {
                Ok(permission) => permission,
                Err(error) => {
                    return Some(
                        json!({
                            "success": false,
                            "error": format!("permission evaluation failed: {error}")
                        })
                        .to_string(),
                    );
                }
            };
        if matches!(permission.decision, PermissionDecision::Ask) {
            let expires_at_ms = chrono::Utc::now().timestamp_millis() + 30_000;
            let prompt = super::permissions::PermissionPromptRequest {
                request_id: format!(
                    "perm_{}_{}",
                    self.conversation_id,
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
                ),
                conversation_id: self.conversation_id.clone(),
                tool_name: name.to_string(),
                arguments: arguments.clone(),
                reason: permission.reason.clone(),
                mode: match self.permission_mode {
                    PermissionMode::Default => "default".to_string(),
                    PermissionMode::AcceptEdit => "accept_edit".to_string(),
                    PermissionMode::Auto => "auto".to_string(),
                    PermissionMode::Bypass => "bypass".to_string(),
                },
                content: arguments
                    .get("command")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned),
                expires_at_ms,
            };
            let _ = self
                .app_handle
                .emit("chat:permission_request", prompt.clone());
            let approved = match self
                .permission_store
                .wait_for_resolution(&self.user_id, prompt.clone(), Duration::from_secs(30))
                .await
            {
                Ok(allow) => allow,
                Err(error) => {
                    return Some(
                        json!({
                            "success": false,
                            "error": format!("permission resolution failed: {error}"),
                            "permission_decision": "deny",
                        })
                        .to_string(),
                    );
                }
            };
            if !approved {
                return Some(
                    json!({
                        "success": false,
                        "error": format!("{} approval was denied or timed out.", name),
                        "permission_decision": "deny",
                    })
                    .to_string(),
                );
            }
        } else if !matches!(permission.decision, PermissionDecision::Allow) {
            let decision = match permission.decision {
                PermissionDecision::Ask => "ask",
                PermissionDecision::Deny => "deny",
                PermissionDecision::Allow => "allow",
            };
            return Some(
                json!({
                    "success": false,
                    "error": permission.reason,
                    "permission_decision": decision,
                })
                .to_string(),
            );
        }

        Some(match self.registry.execute(name, arguments).await {
            Ok(value) => value.to_string(),
            Err(error) => json!({
                "success": false,
                "error": error.to_string()
            })
            .to_string(),
        })
    }
}

async fn register_workspace_tools(
    registry: &Arc<ToolRegistry>,
    workspace: Arc<dyn WorkspaceProvider>,
    can_write: bool,
    enabled_deferred_tools: Arc<RwLock<HashSet<String>>>,
    app_handle: AppHandle,
    user_id: String,
    conversation_id: String,
    work_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    question_store: QuestionStore,
) {
    let ah = app_handle.clone();
    let cid = conversation_id.clone();
    let task_store = Arc::new(TaskStore::new(user_id.clone(), cid.clone(), data_dir));
    register_tool_search(registry, enabled_deferred_tools).await;
    register_file_read(registry, workspace.clone(), "file_read").await;
    register_file_read(registry, workspace.clone(), "read_file").await;
    if can_write {
        register_file_write(registry, workspace.clone(), "file_write").await;
        register_file_write(registry, workspace.clone(), "write_file").await;

        registry
            .register(
                McpTool::new(
                    "file_edit",
                    "Edit a file by replacing one exact string with another.",
                    json!({
                        "type": "object",
                        "properties": {
                            "path": { "type": "string", "description": "Workspace-relative file path" },
                            "file_path": { "type": "string", "description": "Alias for path" },
                            "old_content": { "type": "string", "description": "Exact text to replace" },
                            "new_content": { "type": "string", "description": "Replacement text" }
                        },
                        "required": ["old_content", "new_content"]
                    }),
                ),
                Arc::new(FileEditExecutor {
                    workspace: workspace.clone(),
                }),
            )
            .await;
    }

    registry
        .register(
            McpTool::new(
                "web_search",
                "Search the web through the configured SearXNG host.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "max_results": { "type": "integer", "minimum": 1, "maximum": 20 }
                    },
                    "required": ["query"]
                }),
            ),
            Arc::new(WebSearchExecutor),
        )
        .await;

    registry
        .register(
            McpTool::new(
                "save_reference",
                "Save a useful reference into references/*.md in the current workspace.",
                json!({
                    "type": "object",
                    "properties": {
                        "title": { "type": "string" },
                        "url": { "type": "string" },
                        "summary": { "type": "string" },
                        "category": { "type": "string", "enum": ["literature", "dataset", "code", "formula", "competition"] },
                        "methodology": { "type": "string" },
                        "key_parameters": { "type": "string" },
                        "ai_relevance": { "type": "string" }
                    },
                    "required": ["title", "url", "summary", "category"]
                }),
            ),
            Arc::new(SaveReferenceExecutor {
                workspace: workspace.clone(),
            }),
        )
        .await;

    registry
        .register(
            McpTool::new(
                "fetch_url",
                "Fetch a URL as markdown through Jina Reader fallback. Use Research for Firecrawl search.",
                json!({
                    "type": "object",
                    "properties": {
                        "url": { "type": "string" }
                    },
                    "required": ["url"]
                }),
            ),
            Arc::new(FetchUrlExecutor),
        )
        .await;

    registry
        .register(
            McpTool::new(
                "start_background_task",
                "Start a background copilot task and report progress in the chat UI.",
                json!({
                    "type": "object",
                    "properties": {
                        "task_type": { "type": "string", "enum": ["research", "review", "modeling", "analysis"] },
                        "prompt": { "type": "string" }
                    },
                    "required": ["task_type", "prompt"]
                }),
            ),
            Arc::new(BackgroundTaskExecutor {
                app_handle,
                conversation_id,
            }),
        )
        .await;

    registry
        .register(
            McpTool::new(
                "list_files",
                "List the current workspace file tree.",
                json!({
                    "type": "object",
                    "properties": {}
                }),
            ),
            Arc::new(ListFilesExecutor {
                workspace: workspace.clone(),
            }),
        )
        .await;

    registry
        .register(
            McpTool::new(
                "execute_command",
                "Execute a shell command in the current workspace. Available only in Host Local mode.",
                json!({
                    "type": "object",
                    "properties": {
                        "command": { "type": "string", "description": "Command to execute" },
                        "cwd": { "type": "string", "description": "Optional workspace-relative working directory" }
                    },
                    "required": ["command"]
                }),
            ),
            Arc::new(ExecuteCommandExecutor {
                workspace: workspace.clone(),
            }),
        )
        .await;

    registry
        .register(
            McpTool::new(
                "search_files",
                "Search for a text pattern in workspace files.",
                json!({
                    "type": "object",
                    "properties": {
                        "pattern": { "type": "string", "description": "Text pattern to search for" },
                        "path": { "type": "string", "description": "Optional workspace-relative directory or file" }
                    },
                    "required": ["pattern"]
                }),
            ),
            Arc::new(SearchFilesExecutor { workspace: workspace.clone() }),
        )
        .await;

    registry
        .register(
            McpTool::new(
                "git_operations",
                "Execute git version control operations. Supports: status, add, commit, push, pull, log, diff, branch, checkout. Use commit with -m for message.",
                json!({
                    "type": "object",
                    "properties": {
                        "operation": {
                            "type": "string",
                            "enum": ["status", "add", "commit", "push", "pull", "log", "diff", "branch", "checkout"],
                            "description": "Git operation to perform"
                        },
                        "message": { "type": "string", "description": "Commit message (required for commit)" },
                        "files": { "type": "array", "items": { "type": "string" }, "description": "Files to add (for add operation)" },
                        "branch": { "type": "string", "description": "Branch name (for branch, checkout, push, pull)" },
                        "remote": { "type": "string", "description": "Remote name (defaults to origin)" },
                        "args": { "type": "array", "items": { "type": "string" }, "description": "Additional args" }
                    },
                    "required": ["operation"]
                }),
            ),
            Arc::new(GitExecutor {
                work_dir: work_dir.clone(),
            }),
        )
        .await;

    // Task tools
    register_task_create(registry, task_store.clone()).await;
    register_task_update(registry, task_store.clone()).await;
    register_task_list(registry, task_store.clone()).await;
    register_task_get(registry, task_store).await;

    // AskUserQuestion tool
    registry
        .register(
            McpTool::new(
                "ask_user_question",
                "Ask the user a structured question with multiple-choice options when you need their input to proceed.",
                json!({
                    "type": "object",
                    "properties": {
                        "questions": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "question": { "type": "string", "description": "The complete question to ask the user" },
                                    "header": { "type": "string", "description": "Short label (max 12 chars)" },
                                    "options": {
                                        "type": "array",
                                        "items": {
                                            "type": "object",
                                            "properties": {
                                                "label": { "type": "string", "description": "Display text for this option" },
                                                "description": { "type": "string", "description": "What this option means" }
                                            },
                                            "required": ["label", "description"]
                                        },
                                        "minItems": 2,
                                        "maxItems": 4
                                    },
                                    "multiSelect": { "type": "boolean", "description": "Allow multiple selections" }
                                },
                                "required": ["question", "header", "options"]
                            },
                            "minItems": 1,
                            "maxItems": 4
                        }
                    },
                    "required": ["questions"]
                }),
            ),
            Arc::new(AskUserQuestionExecutor {
                question_store: question_store.clone(),
                app_handle: ah,
                conversation_id: cid,
                user_id: user_id.clone(),
            }),
        )
        .await;
}

async fn register_tool_search(
    registry: &Arc<ToolRegistry>,
    enabled_deferred_tools: Arc<RwLock<HashSet<String>>>,
) {
    registry
        .register(
            McpTool::new(
                "tool_search",
                "Find and enable optional tools by keyword. Use this before calling deferred tools.",
                json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" },
                        "select": { "type": "array", "items": { "type": "string" } },
                        "limit": { "type": "integer", "minimum": 1, "maximum": 8 }
                    }
                }),
            ),
            Arc::new(ToolSearchExecutor {
                enabled_deferred_tools,
            }),
        )
        .await;
}

async fn register_file_read(
    registry: &Arc<ToolRegistry>,
    workspace: Arc<dyn WorkspaceProvider>,
    name: &'static str,
) {
    registry
        .register(
            McpTool::new(
                name,
                "Read a file from the current workspace.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Workspace-relative file path" },
                        "file_path": { "type": "string", "description": "Alias for path" }
                    }
                }),
            ),
            Arc::new(FileReadExecutor { workspace }),
        )
        .await;
}

async fn register_file_write(
    registry: &Arc<ToolRegistry>,
    workspace: Arc<dyn WorkspaceProvider>,
    name: &'static str,
) {
    registry
        .register(
            McpTool::new(
                name,
                "Create or overwrite a file inside the current workspace.",
                json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Workspace-relative file path" },
                        "file_path": { "type": "string", "description": "Alias for path" },
                        "content": { "type": "string", "description": "File content" }
                    },
                    "required": ["content"]
                }),
            ),
            Arc::new(FileWriteExecutor { workspace }),
        )
        .await;
}

fn is_core_tool(name: &str) -> bool {
    TOOL_CATALOG
        .iter()
        .any(|entry| entry.name == name && matches!(entry.exposure, ToolExposure::Core))
}

fn is_deferred_tool(name: &str) -> bool {
    TOOL_CATALOG
        .iter()
        .any(|entry| entry.name == name && matches!(entry.exposure, ToolExposure::Deferred))
}

fn tool_by_name_or_alias(name: &str) -> Option<&'static ToolCatalogEntry> {
    TOOL_CATALOG.iter().find(|entry| {
        entry.name.eq_ignore_ascii_case(name)
            || entry
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    })
}

pub fn is_tool_concurrency_safe(name: &str) -> bool {
    tool_by_name_or_alias(name)
        .map(|entry| entry.is_concurrency_safe)
        .unwrap_or(false)
}

pub fn is_tool_read_only(name: &str) -> bool {
    tool_by_name_or_alias(name)
        .map(|entry| entry.is_read_only)
        .unwrap_or(false)
}

fn split_search_tokens(value: &str) -> Vec<String> {
    let mut normalized = String::new();
    let mut previous_lowercase = false;
    for ch in value.chars() {
        if ch == '_' || ch == '-' || ch.is_whitespace() {
            normalized.push(' ');
            previous_lowercase = false;
            continue;
        }
        if ch.is_uppercase() && previous_lowercase {
            normalized.push(' ');
        }
        if ch.is_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            previous_lowercase = ch.is_lowercase() || ch.is_ascii_digit();
        } else {
            normalized.push(' ');
            previous_lowercase = false;
        }
    }
    normalized
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

fn contains_term(value: &str, term: &str) -> bool {
    value.to_ascii_lowercase().contains(term)
}

fn tool_matches_required_term(entry: &ToolCatalogEntry, term: &str) -> bool {
    let name_parts = split_search_tokens(entry.name);
    name_parts.iter().any(|part| part.contains(term))
        || entry.aliases.iter().any(|alias| contains_term(alias, term))
        || entry.keywords.iter().any(|keyword| keyword.contains(term))
        || contains_term(entry.search_hint, term)
        || contains_term(entry.description, term)
}

fn score_tool_for_terms(entry: &ToolCatalogEntry, terms: &[String]) -> i32 {
    let name_parts = split_search_tokens(entry.name);
    let mut score = 0;
    for term in terms {
        if name_parts.iter().any(|part| part == term) {
            score += 10;
        } else if name_parts.iter().any(|part| part.contains(term)) {
            score += 5;
        }
        if entry.aliases.iter().any(|alias| contains_term(alias, term)) {
            score += 5;
        }
        if contains_term(entry.search_hint, term) {
            score += 4;
        }
        if entry.keywords.iter().any(|keyword| keyword.contains(term)) {
            score += 2;
        }
        if contains_term(entry.description, term) {
            score += 2;
        }
    }
    score
}

fn direct_tool_selection(names: &[String]) -> Vec<&'static ToolCatalogEntry> {
    let mut matches = Vec::new();
    let mut seen = HashSet::new();
    for name in names {
        if let Some(entry) = tool_by_name_or_alias(name.trim()) {
            if seen.insert(entry.name) {
                matches.push(entry);
            }
        }
    }
    matches
}

fn search_tool_catalog(
    query: &str,
    selected: &[String],
    limit: usize,
) -> Vec<&'static ToolCatalogEntry> {
    let query = query.trim();
    let mut direct_selected = selected.to_vec();
    if direct_selected.is_empty() && query.to_ascii_lowercase().starts_with("select:") {
        direct_selected = query[7..]
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .collect();
    }
    if !direct_selected.is_empty() {
        return direct_tool_selection(&direct_selected);
    }

    if let Some(entry) = tool_by_name_or_alias(query) {
        if matches!(entry.exposure, ToolExposure::Deferred) {
            return vec![entry];
        }
    }

    let terms = query
        .to_ascii_lowercase()
        .split_whitespace()
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let mut required_terms = Vec::new();
    let mut optional_terms = Vec::new();
    for term in terms {
        if let Some(required) = term.strip_prefix('+') {
            if !required.is_empty() {
                required_terms.push(required.to_string());
            }
        } else {
            optional_terms.push(term);
        }
    }
    let scoring_terms = if required_terms.is_empty() {
        optional_terms
    } else {
        required_terms
            .iter()
            .chain(optional_terms.iter())
            .cloned()
            .collect()
    };

    let mut scored = TOOL_CATALOG
        .iter()
        .filter(|entry| matches!(entry.exposure, ToolExposure::Deferred))
        .filter(|entry| {
            required_terms
                .iter()
                .all(|term| tool_matches_required_term(entry, term))
        })
        .map(|entry| (score_tool_for_terms(entry, &scoring_terms), entry))
        .filter(|(score, _)| *score > 0 || scoring_terms.is_empty())
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.name.len().cmp(&b.1.name.len()))
            .then_with(|| a.1.name.cmp(b.1.name))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, entry)| entry)
        .collect()
}

fn urlencoding(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char)
            }
            b' ' => result.push_str("%20"),
            _ => result.push_str(&format!("%{:02X}", byte)),
        }
    }
    result
}

fn title_to_slug(title: &str) -> String {
    let slug: String = title
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let slug = slug.trim_matches('_');
    if slug.len() > 64 {
        slug[..64].to_string()
    } else {
        slug.to_string()
    }
}

struct FileReadExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for FileReadExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let path = tool_path_arg(&input)?;
        match self.workspace.read_file(path).await {
            Ok(content) => Ok(json!({
                "success": true,
                "workspace": self.workspace.label(),
                "path": path,
                "content": content,
            })),
            Err(e) => Ok(json!({
                "success": false,
                "error": format!("Cannot read '{path}': {e}. Use list_files to see available files."),
            })),
        }
    }
}

struct FileWriteExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for FileWriteExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let path = tool_path_arg(&input)?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("content is required"))?;
        self.workspace.write_file(path, content).await?;
        Ok(json!({
            "success": true,
            "workspace": self.workspace.label(),
            "path": path
        }))
    }
}

struct FileEditExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for FileEditExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let path = tool_path_arg(&input)?;
        let old_content = input["old_content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("old_content is required"))?;
        let new_content = input["new_content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("new_content is required"))?;
        self.workspace
            .edit_file(path, old_content, new_content)
            .await?;
        Ok(json!({
            "success": true,
            "workspace": self.workspace.label(),
            "path": path
        }))
    }
}

struct ListFilesExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for ListFilesExecutor {
    async fn execute(&self, _input: Value) -> anyhow::Result<Value> {
        let tree = self.workspace.list_files().await?;
        Ok(json!({
            "success": true,
            "workspace": self.workspace.label(),
            "tree": tree
        }))
    }
}

struct ExecuteCommandExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for ExecuteCommandExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("command is required"))?;
        let cwd = input["cwd"]
            .as_str()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(".");
        self.workspace.execute_command(command, cwd).await
    }
}

struct ToolSearchExecutor {
    enabled_deferred_tools: Arc<RwLock<HashSet<String>>>,
}

#[async_trait]
impl ToolExecutor for ToolSearchExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let query = input["query"].as_str().unwrap_or("");
        let selected = input["select"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let limit = input["limit"].as_u64().unwrap_or(5).clamp(1, 8) as usize;

        let matches = search_tool_catalog(query, &selected, limit);

        let mut enabled = self.enabled_deferred_tools.write().await;
        for entry in &matches {
            if matches!(entry.exposure, ToolExposure::Deferred) {
                enabled.insert(entry.name.to_string());
            }
        }

        Ok(json!({
            "success": true,
            "enabled_tools": matches.iter().map(|entry| entry.name).collect::<Vec<_>>(),
            "tools": matches.iter().map(|entry| json!({
                "name": entry.name,
                "description": entry.description,
                "keywords": entry.keywords,
                "search_hint": entry.search_hint,
                "aliases": entry.aliases,
                "is_concurrency_safe": is_tool_concurrency_safe(entry.name),
                "is_read_only": is_tool_read_only(entry.name)
            })).collect::<Vec<_>>()
        }))
    }
}

struct WebSearchExecutor;

#[async_trait]
impl ToolExecutor for WebSearchExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let query = input["query"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("query is required"))?;
        let max_results = input["max_results"].as_u64().unwrap_or(10).clamp(1, 20);
        let url = format!(
            "http://localhost:8080/search?q={}&format=json&categories=general&pageno=1",
            urlencoding(query)
        );
        let data = reqwest::get(&url).await?.json::<Value>().await?;
        let results = data["results"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .take(max_results as usize)
                    .map(|r| {
                        json!({
                            "title": r["title"].as_str().unwrap_or(""),
                            "url": r["url"].as_str().unwrap_or(""),
                            "snippet": r["content"].as_str().unwrap_or("")
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        Ok(json!({
            "success": true,
            "query": query,
            "results": results
        }))
    }
}

struct FetchUrlExecutor;

#[async_trait]
impl ToolExecutor for FetchUrlExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let url = input["url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("url is required"))?;
        let reader_url = format!("https://r.jina.ai/{url}");
        let markdown = reqwest::get(&reader_url).await?.text().await?;
        let content = if markdown.len() > 8000 {
            format!(
                "{}...\n\n(Content truncated at 8000 characters)",
                &markdown[..8000]
            )
        } else {
            markdown
        };
        Ok(json!({
            "success": true,
            "url": url,
            "content": content,
            "provider": "jina-reader"
        }))
    }
}

struct SaveReferenceExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for SaveReferenceExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let title = input["title"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("title is required"))?;
        let url = input["url"].as_str().unwrap_or("");
        let summary = input["summary"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("summary is required"))?;
        let category = input["category"].as_str().unwrap_or("literature");
        let methodology = input["methodology"].as_str().unwrap_or("");
        let key_parameters = input["key_parameters"].as_str().unwrap_or("");
        let ai_relevance = input["ai_relevance"].as_str().unwrap_or("");
        let slug = title_to_slug(title);
        let now = chrono::Utc::now().format("%Y-%m-%d");
        let path = format!("references/{slug}.md");
        let content = format!(
            "# {title}\n- **URL**: {url}\n- **Category**: {category}\n- **Methodology**: {methodology}\n- **Key Parameters**: {key_parameters}\n- **Saved**: {now}\n\n## AI Summary\n{summary}\n\n## Relevance to Project\n{ai_relevance}\n\n## Notes\n<!-- Add your notes here -->\n"
        );
        self.workspace.write_file(&path, &content).await?;
        Ok(json!({
            "success": true,
            "path": path,
            "title": title
        }))
    }
}

struct BackgroundTaskExecutor {
    app_handle: AppHandle,
    conversation_id: String,
}

#[async_trait]
impl ToolExecutor for BackgroundTaskExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let task_type = input["task_type"]
            .as_str()
            .unwrap_or("analysis")
            .to_string();
        let prompt = input["prompt"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("prompt is required"))?
            .to_string();
        let task_id = format!("bg-{}", chrono::Utc::now().timestamp_millis());
        let conversation_id = self.conversation_id.clone();
        let app = self.app_handle.clone();
        let spawned_task_id = task_id.clone();
        let spawned_task_type = task_type.clone();
        let spawned_prompt = prompt.clone();
        emit_background_task(
            &app,
            &conversation_id,
            &task_id,
            &task_type,
            &prompt,
            "running",
            "",
        );
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(900)).await;
            let result = format!(
                "Background {spawned_task_type} task queued for: {spawned_prompt}\n\nUse this result as a tracking item. Full multi-agent execution can be attached in the next runtime pass."
            );
            emit_background_task(
                &app,
                &conversation_id,
                &spawned_task_id,
                &spawned_task_type,
                &spawned_prompt,
                "completed",
                &result,
            );
        });
        Ok(json!({
            "success": true,
            "task_id": task_id,
            "task_type": task_type,
            "status": "running"
        }))
    }
}

fn emit_background_task(
    app: &AppHandle,
    conversation_id: &str,
    task_id: &str,
    task_type: &str,
    prompt: &str,
    status: &str,
    result: &str,
) {
    let _ = app.emit(
        "chat:background_task",
        json!({
            "conversation_id": conversation_id,
            "task_id": task_id,
            "task_type": task_type,
            "prompt": prompt,
            "status": status,
            "result": result
        }),
    );
}

struct SearchFilesExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for SearchFilesExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("pattern is required"))?;
        let path = tool_optional_path_arg(&input, ".");
        let results = self.workspace.search_files(pattern, path).await?;

        Ok(json!({
            "success": true,
            "workspace": self.workspace.label(),
            "pattern": pattern,
            "results": results
        }))
    }
}

// ── Task tool registration ──

async fn register_task_create(
    registry: &Arc<ToolRegistry>,
    store: Arc<TaskStore>,
) {
    let store = store.clone();
    registry
        .register(
            McpTool::new(
                "task_create",
                "Create a new task to track progress and organize work. Use this to break down complex work into manageable pieces.",
                json!({
                    "type": "object",
                    "properties": {
                        "subject": { "type": "string", "description": "Brief title for the task" },
                        "description": { "type": "string", "description": "What needs to be done" },
                        "priority": { "type": "string", "enum": ["low", "medium", "high", "critical"], "description": "Task priority" },
                        "blocks": { "type": "array", "items": { "type": "string" }, "description": "Task IDs that this task blocks" },
                        "tags": { "type": "array", "items": { "type": "string" }, "description": "Tags for categorization" }
                    },
                    "required": ["subject", "description"]
                }),
            ),
            Arc::new(TaskCreateExecutor { store }),
        )
        .await;
}

struct TaskCreateExecutor {
    store: Arc<TaskStore>,
}

#[async_trait]
impl ToolExecutor for TaskCreateExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let subject = input["subject"].as_str().ok_or_else(|| anyhow::anyhow!("subject required"))?;
        let description = input["description"].as_str().ok_or_else(|| anyhow::anyhow!("description required"))?;
        let priority = input["priority"].as_str();
        let blocks: Option<Vec<String>> = input["blocks"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        let tags: Option<Vec<String>> = input["tags"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect());
        let task = self.store.create(subject, description, priority, blocks, tags);
        Ok(serde_json::to_value(&task)?)
    }
}

async fn register_task_update(
    registry: &Arc<ToolRegistry>,
    store: Arc<TaskStore>,
) {
    let store = store.clone();
    registry
        .register(
            McpTool::new(
                "task_update",
                "Update an existing task's status, subject, description, or priority.",
                json!({
                    "type": "object",
                    "properties": {
                        "task_id": { "type": "string", "description": "ID of the task to update" },
                        "status": { "type": "string", "enum": ["pending", "in_progress", "completed", "deleted"], "description": "New status" },
                        "subject": { "type": "string", "description": "New subject" },
                        "description": { "type": "string", "description": "New description" },
                        "priority": { "type": "string", "enum": ["low", "medium", "high", "critical"] }
                    },
                    "required": ["task_id"]
                }),
            ),
            Arc::new(TaskUpdateExecutor { store }),
        )
        .await;
}

struct TaskUpdateExecutor {
    store: Arc<TaskStore>,
}

#[async_trait]
impl ToolExecutor for TaskUpdateExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let task_id = input["task_id"].as_str().ok_or_else(|| anyhow::anyhow!("task_id required"))?;
        let status = input["status"].as_str();
        let subject = input["subject"].as_str();
        let description = input["description"].as_str();
        let priority = input["priority"].as_str();
        self.store.update(task_id, status, subject, description, priority)
            .map(|t| serde_json::to_value(&t).map_err(anyhow::Error::from))
            .unwrap_or_else(|| Ok(json!({ "error": "task not found" })))
    }
}

async fn register_task_list(
    registry: &Arc<ToolRegistry>,
    store: Arc<TaskStore>,
) {
    let store = store.clone();
    registry
        .register(
            McpTool::new(
                "task_list",
                "List all tasks, optionally filtered by status or priority.",
                json!({
                    "type": "object",
                    "properties": {
                        "status": { "type": "string", "enum": ["pending", "in_progress", "completed"], "description": "Filter by status" },
                        "priority": { "type": "string", "enum": ["low", "medium", "high", "critical"], "description": "Filter by priority" }
                    },
                    "required": []
                }),
            ),
            Arc::new(TaskListExecutor { store }),
        )
        .await;
}

struct TaskListExecutor {
    store: Arc<TaskStore>,
}

#[async_trait]
impl ToolExecutor for TaskListExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let status = input["status"].as_str();
        let priority = input["priority"].as_str();
        let tasks = self.store.list(status, priority);
        Ok(serde_json::to_value(&tasks)?)
    }
}

async fn register_task_get(
    registry: &Arc<ToolRegistry>,
    store: Arc<TaskStore>,
) {
    let store = store.clone();
    registry
        .register(
            McpTool::new(
                "task_get",
                "Get full details of a specific task by ID.",
                json!({
                    "type": "object",
                    "properties": {
                        "task_id": { "type": "string", "description": "ID of the task" }
                    },
                    "required": ["task_id"]
                }),
            ),
            Arc::new(TaskGetExecutor { store }),
        )
        .await;
}

struct TaskGetExecutor {
    store: Arc<TaskStore>,
}

#[async_trait]
impl ToolExecutor for TaskGetExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let task_id = input["task_id"].as_str().ok_or_else(|| anyhow::anyhow!("task_id required"))?;
        self.store.get(task_id)
            .map(|t| serde_json::to_value(&t).map_err(anyhow::Error::from))
            .unwrap_or_else(|| Ok(json!({ "error": "task not found" })))
    }
}

struct SkillInvokeExecutor {
    skills: Arc<SkillRegistry>,
}

#[async_trait]
impl ToolExecutor for SkillInvokeExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let skill_name = input["skill"].as_str()
            .ok_or_else(|| anyhow::anyhow!("skill name required"))?;
        let _task = input["input"].as_str().unwrap_or("");
        match self.skills.set_active_skill(skill_name).await {
            Some(skill) => Ok(json!({
                "activated": skill.name,
                "description": skill.description,
                "category": skill.category,
                "tools_used": skill.tools_used,
                "effect": "System prompt augmented with skill guidance on next turn."
            })),
            None => {
                // Try matching by suffix
                let available: Vec<_> = self.skills.list_all().await;
                match available.iter().find(|s| s.name.ends_with(&format!(":{skill_name}"))) {
                    Some(skill) => Ok(json!({
                        "activated": skill.name,
                        "description": skill.description,
                        "effect": "System prompt augmented with skill guidance on next turn."
                    })),
                    None => Ok(json!({ "error": "unknown skill", "available": available.iter().map(|s| &s.name).collect::<Vec<_>>() })),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_tool_concurrency_safe, is_tool_read_only, search_tool_catalog, tool_by_name_or_alias,
    };

    #[test]
    fn catalog_exposes_read_only_and_concurrency_metadata() {
        let read = tool_by_name_or_alias("file_read").expect("file_read exists");
        assert!(read.is_read_only);
        assert!(read.is_concurrency_safe);

        let write = tool_by_name_or_alias("file_write").expect("file_write exists");
        assert!(!write.is_read_only);
        assert!(!write.is_concurrency_safe);

        let bash = tool_by_name_or_alias("Bash").expect("Bash alias resolves");
        assert_eq!(bash.name, "execute_command");
        assert!(!bash.is_read_only);
        assert!(!bash.is_concurrency_safe);

        let search = tool_by_name_or_alias("tool_search").expect("tool_search exists");
        assert!(search.is_read_only);
        assert!(search.is_concurrency_safe);
    }

    #[test]
    fn tool_search_supports_select_prefix_and_aliases() {
        let matches = search_tool_catalog("select:Bash,search_files", &[], 8);
        let names = matches.iter().map(|entry| entry.name).collect::<Vec<_>>();
        assert_eq!(names, vec!["execute_command", "search_files"]);
    }

    #[test]
    fn tool_search_requires_plus_terms() {
        let matches = search_tool_catalog("+shell pattern", &[], 8);
        let names = matches.iter().map(|entry| entry.name).collect::<Vec<_>>();
        assert!(names.contains(&"execute_command"));
        assert!(!names.contains(&"search_files"));
    }

    #[test]
    fn tool_search_scores_camel_case_alias_hint_and_description() {
        let bash_matches = search_tool_catalog("bash", &[], 3);
        assert_eq!(
            bash_matches.first().map(|entry| entry.name),
            Some("execute_command")
        );

        let docs_matches = search_tool_catalog("markdown webpage", &[], 3);
        assert_eq!(
            docs_matches.first().map(|entry| entry.name),
            Some("fetch_url")
        );

        let background_matches = search_tool_catalog("background subagent", &[], 3);
        assert_eq!(
            background_matches.first().map(|entry| entry.name),
            Some("start_background_task")
        );
    }

    #[test]
    fn tool_metadata_helpers_use_aliases_and_fail_closed() {
        assert!(is_tool_read_only("Read"));
        assert!(is_tool_concurrency_safe("tool_search"));
        assert!(!is_tool_read_only("Write"));
        assert!(!is_tool_concurrency_safe("definitely_missing_tool"));
    }
}
