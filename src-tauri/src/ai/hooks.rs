#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// Per-user hook store. Layout is `data_dir/hooks/<user_id>.json` so
/// User A's `pre_tool_use` script never runs against User B's tool
/// calls — that would be arbitrary-code-execution-as-a-service.
/// Hooks can call any `sh -c "<script_path>"` (see `execute_script`),
/// and previously a single `hooks.json` was shared across every
/// account on the same desktop install.
pub struct HookManager {
    /// user_id -> ordered list of hooks
    hooks: Arc<RwLock<HashMap<String, Vec<Hook>>>>,
    /// Root directory holding per-user `<user_id>.json` files.
    config_dir: PathBuf,
}

/// Sanitize a user id to a safe filename component. Mirrors the
/// `sanitize_user_id` helper in `session.rs` — kept local here so the
/// module has no cross-deps just for a one-line filter.
fn sanitize_user_id(user_id: &str) -> String {
    let cleaned: String = user_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

impl HookManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let config_dir = data_dir.join("hooks");
        let _ = std::fs::create_dir_all(&config_dir);

        // One-shot migration: the previous layout used a single
        // `hooks.json` at the data-dir root. It held hooks installed by
        // whoever happened to use the desktop first; we can't safely
        // attribute them to any user_id, so drop the file. Hooks that
        // someone wants kept can be re-installed under their account.
        let legacy = data_dir.join("hooks.json");
        if legacy.exists() {
            let _ = std::fs::remove_file(&legacy);
        }

        Self {
            hooks: Arc::new(RwLock::new(HashMap::new())),
            config_dir,
        }
    }

    fn user_config_path(&self, user_id: &str) -> PathBuf {
        self.config_dir.join(format!("{}.json", sanitize_user_id(user_id)))
    }

    /// Load the hook list for a user from disk, populating the in-memory
    /// cache on first access. Subsequent calls hit the cache.
    async fn ensure_user_loaded(&self, user_id: &str) -> Vec<Hook> {
        {
            let map = self.hooks.read().await;
            if let Some(hooks) = map.get(user_id) {
                return hooks.clone();
            }
        }

        let path = self.user_config_path(user_id);
        let hooks: Vec<Hook> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str::<Vec<Hook>>(&data).ok())
            .unwrap_or_default();

        let mut map = self.hooks.write().await;
        // Another caller may have raced us into populating; that's fine —
        // their list is identical because the file is deterministic.
        map.entry(user_id.to_string()).or_insert(hooks.clone());
        hooks
    }

    async fn save(&self, user_id: &str, hooks: &[Hook]) {
        let path = self.user_config_path(user_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(json) = serde_json::to_string_pretty(hooks) {
            let _ = std::fs::write(&path, json);
        }
    }

    /// Append a hook to the user's list and persist. Updates the in-memory
    /// cache so subsequent `list_hooks` / `execute` calls see the new
    /// hook without re-reading from disk.
    pub async fn add_hook(&self, user_id: &str, hook: Hook) {
        let hooks = {
            let mut map = self.hooks.write().await;
            let list = map.entry(user_id.to_string()).or_insert_with(Vec::new);
            list.push(hook);
            list.clone()
        };
        self.save(user_id, &hooks).await;
    }

    /// Remove a hook by name. No-op if not found. Updates the in-memory
    /// cache.
    pub async fn remove_hook(&self, user_id: &str, name: &str) {
        let hooks = {
            let mut map = self.hooks.write().await;
            let list = map.entry(user_id.to_string()).or_insert_with(Vec::new);
            list.retain(|h| h.name != name);
            list.clone()
        };
        self.save(user_id, &hooks).await;
    }

    /// Snapshot the user's hook list (no scripts run).
    pub async fn list_hooks(&self, user_id: &str) -> Vec<Hook> {
        self.ensure_user_loaded(user_id).await
    }

    /// Toggle a hook's enabled flag by name. Returns `true` if a hook
    /// with the given name was found. Updates the in-memory cache.
    pub async fn toggle_hook(&self, user_id: &str, name: &str, enabled: bool) -> bool {
        let (found, hooks) = {
            let mut map = self.hooks.write().await;
            let list = map.entry(user_id.to_string()).or_insert_with(Vec::new);
            let found = list.iter_mut().find(|h| h.name == name).is_some();
            if let Some(h) = list.iter_mut().find(|h| h.name == name) {
                h.enabled = enabled;
            }
            (found, list.clone())
        };
        if found {
            self.save(user_id, &hooks).await;
        }
        found
    }

    /// Execute all of the user's hooks for a given point. Returns false
    /// if any hook blocks execution. Only this user's hooks run — the
    /// `user_id` argument is the security boundary: the previous
    /// implementation shared a single `Vec<Hook>` across every account
    /// on the install.
    pub async fn execute(&self, user_id: &str, point: HookPoint, ctx: &HookContext) -> bool {
        let hooks = self.ensure_user_loaded(user_id).await;
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

#[cfg(test)]
mod tests {
    use super::{sanitize_user_id, Hook, HookManager, HookPoint};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn unique_tmp_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("modeler-hooks-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitize_user_id_keeps_alphanumeric_dash_underscore() {
        assert_eq!(sanitize_user_id("user-abc_123"), "user-abc_123");
        assert_eq!(sanitize_user_id("../../etc/passwd"), "etcpasswd");
        assert_eq!(sanitize_user_id(""), "unknown");
    }

    #[test]
    fn new_drops_legacy_hooks_json() {
        let root = unique_tmp_dir("legacy-hooks");
        // Pre-existing legacy file at the old shared location.
        std::fs::write(
            root.join("hooks.json"),
            r#"[{"name":"legacy","point":"pre_tool_use","enabled":true}]"#,
        )
        .unwrap();

        let _manager = HookManager::new(root.clone());

        assert!(!root.join("hooks.json").exists(), "legacy hooks.json must be removed");
        // Per-user dir is created eagerly.
        assert!(root.join("hooks").is_dir());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hooks_are_isolated_per_user() {
        let root = unique_tmp_dir("per-user-hooks");
        let manager = HookManager::new(root.clone());

        // Alice installs a pre_tool_use hook with a script path that would
        // (if executed) write a file. Bob must never see it.
        let alice_hook = Hook {
            name: "alice-only".to_string(),
            point: HookPoint::PreToolUse,
            enabled: true,
            handler_type: "script".to_string(),
            script_path: Some("echo alice > /tmp/alice-marker".to_string()),
            condition: None,
        };
        // Bob has no hooks.

        // Drive the manager synchronously via tokio.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            manager.add_hook("user-alice", alice_hook).await;

            let alice_list = manager.list_hooks("user-alice").await;
            let bob_list = manager.list_hooks("user-bob").await;

            assert_eq!(alice_list.len(), 1, "Alice should see her hook");
            assert_eq!(bob_list.len(), 0, "Bob must NOT see Alice's hook");

            // Disk layout: alice's hook lives in hooks/user-alice.json;
            // no file is created for users with no hooks.
            assert!(root.join("hooks").join("user-alice.json").exists());
            assert!(!root.join("hooks").join("user-bob.json").exists());

            // Toggle isolation: flipping a hook on for one user must not
            // create the file for the other.
            let _ = manager.toggle_hook("user-bob", "alice-only", true).await;
            assert!(!root.join("hooks").join("user-bob.json").exists());
        });

        let _ = std::fs::remove_dir_all(&root);
    }
}
