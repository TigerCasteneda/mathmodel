use crate::agent::events::AgentEvent;
use crate::agent::file_watcher::{self, FileTreeItem};
use crate::agent::state::AgentState;
use base64::Engine;
use std::path::{Component, PathBuf};
use tauri::{Emitter, State};

fn start_workspace_watcher(state: &AgentState, work_dir: PathBuf) -> Result<(), String> {
    if let Some(task) = state
        .watcher_task
        .lock()
        .map_err(|e| e.to_string())?
        .take()
    {
        task.abort();
    }

    let app_handle = state.app_handle.clone();
    let watcher_dir = work_dir.clone();
    let mut watcher = file_watcher::FileWatcher::new(&work_dir).map_err(|e| format!("{e:#}"))?;
    let task = tauri::async_runtime::spawn(async move {
        while let Some(event) = watcher.next_event().await {
            if let Some(content) = event.content {
                let _ = app_handle.emit(
                    "file-change",
                    AgentEvent::FileChange {
                        path: event.path,
                        content,
                    },
                );
            }
            if let Ok(tree) = file_watcher::scan_tree(&watcher_dir) {
                let _ = app_handle.emit("file-tree", AgentEvent::FileTree { tree });
            }
        }
    });

    *state.watcher_task.lock().map_err(|e| e.to_string())? = Some(task);
    Ok(())
}

pub fn validate_and_resolve_path(
    work_dir: &std::path::Path,
    relative_path: &str,
) -> Result<PathBuf, String> {
    let rel = std::path::Path::new(relative_path);
    if rel.is_absolute() {
        return Err("absolute path rejected".into());
    }
    for component in rel.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("path traversal rejected".into());
            }
            _ => {}
        }
    }
    let resolved = work_dir.join(rel);
    let canon_work = work_dir
        .canonicalize()
        .unwrap_or_else(|_| work_dir.to_path_buf());
    if let Ok(canon) = resolved.canonicalize() {
        if !canon.starts_with(&canon_work) {
            return Err("path escapes workspace".into());
        }
    } else if !resolved.starts_with(&canon_work) {
        return Err("path escapes workspace".into());
    }
    Ok(resolved)
}

#[tauri::command]
pub async fn list_files(state: State<'_, AgentState>) -> Result<FileTreeItem, String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    file_watcher::scan_tree(&work_dir).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn read_file(path: String, state: State<'_, AgentState>) -> Result<String, String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    file_watcher::read_workspace_file(&work_dir, &path).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn read_file_base64(
    path: String,
    state: State<'_, AgentState>,
) -> Result<String, String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let resolved = validate_and_resolve_path(&work_dir, &path)?;
    let bytes = std::fs::read(&resolved).map_err(|e| format!("Failed to read file: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

#[tauri::command]
pub async fn write_file(
    path: String,
    content: String,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let resolved = validate_and_resolve_path(&work_dir, &path)?;
    if resolved.exists() && !resolved.is_file() {
        return Err("path is not a file".into());
    }
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{e:#}"))?;
    }
    std::fs::write(&resolved, &content).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn create_file(
    path: String,
    content: String,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let resolved = validate_and_resolve_path(&work_dir, &path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{e:#}"))?;
    }
    if resolved.exists() {
        return Err("file already exists".into());
    }
    std::fs::write(&resolved, &content).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
pub async fn change_work_dir(
    path: String,
    state: State<'_, AgentState>,
) -> Result<FileTreeItem, String> {
    let new_dir = PathBuf::from(&path);
    if !new_dir.is_dir() {
        return Err("path is not a directory".into());
    }
    {
        let mut work_dir = state.work_dir.lock().map_err(|e| e.to_string())?;
        *work_dir = new_dir.clone();
    }
    let _ = state
        .app_handle
        .emit("work-dir", AgentEvent::WorkDir { path: path.clone() });
    let tree = file_watcher::scan_tree(&new_dir).map_err(|e| format!("{e:#}"))?;
    let _ = state
        .app_handle
        .emit("file-tree", AgentEvent::FileTree { tree: tree.clone() });
    start_workspace_watcher(&state, new_dir)?;
    Ok(tree)
}

#[tauri::command]
pub async fn open_folder(state: State<'_, AgentState>) -> Result<Option<String>, String> {
    let folder = rfd::AsyncFileDialog::new()
        .pick_folder()
        .await
        .map(|handle| handle.path().to_string_lossy().to_string());

    if let Some(ref path) = folder {
        let new_dir = PathBuf::from(path);
        {
            let mut work_dir = state.work_dir.lock().map_err(|e| e.to_string())?;
            *work_dir = new_dir.clone();
        }
        let _ = state
            .app_handle
            .emit("work-dir", AgentEvent::WorkDir { path: path.clone() });
        if let Ok(tree) = file_watcher::scan_tree(&new_dir) {
            let _ = state
                .app_handle
                .emit("file-tree", AgentEvent::FileTree { tree });
        }
        start_workspace_watcher(&state, new_dir)?;
    }

    Ok(folder)
}

#[cfg(test)]
mod tests {
    use super::validate_and_resolve_path;
    use std::path::Path;

    #[test]
    fn rejects_path_traversal() {
        let err = validate_and_resolve_path(Path::new("."), "../outside.txt").unwrap_err();
        assert!(err.contains("traversal"));
    }
}
