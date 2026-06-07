use claude_code_rs::Settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub api_key: Option<String>,
    pub base_url: String,
    pub model: String,
    pub firecrawl_api_key: Option<String>,
    pub context7_api_key: Option<String>,
    pub tavily_api_key: Option<String>,
    pub searxng_url: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            firecrawl_api_key: None,
            context7_api_key: None,
            tavily_api_key: None,
            searxng_url: "http://localhost:8080".to_string(),
        }
    }
}

impl AiConfig {
    pub fn to_claude_settings(&self, work_dir: PathBuf) -> Settings {
        let mut settings = Settings::default();
        settings.api.api_key = self.api_key.clone();
        settings.api.base_url = self.base_url.clone();
        settings.model = self.model.clone();
        settings.working_dir = work_dir;
        settings
    }
}

pub struct AiConfigState {
    path: PathBuf,
    inner: Mutex<AiConfig>,
}

impl AiConfigState {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let path = app_data_dir.join("ai-config.json");
        let config = std::fs::read_to_string(&path)
            .ok()
            .and_then(|content| serde_json::from_str::<AiConfig>(&content).ok())
            .unwrap_or_default();

        Self {
            path,
            inner: Mutex::new(config),
        }
    }

    pub fn set(&self, config: AiConfig) -> Result<(), String> {
        *self.inner.lock().map_err(|e| e.to_string())? = config;
        self.persist()
    }

    fn persist(&self) -> Result<(), String> {
        let config = self.inner.lock().map_err(|e| e.to_string())?.clone();
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(&self.path, content).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get(&self) -> Result<AiConfig, String> {
        Ok(self.inner.lock().map_err(|e| e.to_string())?.clone())
    }
}

impl Default for AiConfigState {
    fn default() -> Self {
        Self::new(PathBuf::from("data"))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AiConfigStatus {
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub firecrawl_configured: bool,
    pub context7_configured: bool,
    pub tavily_configured: bool,
    pub searxng_url: String,
}

impl From<AiConfig> for AiConfigStatus {
    fn from(config: AiConfig) -> Self {
        Self {
            configured: config
                .api_key
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty()),
            base_url: config.base_url,
            model: config.model,
            firecrawl_configured: config
                .firecrawl_api_key
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty()),
            context7_configured: config
                .context7_api_key
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty()),
            tavily_configured: config
                .tavily_api_key
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty()),
            searxng_url: config.searxng_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AiConfig, AiConfigState};

    fn temp_config_dir() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "modeler-ai-config-test-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    #[test]
    fn persists_ai_config_in_app_data_dir() {
        let dir = temp_config_dir();
        let state = AiConfigState::new(dir.clone());
        state
            .set(AiConfig {
                api_key: Some("sk-model".to_string()),
                base_url: "https://example.test".to_string(),
                model: "model-x".to_string(),
                firecrawl_api_key: Some("fc-key".to_string()),
                context7_api_key: Some("ctx-key".to_string()),
                tavily_api_key: Some("tvly-key".to_string()),
                searxng_url: "http://localhost:9999".to_string(),
            })
            .unwrap();

        let reloaded = AiConfigState::new(dir).get().unwrap();

        assert_eq!(reloaded.api_key.as_deref(), Some("sk-model"));
        assert_eq!(reloaded.firecrawl_api_key.as_deref(), Some("fc-key"));
        assert_eq!(reloaded.context7_api_key.as_deref(), Some("ctx-key"));
        assert_eq!(reloaded.tavily_api_key.as_deref(), Some("tvly-key"));
        assert_eq!(reloaded.searxng_url, "http://localhost:9999");
    }

    #[test]
    fn status_reports_configured_flags_without_exposing_keys() {
        let status = super::AiConfigStatus::from(AiConfig {
            api_key: Some("sk-model".to_string()),
            firecrawl_api_key: Some("fc-key".to_string()),
            context7_api_key: Some("ctx-key".to_string()),
            tavily_api_key: Some("tvly-key".to_string()),
            ..AiConfig::default()
        });

        assert!(status.configured);
        assert!(status.firecrawl_configured);
        assert!(status.context7_configured);
        assert!(status.tavily_configured);
    }
}
