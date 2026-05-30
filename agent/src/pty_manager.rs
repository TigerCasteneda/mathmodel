use anyhow::Context;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::mpsc;

pub struct PtyManager;

impl PtyManager {
    pub async fn run(
        claude_path: PathBuf,
        work_dir: PathBuf,
        mut input_rx: mpsc::UnboundedReceiver<String>,
        output_tx: mpsc::UnboundedSender<Result<String, String>>,
    ) -> anyhow::Result<()> {
        let mut child = Command::new(claude_path)
            .current_dir(work_dir)
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn process")?;

        let mut stdin = child.stdin.take().context("no stdin")?;
        let mut stdout = child.stdout.take().context("no stdout")?;
        let read_tx = output_tx.clone();

        let read_task = tokio::spawn(async move {
            let mut buf = vec![0; 4096];
            loop {
                match stdout.read(&mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = read_tx.send(Ok(data));
                    }
                    Err(err) => {
                        let _ = read_tx.send(Err(format!("failed to read process output: {err}")));
                        break;
                    }
                }
            }
        });

        while let Some(input) = input_rx.recv().await {
            if let Err(err) = stdin.write_all(input.as_bytes()).await {
                let _ = output_tx.send(Err(format!("failed to write process input: {err}")));
                break;
            }
            if let Err(err) = stdin.flush().await {
                let _ = output_tx.send(Err(format!("failed to flush process input: {err}")));
                break;
            }
        }

        let _ = child.start_kill();
        let _ = read_task.await;
        Ok(())
    }
}
