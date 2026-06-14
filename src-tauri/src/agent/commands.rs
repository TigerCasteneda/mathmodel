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
            } else {
                // No text content → a binary file (or unreadable). Notify viewers
                // by path so open PDF/image previews can re-fetch their bytes.
                let _ = app_handle.emit(
                    "file-binary-change",
                    AgentEvent::FileBinaryChange { path: event.path },
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
    requested_path: &str,
) -> Result<PathBuf, String> {
    relative_or_workspace_path(work_dir, requested_path)
}

fn relative_or_workspace_path(
    work_dir: &std::path::Path,
    requested_path: &str,
) -> Result<PathBuf, String> {
    let mut path_value = requested_path
        .strip_prefix("file://")
        .unwrap_or(requested_path);
    #[cfg(windows)]
    {
        if path_value.starts_with('/') && path_value.as_bytes().get(2) == Some(&b':') {
            path_value = &path_value[1..];
        }
    }

    let requested = std::path::Path::new(path_value);
    let canon_work = work_dir
        .canonicalize()
        .unwrap_or_else(|_| work_dir.to_path_buf());

    let resolved = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        for component in requested.components() {
            match component {
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                    return Err("path traversal rejected".into());
                }
                _ => {}
            }
        }
        canon_work.join(requested)
    };

    let path_for_check = resolved.canonicalize().unwrap_or_else(|_| resolved.clone());
    if !path_for_check.starts_with(&canon_work) {
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

#[derive(serde::Serialize)]
pub struct LatexCompileResult {
    pub success: bool,
    /// Workspace-relative path of the produced PDF, if compilation succeeded.
    pub pdf_path: Option<String>,
    /// Tail of the compiler log — enough to surface the first error without
    /// flooding the UI with the full multi-pass latexmk output.
    pub log: String,
}

/// Compile a `.tex` file to PDF with latexmk. Runs in the file's own directory
/// so `\input`/`\includegraphics` relative paths resolve, and emits the PDF
/// next to the source. The file watcher then fires `file-binary-change`, so an
/// open PDF preview refreshes on its own. Host Local only (needs the local
/// TeX toolchain); guests have no host shell.
#[tauri::command]
pub async fn compile_latex(
    path: String,
    state: State<'_, AgentState>,
) -> Result<LatexCompileResult, String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let resolved = validate_and_resolve_path(&work_dir, &path)?;
    if resolved.extension().and_then(|e| e.to_str()) != Some("tex") {
        return Err("not a .tex file".into());
    }
    if !resolved.is_file() {
        return Err("file not found".into());
    }

    let dir = resolved
        .parent()
        .ok_or_else(|| "file has no parent directory".to_string())?
        .to_path_buf();
    let file_name = resolved
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "invalid file name".to_string())?
        .to_string();

    // latexmk drives the needed pdflatex/biber passes itself. nonstopmode keeps
    // it from blocking on an error prompt; we report failures via exit status.
    let mut cmd = tokio::process::Command::new("latexmk");
    cmd.arg("-pdf")
        .arg("-interaction=nonstopmode")
        .arg("-halt-on-error")
        .arg(&file_name)
        .current_dir(&dir);

    let output = tokio::time::timeout(std::time::Duration::from_secs(180), cmd.output())
        .await
        .map_err(|_| "latex compile timed out after 180s".to_string())?
        .map_err(|e| format!("failed to launch latexmk (is TeX Live installed and on PATH?): {e}"))?;

    let mut log = String::from_utf8_lossy(&output.stdout).into_owned();
    log.push_str(&String::from_utf8_lossy(&output.stderr));
    // Keep only the tail — latexmk output is long and the error is near the end.
    let tail: String = log.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");

    let success = output.status.success();
    let pdf_path = if success {
        let pdf = resolved.with_extension("pdf");
        pdf.strip_prefix(&work_dir)
            .ok()
            .map(|rel| rel.to_string_lossy().replace('\\', "/"))
    } else {
        None
    };

    Ok(LatexCompileResult { success, pdf_path, log: tail })
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
