use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

pub struct SidecarState {
    port: Arc<Mutex<Option<u16>>>,
    child: Arc<Mutex<Option<Child>>>,
    sidecar_dir: PathBuf,
}

impl SidecarState {
    pub fn new(sidecar_dir: PathBuf) -> Self {
        Self {
            port: Arc::new(Mutex::new(None)),
            child: Arc::new(Mutex::new(None)),
            sidecar_dir,
        }
    }

    pub async fn ensure_started(&self, python_path: &str) -> anyhow::Result<u16> {
        let mut port_guard = self.port.lock().await;
        if let Some(port) = *port_guard {
            if Self::health_check(port).await {
                return Ok(port);
            }
        }

        let port = self.spawn_process(python_path).await?;
        *port_guard = Some(port);
        Ok(port)
    }

    pub async fn port(&self) -> Option<u16> {
        let port_guard = self.port.lock().await;
        *port_guard
    }

    pub async fn is_available(&self) -> bool {
        match self.port().await {
            Some(port) => Self::health_check(port).await,
            None => false,
        }
    }

    pub async fn stop(&self) {
        let mut child_guard = self.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            let _ = child.kill().await;
        }
        *self.port.lock().await = None;
    }

    async fn spawn_process(&self, python_path: &str) -> anyhow::Result<u16> {
        let mut child_guard = self.child.lock().await;
        if let Some(mut existing) = child_guard.take() {
            let _ = existing.kill().await;
        }

        let run_script = self.sidecar_dir.join("run.py");
        if !run_script.exists() {
            anyhow::bail!(
                "Sidecar script not found at {}",
                run_script.display()
            );
        }

        let (program, mut args) = Self::parse_python_command(python_path);
        args.push(run_script.to_string_lossy().to_string());

        let mut child = Command::new(&program)
            .args(&args)
            .current_dir(&self.sidecar_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture sidecar stdout"))?;

        let port = tokio::time::timeout(STARTUP_TIMEOUT, async {
            let mut reader = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = reader.next_line().await {
                if let Some(port_str) = line.strip_prefix("SIDECAR_PORT=") {
                    if let Ok(port) = port_str.trim().parse::<u16>() {
                        return Ok(port);
                    }
                }
            }
            anyhow::bail!("Sidecar exited without printing port")
        })
        .await
        .map_err(|_| anyhow::anyhow!("Sidecar startup timed out ({}s)", STARTUP_TIMEOUT.as_secs()))??;

        *child_guard = Some(child);

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if Self::health_check(port).await {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| anyhow::anyhow!("Sidecar started but health check never passed"))?;

        Ok(port)
    }

    async fn health_check(port: u16) -> bool {
        let url = format!("http://127.0.0.1:{port}/health");
        let Ok(response) = reqwest::Client::builder()
            .timeout(HEALTH_TIMEOUT)
            .build()
            .unwrap_or_default()
            .get(&url)
            .send()
            .await
        else {
            return false;
        };
        response.status().is_success()
    }

    pub fn sidecar_dir(&self) -> &Path {
        &self.sidecar_dir
    }

    /// Parse a python command string like "py -3.14" or "python3" into (program, args).
    fn parse_python_command(python_path: &str) -> (String, Vec<String>) {
        let parts: Vec<&str> = python_path.split_whitespace().collect();
        if parts.is_empty() {
            return ("python".to_string(), Vec::new());
        }
        let program = parts[0].to_string();
        let args = parts[1..].iter().map(|s| s.to_string()).collect();
        (program, args)
    }
}

impl Drop for SidecarState {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.start_kill();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_sidecar_state_has_no_port() {
        let state = SidecarState::new(PathBuf::from("/tmp/sidecar"));
        let port = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(state.port());
        assert!(port.is_none());
    }
}
