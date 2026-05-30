use notify::{Event, RecursiveMode, Watcher};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub struct FileWatcher {
    rx: mpsc::UnboundedReceiver<(String, String)>,
    _watcher: notify::RecommendedWatcher,
}

impl FileWatcher {
    pub fn new(work_dir: &PathBuf) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::unbounded_channel();
        let wd = work_dir.clone();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                if !event.kind.is_modify() {
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
