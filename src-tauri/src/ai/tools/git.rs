use async_trait::async_trait;
use claude_code_rs::mcp::ToolExecutor;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command;

use crate::ai::workspace::WorkspaceProvider;

pub struct GitExecutor {
    pub work_dir: PathBuf,
    pub workspace: Arc<dyn WorkspaceProvider>,
}

#[async_trait]
impl ToolExecutor for GitExecutor {
    async fn execute(&self, input: Value) -> anyhow::Result<Value> {
        let operation = input["operation"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("operation is required"))?;

        let args: Vec<String> = input["args"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        match operation {
            "status" => self.run_git(&["status"], &args).await,
            "add" => {
                let files: Vec<String> = input["files"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                    .unwrap_or_else(|| vec![".".to_string()]);
                let mut cmd_args = vec!["add"];
                cmd_args.extend(args.iter().map(|s| s.as_str()));
                cmd_args.extend(files.iter().map(|s| s.as_str()));
                self.run_git(&cmd_args, &[]).await
            }
            "commit" => {
                let message = input["message"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("message is required for commit"))?;
                let mut cmd_args = vec!["commit", "-m", message];
                cmd_args.extend(args.iter().map(|s| s.as_str()));
                self.run_git(&cmd_args, &[]).await
            }
            "push" => {
                let remote = input["remote"].as_str().unwrap_or("origin");
                let branch = input["branch"].as_str().unwrap_or("");
                let mut cmd_args = vec!["push"];
                cmd_args.extend(args.iter().map(|s| s.as_str()));
                let refspec = if !branch.is_empty() {
                    format!("{}:{}", remote, branch)
                } else {
                    remote.to_string()
                };
                cmd_args.push(&refspec);
                self.run_git(&cmd_args, &[]).await
            }
            "pull" => {
                let remote = input["remote"].as_str().unwrap_or("origin");
                let branch = input["branch"].as_str().unwrap_or("");
                let mut cmd_args = vec!["pull", remote];
                if !branch.is_empty() {
                    cmd_args.push(branch);
                }
                cmd_args.extend(args.iter().map(|s| s.as_str()));
                self.run_git(&cmd_args, &[]).await
            }
            "log" => {
                let mut final_args: Vec<&str> = vec!["log"];
                if !args.iter().any(|a| a.starts_with("--pretty=") || a == "--oneline") {
                    final_args.push("--oneline");
                }
                final_args.extend(args.iter().map(|s| s.as_str()));
                self.run_git(&final_args, &[]).await
            }
            "diff" => {
                let mut cmd_args = vec!["diff"];
                cmd_args.extend(args.iter().map(|s| s.as_str()));
                self.run_git(&cmd_args, &[]).await
            }
            "branch" => {
                let branch = input["branch"].as_str();
                let mut cmd_args = vec!["branch"];
                cmd_args.extend(args.iter().map(|s| s.as_str()));
                if let Some(b) = branch {
                    cmd_args.push(b);
                }
                self.run_git(&cmd_args, &[]).await
            }
            "checkout" => {
                let branch = input["branch"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("branch is required for checkout"))?;
                let mut cmd_args = vec!["checkout", branch];
                cmd_args.extend(args.iter().map(|s| s.as_str()));
                self.run_git(&cmd_args, &[]).await
            }
            other => Err(anyhow::anyhow!("unknown git operation: {other}")),
        }
    }
}

impl GitExecutor {
    async fn run_git(&self, base_args: &[&str], _extra: &[String]) -> anyhow::Result<Value> {
        let output = Command::new("git")
            .current_dir(&self.work_dir)
            .args(base_args)
            .output()
            .await
            .map_err(|e| anyhow::anyhow!("failed to run git: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();

        let content = if output.status.success() {
            if stdout.is_empty() && !stderr.is_empty() { stderr } else { stdout }
        } else {
            format!("Git failed (exit {})\n{}\n{}", output.status, stdout, stderr)
        };

        Ok(json!({ "success": output.status.success(), "output": content }))
    }
}
