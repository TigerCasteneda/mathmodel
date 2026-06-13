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

pub struct PluginManager {
    plugins_dir: PathBuf,
    plugins: Arc<RwLock<HashMap<String, PluginState>>>,
}

#[derive(Debug, Clone)]
struct PluginState {
    manifest: PluginManifest,
    enabled: bool,
    loaded: bool,
}

impl PluginManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let plugins_dir = data_dir.join("plugins");
        Self {
            plugins_dir,
            plugins: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn reload(&self) {
        if !self.plugins_dir.exists() {
            return;
        }
        let Ok(mut entries) = tokio::fs::read_dir(&self.plugins_dir).await else {
            return;
        };
        let mut plugins = self.plugins.write().await;
        plugins.clear();

        while let Ok(Some(entry)) = entries.next_entry().await {
            if !entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let manifest_path = entry.path().join("plugin.json");
            if let Ok(data) = tokio::fs::read_to_string(&manifest_path).await {
                if let Ok(manifest) = serde_json::from_str::<PluginManifest>(&data) {
                    plugins.insert(
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

    pub async fn list(&self) -> Vec<PluginInfo> {
        let plugins = self.plugins.read().await;
        plugins
            .values()
            .map(|p| PluginInfo {
                name: p.manifest.name.clone(),
                version: p.manifest.version.clone(),
                description: p.manifest.description.clone(),
                enabled: p.enabled,
                loaded: p.loaded,
            })
            .collect()
    }

    pub async fn toggle(&self, name: &str, enabled: bool) -> bool {
        let mut plugins = self.plugins.write().await;
        if let Some(p) = plugins.get_mut(name) {
            p.enabled = enabled;
            true
        } else {
            false
        }
    }

    pub async fn get_manifest(&self, name: &str) -> Option<PluginManifest> {
        let plugins = self.plugins.read().await;
        plugins.get(name).map(|p| p.manifest.clone())
    }

    pub async fn install_from_git(&self, repo_url: &str) -> Result<String, String> {
        let repo_name = repo_url
            .split('/')
            .last()
            .unwrap_or("plugin")
            .trim_end_matches(".git");
        let target_dir = self.plugins_dir.join(repo_name);

        if target_dir.exists() {
            return Err(format!("plugin '{repo_name}' already installed"));
        }

        std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;

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

        Ok(repo_name.to_string())
    }

    pub async fn remove(&self, name: &str) -> Result<(), String> {
        let plugin_dir = self.plugins_dir.join(name);
        if !plugin_dir.exists() {
            return Err(format!("plugin '{name}' not found"));
        }
        let mut plugins = self.plugins.write().await;
        plugins.remove(name);
        drop(plugins);
        tokio::fs::remove_dir_all(&plugin_dir)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
