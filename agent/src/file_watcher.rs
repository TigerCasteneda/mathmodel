use notify::{Event, EventKind, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub struct FileWatcher {
    rx: mpsc::UnboundedReceiver<(String, String)>,
    _watcher: notify::RecommendedWatcher,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTreeItem {
    pub name: String,
    pub path: String,
    #[serde(rename = "type")]
    pub node_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<FileTreeItem>>,
}

impl FileWatcher {
    pub fn new(work_dir: &PathBuf) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let wd = work_dir.clone();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if !matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
                ) {
                    return;
                }
                for path in event.paths {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        let rel = path
                            .strip_prefix(&wd)
                            .unwrap_or(&path)
                            .to_string_lossy()
                            .replace('\\', "/");
                        let _ = tx.send((rel, content));
                    }
                }
            }
        })?;

        watcher.watch(work_dir, RecursiveMode::Recursive)?;
        Ok(FileWatcher {
            rx,
            _watcher: watcher,
        })
    }

    pub async fn next_event(&mut self) -> Option<(String, String)> {
        self.rx.recv().await
    }
}

pub fn scan_tree(work_dir: &PathBuf) -> anyhow::Result<FileTreeItem> {
    let root_name = work_dir
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("workspace")
        .to_string();

    scan_path(work_dir, work_dir, root_name)
}

pub fn read_workspace_file(work_dir: &PathBuf, path: &str) -> anyhow::Result<String> {
    let full_path = work_dir.join(path);
    let full_path = full_path.canonicalize()?;
    let root = work_dir.canonicalize()?;
    if !full_path.starts_with(root) || !full_path.is_file() {
        anyhow::bail!("file is outside workspace or not a file");
    }
    Ok(std::fs::read_to_string(full_path)?)
}

fn scan_path(root: &PathBuf, path: &PathBuf, name: String) -> anyhow::Result<FileTreeItem> {
    let rel_path = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    if path.is_dir() {
        let mut children = Vec::new();
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let child_path = entry.path();
            let child_name = entry.file_name().to_string_lossy().to_string();
            if should_skip(&child_name) {
                continue;
            }
            if let Ok(child) = scan_path(root, &child_path, child_name) {
                children.push(child);
            }
        }
        children.sort_by(|a, b| {
            b.node_type
                .cmp(&a.node_type)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        return Ok(FileTreeItem {
            name,
            path: rel_path,
            node_type: "folder".into(),
            language: None,
            children: Some(children),
        });
    }

    Ok(FileTreeItem {
        name: name.clone(),
        path: rel_path,
        node_type: "file".into(),
        language: language_for_name(&name),
        children: None,
    })
}

fn should_skip(name: &str) -> bool {
    matches!(
        name,
        ".git" | ".next" | "node_modules" | "target" | ".DS_Store"
    )
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
