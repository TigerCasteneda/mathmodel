#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum HookPoint {
    PreToolUse,
    PostToolUse,
    SessionStart,
    SessionStop,
    FileChanged,
}

impl HookPoint {
    #[allow(dead_code)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pre_tool_use" => Some(Self::PreToolUse),
            "post_tool_use" => Some(Self::PostToolUse),
            "session_start" => Some(Self::SessionStart),
            "session_stop" => Some(Self::SessionStop),
            "file_changed" => Some(Self::FileChanged),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hook {
    pub name: String,
    pub point: HookPoint,
    pub enabled: bool,
    #[serde(default)]
    pub handler_type: String, // "script" | "builtin"
    #[serde(default)]
    pub script_path: Option<String>,
    #[serde(default)]
    pub condition: Option<String>, // tool name pattern: "execute_command", "file_*"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookContext {
    pub hook_point: String,
    pub conversation_id: String,
    pub tool_name: Option<String>,
    pub tool_arguments: Option<serde_json::Value>,
    pub tool_output: Option<serde_json::Value>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResult {
    pub proceed: bool,
    pub modified_arguments: Option<serde_json::Value>,
    pub message: Option<String>,
}

pub struct HookManager {
    hooks: Arc<RwLock<Vec<Hook>>>,
    config_path: PathBuf,
}

impl HookManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let config_path = data_dir.join("hooks.json");
        let hooks = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|data| serde_json::from_str::<Vec<Hook>>(&data).ok())
            .unwrap_or_default();
        Self {
            hooks: Arc::new(RwLock::new(hooks)),
            config_path,
        }
    }

    async fn save(&self) {
        let hooks = self.hooks.read().await;
        if let Ok(json) = serde_json::to_string_pretty(&*hooks) {
            if let Some(parent) = self.config_path.parent() {
                std::fs::create_dir_all(parent).ok();
            }
            std::fs::write(&self.config_path, json).ok();
        }
    }

    #[allow(dead_code)]
    pub async fn add_hook(&self, hook: Hook) {
        let mut hooks = self.hooks.write().await;
        hooks.push(hook);
        drop(hooks);
        self.save().await;
    }

    pub async fn remove_hook(&self, name: &str) {
        let mut hooks = self.hooks.write().await;
        hooks.retain(|h| h.name != name);
        drop(hooks);
        self.save().await;
    }

    pub async fn list_hooks(&self) -> Vec<Hook> {
        self.hooks.read().await.clone()
    }

    pub async fn toggle_hook(&self, name: &str, enabled: bool) -> bool {
        let mut hooks = self.hooks.write().await;
        if let Some(h) = hooks.iter_mut().find(|h| h.name == name) {
            h.enabled = enabled;
            drop(hooks);
            self.save().await;
            true
        } else {
            false
        }
    }

    /// Execute all hooks for a given point. Returns false if any hook blocks execution.
    pub async fn execute(&self, point: HookPoint, ctx: &HookContext) -> bool {
        let hooks = self.hooks.read().await;
        let mut proceed = true;

        for hook in hooks.iter().filter(|h| {
            h.enabled && h.point == point && Self::matches_condition(h, ctx)
        }) {
            let result = match hook.handler_type.as_str() {
                "script" => Self::execute_script(hook, ctx).await,
                "builtin" => Self::execute_builtin(hook, ctx).await,
                _ => HookResult {
                    proceed: true,
                    modified_arguments: None,
                    message: None,
                },
            };
            if !result.proceed {
                proceed = false;
                break;
            }
        }

        proceed
    }

    fn matches_condition(hook: &Hook, ctx: &HookContext) -> bool {
        match &hook.condition {
            None => true,
            Some(pattern) => {
                let tool_name = ctx.tool_name.as_deref().unwrap_or("");
                // Support simple wildcard: "execute_command" or "file_*"
                if let Some(prefix) = pattern.strip_suffix('*') {
                    tool_name.starts_with(prefix)
                } else {
                    tool_name == pattern.as_str()
                }
            }
        }
    }

    async fn execute_script(hook: &Hook, ctx: &HookContext) -> HookResult {
        let script_path = match &hook.script_path {
            Some(p) => p.clone(),
            None => {
                return HookResult {
                    proceed: true,
                    modified_arguments: None,
                    message: Some("no script path configured".into()),
                }
            }
        };

        let input = serde_json::to_string(ctx).unwrap_or_default();
        let output = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&script_path)
            .env("HOOK_INPUT", &input)
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                if stdout.trim().is_empty() {
                    HookResult {
                        proceed: out.status.success(),
                        modified_arguments: None,
                        message: None,
                    }
                } else {
                    serde_json::from_str(&stdout).unwrap_or(HookResult {
                        proceed: out.status.success(),
                        modified_arguments: None,
                        message: Some(stdout),
                    })
                }
            }
            Err(e) => HookResult {
                proceed: true, // Don't block on script errors
                modified_arguments: None,
                message: Some(format!("hook script error: {e}")),
            },
        }
    }

    async fn execute_builtin(hook: &Hook, _ctx: &HookContext) -> HookResult {
        match hook.name.as_str() {
            "log" => {
                tracing::info!("Hook [{}] fired at {:?}", hook.name, hook.point);
                HookResult {
                    proceed: true,
                    modified_arguments: None,
                    message: None,
                }
            }
            "validate" => HookResult {
                proceed: true,
                modified_arguments: None,
                message: Some("builtin validate hook: pass".into()),
            },
            _ => HookResult {
                proceed: true,
                modified_arguments: None,
                message: None,
            },
        }
    }
}
