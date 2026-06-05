use async_trait::async_trait;
use claude_code_rs::api::ToolDefinition;
use claude_code_rs::mcp::{McpTool, ToolExecutor, ToolRegistry};
use claude_code_rs::ApiClient;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::AppHandle;

use super::config::AiConfig;
use super::workspace::{
    build_workspace_provider, tool_optional_path_arg, tool_path_arg, WorkspaceContext,
    WorkspaceProvider,
};
use crate::agent::file_watcher::FileTreeItem;

pub struct ModelerAiRuntime {
    client: ApiClient,
    registry: Arc<ToolRegistry>,
    workspace: Arc<dyn WorkspaceProvider>,
    workspace_label: &'static str,
}

impl ModelerAiRuntime {
    pub async fn new(
        config: AiConfig,
        context: WorkspaceContext,
        app_handle: AppHandle,
    ) -> anyhow::Result<Self> {
        let client = ApiClient::new(config.to_claude_settings(context.work_dir.clone()));
        let workspace_label = context.label();
        let can_write = context.has_capability("files.write") && context.has_capability("ai.write");
        let workspace = build_workspace_provider(context, app_handle)?;
        let registry = Arc::new(ToolRegistry::new());

        register_workspace_tools(&registry, workspace.clone(), can_write).await;

        Ok(Self {
            client,
            registry,
            workspace,
            workspace_label,
        })
    }

    pub fn client(&self) -> &ApiClient {
        &self.client
    }

    pub fn workspace_label(&self) -> &'static str {
        self.workspace_label
    }

    pub async fn workspace_tree(&self) -> anyhow::Result<FileTreeItem> {
        self.workspace.list_files().await
    }

    pub async fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.registry
            .list()
            .await
            .into_iter()
            .map(|tool| ToolDefinition::new(tool.name, tool.description, tool.input_schema))
            .collect()
    }

    pub async fn execute_tool(&self, name: &str, arguments: Value) -> Option<String> {
        if self.registry.get(name).await.is_none() {
            return None;
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
) {
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
            Arc::new(SearchFilesExecutor { workspace }),
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
