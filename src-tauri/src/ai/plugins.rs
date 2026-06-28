#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub hooks: Vec<PluginHookDef>,
    #[serde(default)]
    pub commands: Vec<PluginCommandDef>,
    #[serde(default)]
    pub skills: Vec<PluginSkillDef>,
    #[serde(default)]
    pub isolation: Option<IsolationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginHookDef {
    pub hook_point: String,
    pub handler: String, // script path or "builtin:{name}"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginCommandDef {
    pub name: String,
    pub description: String,
    pub handler: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSkillDef {
    pub name: String,
    pub description: String,
    pub handler: String, // script path
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IsolationConfig {
    #[serde(default)]
    pub allowed_paths: Vec<String>,
    #[serde(default)]
    pub denied_paths: Vec<String>,
    #[serde(default)]
    pub allowed_commands: Vec<String>,
    #[serde(default)]
    pub network_access: bool,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    120
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub enabled: bool,
    pub loaded: bool,
}

/// Per-user plugin store. The previous layout used a single
/// `plugins/<repo_name>/` directory plus one
/// `HashMap<String, PluginState>` shared across the Tauri process.
/// `install_from_git` is the dangerous part: it `git clone`s an
/// arbitrary URL into that shared directory, so a plugin installed
/// by User A would also be loaded by User B on the next reload.
/// Plugin manifests can register hooks / commands / skills, all of
/// which run User B's code on User B's environment.
///
/// Now `plugins/<user_id>/<repo_name>/` is the install path and the
/// in-memory cache is keyed by `(user_id, plugin_name)`. Reload
/// only scans the calling user's subtree.
pub struct PluginManager {
    plugins_root: PathBuf,
    /// user_id -> plugin_name -> state
    plugins: Arc<RwLock<HashMap<String, HashMap<String, PluginState>>>>,
}

#[derive(Debug, Clone)]
struct PluginState {
    manifest: PluginManifest,
    enabled: bool,
    loaded: bool,
}

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

impl PluginManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let plugins_root = data_dir.join("plugins");
        let _ = std::fs::create_dir_all(&plugins_root);

        // One-shot migration: the previous layout used
        // `plugins/<repo_name>/` with no user_id. We can't safely
        // attribute pre-existing plugins to any account — they were
        // installed by whoever ran `install_from_git` first. Drop them.
        // Consistent with chat / hooks / plans migration policy.
        if let Ok(entries) = std::fs::read_dir(&plugins_root) {
            for entry in entries.flatten() {
                let path = entry.path();
                let is_dir = path
                    .metadata()
                    .map(|m| m.is_dir())
                    .unwrap_or(false);
                if is_dir {
                    let _ = std::fs::remove_dir_all(&path);
                }
            }
        }

        Self {
            plugins_root,
            plugins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn user_dir(&self, user_id: &str) -> PathBuf {
        self.plugins_root.join(sanitize_user_id(user_id))
    }

    /// Walk the user's plugin directory and populate the in-memory
    /// cache. Replaces whatever was previously in the cache for the
    /// user, so disabled-then-reinstalled plugins surface correctly.
    pub async fn reload(&self, user_id: &str) {
        let user_dir = self.user_dir(user_id);
        let mut user_map = HashMap::new();

        if user_dir.exists() {
            if let Ok(mut entries) = tokio::fs::read_dir(&user_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                        continue;
                    }
                    let manifest_path = entry.path().join("plugin.json");
                    if let Ok(data) = tokio::fs::read_to_string(&manifest_path).await {
                        if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&data) {
                            user_map.insert(
                                manifest.name.clone(),
                                PluginState {
                                    manifest,
                                    enabled: true,
                                    loaded: true,
                                },
                            );
                        }
                    }
                }
            }
        }

        let mut plugins = self.plugins.write().await;
        plugins.insert(user_id.to_string(), user_map);
    }

    pub async fn list(&self, user_id: &str) -> Vec<PluginInfo> {
        let plugins = self.plugins.read().await;
        plugins
            .get(user_id)
            .map(|m| {
                m.values()
                    .map(|p| PluginInfo {
                        name: p.manifest.name.clone(),
                        version: p.manifest.version.clone(),
                        description: p.manifest.description.clone(),
                        enabled: p.enabled,
                        loaded: p.loaded,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn toggle(&self, user_id: &str, name: &str, enabled: bool) -> bool {
        let mut plugins = self.plugins.write().await;
        let user_map = match plugins.get_mut(user_id) {
            Some(m) => m,
            None => return false,
        };
        if let Some(p) = user_map.get_mut(name) {
            p.enabled = enabled;
            true
        } else {
            false
        }
    }

    pub async fn get_manifest(&self, user_id: &str, name: &str) -> Option<PluginManifest> {
        let plugins = self.plugins.read().await;
        plugins
            .get(user_id)
            .and_then(|m| m.get(name))
            .map(|p| p.manifest.clone())
    }

    /// `git clone <repo_url>` into the user's plugin directory and
    /// register the result. The clone target is per-user, so User A's
    /// malicious plugin can never land in User B's directory.
    pub async fn install_from_git(&self, user_id: &str, repo_url: &str) -> Result<String, String> {
        let repo_name = repo_url
            .split('/')
            .last()
            .unwrap_or("plugin")
            .trim_end_matches(".git");
        let user_dir = self.user_dir(user_id);
        let target_dir = user_dir.join(repo_name);

        if target_dir.exists() {
            return Err(format!("plugin '{repo_name}' already installed"));
        }

        std::fs::create_dir_all(&user_dir).map_err(|e| e.to_string())?;

        let output = tokio::process::Command::new("git")
            .args(["clone", "--depth", "1", repo_url])
            .arg(&target_dir)
            .output()
            .await
            .map_err(|e| format!("git clone failed: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git clone failed: {stderr}"));
        }

        // Refresh the in-memory cache so list/toggle can see the new
        // plugin without an explicit reload.
        self.reload(user_id).await;
        Ok(repo_name.to_string())
    }

    pub async fn remove(&self, user_id: &str, name: &str) -> Result<(), String> {
        let plugin_dir = self.user_dir(user_id).join(name);
        if !plugin_dir.exists() {
            return Err(format!("plugin '{name}' not found"));
        }
        {
            let mut plugins = self.plugins.write().await;
            if let Some(user_map) = plugins.get_mut(user_id) {
                user_map.remove(name);
            }
        }
        tokio::fs::remove_dir_all(&plugin_dir)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{sanitize_user_id, PluginManager};
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
        let dir = std::env::temp_dir().join(format!("modeler-plugins-{label}-{nanos}-{n}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sanitize_user_id_basic() {
        assert_eq!(sanitize_user_id("user-abc_123"), "user-abc_123");
        assert_eq!(sanitize_user_id(""), "unknown");
    }

    #[test]
    fn new_drops_legacy_unscoped_plugins() {
        let root = unique_tmp_dir("legacy-plugins");
        let plugins = root.join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        // A pre-existing unscoped plugin directory.
        let legacy = plugins.join("old-plugin");
        std::fs::create_dir_all(&legacy).unwrap();
        std::fs::write(
            legacy.join("plugin.json"),
            r#"{"name":"old-plugin","version":"0.1.0","description":"x"}"#,
        )
        .unwrap();

        let _manager = PluginManager::new(root.clone());

        assert!(!legacy.exists(), "legacy plugin dir should be removed");
        assert!(plugins.is_dir());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugins_are_isolated_per_user() {
        let root = unique_tmp_dir("per-user-plugins");
        let manager = PluginManager::new(root.clone());

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            // Stage a plugin under Alice's directory manually so we
            // can exercise the cross-user visibility contract without
            // shelling out to git.
            let alice_dir = root.join("plugins").join("user-alice");
            std::fs::create_dir_all(alice_dir.join("alice-plugin")).unwrap();
            std::fs::write(
                alice_dir.join("alice-plugin").join("plugin.json"),
                r#"{"name":"alice-plugin","version":"0.1.0","description":"alice"}"#,
            )
            .unwrap();

            // Reload as Alice and confirm she sees her plugin.
            manager.reload("user-alice").await;
            let alice_list = manager.list("user-alice").await;
            assert_eq!(alice_list.len(), 1);
            assert_eq!(alice_list[0].name, "alice-plugin");

            // Bob has no plugins — list returns empty, even though
            // the on-disk layout under the shared plugins/ root
            // contains Alice's plugin.
            manager.reload("user-bob").await;
            let bob_list = manager.list("user-bob").await;
            assert_eq!(bob_list.len(), 0, "Bob must NOT see Alice's plugin");

            // Reload a second time as Bob, the in-memory cache must
            // not retain Alice's plugin in Bob's slot.
            manager.reload("user-bob").await;
            assert_eq!(manager.list("user-bob").await.len(), 0);
        });

        let _ = std::fs::remove_dir_all(&root);
    }
}
