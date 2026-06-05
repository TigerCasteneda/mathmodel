use crate::agent::commands::validate_and_resolve_path;
use crate::agent::file_watcher::{self, FileTreeItem};
use async_trait::async_trait;
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceMode {
    Host,
    Guest,
}

#[derive(Debug, Clone)]
pub struct WorkspaceContext {
    pub mode: WorkspaceMode,
    pub work_dir: PathBuf,
    pub project_id: Option<String>,
    pub auth_token: Option<String>,
    pub server_base: Option<String>,
    pub capabilities: Vec<String>,
}

impl WorkspaceContext {
    pub fn new(
        work_dir: PathBuf,
        mode: Option<String>,
        project_id: Option<String>,
        auth_token: Option<String>,
        server_base: Option<String>,
        capabilities: Option<Vec<String>>,
    ) -> Self {
        let mode = match mode.as_deref() {
            Some("guest") | Some("remote") => WorkspaceMode::Guest,
            _ => WorkspaceMode::Host,
        };

        Self {
            mode,
            work_dir,
            project_id,
            auth_token,
            server_base,
            capabilities: capabilities.unwrap_or_else(|| {
                vec![
                    "files.read".to_string(),
                    "files.write".to_string(),
                    "ai.read".to_string(),
                    "ai.write".to_string(),
                    "workspace.sync".to_string(),
                ]
            }),
        }
    }

    pub fn label(&self) -> &'static str {
        match self.mode {
            WorkspaceMode::Host => "Host Local",
            WorkspaceMode::Guest => "Guest Remote",
        }
    }

    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities.iter().any(|cap| cap == capability)
    }
}

#[async_trait]
pub trait WorkspaceProvider: Send + Sync {
    fn label(&self) -> &'static str;

    async fn read_file(&self, path: &str) -> anyhow::Result<String>;

    async fn write_file(&self, path: &str, content: &str) -> anyhow::Result<()>;

    async fn edit_file(
        &self,
        path: &str,
        old_content: &str,
        new_content: &str,
    ) -> anyhow::Result<()> {
        let current = self.read_file(path).await?;
        if !current.contains(old_content) {
            anyhow::bail!("old_content not found in file");
        }
        let updated = current.replacen(old_content, new_content, 1);
        self.write_file(path, &updated).await
    }

    async fn list_files(&self) -> anyhow::Result<FileTreeItem>;

    async fn execute_command(&self, command: &str, cwd: &str) -> anyhow::Result<Value>;

    async fn search_files(&self, pattern: &str, path: &str) -> anyhow::Result<Vec<Value>>;
}

pub fn build_workspace_provider(
    context: WorkspaceContext,
    app_handle: AppHandle,
) -> anyhow::Result<Arc<dyn WorkspaceProvider>> {
    match context.mode {
        WorkspaceMode::Host => Ok(Arc::new(LocalWorkspaceProvider {
            work_dir: context.work_dir,
            app_handle,
        })),
        WorkspaceMode::Guest => {
            let project_id = context
                .project_id
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("project_id is required for Guest Remote mode"))?;
            let auth_token = context
                .auth_token
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("auth token is required for Guest Remote mode"))?;
            let server_base = context
                .server_base
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("server_base is required for Guest Remote mode"))?;

            Ok(Arc::new(RemoteWorkspaceProvider {
                client: reqwest::Client::new(),
                server_base,
                project_id,
                auth_token,
            }))
        }
    }
}

fn path_arg<'a>(input: &'a Value, fallback: &'a str) -> &'a str {
    input["path"]
        .as_str()
        .or_else(|| input["file_path"].as_str())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
}

pub fn tool_path_arg(input: &Value) -> anyhow::Result<&str> {
    input["path"]
        .as_str()
        .or_else(|| input["file_path"].as_str())
        .ok_or_else(|| anyhow::anyhow!("path is required"))
}

pub fn tool_optional_path_arg<'a>(input: &'a Value, fallback: &'a str) -> &'a str {
    path_arg(input, fallback)
}

fn resolve_workspace_path(work_dir: &PathBuf, path: &str) -> anyhow::Result<PathBuf> {
    validate_and_resolve_path(work_dir, path).map_err(|message| anyhow::anyhow!(message))
}

fn emit_file_update(app_handle: &AppHandle, work_dir: &PathBuf, path: &str, content: String) {
    let _ = app_handle.emit(
        "file-change",
        json!({
            "type": "file_change",
            "path": path,
            "content": content
        }),
    );
    let _ = app_handle.emit(
        "file-tree",
        json!({
            "type": "file_tree",
            "tree": file_watcher::scan_tree(work_dir).ok()
        }),
    );
}

struct LocalWorkspaceProvider {
    work_dir: PathBuf,
    app_handle: AppHandle,
}

#[async_trait]
impl WorkspaceProvider for LocalWorkspaceProvider {
    fn label(&self) -> &'static str {
        "Host Local"
    }

    async fn read_file(&self, path: &str) -> anyhow::Result<String> {
        file_watcher::read_workspace_file(&self.work_dir, path)
    }

    async fn write_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
        let resolved = resolve_workspace_path(&self.work_dir, path)?;
        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&resolved, content)?;
        emit_file_update(&self.app_handle, &self.work_dir, path, content.to_string());
        Ok(())
    }

    async fn list_files(&self) -> anyhow::Result<FileTreeItem> {
        file_watcher::scan_tree(&self.work_dir)
    }

    async fn execute_command(&self, command: &str, cwd: &str) -> anyhow::Result<Value> {
        let cwd = resolve_workspace_path(&self.work_dir, cwd)?;

        let mut process = if cfg!(target_os = "windows") {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.arg("/C").arg(command);
            cmd
        } else {
            let mut cmd = tokio::process::Command::new("sh");
            cmd.arg("-c").arg(command);
            cmd
        };

        let output =
            tokio::time::timeout(Duration::from_secs(120), process.current_dir(cwd).output())
                .await
                .map_err(|_| anyhow::anyhow!("command timed out"))??;

        Ok(json!({
            "success": output.status.success(),
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "exit_code": output.status.code()
        }))
    }

    async fn search_files(&self, pattern: &str, path: &str) -> anyhow::Result<Vec<Value>> {
        let root = resolve_workspace_path(&self.work_dir, path)?;
        let mut results = Vec::new();
        search_local_path(&root, pattern, &mut results, 100)?;
        Ok(results)
    }
}

fn search_local_path(
    path: &PathBuf,
    pattern: &str,
    results: &mut Vec<Value>,
    limit: usize,
) -> anyhow::Result<()> {
    if results.len() >= limit {
        return Ok(());
    }
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if matches!(name.as_str(), ".git" | ".next" | "node_modules" | "target") {
                continue;
            }
            search_local_path(&entry.path(), pattern, results, limit)?;
            if results.len() >= limit {
                break;
            }
        }
        return Ok(());
    }
    if !path.is_file() {
        return Ok(());
    }

    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(());
    };
    for (index, line) in content.lines().enumerate() {
        if line.contains(pattern) {
            results.push(json!({
                "path": path.to_string_lossy(),
                "line": index + 1,
                "text": line
            }));
            if results.len() >= limit {
                break;
            }
        }
    }

    Ok(())
}

struct RemoteWorkspaceProvider {
    client: reqwest::Client,
    server_base: String,
    project_id: String,
    auth_token: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RemoteFileTree {
    id: String,
    name: String,
    #[serde(rename = "type")]
    node_type: String,
    zone: String,
    children: Option<Vec<RemoteFileTree>>,
}

#[derive(Debug, Deserialize)]
struct RemoteFileNode {
    id: String,
}

#[derive(Debug, Deserialize)]
struct RemoteContent {
    content: String,
}

#[derive(Debug, Serialize)]
struct CreateNodeRequest<'a> {
    name: &'a str,
    #[serde(rename = "type")]
    node_type: &'a str,
    parent_id: Option<&'a str>,
    zone: &'a str,
}

#[derive(Debug, Serialize)]
struct UpdateContentRequest<'a> {
    content: &'a str,
}

#[async_trait]
impl WorkspaceProvider for RemoteWorkspaceProvider {
    fn label(&self) -> &'static str {
        "Guest Remote"
    }

    async fn read_file(&self, path: &str) -> anyhow::Result<String> {
        let node = self
            .find_path(path)
            .await?
            .ok_or_else(|| anyhow::anyhow!("file not found in remote project: {path}"))?;
        if node.node_type != "file" {
            anyhow::bail!("path is not a file: {path}");
        }
        self.get_content(&node.id).await
    }

    async fn write_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
        let file_id = self.ensure_file_path(path).await?;
        self.put_content(&file_id, content).await
    }

    async fn list_files(&self) -> anyhow::Result<FileTreeItem> {
        let nodes = self.tree_nodes().await?;
        Ok(FileTreeItem {
            id: None,
            name: "project".to_string(),
            path: String::new(),
            node_type: "folder".to_string(),
            zone: None,
            language: None,
            children: Some(
                nodes
                    .iter()
                    .map(|node| remote_tree_to_file_item(node, ""))
                    .collect(),
            ),
        })
    }

    async fn execute_command(&self, _command: &str, _cwd: &str) -> anyhow::Result<Value> {
        anyhow::bail!("execute_command is only available in Host Local mode")
    }

    async fn search_files(&self, pattern: &str, path: &str) -> anyhow::Result<Vec<Value>> {
        let root = self.list_files().await?;
        let root_path = normalize_remote_path(path);
        let mut file_paths = Vec::new();
        collect_file_paths(&root, &root_path, &mut file_paths);

        let mut results = Vec::new();
        for file_path in file_paths.into_iter().take(100) {
            let Ok(content) = self.read_file(&file_path).await else {
                continue;
            };
            for (index, line) in content.lines().enumerate() {
                if line.contains(pattern) {
                    results.push(json!({
                        "path": file_path,
                        "line": index + 1,
                        "text": line
                    }));
                    if results.len() >= 100 {
                        return Ok(results);
                    }
                }
            }
        }
        Ok(results)
    }
}

impl RemoteWorkspaceProvider {
    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.server_base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn send_json<T: for<'de> Deserialize<'de>>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> anyhow::Result<T> {
        let mut request = self
            .client
            .request(method, self.url(path))
            .bearer_auth(&self.auth_token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            anyhow::bail!("remote workspace API error ({status}): {text}");
        }
        Ok(response.json::<T>().await?)
    }

    async fn tree_nodes(&self) -> anyhow::Result<Vec<RemoteFileTree>> {
        self.send_json(
            Method::GET,
            &format!("/projects/{}/tree", self.project_id),
            None,
        )
        .await
    }

    async fn get_content(&self, file_id: &str) -> anyhow::Result<String> {
        let response: RemoteContent = self
            .send_json(
                Method::GET,
                &format!("/projects/{}/files/{file_id}/content", self.project_id),
                None,
            )
            .await?;
        Ok(response.content)
    }

    async fn put_content(&self, file_id: &str, content: &str) -> anyhow::Result<()> {
        let _: RemoteContent = self
            .send_json(
                Method::PUT,
                &format!("/projects/{}/files/{file_id}/content", self.project_id),
                Some(json!(UpdateContentRequest { content })),
            )
            .await?;
        Ok(())
    }

    async fn create_node(
        &self,
        name: &str,
        node_type: &str,
        parent_id: Option<&str>,
    ) -> anyhow::Result<RemoteFileNode> {
        self.send_json(
            Method::POST,
            &format!("/projects/{}/files", self.project_id),
            Some(json!(CreateNodeRequest {
                name,
                node_type,
                parent_id,
                zone: "code"
            })),
        )
        .await
    }

    async fn find_path(&self, path: &str) -> anyhow::Result<Option<RemoteFileTree>> {
        let nodes = self.tree_nodes().await?;
        Ok(find_remote_path(&nodes, &normalize_remote_path(path)))
    }

    async fn ensure_file_path(&self, path: &str) -> anyhow::Result<String> {
        let normalized = normalize_remote_path(path);
        if normalized.is_empty() {
            anyhow::bail!("path is required");
        }
        let parts = normalized.split('/').collect::<Vec<_>>();
        if parts.iter().any(|part| part.is_empty() || *part == "." || *part == "..") {
            anyhow::bail!("invalid remote path");
        }

        let mut current_children = self.tree_nodes().await?;
        let mut parent_id: Option<String> = None;

        for folder_name in parts.iter().take(parts.len().saturating_sub(1)) {
            if let Some(existing) = current_children
                .iter()
                .find(|node| node.name == *folder_name && node.node_type == "folder")
                .cloned()
            {
                parent_id = Some(existing.id.clone());
                current_children = existing.children.unwrap_or_default();
                continue;
            }

            let created = self
                .create_node(folder_name, "folder", parent_id.as_deref())
                .await?;
            parent_id = Some(created.id);
            current_children = Vec::new();
        }

        let file_name = parts
            .last()
            .ok_or_else(|| anyhow::anyhow!("path is required"))?;
        if let Some(existing) = current_children
            .iter()
            .find(|node| node.name == *file_name && node.node_type == "file")
        {
            return Ok(existing.id.clone());
        }

        let created = self
            .create_node(file_name, "file", parent_id.as_deref())
            .await?;
        Ok(created.id)
    }
}

fn normalize_remote_path(path: &str) -> String {
    let trimmed = path.trim().trim_matches('/');
    let trimmed = trimmed.strip_prefix("project/").unwrap_or(trimmed);
    if matches!(trimmed, "." | "project") {
        String::new()
    } else {
        trimmed.to_string()
    }
}

fn remote_tree_to_file_item(node: &RemoteFileTree, parent_path: &str) -> FileTreeItem {
    let path = if parent_path.is_empty() {
        node.name.clone()
    } else {
        format!("{parent_path}/{}", node.name)
    };

    FileTreeItem {
        id: Some(node.id.clone()),
        name: node.name.clone(),
        path: path.clone(),
        node_type: node.node_type.clone(),
        zone: Some(node.zone.clone()),
        language: if node.node_type == "file" {
            language_for_name(&node.name)
        } else {
            None
        },
        children: node.children.as_ref().map(|children| {
            children
                .iter()
                .map(|child| remote_tree_to_file_item(child, &path))
                .collect()
        }),
    }
}

fn find_remote_path(nodes: &[RemoteFileTree], path: &str) -> Option<RemoteFileTree> {
    for node in nodes {
        if node.name == path {
            return Some(node.clone());
        }
        if let Some(rest) = path.strip_prefix(&format!("{}/", node.name)) {
            if let Some(children) = &node.children {
                if let Some(found) = find_remote_path(children, rest) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn collect_file_paths(item: &FileTreeItem, root: &str, output: &mut Vec<String>) {
    if item.node_type == "file" && (root.is_empty() || item.path.starts_with(root)) {
        output.push(item.path.clone());
        return;
    }

    if let Some(children) = &item.children {
        for child in children {
            collect_file_paths(child, root, output);
        }
    }
}

fn language_for_name(name: &str) -> Option<String> {
    let ext = name.rsplit('.').next().unwrap_or("");
    let lang = match ext {
        "py" => "python",
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "json" => "json",
        "md" => "markdown",
        "tex" => "latex",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "txt" => "plaintext",
        _ => "plaintext",
    };
    Some(lang.into())
}
