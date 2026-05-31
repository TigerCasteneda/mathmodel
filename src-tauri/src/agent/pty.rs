use crate::agent::events::AgentEvent;
use crate::agent::state::PtyCommand;
use anyhow::Context;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::path::PathBuf;
use tauri::Emitter;

/// Spawn a PTY shell, forwarding output to the frontend via Tauri events.
/// Runs the blocking PTY I/O on a dedicated OS thread.
pub async fn spawn_pty(
    work_dir: PathBuf,
    mut command_rx: tokio::sync::mpsc::UnboundedReceiver<PtyCommand>,
    app_handle: tauri::AppHandle,
) -> anyhow::Result<()> {
    tokio::task::spawn_blocking(move || {
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
        command.cwd(&work_dir);
        let mut child = pair
            .slave
            .spawn_command(command)
            .context("Failed to spawn shell in PTY")?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().context("no PTY reader")?;
        let mut writer = pair.master.take_writer().context("no PTY writer")?;

        let read_app = app_handle.clone();
        let read_thread = std::thread::spawn(move || {
            let mut buf = vec![0; 8192];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = read_app.emit("pty-output", AgentEvent::PtyOutput { data });
                    }
                    Err(err) => {
                        let _ = read_app.emit(
                            "agent-error",
                            AgentEvent::AgentError {
                                message: format!("PTY read error: {err}"),
                            },
                        );
                        break;
                    }
                }
            }
        });

        while let Some(cmd) = command_rx.blocking_recv() {
            match cmd {
                PtyCommand::Input(data) => {
                    if let Err(err) = writer.write_all(data.as_bytes()) {
                        let _ = app_handle.emit(
                            "agent-error",
                            AgentEvent::AgentError {
                                message: format!("PTY write error: {err}"),
                            },
                        );
                        break;
                    }
                    let _ = writer.flush();
                }
                PtyCommand::Resize { cols, rows } => {
                    let _ = pair.master.resize(PtySize {
                        rows,
                        cols,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }
                PtyCommand::Kill => break,
            }
        }

        let _ = child.kill();
        let _ = child.wait();
        let _ = read_thread.join();
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("PTY task join failed")??;

    Ok(())
}

fn shell_command() -> CommandBuilder {
    #[cfg(windows)]
    {
        for shell in ["pwsh.exe", "powershell.exe", "cmd.exe"] {
            if command_exists(shell) {
                let mut cmd = CommandBuilder::new(shell);
                if shell == "cmd.exe" {
                    cmd.arg("/Q");
                }
                return cmd;
            }
        }
        CommandBuilder::new("cmd.exe")
    }

    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut cmd = CommandBuilder::new(shell);
        cmd.arg("-i");
        cmd
    }
}

#[cfg(windows)]
fn command_exists(command: &str) -> bool {
    std::process::Command::new("where")
        .arg(command)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}
