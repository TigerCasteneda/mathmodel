use async_trait::async_trait;
use claude_code_rs::api::ToolDefinition;
use claude_code_rs::mcp::{McpTool, ToolExecutor, ToolRegistry};
use claude_code_rs::ApiClient;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio::sync::RwLock;

use super::config::AiConfig;
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

    fn label(self) -> &'static str {
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
}

const TOOL_CATALOG: &[ToolCatalogEntry] = &[
    ToolCatalogEntry {
        name: "file_read",
        description: "Read a file from the current workspace.",
        exposure: ToolExposure::Core,
        keywords: &["read", "file", "inspect", "view"],
    },
    ToolCatalogEntry {
        name: "read_file",
        description: "Alias for file_read.",
        exposure: ToolExposure::Core,
        keywords: &["read", "file", "alias"],
    },
    ToolCatalogEntry {
        name: "file_write",
        description: "Create or overwrite a file inside the current workspace.",
        exposure: ToolExposure::Core,
        keywords: &["write", "file", "create", "overwrite"],
    },
    ToolCatalogEntry {
        name: "write_file",
        description: "Alias for file_write.",
        exposure: ToolExposure::Core,
        keywords: &["write", "file", "alias"],
    },
    ToolCatalogEntry {
        name: "web_search",
        description: "Search the web through the configured SearXNG host.",
        exposure: ToolExposure::Core,
        keywords: &["web", "search", "paper", "internet"],
    },
    ToolCatalogEntry {
        name: "save_reference",
        description: "Save a useful reference into references/*.md in the workspace.",
        exposure: ToolExposure::Core,
        keywords: &["save", "reference", "paper", "research"],
    },
    ToolCatalogEntry {
        name: "file_edit",
        description: "Edit a file by replacing one exact string with another.",
        exposure: ToolExposure::Deferred,
        keywords: &["edit", "replace", "patch", "file"],
    },
    ToolCatalogEntry {
        name: "list_files",
        description: "List the current workspace file tree.",
        exposure: ToolExposure::Deferred,
        keywords: &["list", "tree", "files", "folder"],
    },
    ToolCatalogEntry {
        name: "execute_command",
        description: "Execute a shell command in Host Local mode.",
        exposure: ToolExposure::Deferred,
        keywords: &["shell", "bash", "command", "run", "test"],
    },
    ToolCatalogEntry {
        name: "search_files",
        description: "Search for a text pattern in workspace files.",
        exposure: ToolExposure::Deferred,
        keywords: &["grep", "search", "pattern", "code"],
    },
    ToolCatalogEntry {
        name: "fetch_url",
        description:
            "Fetch a URL as markdown through Jina Reader fallback. Use Research for Firecrawl search.",
        exposure: ToolExposure::Deferred,
        keywords: &["fetch", "url", "markdown", "webpage", "jina"],
    },
    ToolCatalogEntry {
        name: "start_background_task",
        description: "Start a background copilot task and report progress in the chat UI.",
        exposure: ToolExposure::Deferred,
        keywords: &["background", "subagent", "review", "research", "parallel"],
    },
];

pub struct ModelerAiRuntime {
    client: ApiClient,
    registry: Arc<ToolRegistry>,
    workspace: Arc<dyn WorkspaceProvider>,
    workspace_label: &'static str,
    enabled_deferred_tools: Arc<RwLock<HashSet<String>>>,
    permission_mode: PermissionMode,
}

impl ModelerAiRuntime {
    pub async fn new(
        config: AiConfig,
        context: WorkspaceContext,
        app_handle: AppHandle,
        conversation_id: String,
        permission_mode: PermissionMode,
    ) -> anyhow::Result<Self> {
        let client = ApiClient::new(config.to_claude_settings(context.work_dir.clone()));
        let workspace_label = context.label();
        let can_write = context.has_capability("files.write") && context.has_capability("ai.write");
        let workspace = build_workspace_provider(context, app_handle.clone())?;
        let registry = Arc::new(ToolRegistry::new());
        let enabled_deferred_tools = Arc::new(RwLock::new(HashSet::new()));

        register_workspace_tools(
            &registry,
            workspace.clone(),
            can_write,
            permission_mode,
            enabled_deferred_tools.clone(),
            app_handle,
            conversation_id,
        )
        .await;

        Ok(Self {
            client,
            registry,
            workspace,
            workspace_label,
            enabled_deferred_tools,
            permission_mode,
        })
    }

    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    pub fn workspace_label(&self) -> &'static str {
        self.workspace_label
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
    permission_mode: PermissionMode,
    enabled_deferred_tools: Arc<RwLock<HashSet<String>>>,
    app_handle: AppHandle,
    conversation_id: String,
) {
    register_tool_search(registry, enabled_deferred_tools).await;
    register_file_read(registry, workspace.clone(), "file_read").await;
    register_file_read(registry, workspace.clone(), "read_file").await;
    if can_write {
        register_file_write(registry, workspace.clone(), "file_write", permission_mode).await;
        register_file_write(registry, workspace.clone(), "write_file", permission_mode).await;

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
                    permission_mode,
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
                permission_mode,
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
                permission_mode,
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
            Arc::new(SearchFilesExecutor { workspace }),
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
    permission_mode: PermissionMode,
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
            Arc::new(FileWriteExecutor {
                workspace,
                permission_mode,
            }),
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

fn tool_by_name(name: &str) -> Option<&'static ToolCatalogEntry> {
    TOOL_CATALOG.iter().find(|entry| entry.name == name)
}

fn permission_denied(tool_name: &str, mode: PermissionMode) -> anyhow::Error {
    anyhow::anyhow!(
        "{tool_name} requires a broader permission mode. Current mode is {}.",
        mode.label()
    )
}

fn can_edit_files(mode: PermissionMode) -> bool {
    matches!(
        mode,
        PermissionMode::AcceptEdit | PermissionMode::Auto | PermissionMode::Bypass
    )
}

fn can_execute_command(mode: PermissionMode, command: &str) -> bool {
    match mode {
        PermissionMode::Bypass => true,
        PermissionMode::Auto => is_low_risk_command(command),
        _ => false,
    }
}

fn is_low_risk_command(command: &str) -> bool {
    let trimmed = command.trim().to_lowercase();
    let blocked = [
        "rm ",
        "del ",
        "rmdir",
        "git reset",
        "git clean",
        "shutdown",
        "format ",
    ];
    if blocked
        .iter()
        .any(|prefix| trimmed.starts_with(prefix) || trimmed.contains(&format!("&& {prefix}")))
    {
        return false;
    }
    [
        "dir",
        "ls",
        "pwd",
        "git status",
        "git diff",
        "npm test",
        "npm run test",
        "cargo test",
        "cargo check",
        "npx tsc",
    ]
    .iter()
    .any(|prefix| trimmed.starts_with(prefix))
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
        let content = self.workspace.read_file(path).await?;
        Ok(json!({
            "success": true,
            "workspace": self.workspace.label(),
            "path": path,
            "content": content
        }))
    }
}

struct FileWriteExecutor {
    workspace: Arc<dyn WorkspaceProvider>,
    permission_mode: PermissionMode,
}

#[async_trait]
impl ToolExecutor for FileWriteExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        if !can_edit_files(self.permission_mode) {
            return Err(permission_denied("file_write", self.permission_mode));
        }
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
    permission_mode: PermissionMode,
}

#[async_trait]
impl ToolExecutor for FileEditExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        if !can_edit_files(self.permission_mode) {
            return Err(permission_denied("file_edit", self.permission_mode));
        }
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
    permission_mode: PermissionMode,
}

#[async_trait]
impl ToolExecutor for ExecuteCommandExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("command is required"))?;
        if !can_execute_command(self.permission_mode, command) {
            return Err(permission_denied("execute_command", self.permission_mode));
        }
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
        let query = input["query"].as_str().unwrap_or("").to_lowercase();
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

        let mut matches = Vec::new();
        if !selected.is_empty() {
            for name in selected {
                if let Some(entry) = tool_by_name(&name) {
                    if matches!(entry.exposure, ToolExposure::Deferred) {
                        matches.push(entry);
                    }
                }
            }
        } else {
            let terms = query
                .split_whitespace()
                .filter(|term| !term.is_empty())
                .collect::<Vec<_>>();
            let mut scored = TOOL_CATALOG
                .iter()
                .filter(|entry| matches!(entry.exposure, ToolExposure::Deferred))
                .map(|entry| {
                    let mut score = 0;
                    for term in &terms {
                        if entry.name.contains(term) {
                            score += 8;
                        }
                        if entry.description.to_lowercase().contains(term) {
                            score += 3;
                        }
                        if entry.keywords.iter().any(|keyword| keyword.contains(term)) {
                            score += 5;
                        }
                    }
                    (score, entry)
                })
                .filter(|(score, _)| *score > 0 || terms.is_empty())
                .collect::<Vec<_>>();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(b.1.name)));
            matches.extend(scored.into_iter().take(limit).map(|(_, entry)| entry));
        }

        let mut enabled = self.enabled_deferred_tools.write().await;
        for entry in &matches {
            enabled.insert(entry.name.to_string());
        }

        Ok(json!({
            "success": true,
            "enabled_tools": matches.iter().map(|entry| entry.name).collect::<Vec<_>>(),
            "tools": matches.iter().map(|entry| json!({
                "name": entry.name,
                "description": entry.description,
                "keywords": entry.keywords
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
    permission_mode: PermissionMode,
}

#[async_trait]
impl ToolExecutor for SaveReferenceExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        if !can_edit_files(self.permission_mode) {
            return Err(permission_denied("save_reference", self.permission_mode));
        }
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
