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
    /// A binary (non-UTF-8) file changed on disk. Carries no content — viewers
    /// re-fetch the bytes themselves. Lets PDF/image previews refresh after an
    /// external recompile (e.g. LaTeX → PDF) instead of showing a stale render.
    #[serde(rename = "file_binary_change")]
    FileBinaryChange { path: String },
    #[serde(rename = "file_tree")]
    FileTree { tree: FileTreeItem },
    #[serde(rename = "file_content")]
    FileContent { path: String, content: String },
    #[serde(rename = "work_dir")]
    WorkDir { path: String },
}
