use std::path::PathBuf;
use std::sync::Mutex;
use tauri::AppHandle;

/// Managed Tauri state for local workspace services.
pub struct AgentState {
    pub work_dir: Mutex<PathBuf>,
    pub app_handle: AppHandle,
}
