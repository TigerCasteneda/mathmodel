use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;
use tokio::sync::mpsc;

/// Commands sent to the PTY manager task.
pub enum PtyCommand {
    Input(String),
    Resize { cols: u16, rows: u16 },
    Kill,
}

/// Managed Tauri state for the local agent.
pub struct AgentState {
    pub pty_tx: Mutex<Option<mpsc::UnboundedSender<PtyCommand>>>,
    pub work_dir: Mutex<PathBuf>,
    pub app_handle: AppHandle,
}
