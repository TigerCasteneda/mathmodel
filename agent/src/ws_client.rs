use crate::file_watcher::FileWatcher;
use crate::pty_manager::PtyManager;
use crate::tabbit;
use anyhow::Context;
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AgentMessage {
    #[serde(rename = "terminal_input")]
    TerminalInput { data: String },
    #[serde(rename = "terminal_output")]
    TerminalOutput { data: String },
    #[serde(rename = "claude_command")]
    ClaudeCommand { command: String },
    #[serde(rename = "file_change")]
    FileChange { path: String, content: String },
    #[serde(rename = "tabbit_data")]
    TabbitData {
        payload: tabbit::TabbitPayload,
        raw_json: serde_json::Value,
    },
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "error")]
    AgentError { message: String },
}

pub async fn run(
    server_url: &str,
    token: &str,
    project_id: &str,
    tabbit_port: u16,
    work_dir: &PathBuf,
    claude_path: &PathBuf,
) -> anyhow::Result<()> {
    let url = format!("{}?token={}&project_id={}", server_url, token, project_id);
    let (ws_stream, _) = connect_async(url.as_str())
        .await
        .context("Failed to connect to server")?;
    tracing::info!("Connected to server");

    let (mut ws_tx, mut ws_rx) = ws_stream.split();
    let mut file_watcher = FileWatcher::new(work_dir).context("Failed to start file watcher")?;
    let (pty_input_tx, pty_input_rx) = mpsc::unbounded_channel::<String>();
    let (pty_output_tx, mut pty_output_rx) = mpsc::unbounded_channel::<Result<String, String>>();
    let (outbound_tx, mut outbound_rx) = mpsc::unbounded_channel::<AgentMessage>();

    let pty_work_dir = work_dir.clone();
    let pty_claude_path = claude_path.clone();
    tokio::spawn(async move {
        if let Err(err) = PtyManager::run(
            pty_claude_path,
            pty_work_dir,
            pty_input_rx,
            pty_output_tx.clone(),
        )
        .await
        {
            let _ = pty_output_tx.send(Err(format!("failed to run claude: {err:#}")));
        }
    });

    let tabbit_server = tabbit::run_server(tabbit_port, outbound_tx.clone())
        .await
        .context("Failed to start tabbit server")?;
    tracing::info!("Tabbit listener running at http://127.0.0.1:{tabbit_port}/tabbit");

    outbound_tx
        .send(AgentMessage::Ready)
        .map_err(|_| anyhow::anyhow!("failed to queue ready message"))?;

    // Message loop
    loop {
        tokio::select! {
            server_msg = ws_rx.next() => {
                match server_msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(msg) = serde_json::from_str::<AgentMessage>(&text) {
                            match msg {
                                AgentMessage::TerminalInput { data } => {
                                    let _ = pty_input_tx.send(data);
                                }
                                AgentMessage::ClaudeCommand { command } => {
                                    tracing::info!("claude cmd: {}", command);
                                    let _ = pty_input_tx.send(format!("{command}\n"));
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
