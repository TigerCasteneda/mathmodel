use anyhow::Context;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use tokio::sync::mpsc;

pub struct PtyManager;

pub enum PtyCommand {
    Input(String),
    Resize { cols: u16, rows: u16 },
}

impl PtyManager {
    pub async fn run(
        work_dir: PathBuf,
        command_rx: mpsc::UnboundedReceiver<PtyCommand>,
        output_tx: mpsc::UnboundedSender<Result<String, String>>,
    ) -> anyhow::Result<()> {
        tokio::task::spawn_blocking(move || run_pty(work_dir, command_rx, output_tx))
            .await
            .context("PTY task join failed")?
    }
}

fn run_pty(
    work_dir: PathBuf,
    mut command_rx: mpsc::UnboundedReceiver<PtyCommand>,
    output_tx: mpsc::UnboundedSender<Result<String, String>>,
) -> anyhow::Result<()> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("Failed to open PTY")?;

    let mut command = shell_command();
    command.cwd(work_dir);
    let mut child = pair
        .slave
        .spawn_command(command)
        .context("Failed to spawn shell in PTY")?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().context("no PTY reader")?;
    let mut writer = pair.master.take_writer().context("no PTY writer")?;
    let read_tx = output_tx.clone();

    let read_thread = std::thread::spawn(move || {
        let mut buf = vec![0; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buf[..n]).to_string();
                    let _ = read_tx.send(Ok(data));
                }
                Err(err) => {
                    let _ = read_tx.send(Err(format!("failed to read PTY output: {err}")));
                    break;
                }
            }
        }
    });

    while let Some(command) = command_rx.blocking_recv() {
        match command {
            PtyCommand::Input(data) => {
                if let Err(err) = writer.write_all(data.as_bytes()) {
                    let _ = output_tx.send(Err(format!("failed to write PTY input: {err}")));
                    break;
                }
                if let Err(err) = writer.flush() {
                    let _ = output_tx.send(Err(format!("failed to flush PTY input: {err}")));
                    break;
                }
            }
            PtyCommand::Resize { cols, rows } => {
                if let Err(err) = pair.master.resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                }) {
                    let _ = output_tx.send(Err(format!("failed to resize PTY: {err}")));
                }
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    let _ = read_thread.join();
    Ok(())
}

fn shell_command() -> CommandBuilder {
    #[cfg(windows)]
    {
        for shell in ["pwsh.exe", "powershell.exe", "cmd.exe"] {
            if command_exists(shell) {
                let mut command = CommandBuilder::new(shell);
                if shell == "cmd.exe" {
                    command.arg("/Q");
                }
                return command;
            }
        }
        CommandBuilder::new("cmd.exe")
    }

    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut command = CommandBuilder::new(shell);
        command.arg("-i");
        command
    }
}

#[cfg(windows)]
fn command_exists(command: &str) -> bool {
    std::process::Command::new("where")
        .arg(command)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
