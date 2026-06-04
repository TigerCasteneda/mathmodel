use crate::agent::file_watcher::FileTreeItem;
use serde::{Deserialize, Serialize};

/// Events emitted from the Tauri backend to the frontend.
/// Each variant maps to a Tauri event name for type-safe frontend listening.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    #[serde(rename = "agent_error")]
    AgentError { message: String },
    #[serde(rename = "file_change")]
    FileChange { path: String, content: String },
    #[serde(rename = "file_tree")]
    FileTree { tree: FileTreeItem },
    #[serde(rename = "file_content")]
    FileContent { path: String, content: String },
    #[serde(rename = "work_dir")]
    WorkDir { path: String },
}
