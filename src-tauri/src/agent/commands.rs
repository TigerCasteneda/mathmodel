use crate::agent::events::AgentEvent;
use crate::agent::file_watcher::{self, FileTreeItem};
use crate::agent::pty;
use crate::agent::state::{AgentState, PtyCommand};
use std::path::{Component, PathBuf};
use tauri::{Emitter, State};
use tokio::sync::mpsc;

fn validate_create_path(work_dir: &std::path::Path, relative_path: &str) -> Result<PathBuf, String> {
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
pub async fn pty_spawn(state: State<'_, AgentState>) -> Result<(), String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let (tx, rx) = mpsc::unbounded_channel();
    {
        let mut pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
        if let Some(old_tx) = pty_tx.take() {
            let _ = old_tx.send(PtyCommand::Kill);
        }
        *pty_tx = Some(tx);
    }

    let app_handle = state.app_handle.clone();
    tokio::spawn(async move {
        if let Err(err) = pty::spawn_pty(work_dir, rx, app_handle.clone()).await {
            let _ = app_handle.emit(
                "agent-error",
                AgentEvent::AgentError {
                    message: format!("PTY spawn failed: {err:#}"),
                },
            );
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn pty_write(data: String, state: State<'_, AgentState>) -> Result<(), String> {
    let pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
    match pty_tx.as_ref() {
        Some(tx) => tx
            .send(PtyCommand::Input(data))
            .map_err(|_| "PTY has closed".to_string()),
        None => Err("No PTY session. Call pty_spawn first.".to_string()),
    }
}

#[tauri::command]
pub async fn pty_resize(
    cols: u16,
    rows: u16,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
    match pty_tx.as_ref() {
        Some(tx) => tx
            .send(PtyCommand::Resize { cols, rows })
            .map_err(|_| "PTY has closed".to_string()),
        None => Err("No PTY session. Call pty_spawn first.".to_string()),
    }
}

#[tauri::command]
pub async fn pty_kill(state: State<'_, AgentState>) -> Result<(), String> {
    let mut pty_tx = state.pty_tx.lock().map_err(|e| e.to_string())?;
    if let Some(tx) = pty_tx.take() {
        let _ = tx.send(PtyCommand::Kill);
    }
    Ok(())
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
pub async fn create_file(
    path: String,
    content: String,
    state: State<'_, AgentState>,
) -> Result<(), String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let resolved = validate_create_path(&work_dir, &path)?;
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
    let _ = state.app_handle.emit(
        "work-dir",
        AgentEvent::WorkDir {
            path: path.clone(),
        },
    );
    let tree = file_watcher::scan_tree(&new_dir).map_err(|e| format!("{e:#}"))?;
    let _ = state.app_handle.emit(
        "file-tree",
        AgentEvent::FileTree {
            tree: tree.clone(),
        },
    );
    Ok(tree)
}
