use crate::agent::events::AgentEvent;
use crate::agent::file_watcher::{self, FileTreeItem};
use crate::agent::state::AgentState;
use base64::Engine;
use chrono::Utc;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
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
    write_new_file(&work_dir, &path, &content)
}

/// Write a new file under `work_dir`, creating parent dirs as needed.
/// Returns `"file already exists"` if a file is already at the target path.
/// Used by both the agent `create_file` Tauri command and the research
/// save mirror; the latter relies on the no-overwrite guarantee to avoid
/// clobbering user-edited local copies of mirrored `.md` files.
pub(crate) fn write_new_file(
    work_dir: &Path,
    path: &str,
    content: &str,
) -> Result<(), String> {
    let resolved = validate_and_resolve_path(work_dir, path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{e:#}"))?;
    }
    if resolved.exists() {
        return Err("file already exists".into());
    }
    std::fs::write(&resolved, content).map_err(|e| format!("{e:#}"))
}

/// Mirror a server-saved research item set into the host workspace under
/// `host_folder/references/`. Returns a `serde_json::Value` summary that the
/// Tauri command embeds in its response under the `local_mirror` key.
///
/// Behavior:
/// - `workspace_mode == "host"` + `host_folder` set: write each entry via
///   `write_new_file` (no-overwrite). Update `references/.sasu-manifest.json`
///   with `(relative_path, cloud_file_id, title, url, saved_at)` entries.
/// - Otherwise: mark all server mirrors as skipped; do not touch disk.
///
/// Failures are non-fatal: each mirror entry is best-effort, errors are
/// captured into `errors[]` and the function never returns `Err`.
pub(crate) fn mirror_research_save_to_host(
    workspace_mode: Option<&str>,
    host_folder: Option<&str>,
    project_id: &str,
    server_response: &Value,
) -> Value {
    let mirrors = server_response
        .get("mirrors")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let work_dir_path = match (workspace_mode, host_folder) {
        (Some(mode), Some(folder)) if mode == "host" && !folder.trim().is_empty() => {
            Some(PathBuf::from(folder))
        }
        _ => None,
    };

    let Some(work_dir) = work_dir_path else {
        return json!({
            "attempted": 0,
            "created": 0,
            "skipped": mirrors.len() as i64,
            "errors": [],
        });
    };

    let mut attempted: i64 = 0;
    let mut created: i64 = 0;
    let mut errors: Vec<Value> = Vec::new();
    let mut manifest_entries: Vec<Value> = Vec::new();
    let now = Utc::now().timestamp();

    for mirror in &mirrors {
        let obj = match mirror.as_object() {
            Some(obj) => obj,
            None => continue,
        };
        let file_name = obj
            .get("file_name")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let cloud_file_id = obj
            .get("cloud_file_id")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let title = obj
            .get("title")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let url = obj
            .get("url")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        let body_md = obj
            .get("body_md")
            .and_then(|value| value.as_str())
            .unwrap_or_default();

        if file_name.is_empty() {
            continue;
        }

        attempted += 1;
        let rel_path = format!("references/{file_name}");
        match write_new_file(&work_dir, &rel_path, body_md) {
            Ok(()) => {
                created += 1;
                manifest_entries.push(json!({
                    "relative_path": rel_path,
                    "cloud_file_id": cloud_file_id,
                    "title": title,
                    "url": url,
                    "saved_at": now,
                }));

                // Optional bib companion file. Failures are non-fatal and
                // intentionally not surfaced in the top-level summary so the
                // markdown mirror's success is what the UI celebrates.
                if let Some(bib_name) = obj.get("bib_file_name").and_then(|value| value.as_str()) {
                    let bib_body = obj
                        .get("body_bib")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default();
                    let bib_rel = format!("references/{bib_name}");
                    match write_new_file(&work_dir, &bib_rel, bib_body) {
                        Ok(()) => {
                            manifest_entries.push(json!({
                                "relative_path": bib_rel,
                                "cloud_file_id": String::new(),
                                "title": title,
                                "url": url,
                                "saved_at": now,
                                "kind": "bib",
                            }));
                        }
                        Err(err) => {
                            tracing::warn!(
                                "Local bib mirror skipped for {}: {err}",
                                bib_rel
                            );
                        }
                    }
                }
            }
            Err(err) => {
                errors.push(json!({
                    "file_name": file_name,
                    "error": err,
                }));
            }
        }
    }

    if !manifest_entries.is_empty() {
        if let Err(err) = append_manifest_entries(&work_dir, project_id, &manifest_entries) {
            tracing::warn!("Failed to update references/.sasu-manifest.json: {err}");
        }
    }

    json!({
        "attempted": attempted,
        "created": created,
        "skipped": 0,
        "errors": errors,
    })
}

/// Merge new entries into the host's `references/.sasu-manifest.json`. The
/// manifest maps `(relative_path, cloud_file_id, title, url, saved_at)` and
/// is the seam reserved for future reverse-sync work; this function only
/// appends and never reads the file.
fn append_manifest_entries(
    work_dir: &Path,
    project_id: &str,
    new_entries: &[Value],
) -> Result<(), String> {
    let manifest_path_rel = "references/.sasu-manifest.json";
    let resolved = validate_and_resolve_path(work_dir, manifest_path_rel)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{e:#}"))?;
    }

    let mut existing_entries: Vec<Value> = Vec::new();
    let mut project_id_match = false;
    let mut version_seen = false;
    if resolved.exists() {
        match std::fs::read_to_string(&resolved) {
            Ok(content) => match serde_json::from_str::<Value>(&content) {
                Ok(parsed) => {
                    if let Some(arr) = parsed.get("entries").and_then(|value| value.as_array()) {
                        existing_entries = arr.clone();
                    }
                    if parsed
                        .get("project_id")
                        .and_then(|value| value.as_str())
                        .map(|value| value == project_id)
                        .unwrap_or(false)
                    {
                        project_id_match = true;
                    }
                    if parsed.get("version").is_some() {
                        version_seen = true;
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Existing manifest at {} is not valid JSON; rewriting ({err})",
                        resolved.display()
                    );
                }
            },
            Err(err) => {
                tracing::warn!(
                    "Failed to read existing manifest at {}: {err}",
                    resolved.display()
                );
            }
        }
    }

    existing_entries.extend(new_entries.iter().cloned());
    let updated = json!({
        "version": if version_seen { 1 } else { 1 },
        "project_id": if project_id_match || project_id.is_empty() {
            project_id
        } else {
            project_id
        },
        "updated_at": Utc::now().timestamp(),
        "entries": existing_entries,
    });
    let serialized = serde_json::to_string_pretty(&updated)
        .map_err(|err| format!("serialize manifest: {err}"))?;
    std::fs::write(&resolved, serialized).map_err(|err| format!("{err:#}"))?;
    Ok(())
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

/// Return the current host workspace root, or `None` if the user has not
/// opened a folder. Used by the research save flow to decide whether to
/// mirror server-saved items into `work_dir/references/`.
#[tauri::command]
pub async fn get_work_dir(state: State<'_, AgentState>) -> Result<Option<String>, String> {
    let work_dir = state.work_dir.lock().map_err(|e| e.to_string())?.clone();
    let path_str = work_dir.to_string_lossy().to_string();
    if path_str.trim().is_empty() {
        Ok(None)
    } else {
        Ok(Some(path_str))
    }
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
    use super::{mirror_research_save_to_host, validate_and_resolve_path, write_new_file};
    use serde_json::{json, Value};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// Create a unique temp directory for a test. Each test gets its own dir
    /// so they can run in parallel without colliding.
    fn unique_tmp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("modeler-{label}-{nanos}-{n}"));
        fs::create_dir_all(&dir).expect("create tmp dir");
        dir
    }

    #[test]
    fn rejects_path_traversal() {
        let err = validate_and_resolve_path(Path::new("."), "../outside.txt").unwrap_err();
        assert!(err.contains("traversal"));
    }

    #[test]
    fn write_new_file_creates_parent_dirs_and_writes_content() {
        let work_dir = unique_tmp_dir("wnf-create-parents");
        let rel = "references/sub/dir/note.md";

        write_new_file(&work_dir, rel, "# hello\n").expect("write should succeed");

        let on_disk = work_dir.join("references").join("sub").join("dir").join("note.md");
        assert!(on_disk.exists(), "file should exist on disk");
        assert_eq!(fs::read_to_string(&on_disk).unwrap(), "# hello\n");

        let _ = fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn write_new_file_rejects_overwrite() {
        let work_dir = unique_tmp_dir("wnf-no-overwrite");
        let rel = "notes.md";
        let path = work_dir.join(rel);
        fs::write(&path, "user edited this").unwrap();

        let err = write_new_file(&work_dir, rel, "mirror would clobber").unwrap_err();
        assert_eq!(err, "file already exists");

        // Local content is preserved untouched.
        assert_eq!(fs::read_to_string(&path).unwrap(), "user edited this");

        let _ = fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn write_new_file_rejects_path_traversal() {
        let work_dir = unique_tmp_dir("wnf-traversal");
        let err = write_new_file(&work_dir, "../escape.md", "x").unwrap_err();
        assert!(err.contains("traversal") || err.contains("escapes"));

        let _ = fs::remove_dir_all(&work_dir);
    }

    fn sample_server_response(file_name: &str, body: &str) -> Value {
        json!({
            "saved": 1,
            "items": [],
            "files_created": 1,
            "warnings": [],
            "mirrors": [{
                "cloud_file_id": "7f3a8c12-aaaa",
                "file_name": file_name,
                "body_md": body,
                "title": "Bayesian SIR",
                "url": "https://example.com/paper",
                "bib_file_name": Value::Null,
                "body_bib": Value::Null,
            }],
        })
    }

    #[test]
    fn mirror_skips_in_guest_mode() {
        let resp = sample_server_response("bayesian_sir-7f3a8c12.md", "# body");
        let summary = mirror_research_save_to_host(
            Some("guest"),
            Some("C:/should/not/be/touched"),
            "proj-1",
            &resp,
        );
        assert_eq!(summary["attempted"], 0);
        assert_eq!(summary["created"], 0);
        assert_eq!(summary["skipped"], 1);
        assert_eq!(summary["errors"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn mirror_skips_when_host_folder_missing() {
        let resp = sample_server_response("note-7f3a8c12.md", "# body");
        let summary = mirror_research_save_to_host(
            Some("host"),
            None,
            "proj-1",
            &resp,
        );
        assert_eq!(summary["attempted"], 0);
        assert_eq!(summary["skipped"], 1);
    }

    #[test]
    fn mirror_writes_files_and_manifest_in_host_mode() {
        let work_dir = unique_tmp_dir("mirror-write");
        let resp = sample_server_response("bayesian_sir-7f3a8c12.md", "# body\n");
        let summary = mirror_research_save_to_host(
            Some("host"),
            Some(work_dir.to_string_lossy().as_ref()),
            "proj-1",
            &resp,
        );
        assert_eq!(summary["attempted"], 1);
        assert_eq!(summary["created"], 1);
        assert_eq!(summary["errors"].as_array().unwrap().len(), 0);

        let on_disk = work_dir.join("references").join("bayesian_sir-7f3a8c12.md");
        assert!(on_disk.exists(), "mirror file should exist");
        assert_eq!(fs::read_to_string(&on_disk).unwrap(), "# body\n");

        let manifest = work_dir.join("references").join(".sasu-manifest.json");
        assert!(manifest.exists(), "manifest should be written");
        let parsed: Value =
            serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
        assert_eq!(parsed["project_id"], "proj-1");
        let entries = parsed["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["relative_path"], "references/bayesian_sir-7f3a8c12.md");
        assert_eq!(entries[0]["cloud_file_id"], "7f3a8c12-aaaa");

        let _ = fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn mirror_records_error_when_local_file_exists() {
        let work_dir = unique_tmp_dir("mirror-exists");
        let refs_dir = work_dir.join("references");
        fs::create_dir_all(&refs_dir).unwrap();
        let existing = refs_dir.join("bayesian_sir-7f3a8c12.md");
        fs::write(&existing, "user edited this").unwrap();

        let resp = sample_server_response("bayesian_sir-7f3a8c12.md", "# body");
        let summary = mirror_research_save_to_host(
            Some("host"),
            Some(work_dir.to_string_lossy().as_ref()),
            "proj-1",
            &resp,
        );
        assert_eq!(summary["attempted"], 1);
        assert_eq!(summary["created"], 0);
        let errors = summary["errors"].as_array().unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["file_name"], "bayesian_sir-7f3a8c12.md");
        assert_eq!(errors[0]["error"], "file already exists");

        // User's edit is preserved.
        assert_eq!(fs::read_to_string(&existing).unwrap(), "user edited this");

        let _ = fs::remove_dir_all(&work_dir);
    }

    #[test]
    fn mirror_writes_bib_companion_file_when_present() {
        let work_dir = unique_tmp_dir("mirror-bib");
        let resp = json!({
            "saved": 1,
            "items": [],
            "files_created": 2,
            "warnings": [],
            "mirrors": [{
                "cloud_file_id": "cid-xxxx",
                "file_name": "note-aaaa1111.md",
                "body_md": "# body",
                "title": "Note",
                "url": "https://example.com",
                "bib_file_name": "note-aaaa1111.bib",
                "body_bib": "@article{key,title={Note}}",
            }],
        });
        let summary = mirror_research_save_to_host(
            Some("host"),
            Some(work_dir.to_string_lossy().as_ref()),
            "proj-1",
            &resp,
        );
        assert_eq!(summary["created"], 1);
        let bib = work_dir.join("references").join("note-aaaa1111.bib");
        assert!(bib.exists());
        assert_eq!(
            fs::read_to_string(&bib).unwrap(),
            "@article{key,title={Note}}"
        );

        let manifest = work_dir.join("references").join(".sasu-manifest.json");
        let parsed: Value =
            serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
        let entries = parsed["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e["relative_path"] == "references/note-aaaa1111.bib"
            && e["kind"] == "bib"));

        let _ = fs::remove_dir_all(&work_dir);
    }
}
