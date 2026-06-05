use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub role: String,
    pub content: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub created_at: i64,
    pub message_count: usize,
}

/// Persisted session store backed by JSON files in the Tauri app data dir.
pub struct ChatSessionStore {
    sessions_dir: PathBuf,
    /// In-memory index: conversation_id -> Session
    active: Mutex<HashMap<String, Session>>,
}

impl ChatSessionStore {
    pub fn new(app_data_dir: PathBuf) -> Self {
        let sessions_dir = app_data_dir.join("chat-sessions");
        let _ = std::fs::create_dir_all(&sessions_dir);

        Self {
            sessions_dir,
            active: Mutex::new(HashMap::new()),
        }
    }

    /// List all saved session infos (metadata only, no messages).
    pub fn list(&self) -> Result<Vec<SessionInfo>, String> {
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(session) = serde_json::from_str::<Session>(&content) {
                            list.push(SessionInfo {
                                id: session.id,
                                name: session.name,
                                created_at: session.created_at,
                                message_count: session.messages.len(),
                            });
                        }
                    }
                }
            }
        }
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(list)
    }

    /// Load or create a session by id.
    pub fn load(&self, conversation_id: &str) -> Result<Session, String> {
        {
            let active = self.active.lock().map_err(|e| e.to_string())?;
            if let Some(session) = active.get(conversation_id) {
                return Ok(session.clone());
            }
        }

        let path = self.sessions_dir.join(format!("{conversation_id}.json"));
        let session = if path.exists() {
            let content =
                std::fs::read_to_string(&path).map_err(|e| format!("read session: {e}"))?;
            serde_json::from_str::<Session>(&content).map_err(|e| format!("parse session: {e}"))?
        } else {
            Session {
                id: conversation_id.to_string(),
                name: "New Chat".to_string(),
                created_at: chrono::Utc::now().timestamp(),
                updated_at: chrono::Utc::now().timestamp(),
                messages: Vec::new(),
            }
        };

        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        active.insert(conversation_id.to_string(), session.clone());
        Ok(session)
    }

    /// Persist a session to disk and update in-memory.
    fn persist(&self, session: &Session) -> Result<(), String> {
        let path = self.sessions_dir.join(format!("{}.json", session.id));
        let content =
            serde_json::to_string_pretty(session).map_err(|e| format!("serialize session: {e}"))?;
        std::fs::write(&path, &content).map_err(|e| format!("write session: {e}"))?;

        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        active.insert(session.id.clone(), session.clone());
        Ok(())
    }

    /// Push a user message and persist.
    pub fn push_user(&self, conversation_id: &str, content: String) -> Result<(), String> {
        let mut session = self.load(conversation_id)?;
        session.messages.push(SessionMessage {
            role: "user".to_string(),
            content,
            timestamp: chrono::Utc::now().timestamp(),
        });
        session.updated_at = chrono::Utc::now().timestamp();
        self.persist(&session)
    }

    /// Push an assistant message and persist.
    pub fn push_assistant(&self, conversation_id: &str, content: String) -> Result<(), String> {
        let mut session = self.load(conversation_id)?;
        session.messages.push(SessionMessage {
            role: "assistant".to_string(),
            content,
            timestamp: chrono::Utc::now().timestamp(),
        });
        session.updated_at = chrono::Utc::now().timestamp();
        self.persist(&session)
    }

    /// Return Vec<ChatMessage> suitable for the API call.
    pub fn history(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<claude_code_rs::api::ChatMessage>, String> {
        let session = self.load(conversation_id)?;
        Ok(session
            .messages
            .iter()
            .map(|m| claude_code_rs::api::ChatMessage {
                role: m.role.clone(),
                content: Some(m.content.clone()),
                tool_calls: None,
                tool_call_id: None,
            })
            .collect())
    }

    pub fn delete(&self, conversation_id: &str) -> Result<(), String> {
        let path = self.sessions_dir.join(format!("{conversation_id}.json"));
        if path.exists() {
            std::fs::remove_file(&path).map_err(|e| format!("delete session: {e}"))?;
        }
        let mut active = self.active.lock().map_err(|e| e.to_string())?;
        active.remove(conversation_id);
        Ok(())
    }
}

/// ── Tauri commands ──

#[tauri::command]
pub fn list_sessions(
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<Vec<SessionInfo>, String> {
    store.list()
}

#[tauri::command]
pub fn load_session(
    conversation_id: Option<String>,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<Session, String> {
    store.load(&conversation_id.unwrap_or_else(|| "default".to_string()))
}

#[tauri::command]
pub fn delete_session(
    conversation_id: String,
    store: tauri::State<'_, ChatSessionStore>,
) -> Result<(), String> {
    store.delete(&conversation_id)
}

impl Default for ChatSessionStore {
    fn default() -> Self {
        Self::new(PathBuf::from("data/chat-sessions"))
    }
}
