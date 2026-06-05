use std::path::PathBuf;
use std::sync::Mutex;
use tauri::async_runtime::JoinHandle;
use tauri::AppHandle;

/// Managed Tauri state for local workspace services.
pub struct AgentState {
    pub work_dir: Mutex<PathBuf>,
    pub watcher_task: Mutex<Option<JoinHandle<()>>>,
    pub app_handle: AppHandle,
}
