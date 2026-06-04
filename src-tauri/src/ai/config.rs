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
    pub searxng_url: String,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: "https://api.deepseek.com".to_string(),
            model: "deepseek-v4-pro".to_string(),
            firecrawl_api_key: None,
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

#[derive(Default)]
pub struct AiConfigState {
    inner: Mutex<AiConfig>,
}

impl AiConfigState {
    pub fn set(&self, config: AiConfig) -> Result<(), String> {
        *self.inner.lock().map_err(|e| e.to_string())? = config;
        Ok(())
    }

    pub fn get(&self) -> Result<AiConfig, String> {
        Ok(self.inner.lock().map_err(|e| e.to_string())?.clone())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct AiConfigStatus {
    pub configured: bool,
    pub base_url: String,
    pub model: String,
    pub firecrawl_configured: bool,
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
            searxng_url: config.searxng_url,
        }
    }
}
