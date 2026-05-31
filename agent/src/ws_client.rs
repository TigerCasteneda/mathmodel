use crate::file_watcher::{read_workspace_file, scan_tree, FileTreeItem, FileWatcher};
use crate::pty_manager::{PtyCommand, PtyManager};
use crate::tabbit;
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::path::Component;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    #[serde(rename = "terminal_input")]
    TerminalInput { data: String },
    #[serde(rename = "terminal_resize")]
    TerminalResize { cols: u16, rows: u16 },
    #[serde(rename = "terminal_output")]
    TerminalOutput { data: String },
    #[serde(rename = "claude_command")]
    ClaudeCommand { command: String },
    #[serde(rename = "file_change")]
    FileChange { path: String, content: String },
    #[serde(rename = "file_tree")]
    FileTree { tree: FileTreeItem },
    #[serde(rename = "list_files")]
    ListFiles,
    #[serde(rename = "open_file")]
    OpenFile { path: String },
    #[serde(rename = "file_content")]
    FileContent { path: String, content: String },
    #[serde(rename = "tabbit_data")]
    TabbitData {
        payload: tabbit::TabbitPayload,
        raw_json: serde_json::Value,
    },
    #[serde(rename = "change_work_dir")]
    ChangeWorkDir { path: String },
    #[serde(rename = "work_dir")]
    WorkDir { path: String },
    #[serde(rename = "new_file")]
    NewFile,
    #[serde(rename = "new_folder")]
    NewFolder,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "create_file")]
    CreateFile { path: String, content: String },
    #[serde(rename = "error")]
    AgentError { message: String },
}

fn validate_create_path(work_dir: &std::path::Path, relative_path: &str) -> Result<std::path::PathBuf, String> {
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
    let canon_work = work_dir.canonicalize().unwrap_or_else(|_| work_dir.to_path_buf());
    if let Ok(canon) = resolved.canonicalize() {
        if !canon.starts_with(&canon_work) {
            return Err("path escapes workspace".into());
        }
    } else if !resolved.starts_with(&canon_work) {
        return Err("path escapes workspace".into());
    }
    Ok(resolved)
}

pub async fn run(
    server_url: &str,
    token: &str,
    project_id: &str,
    tabbit_port: u16,
    work_dir: &PathBuf,
) -> anyhow::Result<()> {
    let url = format!("{}?token={}&project_id={}", server_url, token, project_id);
    let (ws_stream, _) = connect_async(url.as_str())
        .await
        .context("Failed to connect to server")?;
    tracing::info!("Connected to server");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let mut current_work_dir = work_dir.clone();
    let mut file_watcher = FileWatcher::new(&current_work_dir).context("Failed to start file watcher")?;
    let (pty_input_tx, pty_input_rx) = mpsc::unbounded_channel::<PtyCommand>();
    let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<Result<String, String>>();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<AgentMessage>();

    let pty_work_dir = current_work_dir.clone();
    tokio::spawn(async move {
        if let Err(err) = PtyManager::run(pty_work_dir, pty_input_rx, pty_output_tx.clone()).await
        {
            let _ = pty_output_tx.send(Err(format!("failed to run shell: {err:#}")));
        }
    });

    let tabbit_server = tabbit::run_server(tabbit_port, outbound_tx.clone())
        .await
        .context("Failed to start tabbit server")?;
    tracing::info!("Tabbit listener running at http://127.0.0.1:{tabbit_port}/tabbit");

    outbound_tx
        .send(AgentMessage::Ready)
        .map_err(|_| anyhow::anyhow!("failed to queue ready message"))?;
    queue_file_tree(&current_work_dir, &outbound_tx);

    // Message loop
    loop {
        tokio::select! {
            server_msg = ws_rx.next() => {
                match server_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<AgentMessage>(&text) {
                            match msg {
                                AgentMessage::TerminalInput { data } => {
                                    let _ = pty_input_tx.send(PtyCommand::Input(data));
                                }
                                AgentMessage::TerminalResize { cols, rows } => {
                                    let _ = pty_input_tx.send(PtyCommand::Resize { cols, rows });
                                }
                                AgentMessage::ClaudeCommand { command } => {
                                    tracing::info!("claude cmd: {}", command);
                                    let _ = pty_input_tx.send(PtyCommand::Input(format!("{command}\n")));
                                }
                                AgentMessage::ListFiles => {
                                    queue_file_tree(&current_work_dir, &outbound_tx);
                                }
                                AgentMessage::OpenFile { path } => {
                                    match read_workspace_file(&current_work_dir, &path) {
                                        Ok(content) => {
                                            let _ = outbound_tx.send(AgentMessage::FileContent { path, content });
                                        }
                                        Err(err) => {
                                            let _ = outbound_tx.send(AgentMessage::AgentError {
                                                message: format!("failed to open file: {err:#}"),
                                            });
                                        }
                                    }
                                }
                                AgentMessage::ChangeWorkDir { path } => {
                                    let new_dir = PathBuf::from(&path);
                                    match FileWatcher::new(&new_dir) {
                                        Ok(new_watcher) => {
                                            file_watcher = new_watcher;
                                            current_work_dir = new_dir.clone();
                                            let _ = outbound_tx.send(AgentMessage::WorkDir { path });
                                            if let Ok(tree) = scan_tree(&current_work_dir) {
                                                let _ = outbound_tx.send(AgentMessage::FileTree { tree });
                                            }
                                        }
                                        Err(err) => {
                                            let _ = outbound_tx.send(AgentMessage::AgentError {
                                                message: format!("cannot watch directory: {err:#}"),
                                            });
                                        }
                                    }
                                }
                                AgentMessage::NewFile => {
                                    let _ = pty_input_tx.send(PtyCommand::Input(
                                        "code .\n".to_string(),
                                    ));
                                    let _ = outbound_tx.send(AgentMessage::AgentError {
                                        message: "New File: use the terminal to create files".into(),
                                    });
                                }
                                AgentMessage::NewFolder => {
                                    let _ = outbound_tx.send(AgentMessage::AgentError {
                                        message: "New Folder: use the terminal".into(),
                                    });
                                }
                                AgentMessage::CreateFile { path, content } => {
                                    match validate_create_path(&current_work_dir, &path) {
                                        Ok(resolved) => {
                                            if let Some(parent) = resolved.parent() {
                                                if let Err(err) = std::fs::create_dir_all(parent) {
                                                    let _ = outbound_tx.send(AgentMessage::AgentError {
                                                        message: format!("failed to create parent dir: {err:#}"),
                                                    });
                                                    continue;
                                                }
                                            }
                                            if resolved.exists() {
                                                tracing::warn!("create_file: path already exists, skipping: {}", path);
                                                continue;
                                            }
                                            if let Err(err) = std::fs::write(&resolved, &content) {
                                                let _ = outbound_tx.send(AgentMessage::AgentError {
                                                    message: format!("failed to write file: {err:#}"),
                                                });
                                            } else {
                                                tracing::info!("create_file: wrote {}", path);
                                            }
                                        }
                                        Err(err) => {
                                            let _ = outbound_tx.send(AgentMessage::AgentError {
                                                message: format!("create_file rejected: {err}"),
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => {
                        tracing::info!("Server closed connection");
                        break;
                    }
                    _ => continue,
                }
            }
            Some(agent_msg) = outbound_rx.recv() => {
                ws_tx
                    .send(Message::Text(serde_json::to_string(&agent_msg)?))
                    .await?;
            }
            Some(pty_msg) = pty_output_rx.recv() => {
                let agent_msg = match pty_msg {
                    Ok(data) => AgentMessage::TerminalOutput { data },
                    Err(message) => AgentMessage::AgentError { message },
                };
                let _ = outbound_tx.send(agent_msg);
            }
            file_change = file_watcher.next_event() => {
                if let Some((path, content)) = file_change {
                    let msg = AgentMessage::FileChange { path, content };
                    let _ = outbound_tx.send(msg);
                    queue_file_tree(&current_work_dir, &outbound_tx);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                tracing::info!("Ctrl+C");
                break;
            }
        }
    }

    tabbit_server.abort();
    Ok(())
}

fn queue_file_tree(work_dir: &PathBuf, outbound_tx: &mpsc::UnboundedSender<AgentMessage>) {
    match scan_tree(work_dir) {
        Ok(tree) => {
            let _ = outbound_tx.send(AgentMessage::FileTree { tree });
        }
        Err(err) => {
            let _ = outbound_tx.send(AgentMessage::AgentError {
                message: format!("failed to scan workspace: {err:#}"),
            });
        }
    }
}
